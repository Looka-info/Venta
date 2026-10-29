//! Instruction selection: Lowering Vanta MIR to x86-64 Machine Instructions.

use crate::abi::classify_type;
use crate::frame::FrameLayout;
use crate::machine_ir::{ConditionCode, MachineInst, Operand, SimdOperand};
use crate::reg_alloc::{AllocationResult, Location};
use crate::register::{Reg, SimdReg, VectorWidth};
use crate::simd_alloc::{SimdAllocResult, SimdLocation, SimdRegisterAllocator, SimdValueId};
use std::collections::HashMap;
use vanta_mir::{BlockId, Instruction, MirBinaryOp, MirFunction, MirUnaryOp, Terminator, ValueId};
use vanta_sema::{EnumDef, StructDef, Type};

pub struct InstructionSelector<'a> {
    func: &'a MirFunction,
    frame: &'a FrameLayout,
    structs: &'a HashMap<String, StructDef>,
    enums: &'a HashMap<String, EnumDef>,
    alloc_res: Option<&'a AllocationResult>,
    simd_alloc_res: Option<&'a SimdAllocResult>,
    simd_val_map: Option<&'a HashMap<ValueId, SimdValueId>>,
    fn_signatures: Option<&'a HashMap<String, (Vec<Type>, Type)>>,
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
        Self::with_simd(
            func, frame, structs, enums, alloc_res, None, None, None, true, true,
        )
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
        Self::with_simd(
            func,
            frame,
            structs,
            enums,
            alloc_res,
            None,
            None,
            None,
            enable_tail_calls,
            enable_block_layout,
        )
    }

    pub fn with_simd(
        func: &'a MirFunction,
        frame: &'a FrameLayout,
        structs: &'a HashMap<String, StructDef>,
        enums: &'a HashMap<String, EnumDef>,
        alloc_res: Option<&'a AllocationResult>,
        simd_alloc_res: Option<&'a SimdAllocResult>,
        simd_val_map: Option<&'a HashMap<ValueId, SimdValueId>>,
        fn_signatures: Option<&'a HashMap<String, (Vec<Type>, Type)>>,
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
                            if inputs.iter().any(|(_, v)| stack_ptrs.contains(v))
                                && stack_ptrs.insert(*dest)
                            {
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
            simd_alloc_res,
            simd_val_map,
            fn_signatures,
            epilogue_label: format!(".L_{}_epilogue", func.name),
            enable_tail_calls,
            enable_block_layout,
            stack_ptrs,
        }
    }

    fn get_simd_loc(&self, val: ValueId) -> Option<SimdLocation> {
        let simd_id = self.simd_val_map?.get(&val)?;
        self.simd_alloc_res?.assignments.get(simd_id).copied()
    }

    pub fn select_instructions(&self) -> Vec<MachineInst> {
        let mut insts = Vec::new();

        // 1. Function Prologue
        self.frame
            .emit_prologue(&mut insts, self.func, self.structs, self.enums);

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
                                if let Some(pos) =
                                    self.func.blocks.iter().position(|b| b.id == *target)
                                {
                                    next_pick = Some(pos);
                                }
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            // Prioritize unvisited successor: prefer else_block (standard fall-through)
                            if !placed.contains(else_block) {
                                if let Some(pos) =
                                    self.func.blocks.iter().position(|b| b.id == *else_block)
                                {
                                    next_pick = Some(pos);
                                }
                            } else if !placed.contains(then_block) {
                                if let Some(pos) =
                                    self.func.blocks.iter().position(|b| b.id == *then_block)
                                {
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
                    if let Some(unplaced_idx) = (0..self.func.blocks.len())
                        .find(|&i| !placed.contains(&self.func.blocks[i].id))
                    {
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

            let next_block_id = layout
                .get(order_pos + 1)
                .map(|&idx| self.func.blocks[idx].id);
            let is_last_before_epilogue = order_pos + 1 == layout.len();

            let num_instructions = block.instructions.len();
            for (inst_idx, inst) in block.instructions.iter().enumerate() {
                let is_last_inst = inst_idx + 1 == num_instructions;
                self.select_instruction(inst, block, is_last_inst, &mut insts);
            }

            if let Some(term) = &block.terminator {
                self.select_terminator(
                    block.id,
                    term,
                    next_block_id,
                    is_last_before_epilogue,
                    &mut insts,
                );
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
                None => self.frame.get_val_op(val),
            }
        } else {
            self.frame.get_val_op(val)
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
                if let Some(simd_loc) = self.get_simd_loc(*dest) {
                    let (base, offset) = match slot_op {
                        Operand::Mem { base, offset } => (base, offset),
                        _ => (Reg::Rbp, 0),
                    };
                    match simd_loc {
                        SimdLocation::Reg(r) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(r),
                                src: SimdOperand::Mem { base, offset },
                            });
                        }
                        SimdLocation::Spill(spill_slot) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src: SimdOperand::Mem { base, offset },
                            });
                            let spill_op = self.frame.get_simd_spill_op(spill_slot);
                            if let Operand::Mem {
                                base: s_base,
                                offset: s_off,
                            } = spill_op
                            {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem {
                                        base: s_base,
                                        offset: s_off,
                                    },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    return;
                }
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
                let slot_op = self.frame.get_slot_op(*slot);
                if let Some(simd_loc) = self.get_simd_loc(*val) {
                    let (base, offset) = match slot_op {
                        Operand::Mem { base, offset } => (base, offset),
                        _ => (Reg::Rbp, 0),
                    };
                    match simd_loc {
                        SimdLocation::Reg(r) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Mem { base, offset },
                                src: SimdOperand::Reg(r),
                            });
                        }
                        SimdLocation::Spill(spill_slot) => {
                            let spill_op = self.frame.get_simd_spill_op(spill_slot);
                            if let Operand::Mem {
                                base: s_base,
                                offset: s_off,
                            } = spill_op
                            {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                    src: SimdOperand::Mem {
                                        base: s_base,
                                        offset: s_off,
                                    },
                                });
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    return;
                }

                let is_vector_slot = self
                    .func
                    .slots
                    .iter()
                    .find(|s| s.id == *slot)
                    .map(|s| classify_type(&s.ty, self.structs, self.enums).is_vector())
                    .unwrap_or(false);

                if is_vector_slot {
                    let val_op = self.get_val_op(*val);
                    let base_r = match val_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: val_op,
                            });
                            Reg::R10
                        }
                    };
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                        src: SimdOperand::Mem {
                            base: base_r,
                            offset: 0,
                        },
                    });
                    let (base, offset) = match slot_op {
                        Operand::Mem { base, offset } => (base, offset),
                        _ => (Reg::Rbp, 0),
                    };
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Mem { base, offset },
                        src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                    });
                    return;
                }

                let val_op = self.get_val_op(*val);
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
                dest, op, lhs, rhs, ..
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
                                MirBinaryOp::BitAnd | MirBinaryOp::And => {
                                    out.push(MachineInst::And {
                                        dest: Operand::Reg(dst),
                                        src: rhs_op,
                                    })
                                }
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
                                MirBinaryOp::BitAnd | MirBinaryOp::And => {
                                    out.push(MachineInst::And {
                                        dest: Operand::Reg(Reg::R10),
                                        src: rhs_op,
                                    })
                                }
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
                // SIMD Compiler Intrinsics:
                if callee == "venta_simd_vmovups_load256" || callee == "simd_vmovups_load256" {
                    assert!(
                        args.len() >= 2,
                        "vmovups_load256 requires dest_ptr and src_ptr"
                    );
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let src_ptr_op = self.get_val_op(args[1]);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: src_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: dest_ptr_op,
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Reg(SimdReg::ymm(0)),
                        src: SimdOperand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Mem {
                            base: Reg::R11,
                            offset: 0,
                        },
                        src: SimdOperand::Reg(SimdReg::ymm(0)),
                    });
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vmovups_store256" || callee == "simd_vmovups_store256" {
                    assert!(
                        args.len() >= 2,
                        "vmovups_store256 requires dest_ptr and src_ptr"
                    );
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let src_ptr_op = self.get_val_op(args[1]);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: src_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: dest_ptr_op,
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Reg(SimdReg::ymm(0)),
                        src: SimdOperand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Mem {
                            base: Reg::R11,
                            offset: 0,
                        },
                        src: SimdOperand::Reg(SimdReg::ymm(0)),
                    });
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vpaddq256" || callee == "simd_vpaddq256" {
                    assert!(
                        args.len() >= 3,
                        "vpaddq256 requires dest_ptr, a_ptr, and b_ptr"
                    );
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let a_ptr_op = self.get_val_op(args[1]);
                    let b_ptr_op = self.get_val_op(args[2]);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: dest_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: a_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R8),
                        src: b_ptr_op,
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Reg(SimdReg::ymm(0)),
                        src: SimdOperand::Mem {
                            base: Reg::R11,
                            offset: 0,
                        },
                    });
                    out.push(MachineInst::Vpaddq {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Reg(SimdReg::ymm(0)),
                        src1: SimdOperand::Reg(SimdReg::ymm(0)),
                        src2: SimdOperand::Mem {
                            base: Reg::R8,
                            offset: 0,
                        },
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                        src: SimdOperand::Reg(SimdReg::ymm(0)),
                    });
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vfmadd231pd256" || callee == "simd_vfmadd231pd256" {
                    assert!(
                        args.len() >= 4,
                        "vfmadd231pd256 requires dest_ptr, a_ptr, b_ptr, and c_ptr"
                    );
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let a_ptr_op = self.get_val_op(args[1]);
                    let b_ptr_op = self.get_val_op(args[2]);
                    let c_ptr_op = self.get_val_op(args[3]);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: dest_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: a_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R8),
                        src: b_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R9),
                        src: c_ptr_op,
                    });
                    // Load accumulator c_ptr into YMM0
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Reg(SimdReg::ymm(0)),
                        src: SimdOperand::Mem {
                            base: Reg::R9,
                            offset: 0,
                        },
                    });
                    // Load multiplier a_ptr into YMM1
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Reg(SimdReg::ymm(1)),
                        src: SimdOperand::Mem {
                            base: Reg::R11,
                            offset: 0,
                        },
                    });
                    // ymm0 = ymm0 + (ymm1 * [r8])
                    out.push(MachineInst::Vfmadd231pd {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Reg(SimdReg::ymm(0)),
                        src1: SimdOperand::Reg(SimdReg::ymm(1)),
                        src2: SimdOperand::Mem {
                            base: Reg::R8,
                            offset: 0,
                        },
                    });
                    // Store result to dest_ptr [r10]
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V256,
                        dest: SimdOperand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                        src: SimdOperand::Reg(SimdReg::ymm(0)),
                    });
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                // --- Value-based Virtual SIMD Intrinsics (Milestone 26.1) ---
                if callee == "venta_simd_reg_load256" || callee == "simd_reg_load256" {
                    assert!(!args.is_empty(), "reg_load256 requires src_ptr");
                    let src_ptr_op = self.get_val_op(args[0]);
                    let base_r = match src_ptr_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: src_ptr_op,
                            });
                            Reg::R10
                        }
                    };
                    let d = dest.expect("reg_load256 must have destination ValueId");
                    let loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));
                    match loc {
                        SimdLocation::Reg(reg) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(reg),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                        }
                        SimdLocation::Spill(slot) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_reg_store256" || callee == "simd_reg_store256" {
                    assert!(args.len() >= 2, "reg_store256 requires dest_ptr and handle");
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let base_r = match dest_ptr_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: dest_ptr_op,
                            });
                            Reg::R10
                        }
                    };
                    let val_id = args[1];
                    let loc = self
                        .get_simd_loc(val_id)
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));
                    match loc {
                        SimdLocation::Reg(reg) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                                src: SimdOperand::Reg(reg),
                            });
                        }
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                    src: SimdOperand::Mem { base, offset },
                                });
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem {
                                        base: base_r,
                                        offset: 0,
                                    },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_reg_add256" || callee == "simd_reg_add256" {
                    assert!(args.len() >= 2, "reg_add256 requires a_handle and b_handle");
                    let a_loc = self
                        .get_simd_loc(args[0])
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));
                    let b_loc = self
                        .get_simd_loc(args[1])
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(1)));
                    let d = dest.expect("reg_add256 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));

                    let reg_a = match a_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_A
                        }
                    };

                    let reg_b = match b_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_B
                        }
                    };

                    match d_loc {
                        SimdLocation::Reg(reg_d) => {
                            out.push(MachineInst::Vpaddq {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(reg_d),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                        }
                        SimdLocation::Spill(slot_d) => {
                            out.push(MachineInst::Vpaddq {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot_d);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_reg_fma256" || callee == "simd_reg_fma256" {
                    assert!(
                        args.len() >= 3,
                        "reg_fma256 requires acc_handle, a_handle, and b_handle"
                    );
                    let acc_loc = self
                        .get_simd_loc(args[0])
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));
                    let a_loc = self
                        .get_simd_loc(args[1])
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(1)));
                    let b_loc = self
                        .get_simd_loc(args[2])
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(2)));
                    let d = dest.expect("reg_fma256 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));

                    let reg_acc = match acc_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_A
                        }
                    };

                    let reg_a = match a_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_B
                        }
                    };

                    let reg_b = match b_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdReg::ymm(13)),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdReg::ymm(13)
                        }
                    };

                    match d_loc {
                        SimdLocation::Reg(reg_d) => {
                            if reg_d != reg_acc {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(reg_d),
                                    src: SimdOperand::Reg(reg_acc),
                                });
                            }
                            out.push(MachineInst::Vfmadd231pd {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(reg_d),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                        }
                        SimdLocation::Spill(slot_d) => {
                            out.push(MachineInst::Vfmadd231pd {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(reg_acc),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot_d);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(reg_acc),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_reg_load512" || callee == "simd_reg_load512" {
                    assert!(!args.is_empty(), "reg_load512 requires src_ptr");
                    let src_ptr_op = self.get_val_op(args[0]);
                    let base_r = match src_ptr_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: src_ptr_op,
                            });
                            Reg::R10
                        }
                    };
                    let d = dest.expect("reg_load512 must have destination ValueId");
                    let loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));
                    match loc {
                        SimdLocation::Reg(reg) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(reg),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                        }
                        SimdLocation::Spill(slot) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_reg_store512" || callee == "simd_reg_store512" {
                    assert!(args.len() >= 2, "reg_store512 requires dest_ptr and handle");
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let base_r = match dest_ptr_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: dest_ptr_op,
                            });
                            Reg::R10
                        }
                    };
                    let val_id = args[1];
                    let loc = self
                        .get_simd_loc(val_id)
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));
                    match loc {
                        SimdLocation::Reg(reg) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                                src: SimdOperand::Reg(reg),
                            });
                        }
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Mem {
                                        base: base_r,
                                        offset: 0,
                                    },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                });
                            }
                        }
                    }
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_reg_add512" || callee == "simd_reg_add512" {
                    assert!(args.len() >= 2, "reg_add512 requires a_handle and b_handle");
                    let a_loc = self
                        .get_simd_loc(args[0])
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));
                    let b_loc = self
                        .get_simd_loc(args[1])
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(1)));
                    let d = dest.expect("reg_add512 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));

                    let reg_a = match a_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_A_512
                        }
                    };

                    let reg_b = match b_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_B_512
                        }
                    };

                    match d_loc {
                        SimdLocation::Reg(reg_d) => {
                            out.push(MachineInst::Vpaddq {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(reg_d),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                        }
                        SimdLocation::Spill(slot_d) => {
                            out.push(MachineInst::Vpaddq {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot_d);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_reg_fma512" || callee == "simd_reg_fma512" {
                    assert!(
                        args.len() >= 3,
                        "reg_fma512 requires acc_handle, a_handle, and b_handle"
                    );
                    let acc_loc = self
                        .get_simd_loc(args[0])
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));
                    let a_loc = self
                        .get_simd_loc(args[1])
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(1)));
                    let b_loc = self
                        .get_simd_loc(args[2])
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(2)));
                    let d = dest.expect("reg_fma512 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));

                    let reg_acc = match acc_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_A_512
                        }
                    };

                    let reg_a = match a_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_B_512
                        }
                    };

                    let reg_b = match b_loc {
                        SimdLocation::Reg(r) => r,
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdReg::zmm(15)),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdReg::zmm(15)
                        }
                    };

                    match d_loc {
                        SimdLocation::Reg(reg_d) => {
                            if reg_d != reg_acc {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(reg_d),
                                    src: SimdOperand::Reg(reg_acc),
                                });
                            }
                            out.push(MachineInst::Vfmadd231pd {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(reg_d),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                        }
                        SimdLocation::Spill(slot_d) => {
                            out.push(MachineInst::Vfmadd231pd {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(reg_acc),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot_d);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(reg_acc),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_vmovups_load512" || callee == "simd_vmovups_load512" {
                    assert!(
                        args.len() >= 2,
                        "vmovups_load512 requires dest_ptr and src_ptr"
                    );
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let src_ptr_op = self.get_val_op(args[1]);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: src_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: dest_ptr_op,
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Reg(SimdReg::zmm(0)),
                        src: SimdOperand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Mem {
                            base: Reg::R11,
                            offset: 0,
                        },
                        src: SimdOperand::Reg(SimdReg::zmm(0)),
                    });
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vmovups_store512" || callee == "simd_vmovups_store512" {
                    assert!(
                        args.len() >= 2,
                        "vmovups_store512 requires dest_ptr and src_ptr"
                    );
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let src_ptr_op = self.get_val_op(args[1]);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: src_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: dest_ptr_op,
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Reg(SimdReg::zmm(0)),
                        src: SimdOperand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Mem {
                            base: Reg::R11,
                            offset: 0,
                        },
                        src: SimdOperand::Reg(SimdReg::zmm(0)),
                    });
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vpaddq512" || callee == "simd_vpaddq512" {
                    assert!(
                        args.len() >= 3,
                        "vpaddq512 requires dest_ptr, a_ptr, and b_ptr"
                    );
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let a_ptr_op = self.get_val_op(args[1]);
                    let b_ptr_op = self.get_val_op(args[2]);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: dest_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: a_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R8),
                        src: b_ptr_op,
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Reg(SimdReg::zmm(0)),
                        src: SimdOperand::Mem {
                            base: Reg::R11,
                            offset: 0,
                        },
                    });
                    out.push(MachineInst::Vpaddq {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Reg(SimdReg::zmm(0)),
                        src1: SimdOperand::Reg(SimdReg::zmm(0)),
                        src2: SimdOperand::Mem {
                            base: Reg::R8,
                            offset: 0,
                        },
                    });
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                        src: SimdOperand::Reg(SimdReg::zmm(0)),
                    });
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vfmadd231pd512" || callee == "simd_vfmadd231pd512" {
                    assert!(
                        args.len() >= 4,
                        "vfmadd231pd512 requires dest_ptr, a_ptr, b_ptr, and c_ptr"
                    );
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let a_ptr_op = self.get_val_op(args[1]);
                    let b_ptr_op = self.get_val_op(args[2]);
                    let c_ptr_op = self.get_val_op(args[3]);
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: dest_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: a_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R8),
                        src: b_ptr_op,
                    });
                    out.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R9),
                        src: c_ptr_op,
                    });
                    // Load accumulator c_ptr into ZMM0
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Reg(SimdReg::zmm(0)),
                        src: SimdOperand::Mem {
                            base: Reg::R9,
                            offset: 0,
                        },
                    });
                    // Load multiplier a_ptr into ZMM1
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Reg(SimdReg::zmm(1)),
                        src: SimdOperand::Mem {
                            base: Reg::R11,
                            offset: 0,
                        },
                    });
                    // zmm0 = zmm0 + (zmm1 * [r8])
                    out.push(MachineInst::Vfmadd231pd {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Reg(SimdReg::zmm(0)),
                        src1: SimdOperand::Reg(SimdReg::zmm(1)),
                        src2: SimdOperand::Mem {
                            base: Reg::R8,
                            offset: 0,
                        },
                    });
                    // Store result to dest_ptr [r10]
                    out.push(MachineInst::Vmovups {
                        width: VectorWidth::V512,
                        dest: SimdOperand::Mem {
                            base: Reg::R10,
                            offset: 0,
                        },
                        src: SimdOperand::Reg(SimdReg::zmm(0)),
                    });
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vec_load256" || callee == "simd_vec_load256" {
                    assert!(args.len() >= 1, "vec_load256 requires src_ptr");
                    let src_ptr_op = self.get_val_op(args[0]);
                    let d = dest.expect("vec_load256 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));
                    let base_r = match src_ptr_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: src_ptr_op,
                            });
                            Reg::R10
                        }
                    };
                    match d_loc {
                        SimdLocation::Reg(r) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(r),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                        }
                        SimdLocation::Spill(slot) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_vec_store256" || callee == "simd_vec_store256" {
                    assert!(args.len() >= 2, "vec_store256 requires dest_ptr and val");
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let val_loc = self
                        .get_simd_loc(args[1])
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));
                    let base_r = match dest_ptr_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: dest_ptr_op,
                            });
                            Reg::R10
                        }
                    };
                    match val_loc {
                        SimdLocation::Reg(r) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                                src: SimdOperand::Reg(r),
                            });
                        }
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                    src: SimdOperand::Mem { base, offset },
                                });
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem {
                                        base: base_r,
                                        offset: 0,
                                    },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vec_add256" || callee == "simd_vec_add256" {
                    assert!(args.len() >= 2, "vec_add256 requires a and b");
                    let d = dest.expect("vec_add256 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));

                    let reg_a = match self.get_simd_loc(args[0]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_A
                        }
                        None => {
                            let a_op = self.get_val_op(args[0]);
                            let base_r = match a_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: a_op,
                                    });
                                    Reg::R10
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdRegisterAllocator::SCRATCH_A
                        }
                    };

                    let reg_b = match self.get_simd_loc(args[1]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_B
                        }
                        None => {
                            let b_op = self.get_val_op(args[1]);
                            let base_r = match b_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R11),
                                        src: b_op,
                                    });
                                    Reg::R11
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdRegisterAllocator::SCRATCH_B
                        }
                    };

                    match d_loc {
                        SimdLocation::Reg(reg_d) => {
                            out.push(MachineInst::Vpaddq {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(reg_d),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                        }
                        SimdLocation::Spill(slot_d) => {
                            out.push(MachineInst::Vpaddq {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot_d);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_vec_fma256" || callee == "simd_vec_fma256" {
                    assert!(args.len() >= 3, "vec_fma256 requires acc, a, and b");
                    let d = dest.expect("vec_fma256 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::ymm(0)));

                    let reg_acc = match self.get_simd_loc(args[0]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_A
                        }
                        None => {
                            let acc_op = self.get_val_op(args[0]);
                            let base_r = match acc_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: acc_op,
                                    });
                                    Reg::R10
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdRegisterAllocator::SCRATCH_A
                        }
                    };

                    let reg_a = match self.get_simd_loc(args[1]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_B
                        }
                        None => {
                            let a_op = self.get_val_op(args[1]);
                            let base_r = match a_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R11),
                                        src: a_op,
                                    });
                                    Reg::R11
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdRegisterAllocator::SCRATCH_B
                        }
                    };

                    let reg_b = match self.get_simd_loc(args[2]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdReg::ymm(13)),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdReg::ymm(13)
                        }
                        None => {
                            let b_op = self.get_val_op(args[2]);
                            let base_r = match b_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: b_op,
                                    });
                                    Reg::R10
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdReg::ymm(13)),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdReg::ymm(13)
                        }
                    };

                    match d_loc {
                        SimdLocation::Reg(reg_d) => {
                            if reg_d != reg_acc {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(reg_d),
                                    src: SimdOperand::Reg(reg_acc),
                                });
                            }
                            out.push(MachineInst::Vfmadd231pd {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(reg_d),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                        }
                        SimdLocation::Spill(slot_d) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src: SimdOperand::Reg(reg_acc),
                            });
                            out.push(MachineInst::Vfmadd231pd {
                                width: VectorWidth::V256,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot_d);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_vec_load512" || callee == "simd_vec_load512" {
                    assert!(!args.is_empty(), "vec_load512 requires src_ptr");
                    let src_ptr_op = self.get_val_op(args[0]);
                    let base_r = match src_ptr_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: src_ptr_op,
                            });
                            Reg::R10
                        }
                    };
                    let d = dest.expect("vec_load512 must have destination ValueId");
                    let loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));
                    match loc {
                        SimdLocation::Reg(reg) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(reg),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                        }
                        SimdLocation::Spill(slot) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_vec_store512" || callee == "simd_vec_store512" {
                    assert!(args.len() >= 2, "vec_store512 requires dest_ptr and val");
                    let dest_ptr_op = self.get_val_op(args[0]);
                    let val_loc = self
                        .get_simd_loc(args[1])
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));
                    let base_r = match dest_ptr_op {
                        Operand::Reg(r) => r,
                        _ => {
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: dest_ptr_op,
                            });
                            Reg::R10
                        }
                    };
                    match val_loc {
                        SimdLocation::Reg(r) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                                src: SimdOperand::Reg(r),
                            });
                        }
                        SimdLocation::Spill(slot) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Mem {
                                        base: base_r,
                                        offset: 0,
                                    },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                });
                            }
                        }
                    }
                    if let Some(d) = dest {
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(0),
                        });
                    }
                    return;
                }

                if callee == "venta_simd_vec_add512" || callee == "simd_vec_add512" {
                    assert!(args.len() >= 2, "vec_add512 requires a and b");
                    let d = dest.expect("vec_add512 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));

                    let reg_a = match self.get_simd_loc(args[0]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_A_512
                        }
                        None => {
                            let a_op = self.get_val_op(args[0]);
                            let base_r = match a_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: a_op,
                                    });
                                    Reg::R10
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdRegisterAllocator::SCRATCH_A_512
                        }
                    };

                    let reg_b = match self.get_simd_loc(args[1]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_B_512
                        }
                        None => {
                            let b_op = self.get_val_op(args[1]);
                            let base_r = match b_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R11),
                                        src: b_op,
                                    });
                                    Reg::R11
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B_512),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdRegisterAllocator::SCRATCH_B_512
                        }
                    };

                    match d_loc {
                        SimdLocation::Reg(reg_d) => {
                            out.push(MachineInst::Vpaddq {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(reg_d),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                        }
                        SimdLocation::Spill(slot_d) => {
                            out.push(MachineInst::Vpaddq {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot_d);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

                if callee == "venta_simd_vec_fma512" || callee == "simd_vec_fma512" {
                    assert!(args.len() >= 3, "vec_fma512 requires acc, a, and b");
                    let d = dest.expect("vec_fma512 must have destination ValueId");
                    let d_loc = self
                        .get_simd_loc(d)
                        .unwrap_or(SimdLocation::Reg(SimdReg::zmm(0)));

                    let reg_acc = match self.get_simd_loc(args[0]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_A_512
                        }
                        None => {
                            let acc_op = self.get_val_op(args[0]);
                            let base_r = match acc_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: acc_op,
                                    });
                                    Reg::R10
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdRegisterAllocator::SCRATCH_A_512
                        }
                    };

                    let reg_a = match self.get_simd_loc(args[1]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B_512),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdRegisterAllocator::SCRATCH_B_512
                        }
                        None => {
                            let a_op = self.get_val_op(args[1]);
                            let base_r = match a_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R11),
                                        src: a_op,
                                    });
                                    Reg::R11
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_B_512),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdRegisterAllocator::SCRATCH_B_512
                        }
                    };

                    let reg_b = match self.get_simd_loc(args[2]) {
                        Some(SimdLocation::Reg(r)) => r,
                        Some(SimdLocation::Spill(slot)) => {
                            let spill_op = self.frame.get_simd_spill_op(slot);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(SimdReg::zmm(15)),
                                    src: SimdOperand::Mem { base, offset },
                                });
                            }
                            SimdReg::zmm(15)
                        }
                        None => {
                            let b_op = self.get_val_op(args[2]);
                            let base_r = match b_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: b_op,
                                    });
                                    Reg::R10
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdReg::zmm(15)),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                            SimdReg::zmm(15)
                        }
                    };

                    match d_loc {
                        SimdLocation::Reg(reg_d) => {
                            if reg_d != reg_acc {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Reg(reg_d),
                                    src: SimdOperand::Reg(reg_acc),
                                });
                            }
                            out.push(MachineInst::Vfmadd231pd {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(reg_d),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                        }
                        SimdLocation::Spill(slot_d) => {
                            out.push(MachineInst::Vmovups {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                src: SimdOperand::Reg(reg_acc),
                            });
                            out.push(MachineInst::Vfmadd231pd {
                                width: VectorWidth::V512,
                                dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                src1: SimdOperand::Reg(reg_a),
                                src2: SimdOperand::Reg(reg_b),
                            });
                            let spill_op = self.frame.get_simd_spill_op(slot_d);
                            if let Operand::Mem { base, offset } = spill_op {
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V512,
                                    dest: SimdOperand::Mem { base, offset },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A_512),
                                });
                            }
                        }
                    }
                    let dest_op = self.get_val_op(d);
                    out.push(MachineInst::Mov {
                        dest: dest_op,
                        src: Operand::Imm(d.0 as i64),
                    });
                    return;
                }

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
                    && !ret_class.is_vector()
                    && !caller_ret_class.is_vector()
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

                // General System V AMD64 Function Call Lowering
                let mut arg_is_vector = vec![false; args.len()];
                if let Some((sig_params, _)) = self.fn_signatures.and_then(|s| s.get(callee)) {
                    let mut a_idx = 0;
                    for p_ty in sig_params {
                        if a_idx >= args.len() {
                            break;
                        }
                        if classify_type(p_ty, self.structs, self.enums).is_vector() {
                            arg_is_vector[a_idx] = true;
                            a_idx += 1;
                        } else if p_ty.is_str_slice()
                            || p_ty.is_slice()
                            || matches!(p_ty, Type::Ref { inner, .. } if inner.is_str_slice() || inner.is_slice())
                        {
                            // Slice parameter consumes 2 args in MIR (ptr and len)
                            a_idx += 2;
                        } else {
                            a_idx += 1;
                        }
                    }
                } else {
                    for (i, arg) in args.iter().enumerate() {
                        if self
                            .simd_val_map
                            .map(|m| m.contains_key(arg))
                            .unwrap_or(false)
                        {
                            arg_is_vector[i] = true;
                        }
                    }
                }

                enum CallArgDest {
                    IntReg(Reg),
                    VecReg(SimdReg),
                    IntStack(i32),
                    VecStack(i32),
                }

                let mut arg_dests = Vec::with_capacity(args.len());
                let mut int_reg_idx = if ret_class.is_memory() { 1 } else { 0 };
                let mut vec_reg_idx = 0;
                let mut current_stack_offset = 0usize;

                for (i, _arg) in args.iter().enumerate() {
                    if arg_is_vector[i] {
                        if vec_reg_idx < 8 {
                            let reg = SimdReg(vec_reg_idx as u8);
                            vec_reg_idx += 1;
                            arg_dests.push(CallArgDest::VecReg(reg));
                        } else {
                            current_stack_offset = (current_stack_offset + 31) & !31;
                            let off = current_stack_offset as i32;
                            current_stack_offset += 32;
                            arg_dests.push(CallArgDest::VecStack(off));
                        }
                    } else {
                        if int_reg_idx < Reg::ARG_REGS.len() {
                            let reg = Reg::ARG_REGS[int_reg_idx];
                            int_reg_idx += 1;
                            arg_dests.push(CallArgDest::IntReg(reg));
                        } else {
                            current_stack_offset = (current_stack_offset + 7) & !7;
                            let off = current_stack_offset as i32;
                            current_stack_offset += 8;
                            arg_dests.push(CallArgDest::IntStack(off));
                        }
                    }
                }

                let stack_arg_bytes = if current_stack_offset > 0 {
                    (current_stack_offset + 31) & !31
                } else {
                    0
                };

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
                }

                if stack_arg_bytes > 0 {
                    out.push(MachineInst::Sub {
                        dest: Operand::Reg(Reg::Rsp),
                        src: Operand::Imm(stack_arg_bytes as i64),
                    });
                }

                for (idx, arg) in args.iter().enumerate() {
                    match arg_dests[idx] {
                        CallArgDest::VecReg(target_ymm) => {
                            if let Some(loc) = self.get_simd_loc(*arg) {
                                match loc {
                                    SimdLocation::Reg(r) => {
                                        if r != target_ymm {
                                            out.push(MachineInst::Vmovups {
                                                width: VectorWidth::V256,
                                                dest: SimdOperand::Reg(target_ymm),
                                                src: SimdOperand::Reg(r),
                                            });
                                        }
                                    }
                                    SimdLocation::Spill(slot) => {
                                        let spill_op = self.frame.get_simd_spill_op(slot);
                                        if let Operand::Mem { base, offset } = spill_op {
                                            let eff_offset = if base == Reg::Rsp {
                                                offset + (stack_arg_bytes as i32)
                                            } else {
                                                offset
                                            };
                                            out.push(MachineInst::Vmovups {
                                                width: VectorWidth::V256,
                                                dest: SimdOperand::Reg(target_ymm),
                                                src: SimdOperand::Mem {
                                                    base,
                                                    offset: eff_offset,
                                                },
                                            });
                                        }
                                    }
                                }
                            } else {
                                let val_op = self.get_val_op(*arg);
                                let base_r = match val_op {
                                    Operand::Reg(r) => r,
                                    Operand::Mem { base, offset } => {
                                        let eff_offset = if base == Reg::Rsp {
                                            offset + (stack_arg_bytes as i32)
                                        } else {
                                            offset
                                        };
                                        out.push(MachineInst::Mov {
                                            dest: Operand::Reg(Reg::R10),
                                            src: Operand::Mem {
                                                base,
                                                offset: eff_offset,
                                            },
                                        });
                                        Reg::R10
                                    }
                                    _ => Reg::R10,
                                };
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(target_ymm),
                                    src: SimdOperand::Mem {
                                        base: base_r,
                                        offset: 0,
                                    },
                                });
                            }
                        }
                        CallArgDest::IntReg(target_reg) => {
                            let val_op = self.get_val_op(*arg);
                            let eff_op = match val_op {
                                Operand::Mem {
                                    base: Reg::Rsp,
                                    offset,
                                } => Operand::Mem {
                                    base: Reg::Rsp,
                                    offset: offset + (stack_arg_bytes as i32),
                                },
                                op => op,
                            };
                            if eff_op != Operand::Reg(target_reg) {
                                out.push(MachineInst::Mov {
                                    dest: Operand::Reg(target_reg),
                                    src: eff_op,
                                });
                            }
                        }
                        CallArgDest::VecStack(offset) => {
                            if let Some(loc) = self.get_simd_loc(*arg) {
                                match loc {
                                    SimdLocation::Reg(r) => {
                                        out.push(MachineInst::Vmovups {
                                            width: VectorWidth::V256,
                                            dest: SimdOperand::Mem {
                                                base: Reg::Rsp,
                                                offset,
                                            },
                                            src: SimdOperand::Reg(r),
                                        });
                                    }
                                    SimdLocation::Spill(slot) => {
                                        let spill_op = self.frame.get_simd_spill_op(slot);
                                        if let Operand::Mem {
                                            base,
                                            offset: s_off,
                                        } = spill_op
                                        {
                                            let eff_off = if base == Reg::Rsp {
                                                s_off + (stack_arg_bytes as i32)
                                            } else {
                                                s_off
                                            };
                                            out.push(MachineInst::Vmovups {
                                                width: VectorWidth::V256,
                                                dest: SimdOperand::Reg(
                                                    SimdRegisterAllocator::SCRATCH_A,
                                                ),
                                                src: SimdOperand::Mem {
                                                    base,
                                                    offset: eff_off,
                                                },
                                            });
                                            out.push(MachineInst::Vmovups {
                                                width: VectorWidth::V256,
                                                dest: SimdOperand::Mem {
                                                    base: Reg::Rsp,
                                                    offset,
                                                },
                                                src: SimdOperand::Reg(
                                                    SimdRegisterAllocator::SCRATCH_A,
                                                ),
                                            });
                                        }
                                    }
                                }
                            } else {
                                let val_op = self.get_val_op(*arg);
                                let base_r = match val_op {
                                    Operand::Reg(r) => r,
                                    Operand::Mem {
                                        base,
                                        offset: v_off,
                                    } => {
                                        let eff_v_off = if base == Reg::Rsp {
                                            v_off + (stack_arg_bytes as i32)
                                        } else {
                                            v_off
                                        };
                                        out.push(MachineInst::Mov {
                                            dest: Operand::Reg(Reg::R10),
                                            src: Operand::Mem {
                                                base,
                                                offset: eff_v_off,
                                            },
                                        });
                                        Reg::R10
                                    }
                                    _ => Reg::R10,
                                };
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                    src: SimdOperand::Mem {
                                        base: base_r,
                                        offset: 0,
                                    },
                                });
                                out.push(MachineInst::Vmovups {
                                    width: VectorWidth::V256,
                                    dest: SimdOperand::Mem {
                                        base: Reg::Rsp,
                                        offset,
                                    },
                                    src: SimdOperand::Reg(SimdRegisterAllocator::SCRATCH_A),
                                });
                            }
                        }
                        CallArgDest::IntStack(offset) => {
                            let val_op = self.get_val_op(*arg);
                            let eff_op = match val_op {
                                Operand::Mem {
                                    base: Reg::Rsp,
                                    offset: v_off,
                                } => Operand::Mem {
                                    base: Reg::Rsp,
                                    offset: v_off + (stack_arg_bytes as i32),
                                },
                                op => op,
                            };
                            out.push(MachineInst::Mov {
                                dest: Operand::Reg(Reg::R10),
                                src: eff_op,
                            });
                            out.push(MachineInst::Mov {
                                dest: Operand::Mem {
                                    base: Reg::Rsp,
                                    offset,
                                },
                                src: Operand::Reg(Reg::R10),
                            });
                        }
                    }
                }

                out.push(MachineInst::Call(callee.clone()));

                if stack_arg_bytes > 0 {
                    out.push(MachineInst::Add {
                        dest: Operand::Reg(Reg::Rsp),
                        src: Operand::Imm(stack_arg_bytes as i64),
                    });
                }

                if let Some(d) = dest {
                    if ret_class.is_vector() {
                        if let Some(simd_loc) = self.get_simd_loc(*d) {
                            match simd_loc {
                                SimdLocation::Reg(r) => {
                                    if r != SimdReg(0) {
                                        out.push(MachineInst::Vmovups {
                                            width: VectorWidth::V256,
                                            dest: SimdOperand::Reg(r),
                                            src: SimdOperand::Reg(SimdReg(0)),
                                        });
                                    }
                                }
                                SimdLocation::Spill(slot) => {
                                    let spill_op = self.frame.get_simd_spill_op(slot);
                                    if let Operand::Mem { base, offset } = spill_op {
                                        out.push(MachineInst::Vmovups {
                                            width: VectorWidth::V256,
                                            dest: SimdOperand::Mem { base, offset },
                                            src: SimdOperand::Reg(SimdReg(0)),
                                        });
                                    }
                                }
                            }
                        }
                        let dest_op = self.get_val_op(*d);
                        out.push(MachineInst::Mov {
                            dest: dest_op,
                            src: Operand::Imm(d.0 as i64),
                        });
                    } else {
                        let dest_op = self.get_val_op(*d);
                        if dest_op != Operand::Reg(Reg::Rax) {
                            out.push(MachineInst::Mov {
                                dest: dest_op,
                                src: Operand::Reg(Reg::Rax),
                            });
                        }
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
                } else if ret_class.is_vector() {
                    let vec_width = if ret_class.is_vector512() {
                        VectorWidth::V512
                    } else {
                        VectorWidth::V256
                    };
                    if let Some(v) = val {
                        if let Some(simd_loc) = self.get_simd_loc(*v) {
                            match simd_loc {
                                SimdLocation::Reg(r) => {
                                    if r != SimdReg(0) {
                                        out.push(MachineInst::Vmovups {
                                            width: vec_width,
                                            dest: SimdOperand::Reg(SimdReg(0)),
                                            src: SimdOperand::Reg(r),
                                        });
                                    }
                                }
                                SimdLocation::Spill(slot) => {
                                    let spill_op = self.frame.get_simd_spill_op(slot);
                                    if let Operand::Mem { base, offset } = spill_op {
                                        out.push(MachineInst::Vmovups {
                                            width: vec_width,
                                            dest: SimdOperand::Reg(SimdReg(0)),
                                            src: SimdOperand::Mem { base, offset },
                                        });
                                    }
                                }
                            }
                        } else {
                            let val_op = self.get_val_op(*v);
                            let base_r = match val_op {
                                Operand::Reg(r) => r,
                                _ => {
                                    out.push(MachineInst::Mov {
                                        dest: Operand::Reg(Reg::R10),
                                        src: val_op,
                                    });
                                    Reg::R10
                                }
                            };
                            out.push(MachineInst::Vmovups {
                                width: vec_width,
                                dest: SimdOperand::Reg(SimdReg(0)),
                                src: SimdOperand::Mem {
                                    base: base_r,
                                    offset: 0,
                                },
                            });
                        }
                    }
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
