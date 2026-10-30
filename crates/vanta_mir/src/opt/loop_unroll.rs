//! Loop Unrolling pass for Vanta MIR.
//!
//! Replicates loop-body operations to reduce loop-control overhead and expose additional
//! optimization opportunities. Supports full unrolling for small constant trip counts,
//! and bounded partial unrolling (factor 2) for exact-multiple loops.

use crate::ir::*;
use crate::opt::dom::DominatorTree;
use crate::opt::licm::NaturalLoop;
use std::collections::HashMap;
use vanta_sema::Type;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoopUnrollStats {
    pub loops_analyzed: usize,
    pub full_unrolls: usize,
    pub partial_unrolls: usize,
    pub loops_rejected: usize,
}

pub struct LoopUnrolling;

impl LoopUnrolling {
    const MAX_FULL_UNROLL_ITERS: i64 = 16;
    const MAX_FULL_UNROLL_INSTRUCTIONS: usize = 256;
    const MAX_PARTIAL_UNROLL_INSTRUCTIONS: usize = 120;

    /// Runs Loop Unrolling on a single MIR function.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, LoopUnrollStats) {
        let mut overall_changed = false;
        let mut stats = LoopUnrollStats::default();

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

        for loop_info in &loops {
            stats.loops_analyzed += 1;

            // 1. Must be single-latch
            if loop_info.latches.len() != 1 {
                stats.loops_rejected += 1;
                continue;
            }
            let latch_id = loop_info.latches[0];

            // 2. Reject loops with function calls (to avoid code bloat and spill explosion)
            let mut has_call = false;
            let mut body_inst_count = 0;
            for &b_id in &loop_info.blocks {
                if let Some(b) = func.blocks.iter().find(|block| block.id == b_id) {
                    body_inst_count += b.instructions.len();
                    for inst in &b.instructions {
                        if matches!(inst, Instruction::Call { .. }) {
                            has_call = true;
                            break;
                        }
                    }
                }
            }
            if has_call {
                stats.loops_rejected += 1;
                continue;
            }

            // 3. Inspect header terminator: CondBranch { cond, then_block, else_block }
            let (cond_val, body_entry_id, exit_block_id) = {
                let header_block = match func.blocks.iter().find(|b| b.id == loop_info.header) {
                    Some(b) => b,
                    None => {
                        stats.loops_rejected += 1;
                        continue;
                    }
                };

                match &header_block.terminator {
                    Some(Terminator::CondBranch {
                        cond,
                        then_block,
                        else_block,
                    }) => {
                        if loop_info.blocks.contains(then_block)
                            && !loop_info.blocks.contains(else_block)
                        {
                            (*cond, *then_block, *else_block)
                        } else {
                            stats.loops_rejected += 1;
                            continue;
                        }
                    }
                    _ => {
                        stats.loops_rejected += 1;
                        continue;
                    }
                }
            };

            // 4. Obtain preheader
            let (preheader_id, _) =
                NaturalLoop::get_or_create_preheader(func, loop_info, &dom_tree);

            // 5. Build constant map for the function
            let mut const_map: HashMap<ValueId, i64> = HashMap::new();
            for b in &func.blocks {
                for inst in &b.instructions {
                    if let Instruction::ConstInt { dest, val, .. } = inst {
                        const_map.insert(*dest, *val);
                    }
                }
            }

            // 6. Find basic induction variable and loop bound
            let header_idx = func
                .blocks
                .iter()
                .position(|b| b.id == loop_info.header)
                .unwrap();
            let mut phi_nodes = Vec::new();

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
                    }
                }
            }

            // Find the phi that is the induction variable tested in cond_val
            let mut found_trip_count = None;
            for (dest, init_val, latch_val, _) in &phi_nodes {
                if let Some(step) =
                    Self::find_latch_step(func, latch_id, *dest, *latch_val, &const_map)
                {
                    let init_c = const_map.get(init_val).copied();
                    if let Some(trip_count) = Self::compute_trip_count(
                        func,
                        loop_info.header,
                        cond_val,
                        *dest,
                        init_c,
                        Some(step),
                        &const_map,
                    ) {
                        found_trip_count = Some(trip_count);
                        break;
                    }
                }
            }

            if let Some(trip_count) = found_trip_count {
                if trip_count >= 0
                    && trip_count <= Self::MAX_FULL_UNROLL_ITERS
                    && (body_inst_count * trip_count as usize) <= Self::MAX_FULL_UNROLL_INSTRUCTIONS
                {
                    // Case A: FULL UNROLLING (trip_count 0..=16)
                    let changed = Self::apply_full_unroll(
                        func,
                        loop_info,
                        preheader_id,
                        body_entry_id,
                        exit_block_id,
                        trip_count as usize,
                        &phi_nodes,
                    );
                    if changed {
                        overall_changed = true;
                        stats.full_unrolls += 1;
                        break;
                    }
                } else if trip_count > Self::MAX_FULL_UNROLL_ITERS
                    && trip_count % 2 == 0
                    && body_inst_count <= Self::MAX_PARTIAL_UNROLL_INSTRUCTIONS
                {
                    // Case B: PARTIAL UNROLLING (Factor 2)
                    let changed = Self::apply_partial_unroll_2(
                        func,
                        loop_info,
                        body_entry_id,
                        latch_id,
                        &phi_nodes,
                    );
                    if changed {
                        overall_changed = true;
                        stats.partial_unrolls += 1;
                        break;
                    }
                }
            }

            stats.loops_rejected += 1;
        }

        (overall_changed, stats)
    }

    /// Finds the step added to the induction variable at the latch.
    fn find_latch_step(
        func: &MirFunction,
        latch_id: BlockId,
        phi_dest: ValueId,
        latch_val: ValueId,
        const_map: &HashMap<ValueId, i64>,
    ) -> Option<i64> {
        let latch_block = func.blocks.iter().find(|b| b.id == latch_id)?;
        for inst in &latch_block.instructions {
            if let Instruction::Binary {
                dest,
                op: MirBinaryOp::Add,
                lhs,
                rhs,
                ..
            } = inst
            {
                if *dest == latch_val {
                    if *lhs == phi_dest {
                        if let Some(&s) = const_map.get(rhs) {
                            return Some(s);
                        }
                    } else if *rhs == phi_dest {
                        if let Some(&s) = const_map.get(lhs) {
                            return Some(s);
                        }
                    }
                }
            }
        }
        None
    }

    /// Computes the exact constant trip count if provable.
    fn compute_trip_count(
        func: &MirFunction,
        header_id: BlockId,
        cond_val: ValueId,
        phi_dest: ValueId,
        init_c: Option<i64>,
        step_opt: Option<i64>,
        const_map: &HashMap<ValueId, i64>,
    ) -> Option<i64> {
        let init = init_c?;
        let step = step_opt?;
        if step <= 0 {
            return None;
        }

        let header_block = func.blocks.iter().find(|b| b.id == header_id)?;
        for inst in &header_block.instructions {
            if let Instruction::Binary {
                dest, op, lhs, rhs, ..
            } = inst
            {
                if *dest == cond_val {
                    let (is_lt, is_lte, limit_val) = match op {
                        MirBinaryOp::Lt if *lhs == phi_dest => (true, false, *rhs),
                        MirBinaryOp::LtEq if *lhs == phi_dest => (false, true, *rhs),
                        _ => return None,
                    };

                    let limit = *const_map.get(&limit_val)?;
                    if limit < init {
                        return Some(0);
                    }

                    let diff = limit - init;
                    let iters = if is_lt {
                        (diff + step - 1) / step
                    } else if is_lte {
                        diff / step + 1
                    } else {
                        return None;
                    };

                    return Some(iters);
                }
            }
        }

        None
    }

    /// Fully unrolls a loop with constant trip count N.
    fn apply_full_unroll(
        func: &mut MirFunction,
        loop_info: &NaturalLoop,
        preheader_id: BlockId,
        body_entry_id: BlockId,
        exit_block_id: BlockId,
        trip_count: usize,
        phi_nodes: &[(ValueId, ValueId, ValueId, Type)],
    ) -> bool {
        let body_block_ids: Vec<BlockId> = func
            .blocks
            .iter()
            .map(|b| b.id)
            .filter(|b_id| *b_id != loop_info.header && loop_info.blocks.contains(b_id))
            .collect();

        if body_block_ids.is_empty() {
            return false;
        }

        // Identify side-exit blocks (e.g. bounds check fail blocks) reached from within the loop body
        let mut side_exit_blocks: Vec<BlockId> = Vec::new();
        for &b_id in &body_block_ids {
            if let Some(b) = func.blocks.iter().find(|blk| blk.id == b_id) {
                if let Some(term) = &b.terminator {
                    match term {
                        Terminator::Branch(t) => {
                            if !loop_info.blocks.contains(t)
                                && *t != exit_block_id
                                && !side_exit_blocks.contains(t)
                            {
                                side_exit_blocks.push(*t);
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            if !loop_info.blocks.contains(then_block)
                                && *then_block != exit_block_id
                                && !side_exit_blocks.contains(then_block)
                            {
                                side_exit_blocks.push(*then_block);
                            }
                            if !loop_info.blocks.contains(else_block)
                                && *else_block != exit_block_id
                                && !side_exit_blocks.contains(else_block)
                            {
                                side_exit_blocks.push(*else_block);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // All blocks to duplicate for each iteration (preserving topological order of func.blocks)
        let unroll_block_ids: Vec<BlockId> = func
            .blocks
            .iter()
            .map(|b| b.id)
            .filter(|b_id| body_block_ids.contains(b_id) || side_exit_blocks.contains(b_id))
            .collect();

        let mut next_val_id = func.max_value_id() + 1;
        let mut next_block_id = func.max_block_id() + 1;

        // Current values for each phi node
        let mut curr_phi_vals: HashMap<ValueId, ValueId> = HashMap::new();
        for (phi_dest, init_val, _, _) in phi_nodes {
            curr_phi_vals.insert(*phi_dest, *init_val);
        }

        let mut unrolled_blocks: Vec<BasicBlock> = Vec::new();
        let mut last_block_id = preheader_id;

        if trip_count == 0 {
            let p_block = func
                .blocks
                .iter_mut()
                .find(|b| b.id == preheader_id)
                .unwrap();
            p_block.terminator = Some(Terminator::Branch(exit_block_id));
        } else {
            for iter_idx in 0..trip_count {
                let mut val_map: HashMap<ValueId, ValueId> = HashMap::new();
                for (phi_dest, _, _, _) in phi_nodes {
                    val_map.insert(*phi_dest, curr_phi_vals[phi_dest]);
                }

                let mut iter_block_map: HashMap<BlockId, BlockId> = HashMap::new();
                for &b in &unroll_block_ids {
                    iter_block_map.insert(b, BlockId(next_block_id));
                    next_block_id += 1;
                }

                // Assign fresh ValueIds
                for &b_id in &unroll_block_ids {
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
                                val_map.insert(*dest, ValueId(next_val_id));
                                next_val_id += 1;
                            }
                            Instruction::Call { dest: Some(d), .. } => {
                                val_map.insert(*d, ValueId(next_val_id));
                                next_val_id += 1;
                            }
                            Instruction::StrSlice {
                                dest_ptr, dest_len, ..
                            }
                            | Instruction::LoadSlice {
                                dest_ptr, dest_len, ..
                            } => {
                                val_map.insert(*dest_ptr, ValueId(next_val_id));
                                next_val_id += 1;
                                val_map.insert(*dest_len, ValueId(next_val_id));
                                next_val_id += 1;
                            }
                            _ => {}
                        }
                    }
                }

                // Clone instructions
                let is_last_iter = iter_idx == trip_count - 1;
                let iter_entry_id = iter_block_map[&body_entry_id];

                // Connect previous iteration's end to this iteration's start
                if iter_idx == 0 {
                    let p_block = func
                        .blocks
                        .iter_mut()
                        .find(|b| b.id == preheader_id)
                        .unwrap();
                    p_block.terminator = Some(Terminator::Branch(iter_entry_id));
                } else {
                    let prev_block = unrolled_blocks
                        .iter_mut()
                        .find(|b| b.id == last_block_id)
                        .unwrap();
                    prev_block.terminator = Some(Terminator::Branch(iter_entry_id));
                }

                for &b_id in &unroll_block_ids {
                    let orig_b = func.blocks.iter().find(|b| b.id == b_id).unwrap();
                    let new_b_id = iter_block_map[&b_id];

                    let mut cloned_insts = Vec::with_capacity(orig_b.instructions.len());
                    for inst in &orig_b.instructions {
                        let mut cl = inst.clone();
                        Self::remap_instruction(&mut cl, &val_map, &iter_block_map);
                        cloned_insts.push(cl);
                    }

                    let is_latch = b_id == loop_info.latches[0];
                    let cloned_term = if is_latch {
                        last_block_id = new_b_id;
                        if is_last_iter {
                            Some(Terminator::Branch(exit_block_id))
                        } else {
                            None // Will be patched by next iteration
                        }
                    } else {
                        match &orig_b.terminator {
                            Some(Terminator::Branch(t)) => {
                                let new_t = iter_block_map.get(t).copied().unwrap_or(*t);
                                Some(Terminator::Branch(new_t))
                            }
                            Some(Terminator::CondBranch {
                                cond,
                                then_block,
                                else_block,
                            }) => {
                                let new_c = val_map.get(cond).copied().unwrap_or(*cond);
                                let new_then = iter_block_map
                                    .get(then_block)
                                    .copied()
                                    .unwrap_or(*then_block);
                                let new_else = iter_block_map
                                    .get(else_block)
                                    .copied()
                                    .unwrap_or(*else_block);
                                Some(Terminator::CondBranch {
                                    cond: new_c,
                                    then_block: new_then,
                                    else_block: new_else,
                                })
                            }
                            other => other.clone(),
                        }
                    };

                    unrolled_blocks.push(BasicBlock {
                        id: new_b_id,
                        name: format!("{}_unrolled_{}", orig_b.name, iter_idx),
                        instructions: cloned_insts,
                        terminator: cloned_term,
                    });
                }

                // Update curr_phi_vals for next iteration using this iteration's latch output
                for (phi_dest, _, latch_val, _) in phi_nodes {
                    let updated_val = val_map.get(latch_val).copied().unwrap_or(*latch_val);
                    curr_phi_vals.insert(*phi_dest, updated_val);
                }
            }
        }

        let unrolled_block_ids: std::collections::HashSet<BlockId> =
            unrolled_blocks.iter().map(|b| b.id).collect();

        // Insert unrolled blocks into func.blocks right after preheader
        let p_idx = func
            .blocks
            .iter()
            .position(|b| b.id == preheader_id)
            .unwrap();
        for (offset, ub) in unrolled_blocks.into_iter().enumerate() {
            func.blocks.insert(p_idx + 1 + offset, ub);
        }

        // Remove the old loop blocks (header and original body blocks) and original side-exits
        func.blocks
            .retain(|b| !loop_info.blocks.contains(&b.id) && !side_exit_blocks.contains(&b.id));

        // Update exit block phis: replace references from loop.header with last_block_id
        if let Some(exit_block) = func.blocks.iter_mut().find(|b| b.id == exit_block_id) {
            for inst in &mut exit_block.instructions {
                if let Instruction::Phi { inputs, .. } = inst {
                    for (pred, val) in inputs {
                        if *pred == loop_info.header {
                            *pred = last_block_id;
                            if let Some(&final_v) = curr_phi_vals.get(val) {
                                *val = final_v;
                            }
                        }
                    }
                }
            }
        }

        // Update all instructions and terminators in remaining external blocks to use the final phi values
        let empty_block_map = HashMap::new();
        for b in &mut func.blocks {
            if unrolled_block_ids.contains(&b.id) {
                continue;
            }
            for inst in &mut b.instructions {
                Self::remap_instruction(inst, &curr_phi_vals, &empty_block_map);
            }
            if let Some(ref mut term) = b.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        if let Some(&new_c) = curr_phi_vals.get(cond) {
                            *cond = new_c;
                        }
                    }
                    Terminator::Return(Some(val)) => {
                        if let Some(&new_v) = curr_phi_vals.get(val) {
                            *val = new_v;
                        }
                    }
                    _ => {}
                }
            }
        }

        true
    }

    /// Partially unrolls a loop with factor 2 for exact-multiple iteration counts.
    fn apply_partial_unroll_2(
        func: &mut MirFunction,
        loop_info: &NaturalLoop,
        body_entry_id: BlockId,
        latch_id: BlockId,
        phi_nodes: &[(ValueId, ValueId, ValueId, Type)],
    ) -> bool {
        let body_block_ids: Vec<BlockId> = func
            .blocks
            .iter()
            .map(|b| b.id)
            .filter(|b_id| *b_id != loop_info.header && loop_info.blocks.contains(b_id))
            .collect();

        if body_block_ids.is_empty() {
            return false;
        }

        // Identify side-exit blocks (e.g. bounds check fail blocks) reached from within the loop body
        let mut side_exit_blocks: Vec<BlockId> = Vec::new();
        for &b_id in &body_block_ids {
            if let Some(b) = func.blocks.iter().find(|blk| blk.id == b_id) {
                if let Some(term) = &b.terminator {
                    match term {
                        Terminator::Branch(t) => {
                            if !loop_info.blocks.contains(t) && !side_exit_blocks.contains(t) {
                                side_exit_blocks.push(*t);
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            if !loop_info.blocks.contains(then_block)
                                && !side_exit_blocks.contains(then_block)
                            {
                                side_exit_blocks.push(*then_block);
                            }
                            if !loop_info.blocks.contains(else_block)
                                && !side_exit_blocks.contains(else_block)
                            {
                                side_exit_blocks.push(*else_block);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // All blocks to duplicate for the second iteration (preserving topological order)
        let unroll_block_ids: Vec<BlockId> = func
            .blocks
            .iter()
            .map(|b| b.id)
            .filter(|b_id| body_block_ids.contains(b_id) || side_exit_blocks.contains(b_id))
            .collect();

        let mut next_val_id = func.max_value_id() + 1;
        let mut next_block_id = func.max_block_id() + 1;

        // Map for the second copy of the body:
        // Each header phi maps to the output of iteration 1's latch value!
        let mut val_map: HashMap<ValueId, ValueId> = HashMap::new();
        for (phi_dest, _, latch_val, _) in phi_nodes {
            val_map.insert(*phi_dest, *latch_val);
        }

        let mut block_map: HashMap<BlockId, BlockId> = HashMap::new();
        for &b in &unroll_block_ids {
            block_map.insert(b, BlockId(next_block_id));
            next_block_id += 1;
        }

        // Assign fresh ValueIds to defs in the duplicated body
        for &b_id in &unroll_block_ids {
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
                        val_map.insert(*dest, ValueId(next_val_id));
                        next_val_id += 1;
                    }
                    Instruction::Call { dest: Some(d), .. } => {
                        val_map.insert(*d, ValueId(next_val_id));
                        next_val_id += 1;
                    }
                    Instruction::StrSlice {
                        dest_ptr, dest_len, ..
                    }
                    | Instruction::LoadSlice {
                        dest_ptr, dest_len, ..
                    } => {
                        val_map.insert(*dest_ptr, ValueId(next_val_id));
                        next_val_id += 1;
                        val_map.insert(*dest_len, ValueId(next_val_id));
                        next_val_id += 1;
                    }
                    _ => {}
                }
            }
        }

        // Clone instructions for the second iteration
        let mut second_iter_blocks: Vec<BasicBlock> = Vec::new();
        let second_latch_id = block_map[&latch_id];

        for &b_id in &unroll_block_ids {
            let orig_b = func.blocks.iter().find(|b| b.id == b_id).unwrap();
            let new_b_id = block_map[&b_id];

            let mut cloned_insts = Vec::with_capacity(orig_b.instructions.len());
            for inst in &orig_b.instructions {
                let mut cl = inst.clone();
                Self::remap_instruction(&mut cl, &val_map, &block_map);
                cloned_insts.push(cl);
            }

            let is_latch = b_id == latch_id;
            let cloned_term = if is_latch {
                // Second latch branches back to header
                Some(Terminator::Branch(loop_info.header))
            } else {
                match &orig_b.terminator {
                    Some(Terminator::Branch(t)) => {
                        let new_t = block_map.get(t).copied().unwrap_or(*t);
                        Some(Terminator::Branch(new_t))
                    }
                    Some(Terminator::CondBranch {
                        cond,
                        then_block,
                        else_block,
                    }) => {
                        let new_c = val_map.get(cond).copied().unwrap_or(*cond);
                        let new_then = block_map.get(then_block).copied().unwrap_or(*then_block);
                        let new_else = block_map.get(else_block).copied().unwrap_or(*else_block);
                        Some(Terminator::CondBranch {
                            cond: new_c,
                            then_block: new_then,
                            else_block: new_else,
                        })
                    }
                    other => other.clone(),
                }
            };

            second_iter_blocks.push(BasicBlock {
                id: new_b_id,
                name: format!("{}_dup", orig_b.name),
                instructions: cloned_insts,
                terminator: cloned_term,
            });
        }

        // Connect first latch to the second iteration entry
        let second_entry_id = block_map[&body_entry_id];
        let first_latch_block = func.blocks.iter_mut().find(|b| b.id == latch_id).unwrap();
        first_latch_block.terminator = Some(Terminator::Branch(second_entry_id));

        // Update Header Phis to take values from the second latch!
        let header_block = func
            .blocks
            .iter_mut()
            .find(|b| b.id == loop_info.header)
            .unwrap();
        for inst in &mut header_block.instructions {
            if let Instruction::Phi { inputs, .. } = inst {
                for (pred, val) in inputs {
                    if *pred == latch_id {
                        *pred = second_latch_id;
                        if let Some(&final_v) = val_map.get(val) {
                            *val = final_v;
                        }
                    }
                }
            }
        }

        // Insert duplicated blocks into func.blocks right after first latch
        let l_idx = func.blocks.iter().position(|b| b.id == latch_id).unwrap();
        for (offset, b) in second_iter_blocks.into_iter().enumerate() {
            func.blocks.insert(l_idx + 1 + offset, b);
        }

        true
    }

    /// Remaps instruction operands using val_map and block_map.
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
                for a in args {
                    map_val(a);
                }
            }
            Instruction::CallIndirect {
                dest,
                callee_ptr,
                args,
                ..
            } => {
                if let Some(d) = dest {
                    map_val(d);
                }
                map_val(callee_ptr);
                for a in args {
                    map_val(a);
                }
            }
            Instruction::VtableAddr { dest, .. } => {
                map_val(dest);
            }
            Instruction::IndexAddr { dest, index, .. } => {
                map_val(dest);
                map_val(index);
            }
            Instruction::IndexPtrAddr {
                dest,
                base_ptr,
                index,
                ..
            } => {
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
            Instruction::StrSlice {
                dest_ptr, dest_len, ..
            } => {
                map_val(dest_ptr);
                map_val(dest_len);
            }
            Instruction::StoreSlice { ptr, len, .. } => {
                map_val(ptr);
                map_val(len);
            }
            Instruction::LoadSlice {
                dest_ptr, dest_len, ..
            } => {
                map_val(dest_ptr);
                map_val(dest_len);
            }
            Instruction::Phi { dest, inputs, .. } => {
                map_val(dest);
                for (b, v) in inputs {
                    if let Some(&new_b) = block_map.get(b) {
                        *b = new_b;
                    }
                    map_val(v);
                }
            }
            Instruction::Copy { dest, src, .. } => {
                map_val(dest);
                map_val(src);
            }
        }
    }
}
