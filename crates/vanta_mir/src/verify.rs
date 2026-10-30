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

        // 1. Verify Slot definitions & parameters
        let mut slot_ids = HashSet::new();
        for slot in &func.slots {
            if !slot_ids.insert(slot.id) {
                diags.emit(mir_error(format!(
                    "Duplicate SlotId {:?} in function '{}'",
                    slot.id, func.name
                )));
            }
        }

        for (param_slot, _) in &func.params {
            if !slot_ids.contains(param_slot) {
                diags.emit(mir_error(format!(
                    "Parameter slot {:?} not declared in func.slots in '{}'",
                    param_slot, func.name
                )));
            }
        }

        // 2. Compute CFG predecessor relationships
        let mut preds: std::collections::HashMap<BlockId, HashSet<BlockId>> =
            std::collections::HashMap::new();
        for b in &func.blocks {
            if let Some(t) = &b.terminator {
                match t {
                    Terminator::Branch(target) => {
                        preds.entry(*target).or_default().insert(b.id);
                    }
                    Terminator::CondBranch {
                        then_block,
                        else_block,
                        ..
                    } => {
                        preds.entry(*then_block).or_default().insert(b.id);
                        preds.entry(*else_block).or_default().insert(b.id);
                    }
                    Terminator::Return(_) | Terminator::Unreachable => {}
                }
            }
        }

        // 3. Pre-collect all defined values in function (to validate phi loop backedges)
        let mut all_defined_values = HashSet::new();
        for b in &func.blocks {
            for inst in &b.instructions {
                for d in inst.defs() {
                    all_defined_values.insert(d);
                }
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

            let mut seen_non_phi = false;

            // Verify instructions
            for inst in &block.instructions {
                if matches!(inst, Instruction::Phi { .. }) {
                    if seen_non_phi {
                        diags.emit(mir_error(format!(
                            "Phi instruction appears after non-Phi instruction in block {:?} of '{}'",
                            block.id, func.name
                        )));
                    }
                } else {
                    seen_non_phi = true;
                }

                match inst {
                    Instruction::ConstInt { dest, .. } | Instruction::ConstBool { dest, .. } => {
                        defined_values.insert(*dest);
                    }
                    Instruction::SlotAddr { dest, slot } => {
                        if !slot_ids.contains(slot) {
                            diags.emit(mir_error(format!(
                                "Instruction references unallocated SlotId {:?} in '{}'",
                                slot, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::Load { dest, slot, .. } => {
                        if !slot_ids.contains(slot) {
                            diags.emit(mir_error(format!(
                                "Instruction references unallocated SlotId {:?} in '{}'",
                                slot, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::Store { slot, val } => {
                        if !slot_ids.contains(slot) {
                            diags.emit(mir_error(format!(
                                "Instruction references unallocated SlotId {:?} in '{}'",
                                slot, func.name
                            )));
                        }
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
                    Instruction::CallIndirect {
                        dest,
                        callee_ptr,
                        args,
                        ..
                    } => {
                        if !defined_values.contains(callee_ptr) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in call indirect callee_ptr in '{}'",
                                callee_ptr, func.name
                            )));
                        }
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
                    Instruction::VtableAddr { dest, .. } => {
                        defined_values.insert(*dest);
                    }
                    Instruction::IndexAddr {
                        dest, slot, index, ..
                    } => {
                        if !slot_ids.contains(slot) {
                            diags.emit(mir_error(format!(
                                "Instruction references unallocated SlotId {:?} in '{}'",
                                slot, func.name
                            )));
                        }
                        if !defined_values.contains(index) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in index in '{}'",
                                index, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::IndexPtrAddr {
                        dest,
                        base_ptr,
                        index,
                        ..
                    } => {
                        if !defined_values.contains(base_ptr) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in base_ptr in '{}'",
                                base_ptr, func.name
                            )));
                        }
                        if !defined_values.contains(index) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in index in '{}'",
                                index, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::FieldAddr { dest, base_ptr, .. } => {
                        if !defined_values.contains(base_ptr) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in base_ptr in '{}'",
                                base_ptr, func.name
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
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
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
                    Instruction::StoreSlice { slot, ptr, len, .. } => {
                        if !slot_ids.contains(slot) {
                            diags.emit(mir_error(format!(
                                "Instruction references unallocated SlotId {:?} in '{}'",
                                slot, func.name
                            )));
                        }
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
                        slot,
                        dest_ptr,
                        dest_len,
                        ..
                    } => {
                        if !slot_ids.contains(slot) {
                            diags.emit(mir_error(format!(
                                "Instruction references unallocated SlotId {:?} in '{}'",
                                slot, func.name
                            )));
                        }
                        defined_values.insert(*dest_ptr);
                        defined_values.insert(*dest_len);
                    }
                    Instruction::Copy { dest, src, .. } => {
                        if !defined_values.contains(src) {
                            diags.emit(mir_error(format!(
                                "Value {:?} used before definition in copy in '{}'",
                                src, func.name
                            )));
                        }
                        defined_values.insert(*dest);
                    }
                    Instruction::Phi { dest, inputs, .. } => {
                        let block_preds = preds.get(&block.id);
                        for (pred_block, val) in inputs {
                            if !block_ids.contains(pred_block) {
                                diags.emit(mir_error(format!(
                                    "Phi input references non-existent block {:?} in '{}'",
                                    pred_block, func.name
                                )));
                            } else if let Some(p) = block_preds {
                                if !p.contains(pred_block) {
                                    diags.emit(mir_error(format!(
                                        "Phi input block {:?} is not a CFG predecessor of block {:?} in '{}'",
                                        pred_block, block.id, func.name
                                    )));
                                }
                            } else {
                                diags.emit(mir_error(format!(
                                    "Phi in block {:?} which has no CFG predecessors in '{}'",
                                    block.id, func.name
                                )));
                            }

                            if !all_defined_values.contains(val) {
                                diags.emit(mir_error(format!(
                                    "Phi input value {:?} is never defined in function '{}'",
                                    val, func.name
                                )));
                            }
                        }
                        defined_values.insert(*dest);
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
