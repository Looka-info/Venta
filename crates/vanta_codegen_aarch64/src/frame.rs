//! Stack frame layout and AArch64 ABI prologue/epilogue generation.

use crate::abi::classify_arm_type;
use crate::encoding::{ArmCodeEmitter, ArmInst};
use crate::reg_alloc::ArmAllocationResult;
use crate::register::AArch64Reg;
use std::collections::HashMap;
use vanta_mir::{MirFunction, SlotId, ValueId};
use vanta_sema::{EnumDef, StructDef, Type};

#[derive(Debug)]
pub struct ArmFrameLayout {
    pub slot_offsets: HashMap<SlotId, u32>,
    pub value_offsets: HashMap<ValueId, u32>,
    pub call_return_buffers: HashMap<ValueId, u32>,
    pub spill_offsets: HashMap<i32, u32>,
    pub used_callee_saved: Vec<AArch64Reg>,
    pub sret_offset: Option<u32>,
    pub total_frame_size: u32,
}

impl ArmFrameLayout {
    pub fn compute(
        func: &MirFunction,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
        alloc_res: Option<&ArmAllocationResult>,
    ) -> Self {
        let mut slot_offsets = HashMap::new();
        let mut value_offsets = HashMap::new();
        let mut call_return_buffers = HashMap::new();
        let mut spill_offsets = HashMap::new();

        let used_callee_saved = alloc_res
            .map(|a| a.used_callee_saved.clone())
            .unwrap_or_default();

        // Saved FP (8) + LR (8) = 16 bytes
        let mut current_offset = 16u32;

        // Callee-saved registers space
        let callee_count = used_callee_saved.len() as u32;
        let callee_bytes = (callee_count * 8 + 15) & !15;
        current_offset += callee_bytes;

        // Sret hidden pointer slot
        let ret_class = classify_arm_type(&func.ret_type, structs, enums);
        let sret_offset = if ret_class.is_memory() {
            let off = current_offset;
            current_offset += 8;
            Some(off)
        } else {
            None
        };

        // Local variable slots
        for slot in &func.slots {
            let align = (slot.align.max(8) as u32).max(8);
            let size = (slot.size.max(8) as u32).max(8);
            current_offset = (current_offset + align - 1) & !(align - 1);
            slot_offsets.insert(slot.id, current_offset);
            current_offset += size;
        }

        // Return buffers and temporary value slots
        for block in &func.blocks {
            for inst in &block.instructions {
                if let vanta_mir::Instruction::Call {
                    dest: Some(d),
                    ret_type,
                    ..
                } = inst
                {
                    let call_ret = classify_arm_type(ret_type, structs, enums);
                    if call_ret.is_memory() {
                        let size = (call_ret.size().max(8) as u32).max(8);
                        current_offset = (current_offset + 7) & !7;
                        call_return_buffers.insert(*d, current_offset);
                        current_offset += size;
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
                    let needs_offset = match alloc_res {
                        Some(alloc) => alloc.assignments.get(&v).is_none(),
                        None => true,
                    };
                    if needs_offset {
                        value_offsets.entry(v).or_insert_with(|| {
                            let off = current_offset;
                            current_offset += 8;
                            off
                        });
                    }
                }
            }
        }

        // Spills
        if let Some(alloc) = alloc_res {
            for spill_idx in 0..(alloc.num_spills as i32) {
                let off = current_offset;
                current_offset += 8;
                spill_offsets.insert(spill_idx, off);
            }
        }

        // Ensure total frame size is 16-byte aligned (minimum 32 bytes)
        let total_frame_size = (current_offset + 15) & !15;
        let total_frame_size = total_frame_size.max(32);

        Self {
            slot_offsets,
            value_offsets,
            call_return_buffers,
            spill_offsets,
            used_callee_saved,
            sret_offset,
            total_frame_size,
        }
    }

    pub fn emit_prologue(
        &self,
        emitter: &mut ArmCodeEmitter,
        func: &MirFunction,
        _structs: &HashMap<String, StructDef>,
        _enums: &HashMap<String, EnumDef>,
    ) {
        // stp x29, x30, [sp, -frame_size]!
        emitter.encode_instruction(&ArmInst::StpPreIndex {
            r1: AArch64Reg::X29,
            r2: AArch64Reg::X30,
            base: AArch64Reg::SP,
            imm: -(self.total_frame_size as i32),
        });

        // mov x29, sp
        emitter.encode_instruction(&ArmInst::MovReg {
            dest: AArch64Reg::X29,
            src: AArch64Reg::SP,
        });

        // Save callee-saved registers
        let mut save_off = 16u32;
        for reg in &self.used_callee_saved {
            emitter.encode_instruction(&ArmInst::Str {
                src: *reg,
                base: AArch64Reg::X29,
                offset: save_off,
            });
            save_off += 8;
        }

        // Save incoming sret pointer from x8 if needed
        if let Some(sret_off) = self.sret_offset {
            emitter.encode_instruction(&ArmInst::Str {
                src: AArch64Reg::X8,
                base: AArch64Reg::X29,
                offset: sret_off,
            });
        }

        // Spill incoming arguments into their allocated stack slots
        let mut reg_idx = 0usize;
        let mut current_stack_offset = 0usize;

        for (slot, ty) in &func.params {
            let slot_offset = self.slot_offsets.get(slot).copied().unwrap_or(16);
            if ty.is_str_slice() || ty.is_slice() || ty.is_trait_object() {
                if reg_idx + 1 < AArch64Reg::ARG_REGS.len() {
                    let r1 = AArch64Reg::ARG_REGS[reg_idx];
                    let r2 = AArch64Reg::ARG_REGS[reg_idx + 1];
                    reg_idx += 2;
                    emitter.encode_instruction(&ArmInst::Str {
                        src: r1,
                        base: AArch64Reg::X29,
                        offset: slot_offset,
                    });
                    emitter.encode_instruction(&ArmInst::Str {
                        src: r2,
                        base: AArch64Reg::X29,
                        offset: slot_offset + 8,
                    });
                } else {
                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let stack_off1 = (self.total_frame_size as usize) + current_stack_offset;
                    current_stack_offset += 8;
                    emitter.encode_instruction(&ArmInst::Ldr {
                        dest: AArch64Reg::X9,
                        base: AArch64Reg::X29,
                        offset: stack_off1 as u32,
                    });
                    emitter.encode_instruction(&ArmInst::Str {
                        src: AArch64Reg::X9,
                        base: AArch64Reg::X29,
                        offset: slot_offset,
                    });

                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let stack_off2 = (self.total_frame_size as usize) + current_stack_offset;
                    current_stack_offset += 8;
                    emitter.encode_instruction(&ArmInst::Ldr {
                        dest: AArch64Reg::X9,
                        base: AArch64Reg::X29,
                        offset: stack_off2 as u32,
                    });
                    emitter.encode_instruction(&ArmInst::Str {
                        src: AArch64Reg::X9,
                        base: AArch64Reg::X29,
                        offset: slot_offset + 8,
                    });
                }
            } else if matches!(ty, Type::Struct(_) | Type::Enum(_)) {
                let slot_size = func
                    .slots
                    .iter()
                    .find(|s| s.id == *slot)
                    .map(|s| s.size)
                    .unwrap_or(8);

                let ptr_reg = if reg_idx < AArch64Reg::ARG_REGS.len() {
                    let r = AArch64Reg::ARG_REGS[reg_idx];
                    reg_idx += 1;
                    r
                } else {
                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let stack_off = (self.total_frame_size as usize) + current_stack_offset;
                    current_stack_offset += 8;
                    emitter.encode_instruction(&ArmInst::Ldr {
                        dest: AArch64Reg::X16,
                        base: AArch64Reg::X29,
                        offset: stack_off as u32,
                    });
                    AArch64Reg::X16
                };

                for off in (0..slot_size).step_by(8) {
                    emitter.encode_instruction(&ArmInst::Ldr {
                        dest: AArch64Reg::X9,
                        base: ptr_reg,
                        offset: off as u32,
                    });
                    emitter.encode_instruction(&ArmInst::Str {
                        src: AArch64Reg::X9,
                        base: AArch64Reg::X29,
                        offset: slot_offset + (off as u32),
                    });
                }
            } else if reg_idx < AArch64Reg::ARG_REGS.len() {
                let r = AArch64Reg::ARG_REGS[reg_idx];
                reg_idx += 1;
                emitter.encode_instruction(&ArmInst::Str {
                    src: r,
                    base: AArch64Reg::X29,
                    offset: slot_offset,
                });
            } else {
                current_stack_offset = (current_stack_offset + 7) & !7;
                let stack_off = (self.total_frame_size as usize) + current_stack_offset;
                current_stack_offset += 8;
                emitter.encode_instruction(&ArmInst::Ldr {
                    dest: AArch64Reg::X9,
                    base: AArch64Reg::X29,
                    offset: stack_off as u32,
                });
                emitter.encode_instruction(&ArmInst::Str {
                    src: AArch64Reg::X9,
                    base: AArch64Reg::X29,
                    offset: slot_offset,
                });
            }
        }
    }

    pub fn emit_epilogue(&self, emitter: &mut ArmCodeEmitter) {
        // Restore callee-saved registers
        let mut save_off = 16u32;
        for reg in &self.used_callee_saved {
            emitter.encode_instruction(&ArmInst::Ldr {
                dest: *reg,
                base: AArch64Reg::X29,
                offset: save_off,
            });
            save_off += 8;
        }

        // ldp x29, x30, [sp], frame_size
        emitter.encode_instruction(&ArmInst::LdpPostIndex {
            r1: AArch64Reg::X29,
            r2: AArch64Reg::X30,
            base: AArch64Reg::SP,
            imm: self.total_frame_size as i32,
        });

        // ret
        emitter.encode_instruction(&ArmInst::Ret);
    }
}
