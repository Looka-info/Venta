//! Instruction selection: Lowering Vanta MIR to x86-64 Machine Instructions.

use crate::abi::classify_type;
use crate::frame::FrameLayout;
use crate::machine_ir::{ConditionCode, MachineInst, Operand};
use crate::reg_alloc::{AllocationResult, Location};
use crate::register::Reg;
use std::collections::HashMap;
use vanta_mir::{BlockId, Instruction, MirBinaryOp, MirFunction, MirUnaryOp, Terminator, ValueId};
use vanta_sema::{EnumDef, StructDef};

pub struct InstructionSelector<'a> {
    func: &'a MirFunction,
    frame: &'a FrameLayout,
    structs: &'a HashMap<String, StructDef>,
    enums: &'a HashMap<String, EnumDef>,
    alloc_res: Option<&'a AllocationResult>,
    epilogue_label: String,
    enable_tail_calls: bool,
    enable_block_layout: bool,
    stack_ptrs: std::collections::HashSet<ValueId>,
}

impl<'a> InstructionSelector<'a> {
    pub fn new(
        func: &'a MirFunction,
        frame: &'a FrameLayout,
        structs: &'a HashMap<String, StructDef>,
        enums: &'a HashMap<String, EnumDef>,
        alloc_res: Option<&'a AllocationResult>,
    ) -> Self {
        Self::with_options(func, frame, structs, enums, alloc_res, true, true)
    }

    pub fn with_options(
        func: &'a MirFunction,
        frame: &'a FrameLayout,
        structs: &'a HashMap<String, StructDef>,
        enums: &'a HashMap<String, EnumDef>,
        alloc_res: Option<&'a AllocationResult>,
        enable_tail_calls: bool,
        enable_block_layout: bool,
    ) -> Self {
        let mut stack_ptrs = std::collections::HashSet::new();
        let mut changed = true;
        while changed {
            changed = false;
            for b in &func.blocks {
                for inst in &b.instructions {
                    match inst {
                        Instruction::SlotAddr { dest, .. } => {
                            if stack_ptrs.insert(*dest) {
                                changed = true;
                            }
                        }
                        Instruction::IndexAddr { dest, .. } => {
                            if stack_ptrs.insert(*dest) {
                                changed = true;
                            }
                        }
                        Instruction::FieldAddr { dest, base_ptr, .. }
                        | Instruction::IndexPtrAddr { dest, base_ptr, .. } => {
                            if stack_ptrs.contains(base_ptr) && stack_ptrs.insert(*dest) {
                                changed = true;
                            }
                        }
                        Instruction::Copy { dest, src, .. } => {
                            if stack_ptrs.contains(src) && stack_ptrs.insert(*dest) {
                                changed = true;
                            }
                        }
                        Instruction::Phi { dest, inputs, .. } => {
                            if inputs.iter().any(|(_, v)| stack_ptrs.contains(v)) && stack_ptrs.insert(*dest) {
                                changed = true;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Self {
            func,
            frame,
            structs,
            enums,
            alloc_res,
            epilogue_label: format!(".L_{}_epilogue", func.name),
            enable_tail_calls,
            enable_block_layout,
            stack_ptrs,
        }
    }

    pub fn select_instructions(&self) -> Vec<MachineInst> {
        let mut insts = Vec::new();

        // 1. Function Prologue
        self.frame.emit_prologue(&mut insts, self.func);

        if self.func.blocks.is_empty() {
            insts.push(MachineInst::Label(self.epilogue_label.clone()));
            self.frame.emit_epilogue(&mut insts);
            return insts;
        }

        // Determine basic-block layout ordering
        let layout: Vec<usize> = if self.enable_block_layout {
            let mut order = Vec::with_capacity(self.func.blocks.len());
            let mut placed = std::collections::HashSet::new();

            order.push(0);
            placed.insert(self.func.blocks[0].id);
            let mut curr_idx = 0;

            loop {
                let curr_b = &self.func.blocks[curr_idx];
                let mut next_pick = None;

                if let Some(term) = &curr_b.terminator {
                    match term {
                        Terminator::Branch(target) => {
                            if !placed.contains(target) {
                                if let Some(pos) = self.func.blocks.iter().position(|b| b.id == *target) {
                                    next_pick = Some(pos);
                                }
                            }
                        }
                        Terminator::CondBranch { then_block, else_block, .. } => {
                            // Prioritize unvisited successor: prefer else_block (standard fall-through)
                            if !placed.contains(else_block) {
                                if let Some(pos) = self.func.blocks.iter().position(|b| b.id == *else_block) {
                                    next_pick = Some(pos);
                                }
                            } else if !placed.contains(then_block) {
                                if let Some(pos) = self.func.blocks.iter().position(|b| b.id == *then_block) {
                                    next_pick = Some(pos);
                                }
                            }
                        }
                        Terminator::Return(_) | Terminator::Unreachable => {}
                    }
                }

                if let Some(next_idx) = next_pick {
                    order.push(next_idx);
                    placed.insert(self.func.blocks[next_idx].id);
                    curr_idx = next_idx;
                } else {
                    if let Some(unplaced_idx) = (0..self.func.blocks.len()).find(|&i| !placed.contains(&self.func.blocks[i].id)) {
                        order.push(unplaced_idx);
                        placed.insert(self.func.blocks[unplaced_idx].id);
                        curr_idx = unplaced_idx;
                    } else {
                        break;
                    }
                }
            }
            order
        } else {
            (0..self.func.blocks.len()).collect()
        };

        // 2. Body Blocks in layout order
        for (order_pos, &b_idx) in layout.iter().enumerate() {
            let block = &self.func.blocks[b_idx];
            let label_name = format!(".L_{}_bb{}", self.func.name, block.id.0);
            // Always emit label so any jumps can target this block
            insts.push(MachineInst::Label(label_name));

            let next_block_id = layout.get(order_pos + 1).map(|&idx| self.func.blocks[idx].id);
            let is_last_before_epilogue = order_pos + 1 == layout.len();

            let num_instructions = block.instructions.len();
            for (inst_idx, inst) in block.instructions.iter().enumerate() {
                let is_last_inst = inst_idx + 1 == num_instructions;
                self.select_instruction(inst, block, is_last_inst, &mut insts);
            }

            if let Some(term) = &block.terminator {
                self.select_terminator(block.id, term, next_block_id, is_last_before_epilogue, &mut insts);
            }
        }

        // 3. Epilogue Label and Instructions
        insts.push(MachineInst::Label(self.epilogue_label.clone()));
        self.frame.emit_epilogue(&mut insts);

        insts
    }

    fn get_val_op(&self, val: ValueId) -> Operand {
        if let Some(alloc_res) = self.alloc_res {
            match alloc_res.assignments.get(&val) {
                Some(Location::Reg(r)) => Operand::Reg(*r),
                Some(Location::Spill(spill_idx)) => {
                    let offset = self
                        .frame
                        .spill_offsets
                        .get(spill_idx)
                        .copied()
                        .unwrap_or(8);
                    Operand::Mem {
                        base: Reg::Rbp,
                        offset: -offset,
                    }
                }
                None => self.frame.get_value_op(val),
            }
        } else {
            self.frame.get_value_op(val)
        }
    }

    fn select_instruction(
        &self,
        inst: &Instruction,
        block: &vanta_mir::BasicBlock,
        is_last_inst: bool,
        out: &mut Vec<MachineInst>,
    ) {
        match inst {
            Instruction::ConstInt { dest, val, .. } => {
                let dest_op = self.get_val_op(*dest);
                match dest_op {
                    Operand::Reg(dst) => {
                        if *val == 0 {
                            out.push(MachineInst::Xor {
                                dest: Operand::Reg(dst),
                                src: Operand::Reg(dst),
                            });
                        } else {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(dst),
                                src: Operand::Imm(*val),
                            });
                        }
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: Operand::Imm(*val),
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::ConstBool { dest, val } => {
                let dest_op = self.get_val_op(*dest);
                match dest_op {
                    Operand::Reg(dst) => {
                        if !*val {
                            out.push(MachineInst::Xor {
                                dest: Operand::Reg(dst),
                                src: Operand::Reg(dst),
                            });
                        } else {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(dst),
                                src: Operand::Imm(1),
                            });
                        }
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: Operand::Imm(if *val { 1 } else { 0 }),
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::SlotAddr { dest, slot } => {
                let slot_op = self.frame.get_slot_op(*slot);
                let dest_op = self.get_val_op(*dest);
                match dest_op {
                    Operand::Reg(dst) => {
                        out.push(MachineInst::Lea {
                            dest: dst,
                            src: slot_op,
                        });
                    }
                    _ => {
                        out.push(MachineInst::Lea {
                            dest: Reg::R10,
                            src: slot_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::Load { dest, slot, .. } => {
                let slot_op = self.frame.get_slot_op(*slot);
                let dest_op = self.get_val_op(*dest);
                match dest_op {
                    Operand::Reg(dst) => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(dst),
                            src: slot_op,
                        });
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: slot_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::Store { slot, val } => {
                let val_op = self.get_val_op(*val);
                let slot_op = self.frame.get_slot_op(*slot);
                match val_op {
                    Operand::Reg(src_r) => {
                        out.push(MachineInst::Mov {
                            dest: slot_op,
                            src: Operand::Reg(src_r),
                        });
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: val_op,
                        });
                        out.push(MachineInst::Mov {
                            dest: slot_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::Binary {
                dest,
                op,
                lhs,
                rhs,
                ..
            } => {
                let lhs_op = self.get_val_op(*lhs);
                let rhs_op = self.get_val_op(*rhs);
                let dest_op = self.get_val_op(*dest);

                match op {
                    MirBinaryOp::Add
                    | MirBinaryOp::Sub
                    | MirBinaryOp::Mul
                    | MirBinaryOp::BitAnd
                    | MirBinaryOp::And
                    | MirBinaryOp::BitOr
                    | MirBinaryOp::Or
                    | MirBinaryOp::BitXor => match dest_op {
                        Operand::Reg(dst) => {
                            if lhs_op != Operand::Reg(dst) {
                                out.push(MachineInst::Mov {
                                    dest: Operand::Reg(dst),
                                    src: lhs_op,
                                });
                            }
                            match op {
                                MirBinaryOp::Add => out.push(MachineInst::Add {
                                    dest: Operand::Reg(dst),
                                    src: rhs_op,
                                }),
                                MirBinaryOp::Sub => out.push(MachineInst::Sub {
                                    dest: Operand::Reg(dst),
                                    src: rhs_op,
                                }),
                                MirBinaryOp::Mul => out.push(MachineInst::Imul {
                                    dest: dst,
                                    src: rhs_op,
                                }),
                                MirBinaryOp::BitAnd | MirBinaryOp::And => out.push(MachineInst::And {
                                    dest: Operand::Reg(dst),
                                    src: rhs_op,
                                }),
                                MirBinaryOp::BitOr | MirBinaryOp::Or => out.push(MachineInst::Or {
                                    dest: Operand::Reg(dst),
                                    src: rhs_op,
                                }),
                                MirBinaryOp::BitXor => out.push(MachineInst::Xor {
                                    dest: Operand::Reg(dst),
                                    src: rhs_op,
                                }),
                                _ => unreachable!(),
                            }
                        }
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: lhs_op,
                            });
                            match op {
                                MirBinaryOp::Add => out.push(MachineInst::Add {
                                    dest: Operand::Reg(Reg::R10),
                                    src: rhs_op,
                                }),
                                MirBinaryOp::Sub => out.push(MachineInst::Sub {
                                    dest: Operand::Reg(Reg::R10),
                                    src: rhs_op,
                                }),
                                MirBinaryOp::Mul => out.push(MachineInst::Imul {
                                    dest: Reg::R10,
                                    src: rhs_op,
                                }),
                                MirBinaryOp::BitAnd | MirBinaryOp::And => out.push(MachineInst::And {
                                    dest: Operand::Reg(Reg::R10),
                                    src: rhs_op,
                                }),
                                MirBinaryOp::BitOr | MirBinaryOp::Or => out.push(MachineInst::Or {
                                    dest: Operand::Reg(Reg::R10),
                                    src: rhs_op,
                                }),
                                MirBinaryOp::BitXor => out.push(MachineInst::Xor {
                                    dest: Operand::Reg(Reg::R10),
                                    src: rhs_op,
                                }),
                                _ => unreachable!(),
                            }
                            out.push(MachineInst::Mov {
                                dest: dest_op,
                                src: Operand::Reg(Reg::R10),
                            });
                        }
                    },
                    MirBinaryOp::Div | MirBinaryOp::Rem => {
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
                        let res_reg = if matches!(op, MirBinaryOp::Div) {
                            Reg::Rax
                        } else {
                            Reg::Rdx
                        };
                        if dest_op != Operand::Reg(res_reg) {
                            out.push(MachineInst::Mov {
                                dest: dest_op,
                                src: Operand::Reg(res_reg),
                            });
                        }
                    }
                    MirBinaryOp::Shl | MirBinaryOp::Shr => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::Rcx),
                            src: rhs_op,
                        });
                        match dest_op {
                            Operand::Reg(dst) => {
                                if lhs_op != Operand::Reg(dst) {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(dst),
                                        src: lhs_op,
                                    });
                                }
                                if matches!(op, MirBinaryOp::Shl) {
                                    out.push(MachineInst::Shl {
                                        dest: dst,
                                        count: Operand::Reg(Reg::Rcx),
                                    });
                                } else {
                                    out.push(MachineInst::Shr {
                                        dest: dst,
                                        count: Operand::Reg(Reg::Rcx),
                                    });
                                }
                            }
                            _ => {
                                out.push(MachineInst::Mov {
                                    dest: Operand::Reg(Reg::R10),
                                    src: lhs_op,
                                });
                                if matches!(op, MirBinaryOp::Shl) {
                                    out.push(MachineInst::Shl {
                                        dest: Reg::R10,
                                        count: Operand::Reg(Reg::Rcx),
                                    });
                                } else {
                                    out.push(MachineInst::Shr {
                                        dest: Reg::R10,
                                        count: Operand::Reg(Reg::Rcx),
                                    });
                                }
                                out.push(MachineInst::Mov {
                                    dest: dest_op,
                                    src: Operand::Reg(Reg::R10),
                                });
                            }
                        }
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
                        let cmp_lhs = match lhs_op {
                            Operand::Reg(r) => r,
                            _ => {
                                out.push(MachineInst::Mov {
                                    dest: Operand::Reg(Reg::R10),
                                    src: lhs_op,
                                });
                                Reg::R10
                            }
                        };
                        out.push(MachineInst::Cmp {
                            lhs: Operand::Reg(cmp_lhs),
                            rhs: rhs_op,
                        });
                        out.push(MachineInst::Setcc {
                            cond,
                            dest: Reg::Rax,
                        });
                        match dest_op {
                            Operand::Reg(dst) => {
                                out.push(MachineInst::Movzx {
                                    dest: dst,
                                    src: Reg::Rax,
                                });
                            }
                            _ => {
                                out.push(MachineInst::Movzx {
                                    dest: Reg::R10,
                                    src: Reg::Rax,
                                });
                                out.push(MachineInst::Mov {
                                    dest: dest_op,
                                    src: Operand::Reg(Reg::R10),
                                });
                            }
                        }
                    }
                }
            }
            Instruction::Unary {
                dest, op, operand, ..
            } => {
                let op_val = self.get_val_op(*operand);
                let dest_op = self.get_val_op(*dest);
                match dest_op {
                    Operand::Reg(dst) => {
                        if op_val != Operand::Reg(dst) {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(dst),
                                src: op_val,
                            });
                        }
                        match op {
                            MirUnaryOp::Neg => out.push(MachineInst::Neg(dst)),
                            MirUnaryOp::Not => out.push(MachineInst::Not(dst)),
                        }
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: op_val,
                        });
                        match op {
                            MirUnaryOp::Neg => out.push(MachineInst::Neg(Reg::R10)),
                            MirUnaryOp::Not => out.push(MachineInst::Not(Reg::R10)),
                        }
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::Call {
                dest,
                callee,
                args,
                ret_type,
            } => {
                let ret_class = classify_type(ret_type, self.structs, self.enums);
                let caller_ret_class = classify_type(&self.func.ret_type, self.structs, self.enums);

                // Sibling Tail-Call candidacy check:
                let is_tail_candidate = self.enable_tail_calls
                    && self.func.name != "main"
                    && is_last_inst
                    && callee != &self.func.name
                    && args.len() <= Reg::ARG_REGS.len()
                    && !ret_class.is_memory()
                    && !caller_ret_class.is_memory()
                    && !args.iter().any(|arg| self.stack_ptrs.contains(arg))
                    && ((caller_ret_class.is_void() && ret_class.is_void())
                        || (!caller_ret_class.is_void() && !ret_class.is_void()))
                    && match &block.terminator {
                        Some(Terminator::Return(Some(ret_val))) => dest.as_ref() == Some(ret_val),
                        Some(Terminator::Return(None)) => dest.is_none() && ret_class.is_void(),
                        _ => false,
                    };

                if is_tail_candidate {
                    // Sibling tail call lowering:
                    // 1. Move arguments into argument registers
                    for (idx, arg) in args.iter().enumerate() {
                        let target_reg = Reg::ARG_REGS[idx];
                        let arg_op = self.get_val_op(*arg);
                        if arg_op != Operand::Reg(target_reg) {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(target_reg),
                                src: arg_op,
                            });
                        }
                    }

                    // 2. Restore callee-saved registers and dismantle caller stack frame
                    if !self.frame.used_callee_saved.is_empty() {
                        let callee_bytes = (self.frame.used_callee_saved.len() * 8) as i32;
                        out.push(MachineInst::Lea {
                            dest: Reg::Rsp,
                            src: Operand::Mem {
                                base: Reg::Rbp,
                                offset: -callee_bytes,
                            },
                        });
                        for reg in self.frame.used_callee_saved.iter().rev() {
                            out.push(MachineInst::Pop(*reg));
                        }
                    } else if self.frame.total_frame_size > 0 {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::Rsp),
                            src: Operand::Reg(Reg::Rbp),
                        });
                    }
                    out.push(MachineInst::Pop(Reg::Rbp));

                    // 3. Tail jump to callee
                    out.push(MachineInst::TailJmp(callee.clone()));
                    return;
                }

                let mut reg_idx = 0;

                if ret_class.is_memory() {
                    let d = dest.expect("Call with memory return must have dest ValueId");
                    let buf_offset = self
                        .frame
                        .call_return_buffers
                        .get(&d)
                        .copied()
                        .unwrap_or_else(|| self.frame.value_offsets.get(&d).copied().unwrap_or(8));
                    out.push(MachineInst::Lea {
                        dest: Reg::Rdi,
                        src: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -buf_offset,
                        },
                    });
                    reg_idx = 1;
                }

                let mut stack_args = Vec::new();
                for arg in args {
                    let arg_op = self.get_val_op(*arg);
                    if reg_idx < Reg::ARG_REGS.len() {
                        let target_reg = Reg::ARG_REGS[reg_idx];
                        if arg_op != Operand::Reg(target_reg) {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(target_reg),
                                src: arg_op,
                            });
                        }
                        reg_idx += 1;
                    } else {
                        stack_args.push(arg_op);
                    }
                }

                let push_count = stack_args.len();
                let pad = if push_count % 2 != 0 {
                    out.push(MachineInst::Sub {
                        dest: Operand::Reg(Reg::Rsp),
                        src: Operand::Imm(8),
                    });
                    true
                } else {
                    false
                };

                for arg_op in stack_args.into_iter().rev() {
                    match arg_op {
                        Operand::Reg(r) => {
                            out.push(MachineInst::Push(r));
                        }
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: arg_op,
                            });
                            out.push(MachineInst::Push(Reg::R10));
                        }
                    }
                }

                out.push(MachineInst::Call(callee.clone()));

                let total_cleanup = (push_count * 8) + if pad { 8 } else { 0 };
                if total_cleanup > 0 {
                    out.push(MachineInst::Add {
                        dest: Operand::Reg(Reg::Rsp),
                        src: Operand::Imm(total_cleanup as i64),
                    });
                }

                if let Some(d) = dest {
                    let dest_op = self.get_val_op(*d);
                    if dest_op != Operand::Reg(Reg::Rax) {
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::Rax),
                        });
                    }
                }
            }
            Instruction::IndexAddr {
                dest,
                slot,
                index,
                elem_size,
            } => {
                let slot_op = self.frame.get_slot_op(*slot);
                let index_op = self.get_val_op(*index);
                let dest_op = self.get_val_op(*dest);

                out.push(MachineInst::Lea {
                    dest: Reg::R10,
                    src: slot_op,
                });
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R11),
                    src: index_op,
                });
                if *elem_size != 1 {
                    out.push(MachineInst::Imul {
                        dest: Reg::R11,
                        src: Operand::Imm(*elem_size as i64),
                    });
                }
                out.push(MachineInst::Add {
                    dest: Operand::Reg(Reg::R10),
                    src: Operand::Reg(Reg::R11),
                });
                match dest_op {
                    Operand::Reg(dst) => {
                        if dst != Reg::R10 {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(dst),
                                src: Operand::Reg(Reg::R10),
                            });
                        }
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::IndexPtrAddr {
                dest,
                base_ptr,
                index,
                elem_size,
            } => {
                let base_op = self.get_val_op(*base_ptr);
                let index_op = self.get_val_op(*index);
                let dest_op = self.get_val_op(*dest);

                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: base_op,
                });
                out.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R11),
                    src: index_op,
                });
                if *elem_size != 1 {
                    out.push(MachineInst::Imul {
                        dest: Reg::R11,
                        src: Operand::Imm(*elem_size as i64),
                    });
                }
                out.push(MachineInst::Add {
                    dest: Operand::Reg(Reg::R10),
                    src: Operand::Reg(Reg::R11),
                });
                match dest_op {
                    Operand::Reg(dst) => {
                        if dst != Reg::R10 {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(dst),
                                src: Operand::Reg(Reg::R10),
                            });
                        }
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::FieldAddr {
                dest,
                base_ptr,
                offset,
            } => {
                let base_op = self.get_val_op(*base_ptr);
                let dest_op = self.get_val_op(*dest);

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
                match dest_op {
                    Operand::Reg(dst) => {
                        if dst != Reg::R10 {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(dst),
                                src: Operand::Reg(Reg::R10),
                            });
                        }
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::LoadIndirect { dest, addr, ty } => {
                let addr_op = self.get_val_op(*addr);
                let dest_op = self.get_val_op(*dest);

                let base_r = match addr_op {
                    Operand::Reg(r) => r,
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: addr_op,
                        });
                        Reg::R10
                    }
                };

                let target_r = match dest_op {
                    Operand::Reg(dst) => dst,
                    _ => Reg::R11,
                };

                if *ty == vanta_sema::Type::U8 || *ty == vanta_sema::Type::I8 {
                    out.push(MachineInst::MovzxMem {
                        dest: target_r,
                        base: base_r,
                        offset: 0,
                    });
                } else if *ty == vanta_sema::Type::I32 || *ty == vanta_sema::Type::U32 {
                    out.push(MachineInst::Mov32Mem {
                        dest: target_r,
                        base: base_r,
                        offset: 0,
                    });
                } else {
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(target_r),
                        src: Operand::Mem {
                            base: base_r,
                            offset: 0,
                        },
                    });
                }

                if let Operand::Mem { .. } = dest_op {
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Reg(target_r),
                    });
                }
            }
            Instruction::StoreIndirect { addr, val } => {
                let addr_op = self.get_val_op(*addr);
                let val_op = self.get_val_op(*val);

                let base_r = match addr_op {
                    Operand::Reg(r) => r,
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: addr_op,
                        });
                        Reg::R10
                    }
                };
                let val_r = match val_op {
                    Operand::Reg(r) => r,
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R11),
                            src: val_op,
                        });
                        Reg::R11
                    }
                };
                out.push(MachineInst::Mov {
                    dest: Operand::Mem {
                        base: base_r,
                        offset: 0,
                    },
                    src: Operand::Reg(val_r),
                });
            }
            Instruction::StoreIndirect32 { addr, val } => {
                let addr_op = self.get_val_op(*addr);
                let val_op = self.get_val_op(*val);

                let base_r = match addr_op {
                    Operand::Reg(r) => r,
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: addr_op,
                        });
                        Reg::R10
                    }
                };
                let val_r = match val_op {
                    Operand::Reg(r) => r,
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R11),
                            src: val_op,
                        });
                        Reg::R11
                    }
                };
                out.push(MachineInst::MovMem32 {
                    base: base_r,
                    offset: 0,
                    src: val_r,
                });
            }
            Instruction::StrSlice {
                dest_ptr,
                dest_len,
                symbol,
                len,
            } => {
                let dest_ptr_op = self.get_val_op(*dest_ptr);
                let dest_len_op = self.get_val_op(*dest_len);

                match dest_ptr_op {
                    Operand::Reg(dst) => {
                        out.push(MachineInst::LeaRip {
                            dest: dst,
                            symbol: symbol.clone(),
                        });
                    }
                    _ => {
                        out.push(MachineInst::LeaRip {
                            dest: Reg::Rax,
                            symbol: symbol.clone(),
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_ptr_op,
                            src: Operand::Reg(Reg::Rax),
                        });
                    }
                }

                match dest_len_op {
                    Operand::Reg(dst) => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(dst),
                            src: Operand::Imm(*len as i64),
                        });
                    }
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: Operand::Imm(*len as i64),
                        });
                        out.push(MachineInst::Mov {
                            dest: dest_len_op,
                            src: Operand::Reg(Reg::R10),
                        });
                    }
                }
            }
            Instruction::StoreSlice { slot, ptr, len } => {
                let ptr_op = self.get_val_op(*ptr);
                let len_op = self.get_val_op(*len);
                let slot_offset = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);

                let ptr_r = match ptr_op {
                    Operand::Reg(r) => r,
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: ptr_op,
                        });
                        Reg::R10
                    }
                };
                out.push(MachineInst::Mov {
                    dest: Operand::Mem {
                        base: Reg::Rbp,
                        offset: -slot_offset,
                    },
                    src: Operand::Reg(ptr_r),
                });

                let len_r = match len_op {
                    Operand::Reg(r) => r,
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R11),
                            src: len_op,
                        });
                        Reg::R11
                    }
                };
                out.push(MachineInst::Mov {
                    dest: Operand::Mem {
                        base: Reg::Rbp,
                        offset: -slot_offset + 8,
                    },
                    src: Operand::Reg(len_r),
                });
            }
            Instruction::LoadSlice {
                dest_ptr,
                dest_len,
                slot,
            } => {
                let dest_ptr_op = self.get_val_op(*dest_ptr);
                let dest_len_op = self.get_val_op(*dest_len);
                let slot_offset = self.frame.slot_offsets.get(slot).copied().unwrap_or(16);

                match dest_ptr_op {
                    Operand::Reg(dst) => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(dst),
                            src: Operand::Mem {
                                base: Reg::Rbp,
                                offset: -slot_offset,
                            },
                        });
                    }
                    _ => {
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
                    }
                }

                match dest_len_op {
                    Operand::Reg(dst) => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(dst),
                            src: Operand::Mem {
                                base: Reg::Rbp,
                                offset: -slot_offset + 8,
                            },
                        });
                    }
                    _ => {
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
            Instruction::Phi { .. } => {}
            Instruction::Copy { dest, src, .. } => {
                let dest_op = self.get_val_op(*dest);
                let src_op = self.get_val_op(*src);
                if dest_op != src_op {
                    match (dest_op, src_op) {
                        (Operand::Reg(dst), _) => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(dst),
                                src: src_op,
                            });
                        }
                        (_, Operand::Reg(src_r)) => {
                            out.push(MachineInst::Mov {
                                dest: dest_op,
                                src: Operand::Reg(src_r),
                            });
                        }
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: src_op,
                            });
                            out.push(MachineInst::Mov {
                                dest: dest_op,
                                src: Operand::Reg(Reg::R10),
                            });
                        }
                    }
                }
            }
        }
    }

    fn select_terminator(
        &self,
        curr_block: BlockId,
        term: &Terminator,
        next_block_id: Option<BlockId>,
        is_last_before_epilogue: bool,
        out: &mut Vec<MachineInst>,
    ) {
        // If the block already terminated in a sibling TailJmp, the frame has been dismantled
        if let Some(MachineInst::TailJmp(_)) = out.last() {
            return;
        }
        match term {
            Terminator::Branch(target) => {
                // Collect parallel transfers for Phis in target block
                let mut transfers: Vec<(Operand, Operand)> = Vec::new();
                if let Some(target_b) = self.func.blocks.iter().find(|b| b.id == *target) {
                    for inst in &target_b.instructions {
                        if let Instruction::Phi { dest, inputs, .. } = inst {
                            for (pred, val) in inputs {
                                if *pred == curr_block {
                                    let dest_op = self.get_val_op(*dest);
                                    let val_op = self.get_val_op(*val);
                                    if dest_op != val_op {
                                        transfers.push((dest_op, val_op));
                                    }
                                    break;
                                }
                            }
                        } else {
                            break;
                        }
                    }
                }

                if transfers.len() == 1 {
                    let (dest_op, src_op) = transfers[0];
                    match (dest_op, src_op) {
                        (Operand::Reg(dst), _) => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(dst),
                                src: src_op,
                            });
                        }
                        (_, Operand::Reg(src_r)) => {
                            out.push(MachineInst::Mov {
                                dest: dest_op,
                                src: Operand::Reg(src_r),
                            });
                        }
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: src_op,
                            });
                            out.push(MachineInst::Mov {
                                dest: dest_op,
                                src: Operand::Reg(Reg::R10),
                            });
                        }
                    }
                } else if transfers.len() > 1 {
                    let has_hazard = transfers.iter().any(|(d, _)| {
                        if let Operand::Reg(r_d) = d {
                            transfers
                                .iter()
                                .any(|(other_d, s)| other_d != d && s == &Operand::Reg(*r_d))
                        } else {
                            false
                        }
                    });

                    if !has_hazard {
                        for (dest_op, src_op) in transfers {
                            match (dest_op, src_op) {
                                (Operand::Reg(dst), _) => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(dst),
                                        src: src_op,
                                    });
                                }
                                (_, Operand::Reg(src_r)) => {
                                    out.push(MachineInst::Mov {
                                        dest: dest_op,
                                        src: Operand::Reg(src_r),
                                    });
                                }
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: src_op,
                                    });
                                    out.push(MachineInst::Mov {
                                        dest: dest_op,
                                        src: Operand::Reg(Reg::R10),
                                    });
                                }
                            }
                        }
                    } else {
                        // Staged push/pop for hazard-free parallel copies
                        for (_, src_op) in &transfers {
                            match src_op {
                                Operand::Reg(r) => {
                                    out.push(MachineInst::Push(*r));
                                }
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: *src_op,
                                    });
                                    out.push(MachineInst::Push(Reg::R10));
                                }
                            }
                        }
                        for (dest_op, _) in transfers.iter().rev() {
                            match dest_op {
                                Operand::Reg(r) => {
                                    out.push(MachineInst::Pop(*r));
                                }
                                _ => {
                                    out.push(MachineInst::Pop(Reg::R10));
                                    out.push(MachineInst::Mov {
                                        dest: *dest_op,
                                        src: Operand::Reg(Reg::R10),
                                    });
                                }
                            }
                        }
                    }
                }

                // Fall-through elimination: if physical next block is the target, omit Jmp!
                if Some(*target) != next_block_id {
                    let target_label = format!(".L_{}_bb{}", self.func.name, target.0);
                    out.push(MachineInst::Jmp(target_label));
                }
            }
            Terminator::CondBranch {
                cond,
                then_block,
                else_block,
            } => {
                let cond_op = self.get_val_op(*cond);
                let then_label = format!(".L_{}_bb{}", self.func.name, then_block.0);
                let else_label = format!(".L_{}_bb{}", self.func.name, else_block.0);

                let cond_r = match cond_op {
                    Operand::Reg(r) => r,
                    _ => {
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: cond_op,
                        });
                        Reg::R10
                    }
                };
                out.push(MachineInst::Test {
                    lhs: cond_r,
                    rhs: cond_r,
                });

                if Some(*then_block) == next_block_id {
                    // Branch inversion: jump to else_block if cond == 0 (Equal); fall through to then_block!
                    out.push(MachineInst::Jcc {
                        cond: ConditionCode::Equal,
                        target: else_label,
                    });
                } else if Some(*else_block) == next_block_id {
                    // Normal jump: jump to then_block if cond != 0 (NotEqual); fall through to else_block!
                    out.push(MachineInst::Jcc {
                        cond: ConditionCode::NotEqual,
                        target: then_label,
                    });
                } else {
                    out.push(MachineInst::Jcc {
                        cond: ConditionCode::NotEqual,
                        target: then_label,
                    });
                    out.push(MachineInst::Jmp(else_label));
                }
            }
            Terminator::Return(val) => {
                let ret_class = classify_type(&self.func.ret_type, self.structs, self.enums);
                if ret_class.is_memory() {
                    let sret_off = self
                        .frame
                        .sret_offset
                        .expect("sret function must have sret_offset");
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -sret_off,
                        },
                    });

                    if let Some(v) = val {
                        let val_op = self.get_val_op(*v);
                        out.push(MachineInst::Mov {
                            dest: Operand::Reg(Reg::R10),
                            src: val_op,
                        });

                        let skip_label = format!(
                            ".L_{}_sret_skip_{}",
                            self.func.name, self.frame.total_frame_size
                        );
                        out.push(MachineInst::Cmp {
                            lhs: Operand::Reg(Reg::R10),
                            rhs: Operand::Reg(Reg::R11),
                        });
                        out.push(MachineInst::Jcc {
                            cond: ConditionCode::Equal,
                            target: skip_label.clone(),
                        });

                        let size = ret_class.size();
                        let aligned_copy_size = (size + 7) & !7;
                        for off in (0..aligned_copy_size).step_by(8) {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::Rax),
                                src: Operand::Mem {
                                    base: Reg::R10,
                                    offset: off as i32,
                                },
                            });
                            out.push(MachineInst::Mov {
                                dest: Operand::Mem {
                                    base: Reg::R11,
                                    offset: off as i32,
                                },
                                src: Operand::Reg(Reg::Rax),
                            });
                        }
                        out.push(MachineInst::Label(skip_label));
                    }

                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::Rax),
                        src: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -sret_off,
                        },
                    });
                    if !is_last_before_epilogue {
                        out.push(MachineInst::Jmp(self.epilogue_label.clone()));
                    }
                } else if ret_class.is_void() {
                    out.push(MachineInst::Xor {
                        dest: Operand::Reg(Reg::Rax),
                        src: Operand::Reg(Reg::Rax),
                    });
                    if !is_last_before_epilogue {
                        out.push(MachineInst::Jmp(self.epilogue_label.clone()));
                    }
                } else {
                    if let Some(v) = val {
                        let val_op = self.get_val_op(*v);
                        if val_op != Operand::Reg(Reg::Rax) {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::Rax),
                                src: val_op,
                            });
                        }
                    }
                    if !is_last_before_epilogue {
                        out.push(MachineInst::Jmp(self.epilogue_label.clone()));
                    }
                }
            }
            Terminator::Unreachable => {}
        }
    }
}
