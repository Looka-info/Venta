//! Verification passes for Vanta MIR.

use crate::ir::*;
use std::collections::HashSet;
use vanta_diagnostics::{codes, Diagnostic, DiagnosticBag};

fn mir_error(msg: impl Into<String>) -> Diagnostic {
    Diagnostic::error(msg).with_code(codes::E0401_MIR_VERIFICATION_FAILED)
}

pub struct MirVerifier;

impl MirVerifier {
    pub fn verify_program(program: &MirProgram, diags: &mut DiagnosticBag) {
        for func in &program.functions {
            Self::verify_function(func, diags);
        }
    }

    pub fn verify_function(func: &MirFunction, diags: &mut DiagnosticBag) {
        if func.blocks.is_empty() {
            diags.emit(mir_error(format!(
                "Function '{}' has no basic blocks",
                func.name
            )));
            return;
        }

        let mut block_ids = HashSet::new();
        for block in &func.blocks {
            if !block_ids.insert(block.id) {
                diags.emit(mir_error(format!(
                    "Duplicate BlockId {:?} in function '{}'",
                    block.id, func.name
                )));
            }
        }

        let mut defined_values = HashSet::new();

        for block in &func.blocks {
            // Check that block has a terminator
            let term = match &block.terminator {
                Some(t) => t,
                None => {
                    diags.emit(mir_error(format!(
                        "Basic block '{}' ({:?}) in function '{}' has no terminator",
                        block.name, block.id, func.name
                    )));
                    continue;
                }
            };

            // Verify instructions
            for inst in &block.instructions {
                match inst {
                    Instruction::ConstInt { dest, .. } | Instruction::ConstBool { dest, .. } => {
                        defined_values.insert(*dest);
                    }
                    Instruction::SlotAddr { dest, .. } => {
                        defined_values.insert(*dest);
                    }
                    Instruction::Load { dest, .. } => {
                        defined_values.insert(*dest);
                    }
                    Instruction::Store { val, .. } => {
                        if !defined_values.contains(val) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in store in '{}'",
                                val, func.name
                            )));
                        }
                    }
                    Instruction::Binary { dest, lhs, rhs, .. } => {
                        if !defined_values.contains(lhs) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in binary LHS in '{}'",
                                lhs, func.name
                            )));
                        }
                        if !defined_values.contains(rhs) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in binary RHS in '{}'",
                                rhs, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::Unary { dest, operand, .. } => {
                        if !defined_values.contains(operand) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in unary operand in '{}'",
                                operand, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::Call { dest, args, .. } => {
                        for arg in args {
                            if !defined_values.contains(arg) {
                                diags.emit(mir_error(format!(
                                    "Value {:?} used before definition in call arg in '{}'",
                                    arg, func.name
                                )));
                            }
                        }
                        if let Some(d) = dest {
                            defined_values.insert(*d);
                        }
                    }
                    Instruction::IndexAddr { dest, index, .. } => {
                        if !defined_values.contains(index) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in index in '{}'",
                                index, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::LoadIndirect { dest, addr, .. } => {
                        if !defined_values.contains(addr) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in load indirect in '{}'",
                                addr, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::StoreIndirect { addr, val } => {
                        if !defined_values.contains(addr) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in store indirect addr in '{}'",
                                addr, func.name
                            )));
                        }
                        if !defined_values.contains(val) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in store indirect val in '{}'",
                                val, func.name
                            )));
                        }
                    }
                    Instruction::StrSlice {
                        dest_ptr, dest_len, ..
                    } => {
                        defined_values.insert(*dest_ptr);
                        defined_values.insert(*dest_len);
                    }
                    Instruction::StoreSlice { ptr, len, .. } => {
                        if !defined_values.contains(ptr) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in store slice ptr in '{}'",
                                ptr, func.name
                            )));
                        }
                        if !defined_values.contains(len) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in store slice len in '{}'",
                                len, func.name
                            )));
                        }
                    }
                    Instruction::LoadSlice {
                        dest_ptr, dest_len, ..
                    } => {
                        defined_values.insert(*dest_ptr);
                        defined_values.insert(*dest_len);
                    }
                }
            }

            // Verify terminator branch targets
            match term {
                Terminator::Branch(target) => {
                    if !block_ids.contains(target) {
                        diags.emit(mir_error(format!(
                            "Branch target {:?} does not exist in function '{}'",
                            target, func.name
                        )));
                    }
                }
                Terminator::CondBranch {
                    cond,
                    then_block,
                    else_block,
                } => {
                    if !defined_values.contains(cond) {
                        diags.emit(mir_error(format!(
                            "CondBranch condition {:?} used before definition in '{}'",
                            cond, func.name
                        )));
                    }
                    if !block_ids.contains(then_block) {
                        diags.emit(mir_error(format!(
                            "CondBranch then target {:?} does not exist in function '{}'",
                            then_block, func.name
                        )));
                    }
                    if !block_ids.contains(else_block) {
                        diags.emit(mir_error(format!(
                            "CondBranch else target {:?} does not exist in function '{}'",
                            else_block, func.name
                        )));
                    }
                }
                Terminator::Return(val) => {
                    if let Some(v) = val {
                        if !defined_values.contains(v) {
                            diags.emit(mir_error(format!(
                                "Return value {:?} used before definition in '{}'",
                                v, func.name
                            )));
                        }
                    }
                }
                Terminator::Unreachable => {}
            }
        }
    }
}
