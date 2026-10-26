//! Pure Rust x86-64 machine code instruction encoder.

use std::collections::HashMap;
use crate::machine_ir::{ConditionCode, MachineInst, Operand};
use crate::register::Reg;

#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: usize,
    pub symbol: String,
    pub addend: i64,
}

pub struct CodeEmitter {
    pub bytes: Vec<u8>,
    pub label_offsets: HashMap<String, usize>,
    pub label_fixups: Vec<(usize, String)>, // (offset_of_rel32, target_label)
    pub relocations: Vec<Relocation>,
}

impl CodeEmitter {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            label_offsets: HashMap::new(),
            label_fixups: Vec::new(),
            relocations: Vec::new(),
        }
    }

    pub fn emit_u8(&mut self, b: u8) {
        self.bytes.push(b);
    }

    pub fn emit_u32(&mut self, val: u32) {
        self.bytes.extend_from_slice(&val.to_le_bytes());
    }

    pub fn emit_i32(&mut self, val: i32) {
        self.bytes.extend_from_slice(&val.to_le_bytes());
    }

    pub fn emit_i64(&mut self, val: i64) {
        self.bytes.extend_from_slice(&val.to_le_bytes());
    }

    /// Construct REX byte: 0100 W R X B
    fn rex(w: bool, r: bool, x: bool, b: bool) -> u8 {
        let mut byte = 0x40;
        if w {
            byte |= 0x08;
        }
        if r {
            byte |= 0x04;
        }
        if x {
            byte |= 0x02;
        }
        if b {
            byte |= 0x01;
        }
        byte
    }

    /// Construct ModR/M byte: [mod: 2][reg: 3][rm: 3]
    fn mod_rm(mod_bits: u8, reg_bits: u8, rm_bits: u8) -> u8 {
        ((mod_bits & 0b11) << 6) | ((reg_bits & 0b111) << 3) | (rm_bits & 0b111)
    }

    /// Construct SIB byte: [scale: 2][index: 3][base: 3]
    fn sib(scale: u8, index: u8, base: u8) -> u8 {
        ((scale & 0b11) << 6) | ((index & 0b111) << 3) | (base & 0b111)
    }

    fn emit_mem_operand(&mut self, reg_field: u8, base: Reg, offset: i32) {
        let base_hw = base.hw_encoding();
        if (-128..=127).contains(&offset) {
            // mod = 01 (disp8)
            let mod_rm = Self::mod_rm(0b01, reg_field, base_hw);
            self.emit_u8(mod_rm);
            if base_hw == 4 {
                // RSP requires SIB byte
                self.emit_u8(Self::sib(0, 4, 4));
            }
            self.emit_u8(offset as i8 as u8);
        } else {
            // mod = 10 (disp32)
            let mod_rm = Self::mod_rm(0b10, reg_field, base_hw);
            self.emit_u8(mod_rm);
            if base_hw == 4 {
                self.emit_u8(Self::sib(0, 4, 4));
            }
            self.emit_i32(offset);
        }
    }

    pub fn encode_instruction(&mut self, inst: &MachineInst) {
        match inst {
            MachineInst::Label(name) => {
                self.label_offsets.insert(name.clone(), self.bytes.len());
            }
            MachineInst::Push(reg) => {
                if reg.is_extended() {
                    self.emit_u8(Self::rex(false, false, false, true));
                }
                self.emit_u8(0x50 + reg.hw_encoding());
            }
            MachineInst::Pop(reg) => {
                if reg.is_extended() {
                    self.emit_u8(Self::rex(false, false, false, true));
                }
                self.emit_u8(0x58 + reg.hw_encoding());
            }
            MachineInst::Ret => {
                self.emit_u8(0xC3);
            }
            MachineInst::Cqo => {
                self.emit_u8(Self::rex(true, false, false, false));
                self.emit_u8(0x99);
            }
            MachineInst::Mov { dest, src } => {
                match (dest, src) {
                    (Operand::Reg(dst), Operand::Reg(s)) => {
                        self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                        self.emit_u8(0x89);
                        self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                    }
                    (Operand::Reg(dst), Operand::Imm(val)) => {
                        // Check if fits in 32-bit sign-extended
                        if *val >= i32::MIN as i64 && *val <= i32::MAX as i64 {
                            self.emit_u8(Self::rex(true, false, false, dst.is_extended()));
                            self.emit_u8(0xC7);
                            self.emit_u8(Self::mod_rm(0b11, 0, dst.hw_encoding()));
                            self.emit_i32(*val as i32);
                        } else {
                            // movabs dst, imm64
                            self.emit_u8(Self::rex(true, false, false, dst.is_extended()));
                            self.emit_u8(0xB8 + dst.hw_encoding());
                            self.emit_i64(*val);
                        }
                    }
                    (Operand::Mem { base, offset }, Operand::Reg(s)) => {
                        self.emit_u8(Self::rex(true, s.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x89);
                        self.emit_mem_operand(s.hw_encoding(), *base, *offset);
                    }
                    (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                        self.emit_u8(Self::rex(true, dst.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x8B);
                        self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                    }
                    (Operand::Mem { base, offset }, Operand::Imm(val)) => {
                        self.emit_u8(Self::rex(true, false, false, base.is_extended()));
                        self.emit_u8(0xC7);
                        self.emit_mem_operand(0, *base, *offset);
                        self.emit_i32(*val as i32);
                    }
                    _ => panic!("Unsupported Mov combination: {:?} -> {:?}", src, dest),
                }
            }
            MachineInst::Add { dest, src } => {
                match (dest, src) {
                    (Operand::Reg(dst), Operand::Reg(s)) => {
                        self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                        self.emit_u8(0x01);
                        self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                    }
                    (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                        self.emit_u8(Self::rex(true, dst.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x03);
                        self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                    }
                    (Operand::Reg(dst), Operand::Imm(val)) => {
                        self.emit_u8(Self::rex(true, false, false, dst.is_extended()));
                        self.emit_u8(0x81);
                        self.emit_u8(Self::mod_rm(0b11, 0, dst.hw_encoding()));
                        self.emit_i32(*val as i32);
                    }
                    _ => panic!("Unsupported Add operands: {:?}", inst),
                }
            }
            MachineInst::Sub { dest, src } => {
                match (dest, src) {
                    (Operand::Reg(dst), Operand::Reg(s)) => {
                        self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                        self.emit_u8(0x29);
                        self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                    }
                    (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                        self.emit_u8(Self::rex(true, dst.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x2B);
                        self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                    }
                    (Operand::Reg(dst), Operand::Imm(val)) => {
                        self.emit_u8(Self::rex(true, false, false, dst.is_extended()));
                        self.emit_u8(0x81);
                        self.emit_u8(Self::mod_rm(0b11, 5, dst.hw_encoding()));
                        self.emit_i32(*val as i32);
                    }
                    _ => panic!("Unsupported Sub operands: {:?}", inst),
                }
            }
            MachineInst::Imul { dest, src } => {
                match src {
                    Operand::Reg(s) => {
                        self.emit_u8(Self::rex(true, dest.is_extended(), false, s.is_extended()));
                        self.emit_u8(0x0F);
                        self.emit_u8(0xAF);
                        self.emit_u8(Self::mod_rm(0b11, dest.hw_encoding(), s.hw_encoding()));
                    }
                    Operand::Mem { base, offset } => {
                        self.emit_u8(Self::rex(true, dest.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x0F);
                        self.emit_u8(0xAF);
                        self.emit_mem_operand(dest.hw_encoding(), *base, *offset);
                    }
                    Operand::Imm(val) => {
                        self.emit_u8(Self::rex(true, dest.is_extended(), false, dest.is_extended()));
                        self.emit_u8(0x69);
                        self.emit_u8(Self::mod_rm(0b11, dest.hw_encoding(), dest.hw_encoding()));
                        self.emit_i32(*val as i32);
                    }
                    _ => panic!("Unsupported Imul operand: {:?}", src),
                }
            }
            MachineInst::Idiv(reg) => {
                self.emit_u8(Self::rex(true, false, false, reg.is_extended()));
                self.emit_u8(0xF7);
                self.emit_u8(Self::mod_rm(0b11, 7, reg.hw_encoding()));
            }
            MachineInst::And { dest, src } => {
                match (dest, src) {
                    (Operand::Reg(dst), Operand::Reg(s)) => {
                        self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                        self.emit_u8(0x21);
                        self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                    }
                    (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                        self.emit_u8(Self::rex(true, dst.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x23);
                        self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                    }
                    _ => panic!("Unsupported And operands: {:?}", inst),
                }
            }
            MachineInst::Or { dest, src } => {
                match (dest, src) {
                    (Operand::Reg(dst), Operand::Reg(s)) => {
                        self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                        self.emit_u8(0x09);
                        self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                    }
                    (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                        self.emit_u8(Self::rex(true, dst.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x0B);
                        self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                    }
                    _ => panic!("Unsupported Or operands: {:?}", inst),
                }
            }
            MachineInst::Xor { dest, src } => {
                match (dest, src) {
                    (Operand::Reg(dst), Operand::Reg(s)) => {
                        self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                        self.emit_u8(0x31);
                        self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                    }
                    (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                        self.emit_u8(Self::rex(true, dst.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x33);
                        self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                    }
                    _ => panic!("Unsupported Xor operands: {:?}", inst),
                }
            }
            MachineInst::Shl { dest, count } => {
                match count {
                    Operand::Reg(Reg::Rcx) => {
                        self.emit_u8(Self::rex(true, false, false, dest.is_extended()));
                        self.emit_u8(0xD3);
                        self.emit_u8(Self::mod_rm(0b11, 4, dest.hw_encoding()));
                    }
                    Operand::Imm(val) => {
                        self.emit_u8(Self::rex(true, false, false, dest.is_extended()));
                        self.emit_u8(0xC1);
                        self.emit_u8(Self::mod_rm(0b11, 4, dest.hw_encoding()));
                        self.emit_u8(*val as u8);
                    }
                    _ => panic!("Unsupported Shl count: {:?}", count),
                }
            }
            MachineInst::Shr { dest, count } => {
                match count {
                    Operand::Reg(Reg::Rcx) => {
                        self.emit_u8(Self::rex(true, false, false, dest.is_extended()));
                        self.emit_u8(0xD3);
                        self.emit_u8(Self::mod_rm(0b11, 5, dest.hw_encoding()));
                    }
                    Operand::Imm(val) => {
                        self.emit_u8(Self::rex(true, false, false, dest.is_extended()));
                        self.emit_u8(0xC1);
                        self.emit_u8(Self::mod_rm(0b11, 5, dest.hw_encoding()));
                        self.emit_u8(*val as u8);
                    }
                    _ => panic!("Unsupported Shr count: {:?}", count),
                }
            }
            MachineInst::Neg(reg) => {
                self.emit_u8(Self::rex(true, false, false, reg.is_extended()));
                self.emit_u8(0xF7);
                self.emit_u8(Self::mod_rm(0b11, 3, reg.hw_encoding()));
            }
            MachineInst::Not(reg) => {
                self.emit_u8(Self::rex(true, false, false, reg.is_extended()));
                self.emit_u8(0xF7);
                self.emit_u8(Self::mod_rm(0b11, 2, reg.hw_encoding()));
            }
            MachineInst::Cmp { lhs, rhs } => {
                match (lhs, rhs) {
                    (Operand::Reg(l), Operand::Reg(r)) => {
                        self.emit_u8(Self::rex(true, r.is_extended(), false, l.is_extended()));
                        self.emit_u8(0x39);
                        self.emit_u8(Self::mod_rm(0b11, r.hw_encoding(), l.hw_encoding()));
                    }
                    (Operand::Reg(l), Operand::Mem { base, offset }) => {
                        self.emit_u8(Self::rex(true, l.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x3B);
                        self.emit_mem_operand(l.hw_encoding(), *base, *offset);
                    }
                    (Operand::Reg(l), Operand::Imm(val)) => {
                        self.emit_u8(Self::rex(true, false, false, l.is_extended()));
                        self.emit_u8(0x81);
                        self.emit_u8(Self::mod_rm(0b11, 7, l.hw_encoding()));
                        self.emit_i32(*val as i32);
                    }
                    _ => panic!("Unsupported Cmp operands: {:?}", inst),
                }
            }
            MachineInst::Test { lhs, rhs } => {
                self.emit_u8(Self::rex(true, rhs.is_extended(), false, lhs.is_extended()));
                self.emit_u8(0x85);
                self.emit_u8(Self::mod_rm(0b11, rhs.hw_encoding(), lhs.hw_encoding()));
            }
            MachineInst::Setcc { cond, dest } => {
                let cond_byte = match cond {
                    ConditionCode::Equal => 0x94,
                    ConditionCode::NotEqual => 0x95,
                    ConditionCode::Less => 0x9C,
                    ConditionCode::LessEqual => 0x9E,
                    ConditionCode::Greater => 0x9F,
                    ConditionCode::GreaterEqual => 0x9D,
                };
                if dest.is_extended() {
                    self.emit_u8(Self::rex(false, false, false, true));
                }
                self.emit_u8(0x0F);
                self.emit_u8(cond_byte);
                self.emit_u8(Self::mod_rm(0b11, 0, dest.hw_encoding()));
            }
            MachineInst::Movzx { dest, src } => {
                let rex = Self::rex(true, dest.is_extended(), false, src.is_extended());
                self.emit_u8(rex);
                self.emit_u8(0x0F);
                self.emit_u8(0xB6);
                self.emit_u8(Self::mod_rm(0b11, dest.hw_encoding(), src.hw_encoding()));
            }
            MachineInst::Lea { dest, src } => {
                match src {
                    Operand::Mem { base, offset } => {
                        self.emit_u8(Self::rex(true, dest.is_extended(), false, base.is_extended()));
                        self.emit_u8(0x8D);
                        self.emit_mem_operand(dest.hw_encoding(), *base, *offset);
                    }
                    _ => panic!("Lea source must be memory: {:?}", src),
                }
            }
            MachineInst::Jmp(target) => {
                self.emit_u8(0xE9);
                let fixup_pos = self.bytes.len();
                self.emit_i32(0); // placeholder
                self.label_fixups.push((fixup_pos, target.clone()));
            }
            MachineInst::Jcc { cond, target } => {
                let cond_byte = match cond {
                    ConditionCode::Equal => 0x84,
                    ConditionCode::NotEqual => 0x85,
                    ConditionCode::Less => 0x8C,
                    ConditionCode::LessEqual => 0x8E,
                    ConditionCode::Greater => 0x8F,
                    ConditionCode::GreaterEqual => 0x8D,
                };
                self.emit_u8(0x0F);
                self.emit_u8(cond_byte);
                let fixup_pos = self.bytes.len();
                self.emit_i32(0); // placeholder
                self.label_fixups.push((fixup_pos, target.clone()));
            }
            MachineInst::Call(symbol) => {
                self.emit_u8(0xE8);
                let rel_offset = self.bytes.len();
                self.emit_i32(-4); // Standard addend for x86-64 CALL relocations
                self.relocations.push(Relocation {
                    offset: rel_offset,
                    symbol: symbol.clone(),
                    addend: -4,
                });
            }
        }
    }

    /// Patch local jump offsets after all instructions and labels are emitted.
    pub fn resolve_fixups(&mut self) {
        for (fixup_offset, target_label) in &self.label_fixups {
            if let Some(&target_offset) = self.label_offsets.get(target_label) {
                // rel32 is relative to the address immediately following the 4-byte immediate
                let next_ip = (fixup_offset + 4) as i32;
                let rel32 = (target_offset as i32) - next_ip;
                self.bytes[*fixup_offset..*fixup_offset + 4]
                    .copy_from_slice(&rel32.to_le_bytes());
            } else {
                panic!("Unresolved local jump label: {}", target_label);
            }
        }
    }
}
