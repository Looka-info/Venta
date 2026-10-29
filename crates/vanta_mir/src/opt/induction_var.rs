//! Induction Variable Optimization (IVO) pass for Vanta MIR.
//!
//! Identifies basic induction variables (BIV) with constant steps within natural loops.
//! Eliminates redundant induction variables with identical initial values and steps,
//! and removes dead induction variables that have no observable uses.

use crate::ir::*;
use crate::opt::dom::DominatorTree;
use crate::opt::licm::NaturalLoop;
use std::collections::{HashMap, HashSet};
use vanta_sema::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicIndVar {
    /// SSA Phi destination in the loop header.
    pub phi_dest: ValueId,
    /// Value entering the loop from the preheader.
    pub init_val: ValueId,
    /// Constant value of the initial value, if known.
    pub init_const: Option<i64>,
    /// The constant step added (or subtracted) each iteration.
    pub step: i64,
    /// The SSA value computed in the loop body that is fed back via the latch.
    pub update_val: ValueId,
    /// The basic block where the update instruction resides.
    pub update_block: BlockId,
    /// Type of the induction variable.
    pub ty: Type,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IvoStats {
    pub loops_analyzed: usize,
    pub induction_vars_found: usize,
    pub redundant_ivs_eliminated: usize,
    pub dead_ivs_eliminated: usize,
    pub candidates_rejected: usize,
}

pub struct InductionVarOpt;

impl InductionVarOpt {
    /// Runs Induction Variable Optimization on a single MIR function.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, IvoStats) {
        let mut overall_changed = false;
        let mut stats = IvoStats::default();

        if func.blocks.is_empty() {
            return (false, stats);
        }

        let dom_tree = match DominatorTree::compute(func) {
            Some(dt) => dt,
            None => return (false, stats),
        };

        let mut loops = NaturalLoop::discover_loops(func, &dom_tree);
        if loops.is_empty() {
            return (false, stats);
        }

        // Process loops
        loops.sort_by_key(|l| l.blocks.len());

        for loop_info in &loops {
            stats.loops_analyzed += 1;

            // Map all constants defined in the function for constant-initial-value matching
            let mut const_map: HashMap<ValueId, i64> = HashMap::new();
            for block in &func.blocks {
                for inst in &block.instructions {
                    if let Instruction::ConstInt { dest, val, .. } = inst {
                        const_map.insert(*dest, *val);
                    }
                }
            }

            let ivs = Self::find_basic_induction_vars(func, loop_info);
            stats.induction_vars_found += ivs.len();

            if ivs.is_empty() {
                continue;
            }

            // 1. Redundant IV Elimination: group by (ty, initial_val_or_const, step)
            let (red_changed, red_count) = Self::eliminate_redundant_ivs(func, &ivs, &const_map);
            if red_changed {
                overall_changed = true;
                stats.redundant_ivs_eliminated += red_count;
            }

            // 2. Dead IV Elimination: remove IVs whose only use is their own update
            let (dead_changed, dead_count) = Self::eliminate_dead_ivs(func, &ivs);
            if dead_changed {
                overall_changed = true;
                stats.dead_ivs_eliminated += dead_count;
            }
        }

        (overall_changed, stats)
    }

    /// Discovers all basic induction variables in a natural loop.
    pub fn find_basic_induction_vars(
        func: &MirFunction,
        loop_info: &NaturalLoop,
    ) -> Vec<BasicIndVar> {
        let mut const_map: HashMap<ValueId, i64> = HashMap::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::ConstInt { dest, val, .. } = inst {
                    const_map.insert(*dest, *val);
                }
            }
        }
        let header = match func.blocks.iter().find(|b| b.id == loop_info.header) {
            Some(b) => b,
            None => return Vec::new(),
        };

        // Find loop preheader: predecessor outside the loop
        let latch_set: HashSet<BlockId> = loop_info.latches.iter().copied().collect();
        let mut ivs = Vec::new();

        for inst in &header.instructions {
            if let Instruction::Phi { dest, inputs, ty } = inst {
                if !matches!(ty, Type::I64 | Type::I32 | Type::U64 | Type::U32) {
                    continue;
                }

                // We expect inputs from outside the loop (init) and from the latch (update)
                let mut ext_inputs = Vec::new();
                let mut loop_inputs = Vec::new();

                for (pred, val) in inputs {
                    if latch_set.contains(pred) || loop_info.blocks.contains(pred) {
                        loop_inputs.push((*pred, *val));
                    } else {
                        ext_inputs.push((*pred, *val));
                    }
                }

                if ext_inputs.len() != 1 || loop_inputs.len() != 1 {
                    continue;
                }

                let (_, init_val) = ext_inputs[0];
                let (_, update_val) = loop_inputs[0];

                // Check if update_val is defined inside the loop as dest + step or dest - step
                if let Some((step, update_block)) =
                    Self::find_step_and_block(func, loop_info, *dest, update_val, &const_map)
                {
                    let init_const = const_map.get(&init_val).copied();
                    ivs.push(BasicIndVar {
                        phi_dest: *dest,
                        init_val,
                        init_const,
                        step,
                        update_val,
                        update_block,
                        ty: ty.clone(),
                    });
                }
            }
        }

        ivs
    }

    fn find_step_and_block(
        func: &MirFunction,
        loop_info: &NaturalLoop,
        phi_dest: ValueId,
        update_val: ValueId,
        const_map: &HashMap<ValueId, i64>,
    ) -> Option<(i64, BlockId)> {
        for &b_id in &loop_info.blocks {
            let block = func.blocks.iter().find(|b| b.id == b_id)?;
            for inst in &block.instructions {
                match inst {
                    Instruction::Binary {
                        dest, op, lhs, rhs, ..
                    } if *dest == update_val => match op {
                        MirBinaryOp::Add => {
                            if *lhs == phi_dest {
                                if let Some(&s) = const_map.get(rhs) {
                                    return Some((s, b_id));
                                }
                            } else if *rhs == phi_dest {
                                if let Some(&s) = const_map.get(lhs) {
                                    return Some((s, b_id));
                                }
                            }
                        }
                        MirBinaryOp::Sub => {
                            if *lhs == phi_dest {
                                if let Some(&s) = const_map.get(rhs) {
                                    return Some((-s, b_id));
                                }
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        None
    }

    /// Identifies and eliminates redundant basic induction variables.
    fn eliminate_redundant_ivs(
        func: &mut MirFunction,
        ivs: &[BasicIndVar],
        _const_map: &HashMap<ValueId, i64>,
    ) -> (bool, usize) {
        if ivs.len() < 2 {
            return (false, 0);
        }

        // Group by (ty, canonical_initial_key, step)
        // For initial key: if init_const is present, use (true, init_const), else (false, init_val.0 as i64)
        let mut groups: HashMap<(Type, (bool, i64), i64), Vec<&BasicIndVar>> = HashMap::new();
        for iv in ivs {
            let key_init = match iv.init_const {
                Some(c) => (true, c),
                None => (false, iv.init_val.0 as i64),
            };
            groups
                .entry((iv.ty.clone(), key_init, iv.step))
                .or_default()
                .push(iv);
        }

        let mut subst_map: HashMap<ValueId, ValueId> = HashMap::new();
        let mut redundant_count = 0;

        for group in groups.values() {
            if group.len() > 1 {
                let canonical = group[0];
                for &redundant in &group[1..] {
                    subst_map.insert(redundant.phi_dest, canonical.phi_dest);
                    subst_map.insert(redundant.update_val, canonical.update_val);
                    redundant_count += 1;
                }
            }
        }

        if subst_map.is_empty() {
            return (false, 0);
        }

        // Substitute redundant IVs throughout the function
        Self::apply_substitutions(func, &subst_map);

        // Remove redundant Phi nodes from header block
        for block in &mut func.blocks {
            block.instructions.retain(|inst| {
                if let Instruction::Phi { dest, .. } = inst {
                    !subst_map.contains_key(dest)
                } else {
                    true
                }
            });
        }

        (true, redundant_count)
    }

    /// Identifies and eliminates dead basic induction variables whose only use is their own update.
    fn eliminate_dead_ivs(func: &mut MirFunction, ivs: &[BasicIndVar]) -> (bool, usize) {
        let mut use_counts: HashMap<ValueId, usize> = HashMap::new();

        for block in &func.blocks {
            for inst in &block.instructions {
                for op in Self::extract_operands(inst) {
                    *use_counts.entry(op).or_insert(0) += 1;
                }
            }
            if let Some(ref term) = block.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        *use_counts.entry(*cond).or_insert(0) += 1;
                    }
                    Terminator::Return(Some(val)) => {
                        *use_counts.entry(*val).or_insert(0) += 1;
                    }
                    _ => {}
                }
            }
        }

        let mut dead_phi_dests = HashSet::new();
        let mut dead_update_vals = HashSet::new();

        for iv in ivs {
            let phi_uses = use_counts.get(&iv.phi_dest).copied().unwrap_or(0);
            let update_uses = use_counts.get(&iv.update_val).copied().unwrap_or(0);

            // phi_dest should only be used in update instruction (1 use)
            // update_val should only be used in phi node (1 use)
            if phi_uses <= 1 && update_uses <= 1 {
                dead_phi_dests.insert(iv.phi_dest);
                dead_update_vals.insert(iv.update_val);
            }
        }

        if dead_phi_dests.is_empty() {
            return (false, 0);
        }

        let count = dead_phi_dests.len();

        for block in &mut func.blocks {
            block.instructions.retain(|inst| match inst {
                Instruction::Phi { dest, .. } => !dead_phi_dests.contains(dest),
                Instruction::Binary { dest, .. } => !dead_update_vals.contains(dest),
                _ => true,
            });
        }

        (true, count)
    }

    fn extract_operands(inst: &Instruction) -> Vec<ValueId> {
        match inst {
            Instruction::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
            Instruction::Unary { operand, .. } => vec![*operand],
            Instruction::Store { val, .. } => vec![*val],
            Instruction::StoreIndirect { addr, val }
            | Instruction::StoreIndirect32 { addr, val } => vec![*addr, *val],
            Instruction::LoadIndirect { addr, .. } => vec![*addr],
            Instruction::IndexAddr { index, .. } => vec![*index],
            Instruction::IndexPtrAddr {
                base_ptr, index, ..
            } => vec![*base_ptr, *index],
            Instruction::FieldAddr { base_ptr, .. } => vec![*base_ptr],
            Instruction::Call { args, .. } => args.clone(),
            Instruction::Copy { src, .. } => vec![*src],
            Instruction::Phi { inputs, .. } => inputs.iter().map(|(_, v)| *v).collect(),
            Instruction::StoreSlice { ptr, len, .. } => vec![*ptr, *len],
            _ => Vec::new(),
        }
    }

    fn apply_substitutions(func: &mut MirFunction, subst: &HashMap<ValueId, ValueId>) {
        for block in &mut func.blocks {
            for inst in &mut block.instructions {
                match inst {
                    Instruction::Binary { lhs, rhs, .. } => {
                        if let Some(&new_lhs) = subst.get(lhs) {
                            *lhs = new_lhs;
                        }
                        if let Some(&new_rhs) = subst.get(rhs) {
                            *rhs = new_rhs;
                        }
                    }
                    Instruction::Unary { operand, .. } => {
                        if let Some(&new_op) = subst.get(operand) {
                            *operand = new_op;
                        }
                    }
                    Instruction::Store { val, .. } => {
                        if let Some(&new_val) = subst.get(val) {
                            *val = new_val;
                        }
                    }
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
                        if let Some(&new_addr) = subst.get(addr) {
                            *addr = new_addr;
                        }
                        if let Some(&new_val) = subst.get(val) {
                            *val = new_val;
                        }
                    }
                    Instruction::LoadIndirect { addr, .. } => {
                        if let Some(&new_addr) = subst.get(addr) {
                            *addr = new_addr;
                        }
                    }
                    Instruction::IndexAddr { index, .. } => {
                        if let Some(&new_idx) = subst.get(index) {
                            *index = new_idx;
                        }
                    }
                    Instruction::IndexPtrAddr {
                        base_ptr, index, ..
                    } => {
                        if let Some(&new_base) = subst.get(base_ptr) {
                            *base_ptr = new_base;
                        }
                        if let Some(&new_idx) = subst.get(index) {
                            *index = new_idx;
                        }
                    }
                    Instruction::FieldAddr { base_ptr, .. } => {
                        if let Some(&new_base) = subst.get(base_ptr) {
                            *base_ptr = new_base;
                        }
                    }
                    Instruction::Call { args, .. } => {
                        for a in args {
                            if let Some(&new_a) = subst.get(a) {
                                *a = new_a;
                            }
                        }
                    }
                    Instruction::Copy { src, .. } => {
                        if let Some(&new_src) = subst.get(src) {
                            *src = new_src;
                        }
                    }
                    Instruction::Phi { inputs, .. } => {
                        for (_, val) in inputs {
                            if let Some(&new_val) = subst.get(val) {
                                *val = new_val;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ref mut term) = block.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        if let Some(&new_cond) = subst.get(cond) {
                            *cond = new_cond;
                        }
                    }
                    Terminator::Return(Some(val)) => {
                        if let Some(&new_val) = subst.get(val) {
                            *val = new_val;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
