//! AArch64 physical and virtual register definitions.
//!
//! Models 64-bit general-purpose registers (X0..X30, SP, XZR) for the Apple Silicon
//! (ARM64 / macOS) target architecture.
//!
//! Note on Darwin ABI constraints:
//! - Register X18 is reserved for the Apple Darwin platform/OS and MUST NEVER be used
//!   or allocated by user code.
//! - Registers X29 (Frame Pointer) and X30 (Link Register) are managed by prologue/epilogue.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AArch64Reg {
    X0,
    X1,
    X2,
    X3,
    X4,
    X5,
    X6,
    X7,
    X8,
    X9,
    X10,
    X11,
    X12,
    X13,
    X14,
    X15,
    X16,
    X17,
    X18, // Reserved by Darwin ABI - never allocatable!
    X19,
    X20,
    X21,
    X22,
    X23,
    X24,
    X25,
    X26,
    X27,
    X28,
    X29, // Frame Pointer (FP)
    X30, // Link Register (LR)
    SP,  // Stack Pointer (encodes as 31 in load/store/add)
    XZR, // Zero Register (encodes as 31 in logical/arithmetic)
}

impl AArch64Reg {
    /// 5-bit hardware encoding for AArch64 instruction operands (0..31).
    pub const fn hw_encoding(&self) -> u8 {
        match self {
            AArch64Reg::X0 => 0,
            AArch64Reg::X1 => 1,
            AArch64Reg::X2 => 2,
            AArch64Reg::X3 => 3,
            AArch64Reg::X4 => 4,
            AArch64Reg::X5 => 5,
            AArch64Reg::X6 => 6,
            AArch64Reg::X7 => 7,
            AArch64Reg::X8 => 8,
            AArch64Reg::X9 => 9,
            AArch64Reg::X10 => 10,
            AArch64Reg::X11 => 11,
            AArch64Reg::X12 => 12,
            AArch64Reg::X13 => 13,
            AArch64Reg::X14 => 14,
            AArch64Reg::X15 => 15,
            AArch64Reg::X16 => 16,
            AArch64Reg::X17 => 17,
            AArch64Reg::X18 => 18,
            AArch64Reg::X19 => 19,
            AArch64Reg::X20 => 20,
            AArch64Reg::X21 => 21,
            AArch64Reg::X22 => 22,
            AArch64Reg::X23 => 23,
            AArch64Reg::X24 => 24,
            AArch64Reg::X25 => 25,
            AArch64Reg::X26 => 26,
            AArch64Reg::X27 => 27,
            AArch64Reg::X28 => 28,
            AArch64Reg::X29 => 29,
            AArch64Reg::X30 => 30,
            AArch64Reg::SP => 31,
            AArch64Reg::XZR => 31,
        }
    }

    /// Apple Silicon ARM64 argument passing registers (x0..x7).
    pub const ARG_REGS: [AArch64Reg; 8] = [
        AArch64Reg::X0,
        AArch64Reg::X1,
        AArch64Reg::X2,
        AArch64Reg::X3,
        AArch64Reg::X4,
        AArch64Reg::X5,
        AArch64Reg::X6,
        AArch64Reg::X7,
    ];

    /// Callee-saved non-volatile registers (x19..x28).
    /// Preserved across function calls by prologue/epilogue.
    pub const CALLEE_SAVED_REGS: [AArch64Reg; 10] = [
        AArch64Reg::X19,
        AArch64Reg::X20,
        AArch64Reg::X21,
        AArch64Reg::X22,
        AArch64Reg::X23,
        AArch64Reg::X24,
        AArch64Reg::X25,
        AArch64Reg::X26,
        AArch64Reg::X27,
        AArch64Reg::X28,
    ];

    /// Caller-saved volatile registers available for local allocation (x9..x15).
    /// Note: x16/x17 are reserved as intra-procedure call scratches (IP0/IP1).
    pub const CALLER_SAVED_REGS: [AArch64Reg; 7] = [
        AArch64Reg::X9,
        AArch64Reg::X10,
        AArch64Reg::X11,
        AArch64Reg::X12,
        AArch64Reg::X13,
        AArch64Reg::X14,
        AArch64Reg::X15,
    ];

    pub const fn name(&self) -> &'static str {
        match self {
            AArch64Reg::X0 => "x0",
            AArch64Reg::X1 => "x1",
            AArch64Reg::X2 => "x2",
            AArch64Reg::X3 => "x3",
            AArch64Reg::X4 => "x4",
            AArch64Reg::X5 => "x5",
            AArch64Reg::X6 => "x6",
            AArch64Reg::X7 => "x7",
            AArch64Reg::X8 => "x8",
            AArch64Reg::X9 => "x9",
            AArch64Reg::X10 => "x10",
            AArch64Reg::X11 => "x11",
            AArch64Reg::X12 => "x12",
            AArch64Reg::X13 => "x13",
            AArch64Reg::X14 => "x14",
            AArch64Reg::X15 => "x15",
            AArch64Reg::X16 => "x16",
            AArch64Reg::X17 => "x17",
            AArch64Reg::X18 => "x18",
            AArch64Reg::X19 => "x19",
            AArch64Reg::X20 => "x20",
            AArch64Reg::X21 => "x21",
            AArch64Reg::X22 => "x22",
            AArch64Reg::X23 => "x23",
            AArch64Reg::X24 => "x24",
            AArch64Reg::X25 => "x25",
            AArch64Reg::X26 => "x26",
            AArch64Reg::X27 => "x27",
            AArch64Reg::X28 => "x28",
            AArch64Reg::X29 => "x29",
            AArch64Reg::X30 => "x30",
            AArch64Reg::SP => "sp",
            AArch64Reg::XZR => "xzr",
        }
    }
}

impl fmt::Display for AArch64Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
