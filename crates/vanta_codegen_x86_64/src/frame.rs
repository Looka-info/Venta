//! Stack frame layout and System V AMD64 ABI alignment.

use crate::abi::classify_type;
use crate::machine_ir::{MachineInst, Operand, SimdOperand};
use crate::register::{Reg, VectorWidth};
use crate::simd_alloc::SimdRegisterAllocator;
use std::collections::HashMap;
use vanta_mir::{MirFunction, SlotId, ValueId};
use vanta_sema::{EnumDef, StructDef, Type};

#[derive(Debug)]
pub struct FrameLayout {
    pub slot_offsets: HashMap<SlotId, i32>,
    pub value_offsets: HashMap<ValueId, i32>,
    pub call_return_buffers: HashMap<ValueId, i32>,
    pub spill_offsets: HashMap<i32, i32>,
    pub simd_spill_offsets: HashMap<u32, i32>,
    pub used_callee_saved: Vec<Reg>,
    pub sret_offset: Option<i32>,
    pub has_simd_spills: bool,
    pub requires_32byte_alignment: bool,
    pub requires_64byte_alignment: bool,
    pub total_frame_size: i32,
}

impl FrameLayout {
    pub fn compute(
        func: &MirFunction,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
        alloc_res: Option<&crate::reg_alloc::AllocationResult>,
    ) -> Self {
        Self::compute_with_simd(func, structs, enums, alloc_res, None)
    }

    pub fn compute_with_simd(
        func: &MirFunction,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
        alloc_res: Option<&crate::reg_alloc::AllocationResult>,
        simd_alloc_res: Option<&crate::simd_alloc::SimdAllocResult>,
    ) -> Self {
        let mut slot_offsets = HashMap::new();
        let mut value_offsets = HashMap::new();
        let mut call_return_buffers = HashMap::new();
        let mut spill_offsets = HashMap::new();
        let mut simd_spill_offsets = HashMap::new();

        let used_callee_saved = alloc_res
            .map(|a| a.used_callee_saved.clone())
            .unwrap_or_default();
        let callee_count = used_callee_saved.len() as i32;

        // In System V AMD64 ABI, callee-saved registers pushed right after `mov rbp, rsp`
        // occupy offsets -8, -16, ..., -(callee_count * 8).
        // Stack locals and spills must therefore start below them.
        let mut current_offset = callee_count * 8;

        // 1. If function returns an aggregate classified as MEMORY, allocate 8 bytes
        // for the incoming hidden %rdi return pointer storage.
        let ret_class = classify_type(&func.ret_type, structs, enums);
        let sret_offset = if ret_class.is_memory() {
            current_offset += 8;
            Some(current_offset)
        } else {
            None
        };

        // 2. Allocate slots for local variables
        for slot in &func.slots {
            let is_vec512 = slot.size == 64 && slot.align >= 64;
            let is_vec256 = slot.size == 32 && slot.align >= 32;
            let align = if is_vec512 {
                64
            } else if is_vec256 {
                32
            } else {
                slot.align.max(8) as i32
            };
            let size = slot.size.max(8) as i32;
            current_offset = (current_offset + align - 1) & !(align - 1);
            current_offset += size;
            slot_offsets.insert(slot.id, current_offset);
        }

        // 3. Allocate return buffers and temporary value slots
        for block in &func.blocks {
            for inst in &block.instructions {
                if let vanta_mir::Instruction::Call {
                    dest: Some(d),
                    ret_type,
                    ..
                } = inst
                {
                    let call_ret_class = classify_type(ret_type, structs, enums);
                    if call_ret_class.is_memory() {
                        let align = match call_ret_class {
                            crate::abi::AbiClass::Memory { align, .. } => align.max(8) as i32,
                            _ => 8,
                        };
                        let size = call_ret_class.size().max(8) as i32;
                        // Allocate `size` bytes for the return buffer
                        current_offset = (current_offset + align - 1) & !(align - 1);
                        current_offset += size;
                        call_return_buffers.insert(*d, current_offset);

                        let needs_val_offset = match alloc_res {
                            Some(alloc) => alloc.assignments.get(d).is_none(),
                            None => true,
                        };
                        if needs_val_offset {
                            current_offset += 8;
                            value_offsets.insert(*d, current_offset);
                        }
                        continue;
                    }
                }

                let val_ids = match inst {
                    vanta_mir::Instruction::ConstInt { dest, .. }
                    | vanta_mir::Instruction::ConstBool { dest, .. }
                    | vanta_mir::Instruction::SlotAddr { dest, .. }
                    | vanta_mir::Instruction::FieldAddr { dest, .. }
                    | vanta_mir::Instruction::Load { dest, .. }
                    | vanta_mir::Instruction::Binary { dest, .. }
                    | vanta_mir::Instruction::Unary { dest, .. }
                    | vanta_mir::Instruction::IndexAddr { dest, .. }
                    | vanta_mir::Instruction::IndexPtrAddr { dest, .. }
                    | vanta_mir::Instruction::LoadIndirect { dest, .. }
                    | vanta_mir::Instruction::Phi { dest, .. }
                    | vanta_mir::Instruction::Copy { dest, .. } => vec![*dest],
                    vanta_mir::Instruction::StrSlice {
                        dest_ptr, dest_len, ..
                    }
                    | vanta_mir::Instruction::LoadSlice {
                        dest_ptr, dest_len, ..
                    } => vec![*dest_ptr, *dest_len],
                    vanta_mir::Instruction::Call { dest: Some(d), .. } => vec![*d],
                    _ => vec![],
                };
                for v in val_ids {
                    let needs_val_offset = match alloc_res {
                        Some(alloc) => alloc.assignments.get(&v).is_none(),
                        None => true,
                    };
                    if needs_val_offset {
                        value_offsets.entry(v).or_insert_with(|| {
                            current_offset += 8;
                            current_offset
                        });
                    }
                }
            }
        }

        // 4. Allocate spill slots if scalar register allocation is active
        if let Some(alloc) = alloc_res {
            for spill_idx in 0..(alloc.num_spills as i32) {
                current_offset += 8;
                spill_offsets.insert(spill_idx, current_offset);
            }
        }

        // 5. Allocate aligned spill slots for SIMD vector values
        let num_simd_spills = simd_alloc_res.map(|s| s.num_spills).unwrap_or(0);
        let has_simd_spills = num_simd_spills > 0;
        let mut simd_current = 0i32;
        let mut has_512_spill = false;
        if let Some(s_res) = simd_alloc_res {
            for spill_idx in 0..num_simd_spills as u32 {
                let width = s_res
                    .spill_widths
                    .get(&spill_idx)
                    .copied()
                    .unwrap_or(VectorWidth::V256);
                let align = width.align() as i32;
                let size = width.byte_size() as i32;
                if width == VectorWidth::V512 {
                    has_512_spill = true;
                }
                simd_current = (simd_current + align - 1) & !(align - 1);
                simd_spill_offsets.insert(spill_idx, simd_current);
                simd_current += size;
            }
        }
        let simd_area_bytes = simd_current;

        let has_vector512_params = func
            .params
            .iter()
            .any(|(_, ty)| classify_type(ty, structs, enums).is_vector512());
        let has_vector512_ret = classify_type(&func.ret_type, structs, enums).is_vector512();
        let has_vector512_slots = func.slots.iter().any(|s| s.size == 64 && s.align >= 64);
        let requires_64byte_alignment =
            has_512_spill || has_vector512_params || has_vector512_ret || has_vector512_slots;

        let has_vector256_params = func
            .params
            .iter()
            .any(|(_, ty)| classify_type(ty, structs, enums).is_vector256());
        let has_vector256_ret = classify_type(&func.ret_type, structs, enums).is_vector256();
        let has_vector256_slots = func.slots.iter().any(|s| s.size == 32 && s.align >= 32);
        let requires_32byte_alignment = requires_64byte_alignment
            || has_simd_spills
            || has_vector256_params
            || has_vector256_ret
            || has_vector256_slots;

        // Align stack frame:
        // When dynamic 64-byte alignment is required, frame size is a multiple of 64 bytes.
        // When dynamic 32-byte alignment is required, frame size is a multiple of 32 bytes.
        let total = current_offset + simd_area_bytes;
        let aligned_total = if requires_64byte_alignment {
            (total + 63) & !63
        } else if requires_32byte_alignment {
            (total + 31) & !31
        } else {
            (total + 15) & !15
        };
        let frame_size = aligned_total - (callee_count * 8);

        Self {
            slot_offsets,
            value_offsets,
            call_return_buffers,
            spill_offsets,
            simd_spill_offsets,
            used_callee_saved,
            sret_offset,
            has_simd_spills,
            requires_32byte_alignment,
            requires_64byte_alignment,
            total_frame_size: frame_size,
        }
    }

    pub fn get_simd_spill_op(&self, slot: u32) -> Operand {
        let offset = self
            .simd_spill_offsets
            .get(&slot)
            .copied()
            .unwrap_or_else(|| (slot as i32) * 32);
        Operand::Mem {
            base: Reg::Rsp,
            offset,
        }
    }

    pub fn get_slot_op(&self, slot: SlotId) -> Operand {
        let offset = self
            .slot_offsets
            .get(&slot)
            .copied()
            .unwrap_or_else(|| slot.0 as i32 * 8);
        Operand::Mem {
            base: Reg::Rbp,
            offset: -offset,
        }
    }

    pub fn get_val_op(&self, val: ValueId) -> Operand {
        let offset = self
            .value_offsets
            .get(&val)
            .copied()
            .unwrap_or_else(|| val.0 as i32 * 8);
        Operand::Mem {
            base: Reg::Rbp,
            offset: -offset,
        }
    }

    pub fn emit_prologue(
        &self,
        insts: &mut Vec<MachineInst>,
        func: &MirFunction,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
    ) {
        // push rbp
        insts.push(MachineInst::Push(Reg::Rbp));
        // mov rbp, rsp
        insts.push(MachineInst::Mov {
            dest: Operand::Reg(Reg::Rbp),
            src: Operand::Reg(Reg::Rsp),
        });
        // push callee-saved registers
        for reg in &self.used_callee_saved {
            insts.push(MachineInst::Push(*reg));
        }
        // If the function contains SIMD operations or spills, dynamically align %rsp to 64 or 32 bytes
        // to guarantee that all vector slots maintain runtime alignment
        // regardless of incoming caller alignment.
        if self.requires_64byte_alignment {
            insts.push(MachineInst::And {
                dest: Operand::Reg(Reg::Rsp),
                src: Operand::Imm(-64),
            });
        } else if self.requires_32byte_alignment {
            insts.push(MachineInst::And {
                dest: Operand::Reg(Reg::Rsp),
                src: Operand::Imm(-32),
            });
        }
        // sub rsp, total_frame_size
        if self.total_frame_size > 0 {
            insts.push(MachineInst::Sub {
                dest: Operand::Reg(Reg::Rsp),
                src: Operand::Imm(self.total_frame_size as i64),
            });
        }

        if let Some(sret_off) = self.sret_offset {
            // Callee received hidden return buffer pointer in %rdi!
            // Spill it into stack slot
            insts.push(MachineInst::Mov {
                dest: Operand::Mem {
                    base: Reg::Rbp,
                    offset: -sret_off,
                },
                src: Operand::Reg(Reg::Rdi),
            });
        }

        let mut arg_reg_idx = if self.sret_offset.is_some() { 1 } else { 0 };
        let mut vec_reg_idx = 0;
        let mut current_stack_offset = 0usize;

        // Spill incoming arguments into their allocated stack slots
        for (slot, ty) in &func.params {
            let class = classify_type(ty, structs, enums);
            if class.is_vector() {
                let slot_op = self.get_slot_op(*slot);
                let is_512 = class.is_vector512();
                let width = if is_512 {
                    VectorWidth::V512
                } else {
                    VectorWidth::V256
                };
                let scratch = SimdRegisterAllocator::scratch_a(is_512);

                if vec_reg_idx < 8 {
                    let vreg = crate::register::SimdReg(vec_reg_idx as u8);
                    vec_reg_idx += 1;
                    if let Operand::Mem { base, offset } = slot_op {
                        insts.push(MachineInst::Vmovups {
                            width,
                            dest: SimdOperand::Mem { base, offset },
                            src: SimdOperand::Reg(vreg),
                        });
                    }
                } else {
                    let align_mask = if is_512 { 63 } else { 31 };
                    let slot_size = if is_512 { 64 } else { 32 };
                    current_stack_offset = (current_stack_offset + align_mask) & !align_mask;
                    let stack_off = current_stack_offset as i32;
                    current_stack_offset += slot_size;
                    if let Operand::Mem {
                        base,
                        offset: slot_off,
                    } = slot_op
                    {
                        insts.push(MachineInst::Vmovups {
                            width,
                            dest: SimdOperand::Reg(scratch),
                            src: SimdOperand::Mem {
                                base: Reg::Rbp,
                                offset: 16 + stack_off,
                            },
                        });
                        insts.push(MachineInst::Vmovups {
                            width,
                            dest: SimdOperand::Mem {
                                base,
                                offset: slot_off,
                            },
                            src: SimdOperand::Reg(scratch),
                        });
                    }
                }
            } else if ty.is_str_slice() || ty.is_slice() {
                let slot_offset = self.slot_offsets.get(slot).copied().unwrap_or(16);
                if arg_reg_idx + 1 < Reg::ARG_REGS.len() {
                    let r1 = Reg::ARG_REGS[arg_reg_idx];
                    let r2 = Reg::ARG_REGS[arg_reg_idx + 1];
                    arg_reg_idx += 2;
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset,
                        },
                        src: Operand::Reg(r1),
                    });
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset + 8,
                        },
                        src: Operand::Reg(r2),
                    });
                } else if arg_reg_idx < Reg::ARG_REGS.len() {
                    let r1 = Reg::ARG_REGS[arg_reg_idx];
                    arg_reg_idx += 1;
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset,
                        },
                        src: Operand::Reg(r1),
                    });

                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let stack_off = current_stack_offset as i32;
                    current_stack_offset += 8;
                    insts.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: Operand::Mem {
                            base: Reg::Rbp,
                            offset: 16 + stack_off,
                        },
                    });
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset + 8,
                        },
                        src: Operand::Reg(Reg::R10),
                    });
                } else {
                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let stack_off1 = current_stack_offset as i32;
                    current_stack_offset += 8;
                    insts.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: Operand::Mem {
                            base: Reg::Rbp,
                            offset: 16 + stack_off1,
                        },
                    });
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset,
                        },
                        src: Operand::Reg(Reg::R10),
                    });

                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let stack_off2 = current_stack_offset as i32;
                    current_stack_offset += 8;
                    insts.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: Operand::Mem {
                            base: Reg::Rbp,
                            offset: 16 + stack_off2,
                        },
                    });
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset + 8,
                        },
                        src: Operand::Reg(Reg::R10),
                    });
                }
            } else if matches!(ty, Type::Struct(_) | Type::Enum(_)) {
                let slot_offset = self.slot_offsets.get(slot).copied().unwrap_or(8);
                let slot_size = func
                    .slots
                    .iter()
                    .find(|s| s.id == *slot)
                    .map(|s| s.size)
                    .unwrap_or(8);

                let arg_ptr_reg = if arg_reg_idx < Reg::ARG_REGS.len() {
                    let reg = Reg::ARG_REGS[arg_reg_idx];
                    arg_reg_idx += 1;
                    reg
                } else {
                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let stack_off = current_stack_offset as i32;
                    current_stack_offset += 8;
                    insts.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R11),
                        src: Operand::Mem {
                            base: Reg::Rbp,
                            offset: 16 + stack_off,
                        },
                    });
                    Reg::R11
                };

                for off in (0..slot_size).step_by(8) {
                    insts.push(MachineInst::Mov {
                        dest: Operand::Reg(Reg::R10),
                        src: Operand::Mem {
                            base: arg_ptr_reg,
                            offset: off as i32,
                        },
                    });
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset + (off as i32),
                        },
                        src: Operand::Reg(Reg::R10),
                    });
                }
            } else if arg_reg_idx < Reg::ARG_REGS.len() {
                let arg_reg = Reg::ARG_REGS[arg_reg_idx];
                arg_reg_idx += 1;
                let slot_op = self.get_slot_op(*slot);
                insts.push(MachineInst::Mov {
                    dest: slot_op,
                    src: Operand::Reg(arg_reg),
                });
            } else {
                current_stack_offset = (current_stack_offset + 7) & !7;
                let stack_off = current_stack_offset as i32;
                current_stack_offset += 8;
                insts.push(MachineInst::Mov {
                    dest: Operand::Reg(Reg::R10),
                    src: Operand::Mem {
                        base: Reg::Rbp,
                        offset: 16 + stack_off,
                    },
                });
                let slot_op = self.get_slot_op(*slot);
                insts.push(MachineInst::Mov {
                    dest: slot_op,
                    src: Operand::Reg(Reg::R10),
                });
            }
        }
    }

    pub fn emit_epilogue(&self, insts: &mut Vec<MachineInst>) {
        if !self.used_callee_saved.is_empty() {
            let callee_bytes = (self.used_callee_saved.len() * 8) as i32;
            insts.push(MachineInst::Lea {
                dest: Reg::Rsp,
                src: Operand::Mem {
                    base: Reg::Rbp,
                    offset: -callee_bytes,
                },
            });
            for reg in self.used_callee_saved.iter().rev() {
                insts.push(MachineInst::Pop(*reg));
            }
        } else if self.total_frame_size > 0
            || self.requires_32byte_alignment
            || self.requires_64byte_alignment
        {
            insts.push(MachineInst::Mov {
                dest: Operand::Reg(Reg::Rsp),
                src: Operand::Reg(Reg::Rbp),
            });
        }
        insts.push(MachineInst::Pop(Reg::Rbp));
        insts.push(MachineInst::Ret);
    }
}
