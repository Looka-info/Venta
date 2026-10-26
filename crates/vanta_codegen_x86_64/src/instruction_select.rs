//! Instruction selection: Lowering Vanta MIR to x86-64 Machine Instructions.

use crate::frame::FrameLayout;
use crate::machine_ir::{ConditionCode, MachineInst, Operand};
use crate::register::Reg;
use vanta_mir::{Instruction, MirBinaryOp, MirFunction, MirUnaryOp, Terminator};

pub struct InstructionSelector<'a> {
    func: &'a MirFunction,
    frame: &'a FrameLayout,
    epilogue_label: String,
}

impl<'a> InstructionSelector<'a> {
    pub fn new(func: &'a MirFunction, frame: &'a FrameLayout) -> Self {
        Self {
            func,
            frame,
            epilogue_label: format!(".L_{}_epilogue", func.name),
        }
    }

    pub fn select_instructions(&self) -> Vec<MachineInst> {
        let mut insts = Vec::new();

        // 1. Function Prologue
        self.frame.emit_prologue(&mut insts, self.func);

        // 2. Body Blocks
        for (idx, block) in self.func.blocks.iter().enumerate() {
            // Emit block label (except entry if fallthrough, but emitting for all ensures jumps work)
            let label_name = format!(".L_{}_bb{}", self.func.name, block.id.0);
            if idx > 0 {
                insts.push(MachineInst::Label(label_name));
            }

            for inst in &block.instructions {
                self.select_instruction(inst, &mut insts);
            }

            if let Some(term) = &block.terminator {
                self.select_terminator(term, &mut insts);
            }
        }

        // 3. Epilogue Label and Instructions
        insts.push(MachineInst::Label(self.epilogue_label.clone()));
        self.frame.emit_epilogue(&mut insts);

        insts
    }

    fn select_instruction(&self, inst: &Instruction, out: &mut Vec<MachineInst>) {
        match inst {
            Instruction::ConstInt { dest, val, .. } => {
                let dest_op = self.frame.get_value_op(*dest);
                // mov QWORD PTR [dest], val (or via r10 if imm is 64-bit)
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: Operand::Imm(*val),
                });
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::ConstBool { dest, val } => {
                let dest_op = self.frame.get_value_op(*dest);
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: Operand::Imm(if *val { 1 } else { 0 }),
                });
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::SlotAddr { dest, slot } => {
                let slot_op = self.frame.get_slot_op(*slot);
                let dest_op = self.frame.get_value_op(*dest);
                out.push(MachineInst::Lea {
                    dest: Reg::R10,
                    src: slot_op,
                });
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::Load { dest, slot, .. } => {
                let slot_op = self.frame.get_slot_op(*slot);
                let dest_op = self.frame.get_value_op(*dest);
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: slot_op,
                });
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::Store { slot, val } => {
                let val_op = self.frame.get_value_op(*val);
                let slot_op = self.frame.get_slot_op(*slot);
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: val_op,
                });
                out.push(MachineInst::Mov {
                    dest: slot_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::Binary {
                dest, op, lhs, rhs, ..
            } => {
                let lhs_op = self.frame.get_value_op(*lhs);
                let rhs_op = self.frame.get_value_op(*rhs);
                let dest_op = self.frame.get_value_op(*dest);

                match op {
                    MirBinaryOp::Add => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Add {
                            dest: Operand::Reg(Reg::R10),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                    MirBinaryOp::Sub => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Sub {
                            dest: Operand::Reg(Reg::R10),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                    MirBinaryOp::Mul => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Imul {
                            dest: Reg::R10,
                            src: rhs_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                    MirBinaryOp::Div => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::Rax),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Cqo);
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R11),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Idiv(Reg::R11));
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::Rax),
                        });
                    }
                    MirBinaryOp::Rem => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::Rax),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Cqo);
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R11),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Idiv(Reg::R11));
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::Rdx),
                        });
                    }
                    MirBinaryOp::Eq
                    | MirBinaryOp::NotEq
                    | MirBinaryOp::Lt
                    | MirBinaryOp::LtEq
                    | MirBinaryOp::Gt
                    | MirBinaryOp::GtEq => {
                        let cond = match op {
                            MirBinaryOp::Eq => ConditionCode::Equal,
                            MirBinaryOp::NotEq => ConditionCode::NotEqual,
                            MirBinaryOp::Lt => ConditionCode::Less,
                            MirBinaryOp::LtEq => ConditionCode::LessEqual,
                            MirBinaryOp::Gt => ConditionCode::Greater,
                            MirBinaryOp::GtEq => ConditionCode::GreaterEqual,
                            _ => unreachable!(),
                        };
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Cmp {
                            lhs: Operand::Reg(Reg::R10),
                            rhs: rhs_op,
                        });
                        out.push(MachineInst::Setcc {
                            cond,
                            dest: Reg::Rax,
                        });
                        out.push(MachineInst::Movzx {
                            dest: Reg::R10,
                            src: Reg::Rax,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                    MirBinaryOp::BitAnd | MirBinaryOp::And => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::And {
                            dest: Operand::Reg(Reg::R10),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                    MirBinaryOp::BitOr | MirBinaryOp::Or => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Or {
                            dest: Operand::Reg(Reg::R10),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                    MirBinaryOp::BitXor => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Xor {
                            dest: Operand::Reg(Reg::R10),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                    MirBinaryOp::Shl => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::Rcx),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Shl {
                            dest: Reg::R10,
                            count: Operand::Reg(Reg::Rcx),
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                    MirBinaryOp::Shr => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: lhs_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::Rcx),
                            src: rhs_op,
                        });
                        out.push(MachineInst::Shr {
                            dest: Reg::R10,
                            count: Operand::Reg(Reg::Rcx),
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::Unary {
                dest, op, operand, ..
            } => {
                let op_val = self.frame.get_value_op(*operand);
                let dest_op = self.frame.get_value_op(*dest);
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: op_val,
                });
                match op {
                    MirUnaryOp::Neg => {
                        out.push(MachineInst::Neg(Reg::R10));
                    }
                    MirUnaryOp::Not => {
                        out.push(MachineInst::Not(Reg::R10));
                    }
                }
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::Call {
                dest, callee, args, ..
            } => {
                // Pass first 6 args in registers
                for (idx, arg) in args.iter().enumerate() {
                    if idx < Reg::ARG_REGS.len() {
                        let arg_op = self.frame.get_value_op(*arg);
                        let target_reg = Reg::ARG_REGS[idx];
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(target_reg),
                            src: arg_op,
                        });
                    }
                }

                out.push(MachineInst::Call(callee.clone()));

                if let Some(d) = dest {
                    let dest_op = self.frame.get_value_op(*d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Reg(Reg::Rax),
                    });
                }
            }
            Instruction::IndexAddr {
                dest,
                slot,
                index,
                elem_size,
            } => {
                let slot_op = self.frame.get_slot_op(*slot);
                let index_op = self.frame.get_value_op(*index);
                let dest_op = self.frame.get_value_op(*dest);

                out.push(MachineInst::Lea {
                    dest: Reg::R10,
                    src: slot_op,
                });
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R11),
                    src: index_op,
                });
                out.push(MachineInst::Imul {
                    dest: Reg::R11,
                    src: Operand::Imm(*elem_size as i64),
                });
                out.push(MachineInst::Add {
                    dest: Operand::Reg(Reg::R10),
                    src: Operand::Reg(Reg::R11),
                });
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::IndexPtrAddr {
                dest,
                base_ptr,
                index,
                elem_size,
            } => {
                let base_op = self.frame.get_value_op(*base_ptr);
                let index_op = self.frame.get_value_op(*index);
                let dest_op = self.frame.get_value_op(*dest);

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: base_op,
                });
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R11),
                    src: index_op,
                });
                out.push(MachineInst::Imul {
                    dest: Reg::R11,
                    src: Operand::Imm(*elem_size as i64),
                });
                out.push(MachineInst::Add {
                    dest: Operand::Reg(Reg::R10),
                    src: Operand::Reg(Reg::R11),
                });
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::FieldAddr {
                dest,
                base_ptr,
                offset,
            } => {
                let base_op = self.frame.get_value_op(*base_ptr);
                let dest_op = self.frame.get_value_op(*dest);

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: base_op,
                });
                if *offset > 0 {
                    out.push(MachineInst::Add {
                        dest: Operand::Reg(Reg::R10),
                        src: Operand::Imm(*offset as i64),
                    });
                }
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
            Instruction::LoadIndirect { dest, addr, ty } => {
                let addr_op = self.frame.get_value_op(*addr);
                let dest_op = self.frame.get_value_op(*dest);

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: addr_op,
                });
                if *ty == vanta_sema::Type::U8 || *ty == vanta_sema::Type::I8 {
                    out.push(MachineInst::MovzxMem {
                        dest: Reg::R11,
                        base: Reg::R10,
                        offset: 0,
                    });
                } else {
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: Operand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                    });
                }
                out.push(MachineInst::Mov {
                    dest: dest_op,
                    src: Operand::Reg(Reg::R11),
                });
            }
            Instruction::StoreIndirect { addr, val } => {
                let addr_op = self.frame.get_value_op(*addr);
                let val_op = self.frame.get_value_op(*val);

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: addr_op,
                });
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R11),
                    src: val_op,
                });
                out.push(MachineInst::Mov {
                    dest: Operand::Mem {
                        base: Reg::R10,
                        offset: 0,
                    },
                    src: Operand::Reg(Reg::R11),
                });
            }
            Instruction::StrSlice {
                dest_ptr,
                dest_len,
                symbol,
                len,
            } => {
                let dest_ptr_op = self.frame.get_value_op(*dest_ptr);
                let dest_len_op = self.frame.get_value_op(*dest_len);

                out.push(MachineInst::LeaRip {
                    dest: Reg::Rax,
                    symbol: symbol.clone(),
                });
                out.push(MachineInst::Mov {
                    dest: dest_ptr_op,
                    src: Operand::Reg(Reg::Rax),
                });
                out.push(MachineInst::Mov {
                    dest: dest_len_op,
                    src: Operand::Imm(*len as i64),
                });
            }
            Instruction::StoreSlice { slot, ptr, len } => {
                let ptr_op = self.frame.get_value_op(*ptr);
                let len_op = self.frame.get_value_op(*len);
                let slot_offset = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: ptr_op,
                });
                out.push(MachineInst::Mov {
                    dest: Operand::Mem {
                        base: Reg::Rbp,
                        offset: -slot_offset,
                    },
                    src: Operand::Reg(Reg::R10),
                });

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R11),
                    src: len_op,
                });
                out.push(MachineInst::Mov {
                    dest: Operand::Mem {
                        base: Reg::Rbp,
                        offset: -slot_offset + 8,
                    },
                    src: Operand::Reg(Reg::R11),
                });
            }
            Instruction::LoadSlice {
                dest_ptr,
                dest_len,
                slot,
            } => {
                let dest_ptr_op = self.frame.get_value_op(*dest_ptr);
                let dest_len_op = self.frame.get_value_op(*dest_len);
                let slot_offset = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: Operand::Mem {
                        base: Reg::Rbp,
                        offset: -slot_offset,
                    },
                });
                out.push(MachineInst::Mov {
                    dest: dest_ptr_op,
                    src: Operand::Reg(Reg::R10),
                });

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R11),
                    src: Operand::Mem {
                        base: Reg::Rbp,
                        offset: -slot_offset + 8,
                    },
                });
                out.push(MachineInst::Mov {
                    dest: dest_len_op,
                    src: Operand::Reg(Reg::R11),
                });
            }
        }
    }

    fn select_terminator(&self, term: &Terminator, out: &mut Vec<MachineInst>) {
        match term {
            Terminator::Branch(target) => {
                let target_label = format!(".L_{}_bb{}", self.func.name, target.0);
                out.push(MachineInst::Jmp(target_label));
            }
            Terminator::CondBranch {
                cond,
                then_block,
                else_block,
            } => {
                let cond_op = self.frame.get_value_op(*cond);
                let then_label = format!(".L_{}_bb{}", self.func.name, then_block.0);
                let else_label = format!(".L_{}_bb{}", self.func.name, else_block.0);

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: cond_op,
                });
                out.push(MachineInst::Test {
                    lhs: Reg::R10,
                    rhs: Reg::R10,
                });
                out.push(MachineInst::Jcc {
                    cond: ConditionCode::NotEqual,
                    target: then_label,
                });
                out.push(MachineInst::Jmp(else_label));
            }
            Terminator::Return(val) => {
                if let Some(v) = val {
                    let val_op = self.frame.get_value_op(*v);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::Rax),
                        src: val_op,
                    });
                }
                out.push(MachineInst::Jmp(self.epilogue_label.clone()));
            }
            Terminator::Unreachable => {}
        }
    }
}
