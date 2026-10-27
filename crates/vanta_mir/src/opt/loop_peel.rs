//! Loop Peeling pass for Vanta MIR.
//!
//! Separates the initial iteration (k=1) of eligible natural loops outside the loop body.
//! This exposes constant propagation and strength reduction opportunities on the first iteration
//! and simplifies the steady-state loop condition.

use crate::ir::*;
use crate::opt::dom::DominatorTree;
use crate::opt::licm::NaturalLoop;
use std::collections::{HashMap, HashSet};
use vanta_sema::Type;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoopPeelStats {
    pub loops_analyzed: usize,
    pub loops_peeled: usize,
    pub candidates_rejected: usize,
}

pub struct LoopPeeling;

impl LoopPeeling {
    /// Maximum body instruction count eligible for peeling (to prevent code bloat).
    const MAX_PEEL_BODY_INSTRUCTIONS: usize = 40;

    /// Runs Loop Peeling on a single MIR function.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, LoopPeelStats) {
        let mut overall_changed = false;
        let mut stats = LoopPeelStats::default();

        if func.blocks.is_empty() {
            return (false, stats);
        }

        let dom_tree = match DominatorTree::compute(func) {
            Some(dt) => dt,
            None => return (false, stats),
        };

        let loops = NaturalLoop::discover_loops(func, &dom_tree);
        if loops.is_empty() {
            return (false, stats);
        }

        let mut peeled_headers = HashSet::new();

        for loop_info in &loops {
            stats.loops_analyzed += 1;

            if peeled_headers.contains(&loop_info.header) {
                stats.candidates_rejected += 1;
                continue;
            }

            // 1. Must have a single latch and a single exit
            if loop_info.latches.len() != 1 || loop_info.exits.len() != 1 {
                stats.candidates_rejected += 1;
                continue;
            }
            let latch_id = loop_info.latches[0];

            // 2. Loop body and purity check: reject calls, returns, unreachables
            let mut total_insts = 0;
            let mut has_forbidden = false;
            for &b_id in &loop_info.blocks {
                if let Some(b) = func.blocks.iter().find(|block| block.id == b_id) {
                    total_insts += b.instructions.len();
                    for inst in &b.instructions {
                        if matches!(inst, Instruction::Call { .. }) {
                            has_forbidden = true;
                            break;
                        }
                    }
                    if matches!(b.terminator, Some(Terminator::Return(_)) | Some(Terminator::Unreachable)) {
                        has_forbidden = true;
                    }
                }
            }
            if has_forbidden || total_insts > Self::MAX_PEEL_BODY_INSTRUCTIONS {
                stats.candidates_rejected += 1;
                continue;
            }

            // 3. Inspect header terminator: must be CondBranch { cond, then_block, else_block }
            let (cond_val, body_entry_id, exit_block_id) = {
                let header_block = match func.blocks.iter().find(|b| b.id == loop_info.header) {
                    Some(b) => b,
                    None => {
                        stats.candidates_rejected += 1;
                        continue;
                    }
                };

                match &header_block.terminator {
                    Some(Terminator::CondBranch { cond, then_block, else_block }) => {
                        if loop_info.blocks.contains(then_block) && !loop_info.blocks.contains(else_block) && *else_block == loop_info.exits[0].1 {
                            (*cond, *then_block, *else_block)
                        } else {
                            stats.candidates_rejected += 1;
                            continue;
                        }
                    }
                    _ => {
                        stats.candidates_rejected += 1;
                        continue;
                    }
                }
            };

            // 4. Header non-phi instructions must all be pure side-effect-free instructions
            let header_idx = func.blocks.iter().position(|b| b.id == loop_info.header).unwrap();
            let mut header_pure = true;
            for inst in &func.blocks[header_idx].instructions {
                match inst {
                    Instruction::Phi { .. }
                    | Instruction::ConstInt { .. }
                    | Instruction::ConstBool { .. }
                    | Instruction::SlotAddr { .. }
                    | Instruction::Load { .. }
                    | Instruction::Binary { .. }
                    | Instruction::Unary { .. }
                    | Instruction::Copy { .. } => {}
                    _ => {
                        header_pure = false;
                        break;
                    }
                }
            }
            if !header_pure {
                stats.candidates_rejected += 1;
                continue;
            }

            // 5. Obtain or create a dedicated preheader
            let (preheader_id, _) = NaturalLoop::get_or_create_preheader(func, loop_info, &dom_tree);

            let header_idx = func.blocks.iter().position(|b| b.id == loop_info.header).unwrap();
            let mut phi_nodes: Vec<(ValueId, ValueId, ValueId, Type)> = Vec::new();
            let mut phi_init_map: HashMap<ValueId, ValueId> = HashMap::new();
            let mut phi_latch_map: HashMap<ValueId, ValueId> = HashMap::new();
            let mut phi_incomplete = false;

            for inst in &func.blocks[header_idx].instructions {
                if let Instruction::Phi { dest, inputs, ty } = inst {
                    let mut init_v = None;
                    let mut latch_v = None;
                    for (pred, val) in inputs {
                        if *pred == preheader_id {
                            init_v = Some(*val);
                        } else if *pred == latch_id {
                            latch_v = Some(*val);
                        }
                    }
                    if let (Some(iv), Some(lv)) = (init_v, latch_v) {
                        phi_nodes.push((*dest, iv, lv, ty.clone()));
                        phi_init_map.insert(*dest, iv);
                        phi_latch_map.insert(*dest, lv);
                    } else {
                        phi_incomplete = true;
                        break;
                    }
                }
            }
            if phi_incomplete {
                stats.candidates_rejected += 1;
                continue;
            }

            // 6. Body blocks to clone for iteration 0
            let body_block_ids: Vec<BlockId> = func.blocks.iter()
                .map(|b| b.id)
                .filter(|b_id| *b_id != loop_info.header && loop_info.blocks.contains(b_id))
                .collect();

            if body_block_ids.is_empty() {
                stats.candidates_rejected += 1;
                continue;
            }

            let mut next_val_id = func.max_value_id() + 1;
            let mut next_block_id = func.max_block_id() + 1;

            // 7. Clone header non-phi instructions into preheader to evaluate iteration 0 condition
            let mut preheader_val_map: HashMap<ValueId, ValueId> = HashMap::new();
            for (&phi_dest, &init_val) in &phi_init_map {
                preheader_val_map.insert(phi_dest, init_val);
            }

            let mut preheader_cloned_insts = Vec::new();
            for inst in &func.blocks[header_idx].instructions {
                if matches!(inst, Instruction::Phi { .. }) {
                    continue;
                }
                let mut cl = inst.clone();
                match &mut cl {
                    Instruction::ConstInt { dest, .. }
                    | Instruction::ConstBool { dest, .. }
                    | Instruction::SlotAddr { dest, .. }
                    | Instruction::Load { dest, .. }
                    | Instruction::Binary { dest, .. }
                    | Instruction::Unary { dest, .. }
                    | Instruction::Copy { dest, .. } => {
                        let new_d = ValueId(next_val_id);
                        next_val_id += 1;
                        preheader_val_map.insert(*dest, new_d);
                        *dest = new_d;
                    }
                    _ => {}
                }
                Self::remap_instruction(&mut cl, &preheader_val_map, &HashMap::new());
                preheader_cloned_insts.push(cl);
            }

            let init_cond_val = preheader_val_map.get(&cond_val).copied().unwrap_or(cond_val);

            // 8. Block and value mapping for peeled body
            let mut block_map: HashMap<BlockId, BlockId> = HashMap::new();
            for &b in &body_block_ids {
                block_map.insert(b, BlockId(next_block_id));
                next_block_id += 1;
            }

            let mut peeled_val_map: HashMap<ValueId, ValueId> = HashMap::new();
            for (&phi_dest, &init_val) in &phi_init_map {
                peeled_val_map.insert(phi_dest, init_val);
            }

            // Assign fresh ValueIds to all defs in peeled body
            for &b_id in &body_block_ids {
                let orig_b = func.blocks.iter().find(|b| b.id == b_id).unwrap();
                for inst in &orig_b.instructions {
                    match inst {
                        Instruction::ConstInt { dest, .. }
                        | Instruction::ConstBool { dest, .. }
                        | Instruction::SlotAddr { dest, .. }
                        | Instruction::Load { dest, .. }
                        | Instruction::Binary { dest, .. }
                        | Instruction::Unary { dest, .. }
                        | Instruction::IndexAddr { dest, .. }
                        | Instruction::IndexPtrAddr { dest, .. }
                        | Instruction::FieldAddr { dest, .. }
                        | Instruction::LoadIndirect { dest, .. }
                        | Instruction::Phi { dest, .. }
                        | Instruction::Copy { dest, .. } => {
                            peeled_val_map.insert(*dest, ValueId(next_val_id));
                            next_val_id += 1;
                        }
                        Instruction::StrSlice { dest_ptr, dest_len, .. }
                        | Instruction::LoadSlice { dest_ptr, dest_len, .. } => {
                            peeled_val_map.insert(*dest_ptr, ValueId(next_val_id));
                            next_val_id += 1;
                            peeled_val_map.insert(*dest_len, ValueId(next_val_id));
                            next_val_id += 1;
                        }
                        _ => {}
                    }
                }
            }

            // Clone body blocks
            let mut cloned_blocks: Vec<BasicBlock> = Vec::new();
            let peeled_latch_id = block_map[&latch_id];

            for &b_id in &body_block_ids {
                let orig_b = func.blocks.iter().find(|b| b.id == b_id).unwrap();
                let new_b_id = block_map[&b_id];

                let mut cloned_insts = Vec::with_capacity(orig_b.instructions.len());
                for inst in &orig_b.instructions {
                    let mut cl = inst.clone();
                    Self::remap_instruction(&mut cl, &peeled_val_map, &block_map);
                    cloned_insts.push(cl);
                }

                // Cloned terminator: if branching back to header, redirect to loop.header!
                let cloned_term = match &orig_b.terminator {
                    Some(Terminator::Branch(target)) => {
                        if *target == loop_info.header {
                            Some(Terminator::Branch(loop_info.header))
                        } else if let Some(&new_t) = block_map.get(target) {
                            Some(Terminator::Branch(new_t))
                        } else {
                            Some(Terminator::Branch(*target))
                        }
                    }
                    Some(Terminator::CondBranch { cond, then_block, else_block }) => {
                        let new_cond = peeled_val_map.get(cond).copied().unwrap_or(*cond);
                        let new_then = block_map.get(then_block).copied().unwrap_or(*then_block);
                        let new_else = block_map.get(else_block).copied().unwrap_or(*else_block);
                        Some(Terminator::CondBranch {
                            cond: new_cond,
                            then_block: new_then,
                            else_block: new_else,
                        })
                    }
                    other => other.clone(),
                };

                cloned_blocks.push(BasicBlock {
                    id: new_b_id,
                    name: format!("{}_peeled", orig_b.name),
                    instructions: cloned_insts,
                    terminator: cloned_term,
                });
            }

            // 9. Update Header Phis to take iteration 1 initial values from peeled latch
            let header_block = func.blocks.iter_mut().find(|b| b.id == loop_info.header).unwrap();
            for inst in &mut header_block.instructions {
                if let Instruction::Phi { dest, inputs, .. } = inst {
                    if let Some(&latch_v) = phi_latch_map.get(dest) {
                        let iter1_val = peeled_val_map.get(&latch_v).copied().unwrap_or(latch_v);
                        for (pred, val) in inputs {
                            if *pred == preheader_id {
                                *pred = peeled_latch_id;
                                *val = iter1_val;
                            }
                        }
                    }
                }
            }

            // 10. Split critical exit edges using dedicated trampolines:
            // This ensures both the zero-trip path and the loop-exit path enter exit_block_id
            // via unconditional Branch, allowing correct SSA Phi transfer codegen!
            let zero_trip_id = BlockId(next_block_id);
            let loop_exit_id = BlockId(next_block_id + 1);

            let peeled_entry_id = block_map[&body_entry_id];
            let preheader_idx = func.blocks.iter().position(|b| b.id == preheader_id).unwrap();
            func.blocks[preheader_idx].instructions.extend(preheader_cloned_insts);
            func.blocks[preheader_idx].terminator = Some(Terminator::CondBranch {
                cond: init_cond_val,
                then_block: peeled_entry_id,
                else_block: zero_trip_id,
            });

            let header_idx = func.blocks.iter().position(|b| b.id == loop_info.header).unwrap();
            func.blocks[header_idx].terminator = Some(Terminator::CondBranch {
                cond: cond_val,
                then_block: body_entry_id,
                else_block: loop_exit_id,
            });

            let zero_trip_block = BasicBlock {
                id: zero_trip_id,
                name: format!("{}_zero_trip", func.blocks[preheader_idx].name),
                instructions: Vec::new(),
                terminator: Some(Terminator::Branch(exit_block_id)),
            };

            let loop_exit_block = BasicBlock {
                id: loop_exit_id,
                name: format!("{}_exit_trampoline", func.blocks[header_idx].name),
                instructions: Vec::new(),
                terminator: Some(Terminator::Branch(exit_block_id)),
            };

            // 11. Insert Phis in exit_block for loop-carried values
            let mut exit_phis_to_insert = Vec::new();
            let mut exit_val_replacement: HashMap<ValueId, ValueId> = HashMap::new();

            for (phi_dest, init_val, _, ty) in &phi_nodes {
                let exit_phi_id = ValueId(next_val_id);
                next_val_id += 1;
                exit_phis_to_insert.push(Instruction::Phi {
                    dest: exit_phi_id,
                    inputs: vec![(zero_trip_id, *init_val), (loop_exit_id, *phi_dest)],
                    ty: ty.clone(),
                });
                exit_val_replacement.insert(*phi_dest, exit_phi_id);
            }

            let exit_idx = func.blocks.iter().position(|b| b.id == exit_block_id).unwrap();
            for (idx, phi_inst) in exit_phis_to_insert.into_iter().enumerate() {
                func.blocks[exit_idx].instructions.insert(idx, phi_inst);
            }

            // Replace uses of phi_dest outside the loop
            let empty_block_map = HashMap::new();
            for b in &mut func.blocks {
                if loop_info.blocks.contains(&b.id) {
                    continue;
                }
                let is_exit = b.id == exit_block_id;
                for inst in &mut b.instructions {
                    if is_exit && matches!(inst, Instruction::Phi { .. }) {
                        continue;
                    }
                    Self::remap_instruction(inst, &exit_val_replacement, &empty_block_map);
                }
                if let Some(ref mut term) = b.terminator {
                    match term {
                        Terminator::CondBranch { cond, .. } => {
                            if let Some(&new_c) = exit_val_replacement.get(cond) {
                                *cond = new_c;
                            }
                        }
                        Terminator::Return(Some(val)) => {
                            if let Some(&new_v) = exit_val_replacement.get(val) {
                                *val = new_v;
                            }
                        }
                        _ => {}
                    }
                }
            }

            // 12. Insert peeled blocks right after preheader
            let p_idx = func.blocks.iter().position(|b| b.id == preheader_id).unwrap();
            for (offset, cb) in cloned_blocks.into_iter().enumerate() {
                func.blocks.insert(p_idx + 1 + offset, cb);
            }

            // 13. Insert trampolines right before exit_block
            let e_idx = func.blocks.iter().position(|b| b.id == exit_block_id).unwrap();
            func.blocks.insert(e_idx, zero_trip_block);
            func.blocks.insert(e_idx + 1, loop_exit_block);

            peeled_headers.insert(loop_info.header);
            overall_changed = true;
            stats.loops_peeled += 1;
            break; // Peel one loop per pass iteration for stability
        }

        (overall_changed, stats)
    }

    /// Remaps all ValueId and BlockId references in an instruction.
    fn remap_instruction(
        inst: &mut Instruction,
        val_map: &HashMap<ValueId, ValueId>,
        block_map: &HashMap<BlockId, BlockId>,
    ) {
        let map_val = |v: &mut ValueId| {
            if let Some(&new_v) = val_map.get(v) {
                *v = new_v;
            }
        };

        match inst {
            Instruction::ConstInt { dest, .. } | Instruction::ConstBool { dest, .. } => {
                map_val(dest);
            }
            Instruction::SlotAddr { dest, .. } => {
                map_val(dest);
            }
            Instruction::Load { dest, .. } => {
                map_val(dest);
            }
            Instruction::Store { val, .. } => {
                map_val(val);
            }
            Instruction::Binary { dest, lhs, rhs, .. } => {
                map_val(dest);
                map_val(lhs);
                map_val(rhs);
            }
            Instruction::Unary { dest, operand, .. } => {
                map_val(dest);
                map_val(operand);
            }
            Instruction::Call { dest, args, .. } => {
                if let Some(d) = dest {
                    map_val(d);
                }
                for arg in args {
                    map_val(arg);
                }
            }
            Instruction::IndexAddr { dest, index, .. } => {
                map_val(dest);
                map_val(index);
            }
            Instruction::IndexPtrAddr { dest, base_ptr, index, .. } => {
                map_val(dest);
                map_val(base_ptr);
                map_val(index);
            }
            Instruction::FieldAddr { dest, base_ptr, .. } => {
                map_val(dest);
                map_val(base_ptr);
            }
            Instruction::LoadIndirect { dest, addr, .. } => {
                map_val(dest);
                map_val(addr);
            }
            Instruction::StoreIndirect { addr, val }
            | Instruction::StoreIndirect32 { addr, val } => {
                map_val(addr);
                map_val(val);
            }
            Instruction::StrSlice { dest_ptr, dest_len, .. } => {
                map_val(dest_ptr);
                map_val(dest_len);
            }
            Instruction::StoreSlice { ptr, len, .. } => {
                map_val(ptr);
                map_val(len);
            }
            Instruction::LoadSlice { dest_ptr, dest_len, .. } => {
                map_val(dest_ptr);
                map_val(dest_len);
            }
            Instruction::Copy { dest, src, .. } => {
                map_val(dest);
                map_val(src);
            }
            Instruction::Phi { dest, inputs, .. } => {
                map_val(dest);
                for (pred, val) in inputs {
                    if let Some(&new_b) = block_map.get(pred) {
                        *pred = new_b;
                    }
                    map_val(val);
                }
            }
        }
    }
}
