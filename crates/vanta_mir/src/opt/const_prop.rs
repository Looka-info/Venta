//! Constant propagation and constant folding optimization for Vanta MIR.

use crate::ir::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstValue {
    Int(i64),
    Bool(bool),
}

pub struct ConstantPropagation;

impl ConstantPropagation {
    /// Runs constant propagation and folding on an MIR function.
    /// Returns true if any transformation was made.
    pub fn run_on_function(func: &mut MirFunction) -> bool {
        let mut changed = false;

        // 1. Identify escaped slots (slots whose address is taken)
        let mut escaped_slots = HashSet::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::SlotAddr { slot, .. } | Instruction::IndexAddr { slot, .. } => {
                        escaped_slots.insert(*slot);
                    }
                    _ => {}
                }
            }
        }

        // 2. Compute Control Flow Graph predecessor map
        let mut predecessors: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
        for block in &func.blocks {
            predecessors.entry(block.id).or_default();
            if let Some(term) = &block.terminator {
                match term {
                    Terminator::Branch(target) => {
                        predecessors.entry(*target).or_default().push(block.id);
                    }
                    Terminator::CondBranch {
                        then_block,
                        else_block,
                        ..
                    } => {
                        predecessors.entry(*then_block).or_default().push(block.id);
                        predecessors.entry(*else_block).or_default().push(block.id);
                    }
                    Terminator::Return(_) | Terminator::Unreachable => {}
                }
            }
        }

        // 3. Fixed-point dataflow analysis for slot constant facts
        // block_in_slots[block_id] = facts at entry to block
        // block_out_slots[block_id] = facts at exit from block
        let mut block_in_slots: HashMap<BlockId, HashMap<SlotId, ConstValue>> = HashMap::new();
        let mut block_out_slots: HashMap<BlockId, HashMap<SlotId, ConstValue>> = HashMap::new();

        let mut worklist: Vec<BlockId> = func.blocks.iter().map(|b| b.id).collect();
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 64;

        while let Some(block_id) = worklist.pop() {
            iterations += 1;
            if iterations > MAX_ITERATIONS * func.blocks.len().max(1) {
                break; // Guarantee termination
            }

            let block = match func.blocks.iter().find(|b| b.id == block_id) {
                Some(b) => b,
                None => continue,
            };

            // Compute block entry facts from predecessors
            let in_facts: HashMap<SlotId, ConstValue> = if block_id == func.blocks[0].id {
                // Entry block has no incoming slot constants
                HashMap::new()
            } else {
                let preds = predecessors.get(&block_id).cloned().unwrap_or_default();
                if preds.is_empty() {
                    HashMap::new()
                } else {
                    let mut meet_facts: Option<HashMap<SlotId, ConstValue>> = None;
                    for p in preds {
                        if let Some(out_facts) = block_out_slots.get(&p) {
                            match &mut meet_facts {
                                None => {
                                    meet_facts = Some(out_facts.clone());
                                }
                                Some(curr) => {
                                    curr.retain(|slot, val| out_facts.get(slot) == Some(val));
                                }
                            }
                        }
                    }
                    meet_facts.unwrap_or_default()
                }
            };

            block_in_slots.insert(block_id, in_facts.clone());

            // Simulate block to compute out_facts
            let mut current_facts = in_facts;
            let mut temp_values: HashMap<ValueId, ConstValue> = HashMap::new();

            for inst in &block.instructions {
                match inst {
                    Instruction::ConstInt { dest, val, .. } => {
                        temp_values.insert(*dest, ConstValue::Int(*val));
                    }
                    Instruction::ConstBool { dest, val } => {
                        temp_values.insert(*dest, ConstValue::Bool(*val));
                    }
                    Instruction::Store { slot, val } => {
                        if !escaped_slots.contains(slot) {
                            if let Some(&c) = temp_values.get(val) {
                                current_facts.insert(*slot, c);
                            } else {
                                current_facts.remove(slot);
                            }
                        }
                    }
                    Instruction::Load { dest, slot, .. } => {
                        if !escaped_slots.contains(slot) {
                            if let Some(&c) = current_facts.get(slot) {
                                temp_values.insert(*dest, c);
                            }
                        }
                    }
                    Instruction::Binary {
                        dest, op, lhs, rhs, ..
                    } => {
                        if let (Some(&c_lhs), Some(&c_rhs)) =
                            (temp_values.get(lhs), temp_values.get(rhs))
                        {
                            if let Some(c_res) = Self::eval_binary(*op, c_lhs, c_rhs) {
                                temp_values.insert(*dest, c_res);
                            }
                        }
                    }
                    Instruction::Unary {
                        dest, op, operand, ..
                    } => {
                        if let Some(&c_op) = temp_values.get(operand) {
                            if let Some(c_res) = Self::eval_unary(*op, c_op) {
                                temp_values.insert(*dest, c_res);
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Check if out_facts changed
            let previous_out = block_out_slots.get(&block_id);
            if previous_out != Some(&current_facts) {
                block_out_slots.insert(block_id, current_facts);
                // Add successors to worklist
                if let Some(term) = &block.terminator {
                    match term {
                        Terminator::Branch(target) => {
                            if !worklist.contains(target) {
                                worklist.push(*target);
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            if !worklist.contains(then_block) {
                                worklist.push(*then_block);
                            }
                            if !worklist.contains(else_block) {
                                worklist.push(*else_block);
                            }
                        }
                        Terminator::Return(_) | Terminator::Unreachable => {}
                    }
                }
            }
        }

        // 4. Transform: replace loads of constant slots and fold constant expressions
        let mut known_values: HashMap<ValueId, ConstValue> = HashMap::new();

        for block in &mut func.blocks {
            let mut current_slot_facts = block_in_slots.get(&block.id).cloned().unwrap_or_default();

            for inst in &mut block.instructions {
                let mut replacement: Option<(Instruction, ValueId, ConstValue)> = None;

                match inst {
                    Instruction::ConstInt { dest, val, .. } => {
                        known_values.insert(*dest, ConstValue::Int(*val));
                    }
                    Instruction::ConstBool { dest, val } => {
                        known_values.insert(*dest, ConstValue::Bool(*val));
                    }
                    Instruction::Store { slot, val } => {
                        if !escaped_slots.contains(slot) {
                            if let Some(&c) = known_values.get(val) {
                                current_slot_facts.insert(*slot, c);
                            } else {
                                current_slot_facts.remove(slot);
                            }
                        }
                    }
                    Instruction::Load { dest, slot, ty } => {
                        if !escaped_slots.contains(slot) {
                            if let Some(&c) = current_slot_facts.get(slot) {
                                let d = *dest;
                                let new_inst = match c {
                                    ConstValue::Int(v) => Instruction::ConstInt {
                                        dest: d,
                                        val: v,
                                        ty: ty.clone(),
                                    },
                                    ConstValue::Bool(b) => {
                                        Instruction::ConstBool { dest: d, val: b }
                                    }
                                };
                                replacement = Some((new_inst, d, c));
                            }
                        }
                    }
                    Instruction::Binary {
                        dest,
                        op,
                        lhs,
                        rhs,
                        ty,
                    } => {
                        if let (Some(&c_lhs), Some(&c_rhs)) =
                            (known_values.get(lhs), known_values.get(rhs))
                        {
                            if let Some(c_res) = Self::eval_binary(*op, c_lhs, c_rhs) {
                                let d = *dest;
                                let new_inst = match c_res {
                                    ConstValue::Int(v) => Instruction::ConstInt {
                                        dest: d,
                                        val: v,
                                        ty: ty.clone(),
                                    },
                                    ConstValue::Bool(b) => {
                                        Instruction::ConstBool { dest: d, val: b }
                                    }
                                };
                                replacement = Some((new_inst, d, c_res));
                            }
                        }
                    }
                    Instruction::Unary {
                        dest,
                        op,
                        operand,
                        ty,
                    } => {
                        if let Some(&c_op) = known_values.get(operand) {
                            if let Some(c_res) = Self::eval_unary(*op, c_op) {
                                let d = *dest;
                                let new_inst = match c_res {
                                    ConstValue::Int(v) => Instruction::ConstInt {
                                        dest: d,
                                        val: v,
                                        ty: ty.clone(),
                                    },
                                    ConstValue::Bool(b) => {
                                        Instruction::ConstBool { dest: d, val: b }
                                    }
                                };
                                replacement = Some((new_inst, d, c_res));
                            }
                        }
                    }
                    _ => {}
                }

                if let Some((new_inst, d, c)) = replacement {
                    *inst = new_inst;
                    known_values.insert(d, c);
                    changed = true;
                }
            }

            // Fold constant conditional branches
            if let Some(term) = &mut block.terminator {
                if let Terminator::CondBranch {
                    cond,
                    then_block,
                    else_block,
                } = term
                {
                    if let Some(c) = known_values.get(cond) {
                        match c {
                            ConstValue::Bool(true) => {
                                *term = Terminator::Branch(*then_block);
                                changed = true;
                            }
                            ConstValue::Bool(false) => {
                                *term = Terminator::Branch(*else_block);
                                changed = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        changed
    }

    /// Evaluates a binary operation on constants.
    /// Strictly handles overflow and division-by-zero preservation: returns None on division by 0
    /// or overflow to ensure runtime trapping behavior is preserved.
    pub fn eval_binary(op: MirBinaryOp, lhs: ConstValue, rhs: ConstValue) -> Option<ConstValue> {
        match (op, lhs, rhs) {
            (MirBinaryOp::Add, ConstValue::Int(a), ConstValue::Int(b)) => {
                a.checked_add(b).map(ConstValue::Int)
            }
            (MirBinaryOp::Sub, ConstValue::Int(a), ConstValue::Int(b)) => {
                a.checked_sub(b).map(ConstValue::Int)
            }
            (MirBinaryOp::Mul, ConstValue::Int(a), ConstValue::Int(b)) => {
                a.checked_mul(b).map(ConstValue::Int)
            }
            (MirBinaryOp::Div, ConstValue::Int(a), ConstValue::Int(b)) => {
                // Preserve division by zero and signed overflow (i64::MIN / -1)
                if b == 0 || (a == i64::MIN && b == -1) {
                    None
                } else {
                    a.checked_div(b).map(ConstValue::Int)
                }
            }
            (MirBinaryOp::Rem, ConstValue::Int(a), ConstValue::Int(b)) => {
                if b == 0 || (a == i64::MIN && b == -1) {
                    None
                } else {
                    a.checked_rem(b).map(ConstValue::Int)
                }
            }
            (MirBinaryOp::BitAnd, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a & b))
            }
            (MirBinaryOp::BitOr, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a | b))
            }
            (MirBinaryOp::BitXor, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Int(a ^ b))
            }
            (MirBinaryOp::Shl, ConstValue::Int(a), ConstValue::Int(b)) => {
                if (0..64).contains(&b) {
                    a.checked_shl(b as u32).map(ConstValue::Int)
                } else {
                    None
                }
            }
            (MirBinaryOp::Shr, ConstValue::Int(a), ConstValue::Int(b)) => {
                if (0..64).contains(&b) {
                    a.checked_shr(b as u32).map(ConstValue::Int)
                } else {
                    None
                }
            }
            (MirBinaryOp::Eq, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a == b))
            }
            (MirBinaryOp::Eq, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(a == b))
            }
            (MirBinaryOp::NotEq, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a != b))
            }
            (MirBinaryOp::NotEq, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(a != b))
            }
            (MirBinaryOp::Lt, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a < b))
            }
            (MirBinaryOp::LtEq, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a <= b))
            }
            (MirBinaryOp::Gt, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a > b))
            }
            (MirBinaryOp::GtEq, ConstValue::Int(a), ConstValue::Int(b)) => {
                Some(ConstValue::Bool(a >= b))
            }
            (MirBinaryOp::And, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(a && b))
            }
            (MirBinaryOp::Or, ConstValue::Bool(a), ConstValue::Bool(b)) => {
                Some(ConstValue::Bool(a || b))
            }
            _ => None,
        }
    }

    /// Evaluates a unary operation on constants.
    pub fn eval_unary(op: MirUnaryOp, operand: ConstValue) -> Option<ConstValue> {
        match (op, operand) {
            (MirUnaryOp::Neg, ConstValue::Int(a)) => a.checked_neg().map(ConstValue::Int),
            (MirUnaryOp::Not, ConstValue::Bool(b)) => Some(ConstValue::Bool(!b)),
            (MirUnaryOp::Not, ConstValue::Int(a)) => Some(ConstValue::Int(!a)),
            _ => None,
        }
    }
}
