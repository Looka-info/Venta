//! Stack frame layout and System V AMD64 ABI alignment.

use crate::machine_ir::{MachineInst, Operand};
use crate::register::Reg;
use std::collections::HashMap;
use vanta_mir::{MirFunction, SlotId, ValueId};

#[derive(Debug)]
pub struct FrameLayout {
    pub slot_offsets: HashMap<SlotId, i32>,
    pub value_offsets: HashMap<ValueId, i32>,
    pub total_frame_size: i32,
}

impl FrameLayout {
    pub fn compute(func: &MirFunction) -> Self {
        let mut slot_offsets = HashMap::new();
        let mut value_offsets = HashMap::new();
        let mut current_offset = 0;

        // Allocate slots for local variables
        for slot in &func.slots {
            let align = slot.align.max(8) as i32;
            let size = slot.size.max(8) as i32;
            current_offset = (current_offset + align - 1) & !(align - 1);
            current_offset += size;
            slot_offsets.insert(slot.id, current_offset);
        }

        // Allocate slots for temporary values
        for block in &func.blocks {
            for inst in &block.instructions {
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
                    | vanta_mir::Instruction::LoadIndirect { dest, .. } => vec![*dest],
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
                    value_offsets.entry(v).or_insert_with(|| {
                        current_offset += 8;
                        current_offset
                    });
                }
            }
        }

        // Align stack frame to 16 bytes (System V AMD64 ABI)
        // Since `call` pushes 8 bytes return address, and `push rbp` pushes 8 bytes,
        // (rsp is 16-byte aligned after push rbp). Thus, total_frame_size must be a multiple of 16.
        let aligned_size = (current_offset + 15) & !15;

        Self {
            slot_offsets,
            value_offsets,
            total_frame_size: aligned_size,
        }
    }

    pub fn get_slot_op(&self, slot: SlotId) -> Operand {
        let offset = self.slot_offsets.get(&slot).copied().unwrap_or(8);
        Operand::Mem {
            base: Reg::Rbp,
            offset: -offset,
        }
    }

    pub fn get_value_op(&self, val: ValueId) -> Operand {
        let offset = self.value_offsets.get(&val).copied().unwrap_or(8);
        Operand::Mem {
            base: Reg::Rbp,
            offset: -offset,
        }
    }

    pub fn emit_prologue(&self, insts: &mut Vec<MachineInst>, func: &MirFunction) {
        // push rbp
        insts.push(MachineInst::Push(Reg::Rbp));
        // mov rbp, rsp
        insts.push(MachineInst::Mov {
            dest: Operand::Reg(Reg::Rbp),
            src: Operand::Reg(Reg::Rsp),
        });
        // sub rsp, total_frame_size
        if self.total_frame_size > 0 {
            insts.push(MachineInst::Sub {
                dest: Operand::Reg(Reg::Rsp),
                src: Operand::Imm(self.total_frame_size as i64),
            });
        }

        // Spill incoming register arguments into their allocated stack slots
        let mut arg_reg_idx = 0;
        for (slot, ty) in &func.params {
            if ty.is_str_slice() || ty.is_slice() {
                if arg_reg_idx + 1 < Reg::ARG_REGS.len() {
                    let ptr_reg = Reg::ARG_REGS[arg_reg_idx];
                    let len_reg = Reg::ARG_REGS[arg_reg_idx + 1];
                    arg_reg_idx += 2;
                    let slot_offset = self.slot_offsets.get(slot).copied().unwrap_or(16);
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset,
                        },
                        src: Operand::Reg(ptr_reg),
                    });
                    insts.push(MachineInst::Mov {
                        dest: Operand::Mem {
                            base: Reg::Rbp,
                            offset: -slot_offset + 8,
                        },
                        src: Operand::Reg(len_reg),
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
            }
        }
    }

    pub fn emit_epilogue(&self, insts: &mut Vec<MachineInst>) {
        if self.total_frame_size > 0 {
            insts.push(MachineInst::Mov {
                dest: Operand::Reg(Reg::Rsp),
                src: Operand::Reg(Reg::Rbp),
            });
        }
        insts.push(MachineInst::Pop(Reg::Rbp));
        insts.push(MachineInst::Ret);
    }
}
