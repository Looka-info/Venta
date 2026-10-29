//! x86-64 Machine Intermediate Representation (MIR / LIR).

use crate::register::{Reg, SimdReg, VectorWidth};
use crate::simd_alloc::SimdValueId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionCode {
    Equal,        // e / z
    NotEqual,     // ne / nz
    Less,         // l (signed)
    LessEqual,    // le (signed)
    Greater,      // g (signed)
    GreaterEqual, // ge (signed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    Reg(Reg),
    Imm(i64),
    /// Memory address: base + offset, e.g. [rbp - 16]
    Mem {
        base: Reg,
        offset: i32,
    },
    /// Indexed memory: base + index * scale + offset
    IndexedMem {
        base: Reg,
        index: Reg,
        scale: u8,
        offset: i32,
    },
}

/// Operand for SIMD vector instructions (register, virtual value, or base+offset memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimdOperand {
    /// Physical SIMD register (after allocation).
    Reg(SimdReg),
    /// Virtual SIMD vector value (before allocation).
    Val(SimdValueId),
    /// Memory address: base + offset.
    Mem { base: Reg, offset: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineInst {
    /// mov dest, src
    Mov { dest: Operand, src: Operand },
    /// add dest, src
    Add { dest: Operand, src: Operand },
    /// sub dest, src
    Sub { dest: Operand, src: Operand },
    /// imul dest, src
    Imul { dest: Reg, src: Operand },
    /// idiv divisor (implicit rdx:rax / divisor)
    Idiv(Reg),
    /// cqo (sign-extend rax into rdx:rax)
    Cqo,
    /// and dest, src
    And { dest: Operand, src: Operand },
    /// or dest, src
    Or { dest: Operand, src: Operand },
    /// xor dest, src
    Xor { dest: Operand, src: Operand },
    /// shl reg, cl or imm
    Shl { dest: Reg, count: Operand },
    /// shr reg, cl or imm
    Shr { dest: Reg, count: Operand },
    /// neg dest
    Neg(Reg),
    /// not dest
    Not(Reg),
    /// cmp lhs, rhs
    Cmp { lhs: Operand, rhs: Operand },
    /// test lhs, rhs
    Test { lhs: Reg, rhs: Reg },
    /// setcc reg (8-bit)
    Setcc { cond: ConditionCode, dest: Reg },
    /// movzx dest, byte_reg (zero-extend 8-bit to 64-bit)
    Movzx { dest: Reg, src: Reg },
    /// movzx dest, byte ptr [base + offset] (zero-extend 8-bit memory to 64-bit)
    MovzxMem { dest: Reg, base: Reg, offset: i32 },
    /// mov dest_32, dword ptr [base + offset] (zero-extend 32-bit memory to 64-bit)
    Mov32Mem { dest: Reg, base: Reg, offset: i32 },
    /// mov dword ptr [base + offset], src_32 (store 32-bit register to memory)
    MovMem32 { base: Reg, offset: i32, src: Reg },
    /// lea dest, [base + offset]
    Lea { dest: Reg, src: Operand },
    /// lea dest, [rip + symbol]
    LeaRip { dest: Reg, symbol: String },
    /// push reg
    Push(Reg),
    /// pop reg
    Pop(Reg),
    /// jmp label
    Jmp(String),
    /// jcc label
    Jcc { cond: ConditionCode, target: String },
    /// call function_name
    Call(String),
    /// Direct sibling tail call jump to external/symbolic callee: jmp symbol
    TailJmp(String),
    /// ret
    Ret,
    /// Local label
    Label(String),
    /// Source location marker for DWARF debug line emission (emits 0 bytes into .text).
    SourceLoc {
        file_id: u32,
        line: u32,
        column: u32,
    },
    /// vmovups dest, src (unaligned packed load, store, or register copy)
    Vmovups {
        width: VectorWidth,
        dest: SimdOperand,
        src: SimdOperand,
    },
    /// vpaddq dest, src1, src2 (packed 64-bit integer addition)
    Vpaddq {
        width: VectorWidth,
        dest: SimdOperand,
        src1: SimdOperand,
        src2: SimdOperand,
    },
    /// vfmadd231pd dest, src1, src2 (packed double precision fused multiply-add: dest += src1 * src2)
    Vfmadd231pd {
        width: VectorWidth,
        dest: SimdOperand,
        src1: SimdOperand,
        src2: SimdOperand,
    },
}
