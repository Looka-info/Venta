//! Instruction selection: Lowering Vanta MIR to AArch64 Machine Instructions.

use crate::encoding::{ArmCodeEmitter, ArmCond, ArmInst};
use crate::frame::ArmFrameLayout;
use crate::reg_alloc::{ArmAllocationResult, ArmLocation};
use crate::register::AArch64Reg;
use std::collections::HashMap;
use vanta_mir::{Instruction, MirBinaryOp, MirFunction, MirUnaryOp, Terminator, ValueId};
use vanta_sema::{EnumDef, StructDef};

pub struct ArmInstructionSelector<'a> {
    func: &'a MirFunction,
    frame: &'a ArmFrameLayout,
    _structs: &'a HashMap<String, StructDef>,
    _enums: &'a HashMap<String, EnumDef>,
    alloc_res: Option<&'a ArmAllocationResult>,
    epilogue_label: String,
}

impl<'a> ArmInstructionSelector<'a> {
    pub fn new(
        func: &'a MirFunction,
        frame: &'a ArmFrameLayout,
        structs: &'a HashMap<String, StructDef>,
        enums: &'a HashMap<String, EnumDef>,
        alloc_res: Option<&'a ArmAllocationResult>,
    ) -> Self {
        Self {
            func,
            frame,
            _structs: structs,
            _enums: enums,
            alloc_res,
            epilogue_label: format!("{}_epilogue", func.name),
        }
    }

    /// Selects machine instructions and emits them into the ArmCodeEmitter.
    pub fn select_instructions(&self, emitter: &mut ArmCodeEmitter) {
        // Function entry label
        emitter.encode_instruction(&ArmInst::Label(self.func.name.clone()));

        // Emit prologue
        self.frame
            .emit_prologue(emitter, self.func, self._structs, self._enums);

        // Emit blocks
        for block in &self.func.blocks {
            emitter.encode_instruction(&ArmInst::Label(format!(
                "{}_{}",
                self.func.name, block.id.0
            )));

            for inst in &block.instructions {
                self.select_instruction(inst, emitter);
            }

            if let Some(term) = &block.terminator {
                self.select_terminator(term, emitter);
            }
        }

        // Epilogue label and code
        emitter.encode_instruction(&ArmInst::Label(self.epilogue_label.clone()));
        self.frame.emit_epilogue(emitter);
    }

    fn load_val_to_reg(&self, val: ValueId, target_reg: AArch64Reg, emitter: &mut ArmCodeEmitter) {
        if let Some(alloc) = self.alloc_res {
            if let Some(&loc) = alloc.assignments.get(&val) {
                match loc {
                    ArmLocation::Reg(r) => {
                        if r != target_reg {
                            emitter.encode_instruction(&ArmInst::MovReg {
                                dest: target_reg,
                                src: r,
                            });
                        }
                        return;
                    }
                    ArmLocation::Spill(slot) => {
                        let off = self.frame.spill_offsets.get(&slot).copied().unwrap_or(16);
                        emitter.encode_instruction(&ArmInst::Ldr {
                            dest: target_reg,
                            base: AArch64Reg::X29,
                            offset: off,
                        });
                        return;
                    }
                }
            }
        }

        let off = self.frame.value_offsets.get(&val).copied().unwrap_or(16);
        emitter.encode_instruction(&ArmInst::Ldr {
            dest: target_reg,
            base: AArch64Reg::X29,
            offset: off,
        });
    }

    fn store_reg_to_val(&self, src_reg: AArch64Reg, dest: ValueId, emitter: &mut ArmCodeEmitter) {
        if let Some(alloc) = self.alloc_res {
            if let Some(&loc) = alloc.assignments.get(&dest) {
                match loc {
                    ArmLocation::Reg(r) => {
                        if r != src_reg {
                            emitter.encode_instruction(&ArmInst::MovReg {
                                dest: r,
                                src: src_reg,
                            });
                        }
                        return;
                    }
                    ArmLocation::Spill(slot) => {
                        let off = self.frame.spill_offsets.get(&slot).copied().unwrap_or(16);
                        emitter.encode_instruction(&ArmInst::Str {
                            src: src_reg,
                            base: AArch64Reg::X29,
                            offset: off,
                        });
                        return;
                    }
                }
            }
        }

        let off = self.frame.value_offsets.get(&dest).copied().unwrap_or(16);
        emitter.encode_instruction(&ArmInst::Str {
            src: src_reg,
            base: AArch64Reg::X29,
            offset: off,
        });
    }

    fn select_instruction(&self, inst: &Instruction, emitter: &mut ArmCodeEmitter) {
        match inst {
            Instruction::ConstInt { dest, val, .. } => {
                let u = *val as u64;
                let w0 = (u & 0xFFFF) as u16;
                let w1 = ((u >> 16) & 0xFFFF) as u16;
                let w2 = ((u >> 32) & 0xFFFF) as u16;
                let w3 = ((u >> 48) & 0xFFFF) as u16;

                emitter.encode_instruction(&ArmInst::Movz {
                    dest: AArch64Reg::X9,
                    imm: w0,
                    shift: 0,
                });
                if w1 != 0 || w2 != 0 || w3 != 0 {
                    emitter.encode_instruction(&ArmInst::Movk {
                        dest: AArch64Reg::X9,
                        imm: w1,
                        shift: 16,
                    });
                }
                if w2 != 0 || w3 != 0 {
                    emitter.encode_instruction(&ArmInst::Movk {
                        dest: AArch64Reg::X9,
                        imm: w2,
                        shift: 32,
                    });
                }
                if w3 != 0 {
                    emitter.encode_instruction(&ArmInst::Movk {
                        dest: AArch64Reg::X9,
                        imm: w3,
                        shift: 48,
                    });
                }
                self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
            }
            Instruction::ConstBool { dest, val } => {
                emitter.encode_instruction(&ArmInst::Movz {
                    dest: AArch64Reg::X9,
                    imm: if *val { 1 } else { 0 },
                    shift: 0,
                });
                self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
            }
            Instruction::SlotAddr { dest, slot } => {
                let off = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);
                emitter.encode_instruction(&ArmInst::AddImm {
                    dest: AArch64Reg::X9,
                    src: AArch64Reg::X29,
                    imm: off,
                });
                self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
            }
            Instruction::FieldAddr {
                dest,
                base_ptr,
                offset,
                ..
            } => {
                self.load_val_to_reg(*base_ptr, AArch64Reg::X9, emitter);
                emitter.encode_instruction(&ArmInst::AddImm {
                    dest: AArch64Reg::X9,
                    src: AArch64Reg::X9,
                    imm: *offset as u32,
                });
                self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
            }
            Instruction::Load { dest, slot, .. } => {
                let off = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);
                emitter.encode_instruction(&ArmInst::Ldr {
                    dest: AArch64Reg::X9,
                    base: AArch64Reg::X29,
                    offset: off,
                });
                self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
            }
            Instruction::Store { slot, val, .. } => {
                let off = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);
                self.load_val_to_reg(*val, AArch64Reg::X9, emitter);
                emitter.encode_instruction(&ArmInst::Str {
                    src: AArch64Reg::X9,
                    base: AArch64Reg::X29,
                    offset: off,
                });
            }
            Instruction::LoadIndirect { dest, addr, .. } => {
                self.load_val_to_reg(*addr, AArch64Reg::X10, emitter);
                emitter.encode_instruction(&ArmInst::Ldr {
                    dest: AArch64Reg::X9,
                    base: AArch64Reg::X10,
                    offset: 0,
                });
                self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
            }
            Instruction::StoreIndirect { addr, val } => {
                self.load_val_to_reg(*addr, AArch64Reg::X10, emitter);
                self.load_val_to_reg(*val, AArch64Reg::X9, emitter);
                emitter.encode_instruction(&ArmInst::Str {
                    src: AArch64Reg::X9,
                    base: AArch64Reg::X10,
                    offset: 0,
                });
            }
            Instruction::StoreIndirect32 { addr, val } => {
                self.load_val_to_reg(*addr, AArch64Reg::X10, emitter);
                self.load_val_to_reg(*val, AArch64Reg::X9, emitter);
                emitter.encode_instruction(&ArmInst::Str32 {
                    src: AArch64Reg::X9,
                    base: AArch64Reg::X10,
                    offset: 0,
                });
            }
            Instruction::Copy { dest, src, .. } => {
                self.load_val_to_reg(*src, AArch64Reg::X9, emitter);
                self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
            }
            Instruction::Binary {
                op, dest, lhs, rhs, ..
            } => {
                self.load_val_to_reg(*lhs, AArch64Reg::X9, emitter);
                self.load_val_to_reg(*rhs, AArch64Reg::X10, emitter);

                match op {
                    MirBinaryOp::Add => {
                        emitter.encode_instruction(&ArmInst::AddReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                    MirBinaryOp::Sub => {
                        emitter.encode_instruction(&ArmInst::SubReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                    MirBinaryOp::Mul => {
                        emitter.encode_instruction(&ArmInst::Mul {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                    MirBinaryOp::Div => {
                        emitter.encode_instruction(&ArmInst::Sdiv {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                    MirBinaryOp::Rem => {
                        // rem = lhs - (lhs / rhs) * rhs
                        emitter.encode_instruction(&ArmInst::Sdiv {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                        emitter.encode_instruction(&ArmInst::Mul {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X11,
                            src2: AArch64Reg::X10,
                        });
                        emitter.encode_instruction(&ArmInst::SubReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X11,
                        });
                    }
                    MirBinaryOp::BitAnd => {
                        emitter.encode_instruction(&ArmInst::AndReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                    MirBinaryOp::BitOr => {
                        emitter.encode_instruction(&ArmInst::OrrReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                    MirBinaryOp::BitXor => {
                        emitter.encode_instruction(&ArmInst::EorReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                    MirBinaryOp::Eq => {
                        emitter.encode_instruction(&ArmInst::CmpReg {
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                        emitter.encode_instruction(&ArmInst::Cset {
                            dest: AArch64Reg::X11,
                            cond: ArmCond::Eq,
                        });
                    }
                    MirBinaryOp::NotEq => {
                        emitter.encode_instruction(&ArmInst::CmpReg {
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                        emitter.encode_instruction(&ArmInst::Cset {
                            dest: AArch64Reg::X11,
                            cond: ArmCond::Ne,
                        });
                    }
                    MirBinaryOp::Lt => {
                        emitter.encode_instruction(&ArmInst::CmpReg {
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                        emitter.encode_instruction(&ArmInst::Cset {
                            dest: AArch64Reg::X11,
                            cond: ArmCond::Lt,
                        });
                    }
                    MirBinaryOp::LtEq => {
                        emitter.encode_instruction(&ArmInst::CmpReg {
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                        emitter.encode_instruction(&ArmInst::Cset {
                            dest: AArch64Reg::X11,
                            cond: ArmCond::Le,
                        });
                    }
                    MirBinaryOp::Gt => {
                        emitter.encode_instruction(&ArmInst::CmpReg {
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                        emitter.encode_instruction(&ArmInst::Cset {
                            dest: AArch64Reg::X11,
                            cond: ArmCond::Gt,
                        });
                    }
                    MirBinaryOp::GtEq => {
                        emitter.encode_instruction(&ArmInst::CmpReg {
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                        emitter.encode_instruction(&ArmInst::Cset {
                            dest: AArch64Reg::X11,
                            cond: ArmCond::Ge,
                        });
                    }
                    _ => {
                        emitter.encode_instruction(&ArmInst::AddReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                }
                self.store_reg_to_val(AArch64Reg::X11, *dest, emitter);
            }
            Instruction::Unary {
                op, dest, operand, ..
            } => {
                self.load_val_to_reg(*operand, AArch64Reg::X9, emitter);
                match op {
                    MirUnaryOp::Neg => {
                        emitter.encode_instruction(&ArmInst::SubReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::XZR,
                            src2: AArch64Reg::X9,
                        });
                    }
                    MirUnaryOp::Not => {
                        emitter.encode_instruction(&ArmInst::Movz {
                            dest: AArch64Reg::X10,
                            imm: 1,
                            shift: 0,
                        });
                        emitter.encode_instruction(&ArmInst::EorReg {
                            dest: AArch64Reg::X11,
                            src1: AArch64Reg::X9,
                            src2: AArch64Reg::X10,
                        });
                    }
                }
                self.store_reg_to_val(AArch64Reg::X11, *dest, emitter);
            }
            Instruction::IndexAddr {
                dest,
                slot,
                index,
                elem_size,
                ..
            } => {
                let off = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);
                emitter.encode_instruction(&ArmInst::AddImm {
                    dest: AArch64Reg::X9,
                    src: AArch64Reg::X29,
                    imm: off,
                });
                self.load_val_to_reg(*index, AArch64Reg::X10, emitter);
                emitter.encode_instruction(&ArmInst::Movz {
                    dest: AArch64Reg::X11,
                    imm: *elem_size as u16,
                    shift: 0,
                });
                emitter.encode_instruction(&ArmInst::Mul {
                    dest: AArch64Reg::X10,
                    src1: AArch64Reg::X10,
                    src2: AArch64Reg::X11,
                });
                emitter.encode_instruction(&ArmInst::AddReg {
                    dest: AArch64Reg::X11,
                    src1: AArch64Reg::X9,
                    src2: AArch64Reg::X10,
                });
                self.store_reg_to_val(AArch64Reg::X11, *dest, emitter);
            }
            Instruction::IndexPtrAddr {
                dest,
                base_ptr,
                index,
                elem_size,
                ..
            } => {
                self.load_val_to_reg(*base_ptr, AArch64Reg::X9, emitter);
                self.load_val_to_reg(*index, AArch64Reg::X10, emitter);
                emitter.encode_instruction(&ArmInst::Movz {
                    dest: AArch64Reg::X11,
                    imm: *elem_size as u16,
                    shift: 0,
                });
                emitter.encode_instruction(&ArmInst::Mul {
                    dest: AArch64Reg::X10,
                    src1: AArch64Reg::X10,
                    src2: AArch64Reg::X11,
                });
                emitter.encode_instruction(&ArmInst::AddReg {
                    dest: AArch64Reg::X11,
                    src1: AArch64Reg::X9,
                    src2: AArch64Reg::X10,
                });
                self.store_reg_to_val(AArch64Reg::X11, *dest, emitter);
            }
            Instruction::Call {
                dest, callee, args, ..
            } => {
                for (i, arg) in args.iter().enumerate() {
                    if i < AArch64Reg::ARG_REGS.len() {
                        let reg = AArch64Reg::ARG_REGS[i];
                        self.load_val_to_reg(*arg, reg, emitter);
                    }
                }

                let callee_symbol = if !callee.starts_with('_') {
                    format!("_{}", callee)
                } else {
                    callee.clone()
                };

                emitter.encode_instruction(&ArmInst::Bl {
                    callee: callee_symbol,
                });

                if let Some(d) = dest {
                    self.store_reg_to_val(AArch64Reg::X0, *d, emitter);
                }
            }
            Instruction::CallIndirect {
                dest,
                callee_ptr,
                args,
                ..
            } => {
                for (i, arg) in args.iter().enumerate() {
                    if i < AArch64Reg::ARG_REGS.len() {
                        let reg = AArch64Reg::ARG_REGS[i];
                        self.load_val_to_reg(*arg, reg, emitter);
                    }
                }
                self.load_val_to_reg(*callee_ptr, AArch64Reg::X16, emitter);
                emitter.encode_instruction(&ArmInst::Blr {
                    callee: AArch64Reg::X16,
                });

                if let Some(d) = dest {
                    self.store_reg_to_val(AArch64Reg::X0, *d, emitter);
                }
            }
            Instruction::Phi { dest, inputs, .. } => {
                if let Some((_, first_val)) = inputs.first() {
                    self.load_val_to_reg(*first_val, AArch64Reg::X9, emitter);
                    self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
                }
            }
            Instruction::StrSlice {
                dest_ptr,
                dest_len,
                symbol,
                len,
            } => {
                let sym_name = format!("_{}", symbol);
                emitter.encode_instruction(&ArmInst::Adrp {
                    dest: AArch64Reg::X9,
                    symbol: sym_name,
                });
                self.store_reg_to_val(AArch64Reg::X9, *dest_ptr, emitter);
                emitter.encode_instruction(&ArmInst::Movz {
                    dest: AArch64Reg::X10,
                    imm: *len as u16,
                    shift: 0,
                });
                self.store_reg_to_val(AArch64Reg::X10, *dest_len, emitter);
            }
            Instruction::LoadSlice {
                dest_ptr,
                dest_len,
                slot,
            } => {
                let off = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);
                emitter.encode_instruction(&ArmInst::Ldr {
                    dest: AArch64Reg::X9,
                    base: AArch64Reg::X29,
                    offset: off,
                });
                emitter.encode_instruction(&ArmInst::Ldr {
                    dest: AArch64Reg::X10,
                    base: AArch64Reg::X29,
                    offset: off + 8,
                });
                self.store_reg_to_val(AArch64Reg::X9, *dest_ptr, emitter);
                self.store_reg_to_val(AArch64Reg::X10, *dest_len, emitter);
            }
            Instruction::StoreSlice { slot, ptr, len } => {
                let off = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);
                self.load_val_to_reg(*ptr, AArch64Reg::X9, emitter);
                self.load_val_to_reg(*len, AArch64Reg::X10, emitter);
                emitter.encode_instruction(&ArmInst::Str {
                    src: AArch64Reg::X9,
                    base: AArch64Reg::X29,
                    offset: off,
                });
                emitter.encode_instruction(&ArmInst::Str {
                    src: AArch64Reg::X10,
                    base: AArch64Reg::X29,
                    offset: off + 8,
                });
            }
            Instruction::VtableAddr { dest, symbol } => {
                let sym_name = format!("_{}", symbol);
                emitter.encode_instruction(&ArmInst::Adrp {
                    dest: AArch64Reg::X9,
                    symbol: sym_name,
                });
                self.store_reg_to_val(AArch64Reg::X9, *dest, emitter);
            }
        }
    }

    fn select_terminator(&self, term: &Terminator, emitter: &mut ArmCodeEmitter) {
        match term {
            Terminator::Return(opt_val) => {
                if let Some(val) = opt_val {
                    self.load_val_to_reg(*val, AArch64Reg::X0, emitter);
                }
                emitter.encode_instruction(&ArmInst::B {
                    label: self.epilogue_label.clone(),
                });
            }
            Terminator::Branch(target) => {
                emitter.encode_instruction(&ArmInst::B {
                    label: format!("{}_{}", self.func.name, target.0),
                });
            }
            Terminator::CondBranch {
                cond,
                then_block,
                else_block,
            } => {
                self.load_val_to_reg(*cond, AArch64Reg::X9, emitter);
                emitter.encode_instruction(&ArmInst::CmpImm {
                    src: AArch64Reg::X9,
                    imm: 0,
                });
                emitter.encode_instruction(&ArmInst::BCond {
                    cond: ArmCond::Ne,
                    label: format!("{}_{}", self.func.name, then_block.0),
                });
                emitter.encode_instruction(&ArmInst::B {
                    label: format!("{}_{}", self.func.name, else_block.0),
                });
            }
            _ => {}
        }
    }
}
