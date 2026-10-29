//! x86-64 physical and virtual register definitions.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorWidth {
    V128,
    V256,
    V512,
}

impl VectorWidth {
    /// Vector width in bytes (16 for 128-bit, 32 for 256-bit, 64 for 512-bit).
    pub const fn byte_size(&self) -> usize {
        match self {
            VectorWidth::V128 => 16,
            VectorWidth::V256 => 32,
            VectorWidth::V512 => 64,
        }
    }

    /// Required alignment in bytes for packed operations / spills.
    pub const fn align(&self) -> usize {
        self.byte_size()
    }

    /// Bits in vector (128, 256, 512).
    pub const fn bit_size(&self) -> usize {
        self.byte_size() * 8
    }

    /// Display string (e.g. "256-bit").
    pub const fn display_name(&self) -> &'static str {
        match self {
            VectorWidth::V128 => "128-bit",
            VectorWidth::V256 => "256-bit",
            VectorWidth::V512 => "512-bit",
        }
    }
}

impl fmt::Display for VectorWidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Physical SIMD register (XMM0..XMM31, YMM0..YMM31, ZMM0..ZMM31).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SimdReg(pub u8);

impl SimdReg {
    pub const fn xmm(index: u8) -> Self {
        assert!(index < 32, "x86-64 vector register index must be 0..31");
        Self(index)
    }

    pub const fn ymm(index: u8) -> Self {
        assert!(index < 32, "x86-64 vector register index must be 0..31");
        Self(index)
    }

    pub const fn zmm(index: u8) -> Self {
        assert!(index < 32, "x86-64 vector register index must be 0..31");
        Self(index)
    }

    /// 3-bit hardware encoding (low 3 bits: bits 0..2).
    pub const fn hw_encoding(&self) -> u8 {
        self.0 & 0b111
    }

    /// Whether this register index has bit 3 set (8..15 or 24..31), requiring REX.R, REX.B, VEX.R, etc.
    pub const fn is_extended(&self) -> bool {
        (self.0 & 0b1000) != 0
    }

    /// Whether this register index has bit 4 set (16..31), requiring EVEX prefix (R' / V').
    pub const fn is_extended_evex(&self) -> bool {
        (self.0 & 0b10000) != 0
    }

    /// Register number index (0..31).
    pub const fn index(&self) -> u8 {
        self.0
    }

    /// Format register name with appropriate prefix: xmm, ymm, or zmm.
    pub fn name(&self, width: VectorWidth) -> String {
        let prefix = match width {
            VectorWidth::V128 => "xmm",
            VectorWidth::V256 => "ymm",
            VectorWidth::V512 => "zmm",
        };
        format!("{}{}", prefix, self.0)
    }

    /// System V AMD64 ABI vector argument registers (xmm0..xmm7).
    pub const ARG_REGS: [SimdReg; 8] = [
        SimdReg(0),
        SimdReg(1),
        SimdReg(2),
        SimdReg(3),
        SimdReg(4),
        SimdReg(5),
        SimdReg(6),
        SimdReg(7),
    ];

    /// In System V AMD64 ABI, all vector registers are caller-saved (scratch) registers.
    pub const SCRATCH_REGS: [SimdReg; 16] = [
        SimdReg(0),
        SimdReg(1),
        SimdReg(2),
        SimdReg(3),
        SimdReg(4),
        SimdReg(5),
        SimdReg(6),
        SimdReg(7),
        SimdReg(8),
        SimdReg(9),
        SimdReg(10),
        SimdReg(11),
        SimdReg(12),
        SimdReg(13),
        SimdReg(14),
        SimdReg(15),
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Reg {
    Rax = 0,
    Rcx = 1,
    Rdx = 2,
    Rbx = 3,
    Rsp = 4,
    Rbp = 5,
    Rsi = 6,
    Rdi = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

impl Reg {
    /// 3-bit hardware encoding (low 3 bits).
    pub fn hw_encoding(&self) -> u8 {
        (*self as u8) & 0b111
    }

    /// Whether this register requires the REX bit extension (R8..R15).
    pub fn is_extended(&self) -> bool {
        (*self as u8) >= 8
    }

    /// System V AMD64 ABI 6 integer parameter registers in order.
    pub const ARG_REGS: [Reg; 6] = [Reg::Rdi, Reg::Rsi, Reg::Rdx, Reg::Rcx, Reg::R8, Reg::R9];

    /// Caller-saved scratch registers available for general computation.
    pub const SCRATCH_REGS: [Reg; 7] = [
        Reg::R10,
        Reg::R11,
        Reg::Rdi,
        Reg::Rsi,
        Reg::Rdx,
        Reg::Rcx,
        Reg::R8,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Reg::Rax => "rax",
            Reg::Rcx => "rcx",
            Reg::Rdx => "rdx",
            Reg::Rbx => "rbx",
            Reg::Rsp => "rsp",
            Reg::Rbp => "rbp",
            Reg::Rsi => "rsi",
            Reg::Rdi => "rdi",
            Reg::R8 => "r8",
            Reg::R9 => "r9",
            Reg::R10 => "r10",
            Reg::R11 => "r11",
            Reg::R12 => "r12",
            Reg::R13 => "r13",
            Reg::R14 => "r14",
            Reg::R15 => "r15",
        }
    }
}
