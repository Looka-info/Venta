//! Dead code elimination and unreachable block elimination for Vanta MIR.

use crate::ir::*;
use std::collections::HashSet;

pub struct DeadCodeElimination;

impl DeadCodeElimination {
    /// Runs dead-code and unreachable-block elimination on an MIR function.
    /// Returns true if any instruction or block was removed.
    pub fn run_on_function(func: &mut MirFunction) -> bool {
        let mut changed = false;

        // 1. Unreachable block elimination
        changed |= Self::eliminate_unreachable_blocks(func);

        // 2. Unused pure instruction elimination (fixed-point)
        loop {
            let removed_any = Self::eliminate_unused_instructions(func);
            if removed_any {
                changed = true;
            } else {
                break;
            }
        }

        changed
    }

    /// Eliminates basic blocks that cannot be reached from the function entry block.
    fn eliminate_unreachable_blocks(func: &mut MirFunction) -> bool {
        if func.blocks.is_empty() {
            return false;
        }

        let entry_id = func.blocks[0].id;
        let mut reachable = HashSet::new();
        let mut stack = vec![entry_id];
        reachable.insert(entry_id);

        let block_map: std::collections::HashMap<BlockId, &BasicBlock> =
            func.blocks.iter().map(|b| (b.id, b)).collect();

        while let Some(current_id) = stack.pop() {
            if let Some(block) = block_map.get(&current_id) {
                if let Some(term) = &block.terminator {
                    match term {
                        Terminator::Branch(target) => {
                            if reachable.insert(*target) {
                                stack.push(*target);
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            if reachable.insert(*then_block) {
                                stack.push(*then_block);
                            }
                            if reachable.insert(*else_block) {
                                stack.push(*else_block);
                            }
                        }
                        Terminator::Return(_) | Terminator::Unreachable => {}
                    }
                }
            }
        }

        let initial_count = func.blocks.len();
        func.blocks.retain(|b| reachable.contains(&b.id));
        func.blocks.len() != initial_count
    }

    /// Eliminates instructions whose results are never used and that produce no side effects.
    fn eliminate_unused_instructions(func: &mut MirFunction) -> bool {
        // Collect all used ValueIds
        let mut used_values = HashSet::new();

        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Store { val, .. } => {
                        used_values.insert(*val);
                    }
                    Instruction::StoreIndirect { addr, val } => {
                        used_values.insert(*addr);
                        used_values.insert(*val);
                    }
                    Instruction::LoadIndirect { addr, .. } => {
                        used_values.insert(*addr);
                    }
                    Instruction::Call { args, .. } => {
                        for arg in args {
                            used_values.insert(*arg);
                        }
                    }
                    Instruction::Binary { lhs, rhs, .. } => {
                        used_values.insert(*lhs);
                        used_values.insert(*rhs);
                    }
                    Instruction::Unary { operand, .. } => {
                        used_values.insert(*operand);
                    }
                    Instruction::IndexAddr { index, .. } => {
                        used_values.insert(*index);
                    }
                    Instruction::ConstInt { .. }
                    | Instruction::ConstBool { .. }
                    | Instruction::SlotAddr { .. }
                    | Instruction::Load { .. } => {}
                }
            }

            if let Some(term) = &block.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        used_values.insert(*cond);
                    }
                    Terminator::Return(Some(val)) => {
                        used_values.insert(*val);
                    }
                    Terminator::Return(None) | Terminator::Branch(_) | Terminator::Unreachable => {}
                }
            }
        }

        let mut changed = false;

        // An instruction can be eliminated if its defined dest is never used,
        // and the instruction itself is pure (no memory writes, no calls, no potential traps).
        for block in &mut func.blocks {
            let initial_len = block.instructions.len();
            block.instructions.retain(|inst| {
                match inst {
                    Instruction::ConstInt { dest, .. }
                    | Instruction::ConstBool { dest, .. }
                    | Instruction::SlotAddr { dest, .. }
                    | Instruction::Load { dest, .. } => used_values.contains(dest),
                    Instruction::Unary { dest, .. } => used_values.contains(dest),
                    Instruction::Binary { dest, op, .. } => {
                        // Preserves Div/Rem if they could potentially trap at runtime
                        match op {
                            MirBinaryOp::Div | MirBinaryOp::Rem => true,
                            _ => used_values.contains(dest),
                        }
                    }
                    Instruction::IndexAddr { dest, .. } => used_values.contains(dest),
                    // NEVER remove instructions with side effects:
                    // Stores write memory; Calls may perform I/O or loop; Indirect loads read memory
                    Instruction::Store { .. }
                    | Instruction::StoreIndirect { .. }
                    | Instruction::LoadIndirect { .. }
                    | Instruction::Call { .. } => true,
                }
            });

            if block.instructions.len() != initial_len {
                changed = true;
            }
        }

        changed
    }
}
