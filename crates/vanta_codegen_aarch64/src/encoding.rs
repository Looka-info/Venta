//! Pure-Rust AArch64 (64-bit ARM) Instruction Encoder.
//!
//! Encodes 32-bit fixed-length instructions (little-endian) conforming to the
//! ARM Architecture Reference Manual ARMv8-A.

use crate::register::AArch64Reg;

/// ARM condition codes for conditional branches (b.cond) and conditional set (cset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmCond {
    Eq = 0x0,
    Ne = 0x1,
    Cs = 0x2, // Carry set / unsigned higher or same (hs)
    Cc = 0x3, // Carry clear / unsigned lower (lo)
    Mi = 0x4, // Minus / negative
    Pl = 0x5, // Plus / positive or zero
    Vs = 0x6, // Overflow
    Vc = 0x7, // No overflow
    Hi = 0x8, // Unsigned higher
    Ls = 0x9, // Unsigned lower or same
    Ge = 0xA, // Signed greater than or equal
    Lt = 0xB, // Signed less than
    Gt = 0xC, // Signed greater than
    Le = 0xD, // Signed less than or equal
    Al = 0xE, // Always (unconditional)
}

impl ArmCond {
    pub const fn invert(&self) -> Self {
        match self {
            ArmCond::Eq => ArmCond::Ne,
            ArmCond::Ne => ArmCond::Eq,
            ArmCond::Cs => ArmCond::Cc,
            ArmCond::Cc => ArmCond::Cs,
            ArmCond::Mi => ArmCond::Pl,
            ArmCond::Pl => ArmCond::Mi,
            ArmCond::Vs => ArmCond::Vc,
            ArmCond::Vc => ArmCond::Vs,
            ArmCond::Hi => ArmCond::Ls,
            ArmCond::Ls => ArmCond::Hi,
            ArmCond::Ge => ArmCond::Lt,
            ArmCond::Lt => ArmCond::Ge,
            ArmCond::Gt => ArmCond::Le,
            ArmCond::Le => ArmCond::Gt,
            ArmCond::Al => ArmCond::Al,
        }
    }

    pub const fn code(&self) -> u8 {
        *self as u8
    }
}

/// Abstract machine instruction representation for AArch64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmInst {
    AddImm {
        dest: AArch64Reg,
        src: AArch64Reg,
        imm: u32,
    },
    SubImm {
        dest: AArch64Reg,
        src: AArch64Reg,
        imm: u32,
    },
    AddReg {
        dest: AArch64Reg,
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    SubReg {
        dest: AArch64Reg,
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    Mul {
        dest: AArch64Reg,
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    Sdiv {
        dest: AArch64Reg,
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    Udiv {
        dest: AArch64Reg,
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    AndReg {
        dest: AArch64Reg,
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    OrrReg {
        dest: AArch64Reg,
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    EorReg {
        dest: AArch64Reg,
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    MovReg {
        dest: AArch64Reg,
        src: AArch64Reg,
    },
    Movz {
        dest: AArch64Reg,
        imm: u16,
        shift: u8,
    },
    Movk {
        dest: AArch64Reg,
        imm: u16,
        shift: u8,
    },
    CmpImm {
        src: AArch64Reg,
        imm: u32,
    },
    CmpReg {
        src1: AArch64Reg,
        src2: AArch64Reg,
    },
    Cset {
        dest: AArch64Reg,
        cond: ArmCond,
    },
    Ldr {
        dest: AArch64Reg,
        base: AArch64Reg,
        offset: u32,
    },
    Str {
        src: AArch64Reg,
        base: AArch64Reg,
        offset: u32,
    },
    Ldr32 {
        dest: AArch64Reg,
        base: AArch64Reg,
        offset: u32,
    },
    Str32 {
        src: AArch64Reg,
        base: AArch64Reg,
        offset: u32,
    },
    Ldrb {
        dest: AArch64Reg,
        base: AArch64Reg,
        offset: u32,
    },
    Strb {
        src: AArch64Reg,
        base: AArch64Reg,
        offset: u32,
    },
    StpPreIndex {
        r1: AArch64Reg,
        r2: AArch64Reg,
        base: AArch64Reg,
        imm: i32,
    },
    LdpPostIndex {
        r1: AArch64Reg,
        r2: AArch64Reg,
        base: AArch64Reg,
        imm: i32,
    },
    StpOffset {
        r1: AArch64Reg,
        r2: AArch64Reg,
        base: AArch64Reg,
        offset: i32,
    },
    LdpOffset {
        r1: AArch64Reg,
        r2: AArch64Reg,
        base: AArch64Reg,
        offset: i32,
    },
    B {
        label: String,
    },
    BCond {
        cond: ArmCond,
        label: String,
    },
    Bl {
        callee: String,
    },
    Blr {
        callee: AArch64Reg,
    },
    Ret,
    Adrp {
        dest: AArch64Reg,
        symbol: String,
    },
    Label(String),
}

/// Relocation record for AArch64 code emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmRelocation {
    pub offset: usize,
    pub symbol: String,
    pub addend: i64,
    pub r_type: u8,
}

pub const ARM64_RELOC_UNSIGNED: u8 = 0;
pub const ARM64_RELOC_BRANCH26: u8 = 2;
pub const ARM64_RELOC_PAGE21: u8 = 3;
pub const ARM64_RELOC_PAGEOFF12: u8 = 4;

/// Encodes AArch64 instructions into little-endian bytes with fixup and relocation tracking.
pub struct ArmCodeEmitter {
    pub bytes: Vec<u8>,
    pub relocations: Vec<ArmRelocation>,
    pub label_offsets: std::collections::HashMap<String, usize>,
    fixups: Vec<ArmFixup>,
}

struct ArmFixup {
    offset: usize,
    label: String,
    is_cond: bool,
    cond: ArmCond,
}

impl Default for ArmCodeEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl ArmCodeEmitter {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            relocations: Vec::new(),
            label_offsets: std::collections::HashMap::new(),
            fixups: Vec::new(),
        }
    }

    pub fn emit_u32(&mut self, word: u32) {
        self.bytes.extend_from_slice(&word.to_le_bytes());
    }

    pub fn encode_instruction(&mut self, inst: &ArmInst) {
        match inst {
            ArmInst::Label(lbl) => {
                self.label_offsets.insert(lbl.clone(), self.bytes.len());
            }
            ArmInst::AddImm { dest, src, imm } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src.hw_encoding() as u32;
                let imm12 = (*imm & 0xFFF) as u32;
                self.emit_u32(0x91000000 | (imm12 << 10) | (rn << 5) | rd);
            }
            ArmInst::SubImm { dest, src, imm } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src.hw_encoding() as u32;
                let imm12 = (*imm & 0xFFF) as u32;
                self.emit_u32(0xD1000000 | (imm12 << 10) | (rn << 5) | rd);
            }
            ArmInst::AddReg { dest, src1, src2 } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                self.emit_u32(0x8B000000 | (rm << 16) | (rn << 5) | rd);
            }
            ArmInst::SubReg { dest, src1, src2 } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                self.emit_u32(0xCB000000 | (rm << 16) | (rn << 5) | rd);
            }
            ArmInst::Mul { dest, src1, src2 } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                // madd xd, xn, xm, xzr
                self.emit_u32(0x9B007C00 | (rm << 16) | (rn << 5) | rd);
            }
            ArmInst::Sdiv { dest, src1, src2 } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                self.emit_u32(0x9AC00C00 | (rm << 16) | (rn << 5) | rd);
            }
            ArmInst::Udiv { dest, src1, src2 } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                self.emit_u32(0x9AC00800 | (rm << 16) | (rn << 5) | rd);
            }
            ArmInst::AndReg { dest, src1, src2 } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                self.emit_u32(0x8A000000 | (rm << 16) | (rn << 5) | rd);
            }
            ArmInst::OrrReg { dest, src1, src2 } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                self.emit_u32(0xAA000000 | (rm << 16) | (rn << 5) | rd);
            }
            ArmInst::EorReg { dest, src1, src2 } => {
                let rd = dest.hw_encoding() as u32;
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                self.emit_u32(0xCA000000 | (rm << 16) | (rn << 5) | rd);
            }
            ArmInst::MovReg { dest, src } => {
                let rd = dest.hw_encoding() as u32;
                let rm = src.hw_encoding() as u32;
                // orr xd, xzr, xm
                self.emit_u32(0xAA0003E0 | (rm << 16) | rd);
            }
            ArmInst::Movz { dest, imm, shift } => {
                let rd = dest.hw_encoding() as u32;
                let hw = ((*shift / 16) & 3) as u32;
                self.emit_u32(0xD2800000 | (hw << 21) | ((*imm as u32) << 5) | rd);
            }
            ArmInst::Movk { dest, imm, shift } => {
                let rd = dest.hw_encoding() as u32;
                let hw = ((*shift / 16) & 3) as u32;
                self.emit_u32(0xF2800000 | (hw << 21) | ((*imm as u32) << 5) | rd);
            }
            ArmInst::CmpImm { src, imm } => {
                let rn = src.hw_encoding() as u32;
                let imm12 = (*imm & 0xFFF) as u32;
                // subs xzr, xn, #imm12
                self.emit_u32(0xF100001F | (imm12 << 10) | (rn << 5));
            }
            ArmInst::CmpReg { src1, src2 } => {
                let rn = src1.hw_encoding() as u32;
                let rm = src2.hw_encoding() as u32;
                // subs xzr, xn, xm
                self.emit_u32(0xEB00001F | (rm << 16) | (rn << 5));
            }
            ArmInst::Cset { dest, cond } => {
                let rd = dest.hw_encoding() as u32;
                let inv = cond.invert().code() as u32;
                // csinc xd, xzr, xzr, invert(cond)
                self.emit_u32(0x9A9F07E0 | (inv << 12) | rd);
            }
            ArmInst::Ldr { dest, base, offset } => {
                let rt = dest.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm12 = ((*offset / 8) & 0xFFF) as u32;
                self.emit_u32(0xF9400000 | (imm12 << 10) | (rn << 5) | rt);
            }
            ArmInst::Str { src, base, offset } => {
                let rt = src.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm12 = ((*offset / 8) & 0xFFF) as u32;
                self.emit_u32(0xF9000000 | (imm12 << 10) | (rn << 5) | rt);
            }
            ArmInst::Ldr32 { dest, base, offset } => {
                let rt = dest.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm12 = ((*offset / 4) & 0xFFF) as u32;
                self.emit_u32(0xB9400000 | (imm12 << 10) | (rn << 5) | rt);
            }
            ArmInst::Str32 { src, base, offset } => {
                let rt = src.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm12 = ((*offset / 4) & 0xFFF) as u32;
                self.emit_u32(0xB9000000 | (imm12 << 10) | (rn << 5) | rt);
            }
            ArmInst::Ldrb { dest, base, offset } => {
                let rt = dest.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm12 = (*offset & 0xFFF) as u32;
                self.emit_u32(0x39400000 | (imm12 << 10) | (rn << 5) | rt);
            }
            ArmInst::Strb { src, base, offset } => {
                let rt = src.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm12 = (*offset & 0xFFF) as u32;
                self.emit_u32(0x39000000 | (imm12 << 10) | (rn << 5) | rt);
            }
            ArmInst::StpPreIndex { r1, r2, base, imm } => {
                let rt1 = r1.hw_encoding() as u32;
                let rt2 = r2.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm7 = ((*imm / 8) & 0x7F) as u32;
                self.emit_u32(0xA9800000 | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt1);
            }
            ArmInst::LdpPostIndex { r1, r2, base, imm } => {
                let rt1 = r1.hw_encoding() as u32;
                let rt2 = r2.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm7 = ((*imm / 8) & 0x7F) as u32;
                self.emit_u32(0xA8C00000 | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt1);
            }
            ArmInst::StpOffset {
                r1,
                r2,
                base,
                offset,
            } => {
                let rt1 = r1.hw_encoding() as u32;
                let rt2 = r2.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm7 = ((*offset / 8) & 0x7F) as u32;
                self.emit_u32(0xA9000000 | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt1);
            }
            ArmInst::LdpOffset {
                r1,
                r2,
                base,
                offset,
            } => {
                let rt1 = r1.hw_encoding() as u32;
                let rt2 = r2.hw_encoding() as u32;
                let rn = base.hw_encoding() as u32;
                let imm7 = ((*offset / 8) & 0x7F) as u32;
                self.emit_u32(0xA9400000 | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt1);
            }
            ArmInst::B { label } => {
                let offset = self.bytes.len();
                self.fixups.push(ArmFixup {
                    offset,
                    label: label.clone(),
                    is_cond: false,
                    cond: ArmCond::Al,
                });
                self.emit_u32(0x14000000); // b placeholder
            }
            ArmInst::BCond { cond, label } => {
                let offset = self.bytes.len();
                self.fixups.push(ArmFixup {
                    offset,
                    label: label.clone(),
                    is_cond: true,
                    cond: *cond,
                });
                self.emit_u32(0x54000000 | (cond.code() as u32)); // b.cond placeholder
            }
            ArmInst::Bl { callee } => {
                let offset = self.bytes.len();
                self.relocations.push(ArmRelocation {
                    offset,
                    symbol: callee.clone(),
                    addend: 0,
                    r_type: ARM64_RELOC_BRANCH26,
                });
                self.emit_u32(0x94000000); // bl placeholder
            }
            ArmInst::Blr { callee } => {
                let rn = callee.hw_encoding() as u32;
                self.emit_u32(0xD63F0000 | (rn << 5));
            }
            ArmInst::Ret => {
                // ret x30
                self.emit_u32(0xD65F03C0);
            }
            ArmInst::Adrp { dest, symbol } => {
                let rd = dest.hw_encoding() as u32;
                let offset = self.bytes.len();
                self.relocations.push(ArmRelocation {
                    offset,
                    symbol: symbol.clone(),
                    addend: 0,
                    r_type: ARM64_RELOC_PAGE21,
                });
                self.emit_u32(0x90000000 | rd);
            }
        }
    }

    /// Resolves local branch target fixups.
    pub fn resolve_fixups(&mut self) {
        for fixup in &self.fixups {
            if let Some(&target_offset) = self.label_offsets.get(&fixup.label) {
                let diff = (target_offset as i64) - (fixup.offset as i64);
                let inst_diff = (diff / 4) as i32;
                if fixup.is_cond {
                    let imm19 = (inst_diff & 0x7FFFF) as u32;
                    let word = 0x54000000 | (imm19 << 5) | (fixup.cond.code() as u32);
                    self.bytes[fixup.offset..fixup.offset + 4].copy_from_slice(&word.to_le_bytes());
                } else {
                    let imm26 = (inst_diff & 0x03FFFFFF) as u32;
                    let word = 0x14000000 | imm26;
                    self.bytes[fixup.offset..fixup.offset + 4].copy_from_slice(&word.to_le_bytes());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arm64_instruction_encodings() {
        let mut emitter = ArmCodeEmitter::new();

        // 1. ret -> 0xD65F03C0
        emitter.encode_instruction(&ArmInst::Ret);
        assert_eq!(&emitter.bytes[0..4], &0xD65F03C0u32.to_le_bytes());

        // 2. add x0, x1, #4 -> 0x91001020
        emitter.encode_instruction(&ArmInst::AddImm {
            dest: AArch64Reg::X0,
            src: AArch64Reg::X1,
            imm: 4,
        });
        assert_eq!(&emitter.bytes[4..8], &0x91001020u32.to_le_bytes());

        // 3. sub sp, sp, #32 -> 0xD10083FF
        emitter.encode_instruction(&ArmInst::SubImm {
            dest: AArch64Reg::SP,
            src: AArch64Reg::SP,
            imm: 32,
        });
        assert_eq!(&emitter.bytes[8..12], &0xD10083FFu32.to_le_bytes());

        // 4. mov x0, x1 (orr x0, xzr, x1) -> 0xAA0103E0
        emitter.encode_instruction(&ArmInst::MovReg {
            dest: AArch64Reg::X0,
            src: AArch64Reg::X1,
        });
        assert_eq!(&emitter.bytes[12..16], &0xAA0103E0u32.to_le_bytes());

        // 5. stp x29, x30, [sp, #-32]! -> 0xA9BE7BFD
        emitter.encode_instruction(&ArmInst::StpPreIndex {
            r1: AArch64Reg::X29,
            r2: AArch64Reg::X30,
            base: AArch64Reg::SP,
            imm: -32,
        });
        assert_eq!(&emitter.bytes[16..20], &0xA9BE7BFDu32.to_le_bytes());

        // 6. ldp x29, x30, [sp], #32 -> 0xA8C27BFD
        emitter.encode_instruction(&ArmInst::LdpPostIndex {
            r1: AArch64Reg::X29,
            r2: AArch64Reg::X30,
            base: AArch64Reg::SP,
            imm: 32,
        });
        assert_eq!(&emitter.bytes[20..24], &0xA8C27BFDu32.to_le_bytes());
    }

    #[test]
    fn test_arm64_branch_fixup() {
        let mut emitter = ArmCodeEmitter::new();
        emitter.encode_instruction(&ArmInst::B {
            label: "target".into(),
        });
        emitter.encode_instruction(&ArmInst::AddImm {
            dest: AArch64Reg::X0,
            src: AArch64Reg::X0,
            imm: 1,
        });
        emitter.encode_instruction(&ArmInst::Label("target".into()));
        emitter.encode_instruction(&ArmInst::Ret);
        emitter.resolve_fixups();

        // branch target is +8 bytes (+2 instructions) -> 0x14000002
        assert_eq!(&emitter.bytes[0..4], &0x14000002u32.to_le_bytes());
    }
}
