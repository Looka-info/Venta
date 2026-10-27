//! Conservative, size-aware function inlining for Vanta MIR.
//!
//! Splices direct call callee bodies into caller CFGs, performs unique ID remapping,
//! argument stores and parameter promotion, multiple return path phi construction,
//! and growth budget enforcement.

use crate::ir::*;
use crate::opt::call_graph::CallGraph;
use std::collections::HashMap;
use vanta_sema::Type;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlinerStats {
    pub call_sites_examined: usize,
    pub calls_inlined: usize,
    pub candidates_rejected: usize,
    pub recursion_rejections: usize,
    pub size_budget_rejections: usize,
    pub estimated_mir_growth: usize,
}

pub struct Inliner;

impl Inliner {
    /// Maximum instruction growth allowed for a single caller function.
    pub const MAX_CALLER_GROWTH: usize = 250;
    /// Maximum inlining rounds per function.
    pub const MAX_INLINE_ROUNDS: usize = 4;

    /// Runs conservative inlining across all functions in the program.
    pub fn run_on_program(program: &mut MirProgram) -> (bool, InlinerStats) {
        let mut overall_stats = InlinerStats::default();
        let mut overall_changed = false;

        // Build direct call graph and summaries
        let call_graph = CallGraph::build(program);

        // Map function names to cloned callee definitions
        let func_map: HashMap<String, MirFunction> = program
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.clone()))
            .collect();

        // Inline eligible calls in each function
        for func in &mut program.functions {
            let (changed, stats) = Self::inline_in_function(func, &func_map, &call_graph);
            if changed {
                overall_changed = true;
                overall_stats.calls_inlined += stats.calls_inlined;
                overall_stats.estimated_mir_growth += stats.estimated_mir_growth;
            }
            overall_stats.call_sites_examined += stats.call_sites_examined;
            overall_stats.candidates_rejected += stats.candidates_rejected;
            overall_stats.recursion_rejections += stats.recursion_rejections;
            overall_stats.size_budget_rejections += stats.size_budget_rejections;
        }

        (overall_changed, overall_stats)
    }

    /// Inlines eligible call sites within a single caller function.
    pub fn inline_in_function(
        caller: &mut MirFunction,
        func_map: &HashMap<String, MirFunction>,
        call_graph: &CallGraph,
    ) -> (bool, InlinerStats) {
        let mut stats = InlinerStats::default();
        let mut changed = false;
        let mut caller_growth = 0;
        let mut round = 0;

        while round < Self::MAX_INLINE_ROUNDS {
            round += 1;
            let mut candidate_site = None;

            // Find the next eligible call site
            'outer: for block in &caller.blocks {
                for (inst_idx, inst) in block.instructions.iter().enumerate() {
                    if let Instruction::Call {
                        dest,
                        callee,
                        args,
                        ret_type,
                    } = inst
                    {
                        stats.call_sites_examined += 1;

                        // Check call graph candidate rules
                        let (eligible, reason) = call_graph.is_inlining_candidate(callee);
                        if !eligible {
                            stats.candidates_rejected += 1;
                            if reason.contains("recursive") {
                                stats.recursion_rejections += 1;
                            } else if reason.contains("size budget") {
                                stats.size_budget_rejections += 1;
                            }
                            continue;
                        }

                        let callee_func = match func_map.get(callee) {
                            Some(f) => f,
                            None => {
                                stats.candidates_rejected += 1;
                                continue;
                            }
                        };

                        // Avoid inlining functions with no blocks or memory class returns
                        if callee_func.blocks.is_empty() {
                            stats.candidates_rejected += 1;
                            continue;
                        }

                        // Parameter count must match arguments 1:1
                        if callee_func.params.len() != args.len() {
                            stats.candidates_rejected += 1;
                            continue;
                        }

                        // Growth budget check
                        let callee_inst_count: usize =
                            callee_func.blocks.iter().map(|b| b.instructions.len()).sum();
                        if caller_growth + callee_inst_count > Self::MAX_CALLER_GROWTH {
                            stats.size_budget_rejections += 1;
                            stats.candidates_rejected += 1;
                            continue;
                        }

                        candidate_site = Some((
                            block.id,
                            inst_idx,
                            callee_func.clone(),
                            *dest,
                            args.clone(),
                            ret_type.clone(),
                            callee_inst_count,
                        ));
                        break 'outer;
                    }
                }
            }

            if let Some((
                block_id,
                inst_idx,
                callee_func,
                caller_dest,
                args,
                ret_type,
                callee_inst_count,
            )) = candidate_site
            {
                Self::inline_call_site(
                    caller,
                    block_id,
                    inst_idx,
                    &callee_func,
                    caller_dest,
                    &args,
                    &ret_type,
                );
                changed = true;
                caller_growth += callee_inst_count;
                stats.calls_inlined += 1;
                stats.estimated_mir_growth += callee_inst_count;
            } else {
                break;
            }
        }

        (changed, stats)
    }

    /// Performs the AST/CFG splicing for a single call site.
    fn inline_call_site(
        caller: &mut MirFunction,
        call_block_id: BlockId,
        call_inst_idx: usize,
        callee: &MirFunction,
        caller_dest: Option<ValueId>,
        args: &[ValueId],
        ret_type: &Type,
    ) {
        let mut next_val_id = caller.max_value_id() + 1;
        let mut next_block_id = caller.max_block_id() + 1;
        let mut next_slot_id = caller.slots.iter().map(|s| s.id.0).max().unwrap_or(0) + 1;

        let mut val_map: HashMap<ValueId, ValueId> = HashMap::new();
        let mut block_map: HashMap<BlockId, BlockId> = HashMap::new();
        let mut slot_map: HashMap<SlotId, SlotId> = HashMap::new();

        // 1. Remap all callee slots and clone them into caller
        for slot in &callee.slots {
            let new_slot_id = SlotId(next_slot_id);
            next_slot_id += 1;
            slot_map.insert(slot.id, new_slot_id);
            caller.slots.push(LocalSlot {
                id: new_slot_id,
                name: format!("{}_inlined_{}", callee.name, slot.name),
                ty: slot.ty.clone(),
                size: slot.size,
                align: slot.align,
            });
        }

        // 2. Map callee parameter slots and build caller argument stores
        let mut param_stores = Vec::new();
        for ((param_slot, _), &arg_val) in callee.params.iter().zip(args.iter()) {
            if let Some(&new_slot) = slot_map.get(param_slot) {
                param_stores.push(Instruction::Store {
                    slot: new_slot,
                    val: arg_val,
                });
            }
        }

        // 3. Remap all callee blocks
        for block in &callee.blocks {
            let new_b_id = BlockId(next_block_id);
            next_block_id += 1;
            block_map.insert(block.id, new_b_id);
        }

        // 4. Remap all callee destination ValueIds
        for block in &callee.blocks {
            for inst in &block.instructions {
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

        let continuation_block_id = BlockId(next_block_id);
        next_block_id += 1;
        let _ = next_block_id;

        // 5. Split the call block into before_block and continuation_block
        let call_block = caller
            .blocks
            .iter_mut()
            .find(|b| b.id == call_block_id)
            .unwrap();

        let mut instrs_before: Vec<Instruction> =
            call_block.instructions[..call_inst_idx].to_vec();
        instrs_before.extend(param_stores);

        let instrs_after: Vec<Instruction> =
            call_block.instructions[call_inst_idx + 1..].to_vec();
        let original_terminator = call_block.terminator.take();

        // Point before_block to the callee entry block
        let callee_entry_id = block_map[&callee.blocks[0].id];
        call_block.instructions = instrs_before;
        call_block.terminator = Some(Terminator::Branch(callee_entry_id));

        // 6. Clone all callee blocks
        let mut return_entries: Vec<(BlockId, ValueId)> = Vec::new();
        let mut cloned_callee_blocks = Vec::new();

        for block in &callee.blocks {
            let cloned_id = block_map[&block.id];
            let mut cloned_instructions = Vec::new();

            for inst in &block.instructions {
                let mut cloned_inst = inst.clone();
                Self::remap_instruction(&mut cloned_inst, &val_map, &slot_map, &block_map);
                cloned_instructions.push(cloned_inst);
            }

            let cloned_term = match &block.terminator {
                Some(Terminator::Return(opt_val)) => {
                    if let Some(ret_val) = opt_val {
                        let mapped_val = val_map.get(ret_val).copied().unwrap_or(*ret_val);
                        return_entries.push((cloned_id, mapped_val));
                    }
                    Terminator::Branch(continuation_block_id)
                }
                Some(Terminator::Branch(target)) => {
                    Terminator::Branch(block_map.get(target).copied().unwrap_or(*target))
                }
                Some(Terminator::CondBranch {
                    cond,
                    then_block,
                    else_block,
                }) => Terminator::CondBranch {
                    cond: val_map.get(cond).copied().unwrap_or(*cond),
                    then_block: block_map.get(then_block).copied().unwrap_or(*then_block),
                    else_block: block_map.get(else_block).copied().unwrap_or(*else_block),
                },
                Some(Terminator::Unreachable) => Terminator::Unreachable,
                None => Terminator::Branch(continuation_block_id),
            };

            cloned_callee_blocks.push(BasicBlock {
                id: cloned_id,
                name: format!("{}_inlined_{}", callee.name, block.name),
                instructions: cloned_instructions,
                terminator: Some(cloned_term),
            });
        }

        // 7. Build continuation block
        let mut cont_instructions = Vec::new();
        if let Some(dest_val) = caller_dest {
            if return_entries.len() == 1 {
                let (_, single_ret_val) = return_entries[0];
                cont_instructions.push(Instruction::Copy {
                    dest: dest_val,
                    src: single_ret_val,
                    ty: ret_type.clone(),
                });
            } else if return_entries.len() > 1 {
                cont_instructions.push(Instruction::Phi {
                    dest: dest_val,
                    inputs: return_entries,
                    ty: ret_type.clone(),
                });
            }
        }
        cont_instructions.extend(instrs_after);

        let continuation_block = BasicBlock {
            id: continuation_block_id,
            name: format!("cont_{}", continuation_block_id.0),
            instructions: cont_instructions,
            terminator: original_terminator,
        };

        // 8. Update any successor Phi nodes that referenced call_block_id
        for block in &mut caller.blocks {
            for inst in &mut block.instructions {
                if let Instruction::Phi { inputs, .. } = inst {
                    for (pred, _) in inputs {
                        if *pred == call_block_id {
                            *pred = continuation_block_id;
                        }
                    }
                }
            }
        }

        // 9. Insert cloned callee blocks and continuation block immediately after call_block
        let call_block_idx = caller
            .blocks
            .iter()
            .position(|b| b.id == call_block_id)
            .unwrap();

        let mut splice_blocks = cloned_callee_blocks;
        splice_blocks.push(continuation_block);

        let insert_idx = call_block_idx + 1;
        caller.blocks.splice(insert_idx..insert_idx, splice_blocks);
    }

    /// Remaps all identifiers inside an inlined instruction.
    fn remap_instruction(
        inst: &mut Instruction,
        val_map: &HashMap<ValueId, ValueId>,
        slot_map: &HashMap<SlotId, SlotId>,
        block_map: &HashMap<BlockId, BlockId>,
    ) {
        let map_val = |v: &mut ValueId| {
            if let Some(&new_v) = val_map.get(v) {
                *v = new_v;
            }
        };
        let map_slot = |s: &mut SlotId| {
            if let Some(&new_s) = slot_map.get(s) {
                *s = new_s;
            }
        };

        match inst {
            Instruction::ConstInt { dest, .. } | Instruction::ConstBool { dest, .. } => {
                map_val(dest);
            }
            Instruction::SlotAddr { dest, slot } => {
                map_val(dest);
                map_slot(slot);
            }
            Instruction::Load { dest, slot, .. } => {
                map_val(dest);
                map_slot(slot);
            }
            Instruction::Store { slot, val } => {
                map_slot(slot);
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
            Instruction::IndexAddr {
                dest, slot, index, ..
            } => {
                map_val(dest);
                map_slot(slot);
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
            Instruction::StoreSlice { slot, ptr, len } => {
                map_slot(slot);
                map_val(ptr);
                map_val(len);
            }
            Instruction::LoadSlice {
                dest_ptr,
                dest_len,
                slot,
            } => {
                map_val(dest_ptr);
                map_val(dest_len);
                map_slot(slot);
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
