//! Memory-to-Register (Mem2Reg) and SSA Construction Optimization Pass for Vanta MIR.
//!
//! Promotes eligible non-escaping scalar local variables (`SlotId`) to SSA `ValueId` values,
//! places `Phi` nodes at control-flow join points using dominance frontiers,
//! splits critical edges, and eliminates redundant stack loads and stores.

use crate::ir::*;
use std::collections::{HashMap, HashSet};
use vanta_sema::Type;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mem2RegStats {
    pub promoted_slots: usize,
    pub eliminated_loads: usize,
    pub eliminated_stores: usize,
    pub inserted_phis: usize,
    pub split_critical_edges: usize,
}

pub struct Mem2Reg;

impl Mem2Reg {
    /// Runs Mem2Reg on an MIR function.
    /// Returns `(changed, stats)`.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, Mem2RegStats) {
        let mut stats = Mem2RegStats::default();

        if func.blocks.is_empty() || func.slots.is_empty() {
            return (false, stats);
        }

        // Clean up dead instructions and unreachable blocks first
        crate::opt::dce::DeadCodeElimination::run_on_function(func);

        // 1. Identify eligible slots
        let eligible_slots = Self::find_eligible_slots(func);
        if eligible_slots.is_empty() {
            return (false, stats);
        }

        // 2. Split critical edges upfront
        let split_count = Self::split_critical_edges(func);
        stats.split_critical_edges = split_count;

        // 3. Compute Dominance Tree & Dominance Frontiers
        let dom_tree = match crate::opt::dom::DominatorTree::compute(func) {
            Some(dt) => dt,
            None => return (false, stats),
        };
        let df = dom_tree.compute_dominance_frontiers();
        let reachable_blocks = dom_tree.reachable_blocks.clone();
        let dom_children = dom_tree.dom_children.clone();
        let successors = dom_tree.successors.clone();
        let entry_block = dom_tree.entry;

        // 4. Compute Iterated Dominance Frontiers (IDF) and Place Phi Placeholders
        let mut next_val_id = func.max_value_id() + 1;
        let mut slot_phi_defs: HashMap<SlotId, HashMap<BlockId, ValueId>> = HashMap::new();
        let mut block_phis: HashMap<BlockId, Vec<(SlotId, ValueId, Type)>> = HashMap::new();

        for &slot_id in &eligible_slots {
            let slot = func.slots.iter().find(|s| s.id == slot_id).unwrap();
            let def_blocks = Self::find_store_blocks(func, slot_id, &reachable_blocks);
            let idf_blocks = Self::compute_idf(&def_blocks, &df);

            let mut phi_map = HashMap::new();
            for b_id in idf_blocks {
                let phi_val = ValueId(next_val_id);
                next_val_id += 1;
                phi_map.insert(b_id, phi_val);
                block_phis
                    .entry(b_id)
                    .or_default()
                    .push((slot_id, phi_val, slot.ty.clone()));
                stats.inserted_phis += 1;
            }
            slot_phi_defs.insert(slot_id, phi_map);
        }

        // Insert Phi instructions at the beginning of basic blocks
        for block in &mut func.blocks {
            if let Some(phis) = block_phis.get(&block.id) {
                let mut phi_insts = Vec::new();
                for (_slot_id, phi_val, ty) in phis {
                    phi_insts.push(Instruction::Phi {
                        dest: *phi_val,
                        inputs: Vec::new(),
                        ty: ty.clone(),
                    });
                }
                // Prepend phis before existing instructions
                phi_insts.append(&mut block.instructions);
                block.instructions = phi_insts;
            }
        }

        // 7. Dominator Tree Preorder Traversal & Renaming
        let mut reaching_defs: HashMap<SlotId, Vec<ValueId>> = HashMap::new();
        let mut val_subst: HashMap<ValueId, ValueId> = HashMap::new();
        let mut loads_to_remove: HashSet<ValueId> = HashSet::new();
        let mut stores_to_remove: HashSet<(BlockId, usize)> = HashSet::new();

        // For each eligible slot, push a default value in case of read-before-write
        for &slot_id in &eligible_slots {
            let slot = func.slots.iter().find(|s| s.id == slot_id).unwrap();
            let default_val = ValueId(next_val_id);
            next_val_id += 1;
            let default_inst = match &slot.ty {
                Type::Bool => Instruction::ConstBool {
                    dest: default_val,
                    val: false,
                },
                _ => Instruction::ConstInt {
                    dest: default_val,
                    val: 0,
                    ty: slot.ty.clone(),
                },
            };
            func.blocks[0].instructions.insert(0, default_inst);
            reaching_defs.insert(slot_id, vec![default_val]);
        }

        Self::rename_in_dom_tree(
            entry_block,
            &dom_children,
            &successors,
            &eligible_slots,
            &slot_phi_defs,
            func,
            &mut reaching_defs,
            &mut val_subst,
            &mut loads_to_remove,
            &mut stores_to_remove,
            &mut stats,
        );

        // 8. Apply value substitutions across all instructions and terminators
        Self::apply_value_substitutions(func, &val_subst);

        // 9. Remove promoted Load and Store instructions
        for block in &mut func.blocks {
            let b_id = block.id;
            let mut inst_idx = 0;
            block.instructions.retain(|inst| {
                let idx = inst_idx;
                inst_idx += 1;
                match inst {
                    Instruction::Load { dest, .. } => !loads_to_remove.contains(dest),
                    Instruction::Store { .. } => !stores_to_remove.contains(&(b_id, idx)),
                    _ => true,
                }
            });
        }

        // 10. Remove promoted slots from func.slots
        let eligible_set: HashSet<SlotId> = eligible_slots.into_iter().collect();
        stats.promoted_slots = eligible_set.len();
        func.slots.retain(|s| !eligible_set.contains(&s.id));

        (true, stats)
    }

    /// Identifies all local slots eligible for Mem2Reg promotion.
    fn find_eligible_slots(func: &MirFunction) -> Vec<SlotId> {
        let param_slots: HashSet<SlotId> = func.params.iter().map(|(s, _)| *s).collect();
        let mut rejected = HashSet::new();
        let mut candidate_slots: HashSet<SlotId> = HashSet::new();

        for slot in &func.slots {
            if param_slots.contains(&slot.id) {
                rejected.insert(slot.id);
                continue;
            }
            if !Self::is_promotable_type(&slot.ty) {
                rejected.insert(slot.id);
                continue;
            }
            candidate_slots.insert(slot.id);
        }

        // Collect all used values to check if SlotAddr actually escapes
        let mut used_values: HashSet<ValueId> = HashSet::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Store { val, .. } => {
                        used_values.insert(*val);
                    }
                    Instruction::Binary { lhs, rhs, .. } => {
                        used_values.insert(*lhs);
                        used_values.insert(*rhs);
                    }
                    Instruction::Unary { operand, .. } => {
                        used_values.insert(*operand);
                    }
                    Instruction::Call { args, .. } => {
                        for a in args {
                            used_values.insert(*a);
                        }
                    }
                    Instruction::IndexAddr { index, .. } => {
                        used_values.insert(*index);
                    }
                    Instruction::IndexPtrAddr {
                        base_ptr, index, ..
                    } => {
                        used_values.insert(*base_ptr);
                        used_values.insert(*index);
                    }
                    Instruction::FieldAddr { base_ptr, .. } => {
                        used_values.insert(*base_ptr);
                    }
                    Instruction::LoadIndirect { addr, .. } => {
                        used_values.insert(*addr);
                    }
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
                        used_values.insert(*addr);
                        used_values.insert(*val);
                    }
                    Instruction::StoreSlice { ptr, len, .. } => {
                        used_values.insert(*ptr);
                        used_values.insert(*len);
                    }
                    Instruction::Copy { src, .. } => {
                        used_values.insert(*src);
                    }
                    Instruction::Phi { inputs, .. } => {
                        for (_, v) in inputs {
                            used_values.insert(*v);
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ref term) = block.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        used_values.insert(*cond);
                    }
                    Terminator::Return(Some(val)) => {
                        used_values.insert(*val);
                    }
                    _ => {}
                }
            }
        }

        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::SlotAddr { dest, slot } => {
                        if used_values.contains(dest) {
                            rejected.insert(*slot);
                        }
                    }
                    Instruction::IndexAddr { slot, .. }
                    | Instruction::StoreSlice { slot, .. }
                    | Instruction::LoadSlice { slot, .. } => {
                        rejected.insert(*slot);
                    }
                    _ => {}
                }
            }
        }

        candidate_slots
            .into_iter()
            .filter(|s| !rejected.contains(s))
            .collect()
    }

    fn is_promotable_type(ty: &Type) -> bool {
        matches!(
            ty,
            Type::I64
                | Type::I32
                | Type::U64
                | Type::U32
                | Type::I16
                | Type::U16
                | Type::I8
                | Type::U8
                | Type::Bool
                | Type::Ref { .. }
        )
    }

    /// Splits critical edges in the CFG upfront.
    /// A critical edge is an edge from a block with multiple successors to a block with multiple predecessors.
    fn split_critical_edges(func: &mut MirFunction) -> usize {
        let mut split_count = 0;
        let mut next_block_id = func.max_block_id() + 1;

        // Compute predecessor counts
        let mut pred_counts: HashMap<BlockId, usize> = HashMap::new();
        for b in &func.blocks {
            if let Some(ref term) = b.terminator {
                match term {
                    Terminator::Branch(t) => {
                        *pred_counts.entry(*t).or_default() += 1;
                    }
                    Terminator::CondBranch {
                        then_block,
                        else_block,
                        ..
                    } => {
                        *pred_counts.entry(*then_block).or_default() += 1;
                        *pred_counts.entry(*else_block).or_default() += 1;
                    }
                    _ => {}
                }
            }
        }

        let mut new_blocks = Vec::new();

        for block in &mut func.blocks {
            let mut new_then = None;
            let mut new_else = None;

            if let Some(Terminator::CondBranch {
                then_block,
                else_block,
                ..
            }) = &block.terminator
            {
                let then_preds = pred_counts.get(then_block).copied().unwrap_or(0);
                if then_preds > 1 {
                    let split_id = BlockId(next_block_id);
                    next_block_id += 1;
                    new_then = Some((*then_block, split_id));
                }

                let else_preds = pred_counts.get(else_block).copied().unwrap_or(0);
                if else_preds > 1 {
                    let split_id = BlockId(next_block_id);
                    next_block_id += 1;
                    new_else = Some((*else_block, split_id));
                }
            }

            if let Some((old_then, split_then)) = new_then {
                if let Some(Terminator::CondBranch {
                    ref mut then_block, ..
                }) = block.terminator
                {
                    *then_block = split_then;
                }
                new_blocks.push(BasicBlock {
                    id: split_then,
                    name: format!("{}_crit_{}", func.name, split_then.0),
                    instructions: Vec::new(),
                    terminator: Some(Terminator::Branch(old_then)),
                });
                split_count += 1;
            }

            if let Some((old_else, split_else)) = new_else {
                if let Some(Terminator::CondBranch {
                    ref mut else_block, ..
                }) = block.terminator
                {
                    *else_block = split_else;
                }
                new_blocks.push(BasicBlock {
                    id: split_else,
                    name: format!("{}_crit_{}", func.name, split_else.0),
                    instructions: Vec::new(),
                    terminator: Some(Terminator::Branch(old_else)),
                });
                split_count += 1;
            }
        }

        func.blocks.extend(new_blocks);
        split_count
    }

    /// Finds all basic blocks that contain a `Store` to `slot_id`.
    fn find_store_blocks(
        func: &MirFunction,
        slot_id: SlotId,
        reachable: &[BlockId],
    ) -> Vec<BlockId> {
        let reachable_set: HashSet<BlockId> = reachable.iter().copied().collect();
        let mut def_blocks = Vec::new();

        for block in &func.blocks {
            if !reachable_set.contains(&block.id) {
                continue;
            }
            for inst in &block.instructions {
                if let Instruction::Store { slot, .. } = inst {
                    if *slot == slot_id {
                        def_blocks.push(block.id);
                        break;
                    }
                }
            }
        }

        def_blocks
    }

    /// Computes Iterated Dominance Frontier (IDF) for a set of defining blocks.
    fn compute_idf(def_blocks: &[BlockId], df: &HashMap<BlockId, HashSet<BlockId>>) -> Vec<BlockId> {
        let mut idf = HashSet::new();
        let mut worklist: Vec<BlockId> = def_blocks.to_vec();

        while let Some(b) = worklist.pop() {
            if let Some(frontiers) = df.get(&b) {
                for &y in frontiers {
                    if idf.insert(y) {
                        worklist.push(y);
                    }
                }
            }
        }

        idf.into_iter().collect()
    }

    /// Preorder traversal of dominator tree to rename variables and wire Phi inputs.
    fn rename_in_dom_tree(
        curr_block: BlockId,
        dom_children: &HashMap<BlockId, Vec<BlockId>>,
        successors: &HashMap<BlockId, Vec<BlockId>>,
        eligible_slots: &[SlotId],
        slot_phi_defs: &HashMap<SlotId, HashMap<BlockId, ValueId>>,
        func: &mut MirFunction,
        reaching_defs: &mut HashMap<SlotId, Vec<ValueId>>,
        val_subst: &mut HashMap<ValueId, ValueId>,
        loads_to_remove: &mut HashSet<ValueId>,
        stores_to_remove: &mut HashSet<(BlockId, usize)>,
        stats: &mut Mem2RegStats,
    ) {
        let mut pushed_counts: HashMap<SlotId, usize> = HashMap::new();

        // 1. Process Phis in curr_block: push phi dest onto reaching definition stack
        for &slot_id in eligible_slots {
            if let Some(phi_map) = slot_phi_defs.get(&slot_id) {
                if let Some(&phi_dest) = phi_map.get(&curr_block) {
                    reaching_defs.get_mut(&slot_id).unwrap().push(phi_dest);
                    *pushed_counts.entry(slot_id).or_default() += 1;
                }
            }
        }

        // 2. Process instructions in curr_block
        let block_idx = func.blocks.iter().position(|b| b.id == curr_block).unwrap();
        for inst_idx in 0..func.blocks[block_idx].instructions.len() {
            let inst = &func.blocks[block_idx].instructions[inst_idx];
            match inst {
                Instruction::Store { slot, val } if eligible_slots.contains(slot) => {
                    let reaching_val = val_subst.get(val).copied().unwrap_or(*val);
                    reaching_defs.get_mut(slot).unwrap().push(reaching_val);
                    *pushed_counts.entry(*slot).or_default() += 1;
                    stores_to_remove.insert((curr_block, inst_idx));
                    stats.eliminated_stores += 1;
                }
                Instruction::Load { dest, slot, .. } if eligible_slots.contains(slot) => {
                    let curr_def = *reaching_defs.get(slot).unwrap().last().unwrap();
                    val_subst.insert(*dest, curr_def);
                    loads_to_remove.insert(*dest);
                    stats.eliminated_loads += 1;
                }
                _ => {}
            }
        }

        // 3. For each CFG successor, populate incoming phi values
        if let Some(succ_list) = successors.get(&curr_block) {
            for &succ in succ_list {
                let succ_idx = func.blocks.iter().position(|b| b.id == succ).unwrap();
                for inst in &mut func.blocks[succ_idx].instructions {
                    if let Instruction::Phi { dest, inputs, .. } = inst {
                        // Find which slot this phi corresponds to
                        for &slot_id in eligible_slots {
                            if let Some(phi_map) = slot_phi_defs.get(&slot_id) {
                                if phi_map.get(&succ) == Some(dest) {
                                    let curr_def =
                                        *reaching_defs.get(&slot_id).unwrap().last().unwrap();
                                    inputs.push((curr_block, curr_def));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. Recurse into dominator tree children
        if let Some(children) = dom_children.get(&curr_block) {
            for &child in children {
                Self::rename_in_dom_tree(
                    child,
                    dom_children,
                    successors,
                    eligible_slots,
                    slot_phi_defs,
                    func,
                    reaching_defs,
                    val_subst,
                    loads_to_remove,
                    stores_to_remove,
                    stats,
                );
            }
        }

        // 5. Pop reaching definitions pushed during this block
        for (slot_id, count) in pushed_counts {
            let stack = reaching_defs.get_mut(&slot_id).unwrap();
            for _ in 0..count {
                stack.pop();
            }
        }
    }

    /// Substitutes ValueIds across all instructions and terminators in the function.
    fn apply_value_substitutions(
        func: &mut MirFunction,
        val_subst: &HashMap<ValueId, ValueId>,
    ) {
        if val_subst.is_empty() {
            return;
        }

        let resolve = |mut v: ValueId| -> ValueId {
            let mut depth = 0;
            while let Some(&next) = val_subst.get(&v) {
                if next == v || depth > 100 {
                    break;
                }
                v = next;
                depth += 1;
            }
            v
        };

        for block in &mut func.blocks {
            for inst in &mut block.instructions {
                match inst {
                    Instruction::Store { val, .. } => {
                        *val = resolve(*val);
                    }
                    Instruction::Binary { lhs, rhs, .. } => {
                        *lhs = resolve(*lhs);
                        *rhs = resolve(*rhs);
                    }
                    Instruction::Unary { operand, .. } => {
                        *operand = resolve(*operand);
                    }
                    Instruction::Call { args, .. } => {
                        for arg in args {
                            *arg = resolve(*arg);
                        }
                    }
                    Instruction::IndexAddr { index, .. } => {
                        *index = resolve(*index);
                    }
                    Instruction::IndexPtrAddr {
                        base_ptr, index, ..
                    } => {
                        *base_ptr = resolve(*base_ptr);
                        *index = resolve(*index);
                    }
                    Instruction::FieldAddr { base_ptr, .. } => {
                        *base_ptr = resolve(*base_ptr);
                    }
                    Instruction::LoadIndirect { addr, .. } => {
                        *addr = resolve(*addr);
                    }
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
                        *addr = resolve(*addr);
                        *val = resolve(*val);
                    }
                    Instruction::StoreSlice { ptr, len, .. } => {
                        *ptr = resolve(*ptr);
                        *len = resolve(*len);
                    }
                    Instruction::Copy { src, .. } => {
                        *src = resolve(*src);
                    }
                    Instruction::Phi { inputs, .. } => {
                        for (_pred, val) in inputs {
                            *val = resolve(*val);
                        }
                    }
                    Instruction::ConstInt { .. }
                    | Instruction::ConstBool { .. }
                    | Instruction::SlotAddr { .. }
                    | Instruction::Load { .. }
                    | Instruction::StrSlice { .. }
                    | Instruction::LoadSlice { .. } => {}
                }
            }

            if let Some(ref mut term) = block.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        *cond = resolve(*cond);
                    }
                    Terminator::Return(Some(val)) => {
                        *val = resolve(*val);
                    }
                    _ => {}
                }
            }
        }
    }
}
