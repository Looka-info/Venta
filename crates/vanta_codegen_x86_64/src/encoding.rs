//! Pure Rust x86-64 machine code instruction encoder.

use crate::machine_ir::{ConditionCode, MachineInst, Operand, SimdOperand};
use crate::register::{Reg, VectorWidth};
use crate::target::TargetFeatures;
use std::collections::HashMap;
use std::fmt;

/// Backend code generation and SIMD encoding error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenError {
    /// Target CPU feature is missing for the requested instruction.
    MissingTargetFeature {
        instruction: &'static str,
        feature: &'static str,
        width: VectorWidth,
    },
    /// Unsupported operand combination for instruction form.
    UnsupportedOperandCombination(&'static str),
    /// Register number exceeds architectural limits for instruction form.
    InvalidRegisterNumber(u8),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::MissingTargetFeature {
                instruction,
                feature,
                width,
            } => {
                write!(
                    f,
                    "SIMD instruction '{}' ({}) requires target feature '+{}'",
                    instruction, width, feature
                )
            }
            CodegenError::UnsupportedOperandCombination(msg) => {
                write!(f, "unsupported operand combination: {}", msg)
            }
            CodegenError::InvalidRegisterNumber(reg) => {
                write!(
                    f,
                    "invalid register number {} for target instruction encoding",
                    reg
                )
            }
        }
    }
}

impl std::error::Error for CodegenError {}

#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: usize,
    pub symbol: String,
    pub addend: i64,
    pub r_type: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineEntry {
    pub code_offset: usize,
    pub file_idx: u32,
    pub line: u32,
    pub column: u32,
    pub is_stmt: bool,
}

pub struct CodeEmitter {
    pub bytes: Vec<u8>,
    pub label_offsets: HashMap<String, usize>,
    pub label_fixups: Vec<(usize, String)>, // (offset_of_rel32, target_label)
    pub relocations: Vec<Relocation>,
    pub target_features: TargetFeatures,
    pub line_entries: Vec<LineEntry>,
}

impl Default for CodeEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeEmitter {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            label_offsets: HashMap::new(),
            label_fixups: Vec::new(),
            relocations: Vec::new(),
            target_features: TargetFeatures::baseline(),
            line_entries: Vec::new(),
        }
    }

    pub fn with_target(target_features: TargetFeatures) -> Self {
        Self {
            bytes: Vec::new(),
            label_offsets: HashMap::new(),
            label_fixups: Vec::new(),
            relocations: Vec::new(),
            target_features,
            line_entries: Vec::new(),
        }
    }

    pub fn record_source_loc(&mut self, file_id: u32, line: u32, column: u32) {
        let code_offset = self.bytes.len();
        if let Some(last) = self.line_entries.last() {
            if last.code_offset == code_offset {
                if let Some(last_mut) = self.line_entries.last_mut() {
                    last_mut.file_idx = file_id;
                    last_mut.line = line;
                    last_mut.column = column;
                }
                return;
            }
            if last.file_idx == file_id && last.line == line && last.column == column {
                return;
            }
        }
        self.line_entries.push(LineEntry {
            code_offset,
            file_idx: file_id,
            line,
            column,
            is_stmt: true,
        });
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
        if offset == 0 && base_hw != 5 {
            // mod = 00 (no displacement, except RBP/R13 where rm=5 encodes RIP-relative)
            let mod_rm = Self::mod_rm(0b00, reg_field, base_hw);
            self.emit_u8(mod_rm);
            if base_hw == 4 {
                // RSP / R12 requires SIB byte
                self.emit_u8(Self::sib(0, 4, 4));
            }
        } else if (-128..=127).contains(&offset) {
            // mod = 01 (disp8)
            let mod_rm = Self::mod_rm(0b01, reg_field, base_hw);
            self.emit_u8(mod_rm);
            if base_hw == 4 {
                // RSP / R12 requires SIB byte
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

    /// Emit memory operand for EVEX instructions using Compressed Displacement (Disp8*N).
    fn emit_evex_mem_operand(&mut self, reg_field: u8, base: Reg, offset: i32, scale_n: i32) {
        let base_hw = base.hw_encoding();
        if offset == 0 && base_hw != 5 {
            // mod = 00 (no displacement, except RBP/R13 where rm=5 encodes RIP-relative)
            let mod_rm = Self::mod_rm(0b00, reg_field, base_hw);
            self.emit_u8(mod_rm);
            if base_hw == 4 {
                // RSP / R12 requires SIB byte
                self.emit_u8(Self::sib(0, 4, 4));
            }
        } else if scale_n > 0 && offset % scale_n == 0 && (-128..=127).contains(&(offset / scale_n))
        {
            // mod = 01 (disp8 compressed by scale_n)
            let mod_rm = Self::mod_rm(0b01, reg_field, base_hw);
            self.emit_u8(mod_rm);
            if base_hw == 4 {
                self.emit_u8(Self::sib(0, 4, 4));
            }
            let disp8 = (offset / scale_n) as i8 as u8;
            self.emit_u8(disp8);
        } else {
            // mod = 10 (disp32, uncompressed)
            let mod_rm = Self::mod_rm(0b10, reg_field, base_hw);
            self.emit_u8(mod_rm);
            if base_hw == 4 {
                self.emit_u8(Self::sib(0, 4, 4));
            }
            self.emit_i32(offset);
        }
    }

    /// Emit 2-byte (0xC5) or 3-byte (0xC4) VEX prefix.
    ///
    /// - `map`: 1 for 0F, 2 for 0F38, 3 for 0F3A
    /// - `w`: true for 64-bit operand or FMA double-precision
    /// - `r_bit`: reg register extended bit (bit 3)
    /// - `x_bit`: SIB index extended bit (bit 3)
    /// - `b_bit`: r/m or base register extended bit (bit 3)
    /// - `vvvv`: non-destructive register index (0..15)
    /// - `use_vvvv`: whether vvvv is used
    /// - `l_bit`: true for 256-bit vector, false for 128-bit
    /// - `pp`: 0 for None, 1 for 66, 2 for F3, 3 for F2
    pub fn emit_vex(
        &mut self,
        map: u8,
        w: bool,
        r_bit: bool,
        x_bit: bool,
        b_bit: bool,
        vvvv: u8,
        use_vvvv: bool,
        l_bit: bool,
        pp: u8,
    ) {
        let not_r = !r_bit;
        let not_x = !x_bit;
        let not_b = !b_bit;
        let not_vvvv = if use_vvvv { (!vvvv) & 0x0F } else { 0x0F };

        // 2-byte VEX (0xC5) is architecturally valid ONLY if:
        // map == 1 (0F), w == false, not_x == true, not_b == true
        if map == 1 && !w && not_x && not_b {
            let mut b1 = 0u8;
            if not_r {
                b1 |= 0x80;
            }
            b1 |= (not_vvvv & 0x0F) << 3;
            if l_bit {
                b1 |= 0x04;
            }
            b1 |= pp & 0x03;
            self.emit_u8(0xC5);
            self.emit_u8(b1);
        } else {
            // 3-byte VEX (0xC4)
            let mut b1 = 0u8;
            if not_r {
                b1 |= 0x80;
            }
            if not_x {
                b1 |= 0x40;
            }
            if not_b {
                b1 |= 0x20;
            }
            b1 |= map & 0x1F;

            let mut b2 = 0u8;
            if w {
                b2 |= 0x80;
            }
            b2 |= (not_vvvv & 0x0F) << 3;
            if l_bit {
                b2 |= 0x04;
            }
            b2 |= pp & 0x03;

            self.emit_u8(0xC4);
            self.emit_u8(b1);
            self.emit_u8(b2);
        }
    }

    /// Emit EVEX prefix (0x62) for 512-bit AVX-512 operations.
    pub fn emit_evex(
        &mut self,
        map: u8,
        w: bool,
        reg: u8,
        rm_reg: u8,
        rm_is_extended: bool,
        vvvv: u8,
        use_vvvv: bool,
        width: VectorWidth,
        pp: u8,
    ) {
        self.emit_u8(0x62);

        // Byte 1: [~R:1][~X:1][~B:1][~R':1][00:2][mm:2]
        let not_r = (reg & 0b1000) == 0;
        let not_x = (rm_reg & 0b10000) == 0;
        let not_b = !rm_is_extended;
        let not_r_prime = (reg & 0b10000) == 0;

        let mut b1 = 0u8;
        if not_r {
            b1 |= 0x80;
        }
        if not_x {
            b1 |= 0x40;
        }
        if not_b {
            b1 |= 0x20;
        }
        if not_r_prime {
            b1 |= 0x10;
        }
        b1 |= map & 0x03;
        self.emit_u8(b1);

        // Byte 2: [W:1][~vvvv:4][1:1][pp:2]
        let not_vvvv = if use_vvvv {
            (!(vvvv & 0x0F)) & 0x0F
        } else {
            0x0F
        };
        let mut b2 = 0u8;
        if w {
            b2 |= 0x80;
        }
        b2 |= not_vvvv << 3;
        b2 |= 0x04; // fixed 1 bit
        b2 |= pp & 0x03;
        self.emit_u8(b2);

        // Byte 3: [z:1][L'L:2][b:1][~V':1][aaa:3]
        let ll_bits = match width {
            VectorWidth::V128 => 0b00,
            VectorWidth::V256 => 0b01,
            VectorWidth::V512 => 0b10,
        };
        let not_v_prime = if use_vvvv {
            (vvvv & 0b10000) == 0
        } else {
            true
        };
        let mut b3 = 0u8;
        b3 |= (ll_bits & 0x03) << 5;
        if not_v_prime {
            b3 |= 0x08;
        }
        self.emit_u8(b3);
    }

    pub fn encode_instruction_checked(
        &mut self,
        inst: &MachineInst,
        target: &TargetFeatures,
    ) -> Result<(), CodegenError> {
        match inst {
            MachineInst::SourceLoc {
                file_id,
                line,
                column,
            } => {
                self.record_source_loc(*file_id, *line, *column);
                return Ok(());
            }
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
                        self.emit_u8(Self::rex(
                            true,
                            dst.is_extended(),
                            false,
                            base.is_extended(),
                        ));
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
            MachineInst::Add { dest, src } => match (dest, src) {
                (Operand::Reg(dst), Operand::Reg(s)) => {
                    self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                    self.emit_u8(0x01);
                    self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                }
                (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                    self.emit_u8(Self::rex(
                        true,
                        dst.is_extended(),
                        false,
                        base.is_extended(),
                    ));
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
            },
            MachineInst::Sub { dest, src } => match (dest, src) {
                (Operand::Reg(dst), Operand::Reg(s)) => {
                    self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                    self.emit_u8(0x29);
                    self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                }
                (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                    self.emit_u8(Self::rex(
                        true,
                        dst.is_extended(),
                        false,
                        base.is_extended(),
                    ));
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
            },
            MachineInst::Imul { dest, src } => match src {
                Operand::Reg(s) => {
                    self.emit_u8(Self::rex(true, dest.is_extended(), false, s.is_extended()));
                    self.emit_u8(0x0F);
                    self.emit_u8(0xAF);
                    self.emit_u8(Self::mod_rm(0b11, dest.hw_encoding(), s.hw_encoding()));
                }
                Operand::Mem { base, offset } => {
                    self.emit_u8(Self::rex(
                        true,
                        dest.is_extended(),
                        false,
                        base.is_extended(),
                    ));
                    self.emit_u8(0x0F);
                    self.emit_u8(0xAF);
                    self.emit_mem_operand(dest.hw_encoding(), *base, *offset);
                }
                Operand::Imm(val) => {
                    self.emit_u8(Self::rex(
                        true,
                        dest.is_extended(),
                        false,
                        dest.is_extended(),
                    ));
                    self.emit_u8(0x69);
                    self.emit_u8(Self::mod_rm(0b11, dest.hw_encoding(), dest.hw_encoding()));
                    self.emit_i32(*val as i32);
                }
                _ => panic!("Unsupported Imul operand: {:?}", src),
            },
            MachineInst::Idiv(reg) => {
                self.emit_u8(Self::rex(true, false, false, reg.is_extended()));
                self.emit_u8(0xF7);
                self.emit_u8(Self::mod_rm(0b11, 7, reg.hw_encoding()));
            }
            MachineInst::And { dest, src } => match (dest, src) {
                (Operand::Reg(dst), Operand::Reg(s)) => {
                    self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                    self.emit_u8(0x21);
                    self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                }
                (Operand::Reg(dst), Operand::Imm(val)) => {
                    self.emit_u8(Self::rex(true, false, false, dst.is_extended()));
                    if (-128..=127).contains(val) {
                        self.emit_u8(0x83);
                        self.emit_u8(Self::mod_rm(0b11, 4, dst.hw_encoding()));
                        self.emit_u8(*val as i8 as u8);
                    } else {
                        self.emit_u8(0x81);
                        self.emit_u8(Self::mod_rm(0b11, 4, dst.hw_encoding()));
                        self.emit_i32(*val as i32);
                    }
                }
                (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                    self.emit_u8(Self::rex(
                        true,
                        dst.is_extended(),
                        false,
                        base.is_extended(),
                    ));
                    self.emit_u8(0x23);
                    self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                }
                _ => panic!("Unsupported And operands: {:?}", inst),
            },
            MachineInst::Or { dest, src } => match (dest, src) {
                (Operand::Reg(dst), Operand::Reg(s)) => {
                    self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                    self.emit_u8(0x09);
                    self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                }
                (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                    self.emit_u8(Self::rex(
                        true,
                        dst.is_extended(),
                        false,
                        base.is_extended(),
                    ));
                    self.emit_u8(0x0B);
                    self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                }
                _ => panic!("Unsupported Or operands: {:?}", inst),
            },
            MachineInst::Xor { dest, src } => match (dest, src) {
                (Operand::Reg(dst), Operand::Reg(s)) => {
                    self.emit_u8(Self::rex(true, s.is_extended(), false, dst.is_extended()));
                    self.emit_u8(0x31);
                    self.emit_u8(Self::mod_rm(0b11, s.hw_encoding(), dst.hw_encoding()));
                }
                (Operand::Reg(dst), Operand::Mem { base, offset }) => {
                    self.emit_u8(Self::rex(
                        true,
                        dst.is_extended(),
                        false,
                        base.is_extended(),
                    ));
                    self.emit_u8(0x33);
                    self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                }
                _ => panic!("Unsupported Xor operands: {:?}", inst),
            },
            MachineInst::Shl { dest, count } => match count {
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
            },
            MachineInst::Shr { dest, count } => match count {
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
            },
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
            MachineInst::Cmp { lhs, rhs } => match (lhs, rhs) {
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
            },
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
            MachineInst::MovzxMem { dest, base, offset } => {
                let rex = Self::rex(true, dest.is_extended(), false, base.is_extended());
                self.emit_u8(rex);
                self.emit_u8(0x0F);
                self.emit_u8(0xB6);
                self.emit_mem_operand(dest.hw_encoding(), *base, *offset);
            }
            MachineInst::Mov32Mem { dest, base, offset } => {
                if dest.is_extended() || base.is_extended() {
                    self.emit_u8(Self::rex(
                        false,
                        dest.is_extended(),
                        false,
                        base.is_extended(),
                    ));
                }
                self.emit_u8(0x8B);
                self.emit_mem_operand(dest.hw_encoding(), *base, *offset);
            }
            MachineInst::MovMem32 { base, offset, src } => {
                if src.is_extended() || base.is_extended() {
                    self.emit_u8(Self::rex(
                        false,
                        src.is_extended(),
                        false,
                        base.is_extended(),
                    ));
                }
                self.emit_u8(0x89);
                self.emit_mem_operand(src.hw_encoding(), *base, *offset);
            }
            MachineInst::Lea { dest, src } => match src {
                Operand::Mem { base, offset } => {
                    self.emit_u8(Self::rex(
                        true,
                        dest.is_extended(),
                        false,
                        base.is_extended(),
                    ));
                    self.emit_u8(0x8D);
                    self.emit_mem_operand(dest.hw_encoding(), *base, *offset);
                }
                _ => panic!("Lea source must be memory: {:?}", src),
            },
            MachineInst::LeaRip { dest, symbol } => {
                let rex = Self::rex(true, dest.is_extended(), false, false);
                self.emit_u8(rex);
                self.emit_u8(0x8D); // LEA opcode
                                    // ModRM: mod=00, reg=dest, rm=101 (RIP-relative)
                let mod_rm = Self::mod_rm(0b00, dest.hw_encoding(), 0b101);
                self.emit_u8(mod_rm);
                let rel_offset = self.bytes.len();
                self.emit_i32(-4); // Standard displacement / addend for R_X86_64_PC32
                self.relocations.push(Relocation {
                    offset: rel_offset,
                    symbol: symbol.clone(),
                    addend: -4,
                    r_type: 2, // R_X86_64_PC32
                });
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
                    r_type: 4, // R_X86_64_PLT32
                });
            }
            MachineInst::CallReg(reg) => {
                if reg.is_extended() {
                    self.emit_u8(0x41); // REX.B
                }
                self.emit_u8(0xFF);
                self.emit_u8(0xD0 | reg.hw_encoding());
            }
            MachineInst::TailJmp(symbol) => {
                self.emit_u8(0xE9); // JMP rel32 opcode
                let rel_offset = self.bytes.len();
                self.emit_i32(-4); // Standard addend for x86-64 PLT32 relocations
                self.relocations.push(Relocation {
                    offset: rel_offset,
                    symbol: symbol.clone(),
                    addend: -4,
                    r_type: 4, // R_X86_64_PLT32
                });
            }
            MachineInst::Vmovups { width, dest, src } => {
                match width {
                    VectorWidth::V128 | VectorWidth::V256 => {
                        if !target.avx {
                            return Err(CodegenError::MissingTargetFeature {
                                instruction: "vmovups",
                                feature: "avx",
                                width: *width,
                            });
                        }
                    }
                    VectorWidth::V512 => {
                        if !target.avx512f {
                            return Err(CodegenError::MissingTargetFeature {
                                instruction: "vmovups",
                                feature: "avx512f",
                                width: *width,
                            });
                        }
                    }
                }

                let is_512 = *width == VectorWidth::V512;
                let l_bit = *width == VectorWidth::V256;

                match (dest, src) {
                    (SimdOperand::Reg(dst), SimdOperand::Reg(s)) => {
                        if !is_512 && (dst.index() >= 16 || s.index() >= 16) {
                            return Err(CodegenError::InvalidRegisterNumber(
                                dst.index().max(s.index()),
                            ));
                        }
                        if is_512 {
                            self.emit_evex(
                                1,
                                false,
                                dst.0,
                                s.0,
                                s.is_extended(),
                                0,
                                false,
                                *width,
                                0,
                            );
                        } else {
                            self.emit_vex(
                                1,
                                false,
                                dst.is_extended(),
                                false,
                                s.is_extended(),
                                0,
                                false,
                                l_bit,
                                0,
                            );
                        }
                        self.emit_u8(0x10);
                        self.emit_u8(Self::mod_rm(0b11, dst.hw_encoding(), s.hw_encoding()));
                    }
                    (SimdOperand::Reg(dst), SimdOperand::Mem { base, offset }) => {
                        if !is_512 && dst.index() >= 16 {
                            return Err(CodegenError::InvalidRegisterNumber(dst.index()));
                        }
                        if is_512 {
                            self.emit_evex(
                                1,
                                false,
                                dst.0,
                                *base as u8,
                                base.is_extended(),
                                0,
                                false,
                                *width,
                                0,
                            );
                            self.emit_u8(0x10);
                            let scale_n = if *width == VectorWidth::V512 { 64 } else { 32 };
                            self.emit_evex_mem_operand(dst.hw_encoding(), *base, *offset, scale_n);
                        } else {
                            self.emit_vex(
                                1,
                                false,
                                dst.is_extended(),
                                false,
                                base.is_extended(),
                                0,
                                false,
                                l_bit,
                                0,
                            );
                            self.emit_u8(0x10);
                            self.emit_mem_operand(dst.hw_encoding(), *base, *offset);
                        }
                    }
                    (SimdOperand::Mem { base, offset }, SimdOperand::Reg(s)) => {
                        if !is_512 && s.index() >= 16 {
                            return Err(CodegenError::InvalidRegisterNumber(s.index()));
                        }
                        if is_512 {
                            self.emit_evex(
                                1,
                                false,
                                s.0,
                                *base as u8,
                                base.is_extended(),
                                0,
                                false,
                                *width,
                                0,
                            );
                            self.emit_u8(0x11);
                            let scale_n = if *width == VectorWidth::V512 { 64 } else { 32 };
                            self.emit_evex_mem_operand(s.hw_encoding(), *base, *offset, scale_n);
                        } else {
                            self.emit_vex(
                                1,
                                false,
                                s.is_extended(),
                                false,
                                base.is_extended(),
                                0,
                                false,
                                l_bit,
                                0,
                            );
                            self.emit_u8(0x11);
                            self.emit_mem_operand(s.hw_encoding(), *base, *offset);
                        }
                    }
                    _ => {
                        return Err(CodegenError::UnsupportedOperandCombination(
                            "vmovups does not support memory-to-memory encoding",
                        ));
                    }
                }
            }
            MachineInst::Vpaddq {
                width,
                dest,
                src1,
                src2,
            } => {
                let dest = match dest {
                    SimdOperand::Reg(r) => *r,
                    _ => panic!("Expected allocated physical SIMD register for dest in Vpaddq"),
                };
                let src1 = match src1 {
                    SimdOperand::Reg(r) => *r,
                    _ => panic!("Expected allocated physical SIMD register for src1 in Vpaddq"),
                };

                match width {
                    VectorWidth::V128 | VectorWidth::V256 => {
                        if !target.avx2 {
                            return Err(CodegenError::MissingTargetFeature {
                                instruction: "vpaddq",
                                feature: "avx2",
                                width: *width,
                            });
                        }
                    }
                    VectorWidth::V512 => {
                        if !target.avx512f {
                            return Err(CodegenError::MissingTargetFeature {
                                instruction: "vpaddq",
                                feature: "avx512f",
                                width: *width,
                            });
                        }
                    }
                }

                let is_512 = *width == VectorWidth::V512;
                let l_bit = *width == VectorWidth::V256;

                if !is_512 && (dest.index() >= 16 || src1.index() >= 16) {
                    return Err(CodegenError::InvalidRegisterNumber(
                        dest.index().max(src1.index()),
                    ));
                }

                match src2 {
                    SimdOperand::Reg(s2) => {
                        if !is_512 && s2.index() >= 16 {
                            return Err(CodegenError::InvalidRegisterNumber(s2.index()));
                        }
                        if is_512 {
                            self.emit_evex(
                                1,
                                true,
                                dest.0,
                                s2.0,
                                s2.is_extended(),
                                src1.0,
                                true,
                                *width,
                                1,
                            );
                        } else {
                            self.emit_vex(
                                1,
                                false,
                                dest.is_extended(),
                                false,
                                s2.is_extended(),
                                src1.0,
                                true,
                                l_bit,
                                1,
                            );
                        }
                        self.emit_u8(0xD4);
                        self.emit_u8(Self::mod_rm(0b11, dest.hw_encoding(), s2.hw_encoding()));
                    }
                    SimdOperand::Mem { base, offset } => {
                        if is_512 {
                            self.emit_evex(
                                1,
                                true,
                                dest.0,
                                *base as u8,
                                base.is_extended(),
                                src1.0,
                                true,
                                *width,
                                1,
                            );
                            self.emit_u8(0xD4);
                            let scale_n = if *width == VectorWidth::V512 { 64 } else { 32 };
                            self.emit_evex_mem_operand(dest.hw_encoding(), *base, *offset, scale_n);
                        } else {
                            self.emit_vex(
                                1,
                                false,
                                dest.is_extended(),
                                false,
                                base.is_extended(),
                                src1.0,
                                true,
                                l_bit,
                                1,
                            );
                            self.emit_u8(0xD4);
                            self.emit_mem_operand(dest.hw_encoding(), *base, *offset);
                        }
                    }
                    SimdOperand::Val(_) => {
                        panic!("Virtual SIMD operand in vpaddq was not allocated prior to code emission");
                    }
                }
            }
            MachineInst::Vfmadd231pd {
                width,
                dest,
                src1,
                src2,
            } => {
                let dest = match dest {
                    SimdOperand::Reg(r) => *r,
                    _ => {
                        panic!("Expected allocated physical SIMD register for dest in Vfmadd231pd")
                    }
                };
                let src1 = match src1 {
                    SimdOperand::Reg(r) => *r,
                    _ => {
                        panic!("Expected allocated physical SIMD register for src1 in Vfmadd231pd")
                    }
                };

                match width {
                    VectorWidth::V128 | VectorWidth::V256 => {
                        if !target.avx {
                            return Err(CodegenError::MissingTargetFeature {
                                instruction: "vfmadd231pd",
                                feature: "avx",
                                width: *width,
                            });
                        }
                        if !target.fma {
                            return Err(CodegenError::MissingTargetFeature {
                                instruction: "vfmadd231pd",
                                feature: "fma",
                                width: *width,
                            });
                        }
                    }
                    VectorWidth::V512 => {
                        if !target.avx512f {
                            return Err(CodegenError::MissingTargetFeature {
                                instruction: "vfmadd231pd",
                                feature: "avx512f",
                                width: *width,
                            });
                        }
                    }
                }

                let is_512 = *width == VectorWidth::V512;
                let l_bit = *width == VectorWidth::V256;

                if !is_512 && (dest.index() >= 16 || src1.index() >= 16) {
                    return Err(CodegenError::InvalidRegisterNumber(
                        dest.index().max(src1.index()),
                    ));
                }

                match src2 {
                    SimdOperand::Reg(s2) => {
                        if !is_512 && s2.index() >= 16 {
                            return Err(CodegenError::InvalidRegisterNumber(s2.index()));
                        }
                        if is_512 {
                            self.emit_evex(
                                2,
                                true,
                                dest.0,
                                s2.0,
                                s2.is_extended(),
                                src1.0,
                                true,
                                *width,
                                1,
                            );
                        } else {
                            self.emit_vex(
                                2,
                                true,
                                dest.is_extended(),
                                false,
                                s2.is_extended(),
                                src1.0,
                                true,
                                l_bit,
                                1,
                            );
                        }
                        self.emit_u8(0xB8);
                        self.emit_u8(Self::mod_rm(0b11, dest.hw_encoding(), s2.hw_encoding()));
                    }
                    SimdOperand::Mem { base, offset } => {
                        if is_512 {
                            self.emit_evex(
                                2,
                                true,
                                dest.0,
                                *base as u8,
                                base.is_extended(),
                                src1.0,
                                true,
                                *width,
                                1,
                            );
                            self.emit_u8(0xB8);
                            let scale_n = if *width == VectorWidth::V512 { 64 } else { 32 };
                            self.emit_evex_mem_operand(dest.hw_encoding(), *base, *offset, scale_n);
                        } else {
                            self.emit_vex(
                                2,
                                true,
                                dest.is_extended(),
                                false,
                                base.is_extended(),
                                src1.0,
                                true,
                                l_bit,
                                1,
                            );
                            self.emit_u8(0xB8);
                            self.emit_mem_operand(dest.hw_encoding(), *base, *offset);
                        }
                    }
                    SimdOperand::Val(_) => {
                        panic!("Virtual SIMD operand in vfmadd231pd was not allocated prior to code emission");
                    }
                }
            }
        }
        Ok(())
    }

    /// Encode instruction using the emitter's configured target features.
    pub fn encode_instruction(&mut self, inst: &MachineInst) {
        let target = self.target_features;
        if let Err(err) = self.encode_instruction_checked(inst, &target) {
            panic!("Code emission error: {}", err);
        }
    }

    /// Patch local jump offsets after all instructions and labels are emitted.
    pub fn resolve_fixups(&mut self) {
        for (fixup_offset, target_label) in &self.label_fixups {
            if let Some(&target_offset) = self.label_offsets.get(target_label) {
                // rel32 is relative to the address immediately following the 4-byte immediate
                let next_ip = (fixup_offset + 4) as i32;
                let rel32 = (target_offset as i32) - next_ip;
                self.bytes[*fixup_offset..*fixup_offset + 4].copy_from_slice(&rel32.to_le_bytes());
            } else {
                panic!("Unresolved local jump label: {}", target_label);
            }
        }
    }
}
