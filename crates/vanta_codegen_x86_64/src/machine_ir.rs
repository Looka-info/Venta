//! x86-64 Machine Intermediate Representation (MIR / LIR).

use crate::register::Reg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionCode {
    Equal,       // e / z
    NotEqual,    // ne / nz
    Less,        // l (signed)
    LessEqual,   // le (signed)
    Greater,     // g (signed)
    GreaterEqual,// ge (signed)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineInst {
    /// mov dest, src
    Mov {
        dest: Operand,
        src: Operand,
    },
    /// add dest, src
    Add {
        dest: Operand,
        src: Operand,
    },
    /// sub dest, src
    Sub {
        dest: Operand,
        src: Operand,
    },
    /// imul dest, src
    Imul {
        dest: Reg,
        src: Operand,
    },
    /// idiv divisor (implicit rdx:rax / divisor)
    Idiv(Reg),
    /// cqo (sign-extend rax into rdx:rax)
    Cqo,
    /// and dest, src
    And {
        dest: Operand,
        src: Operand,
    },
    /// or dest, src
    Or {
        dest: Operand,
        src: Operand,
    },
    /// xor dest, src
    Xor {
        dest: Operand,
        src: Operand,
    },
    /// shl reg, cl or imm
    Shl {
        dest: Reg,
        count: Operand,
    },
    /// shr reg, cl or imm
    Shr {
        dest: Reg,
        count: Operand,
    },
    /// neg dest
    Neg(Reg),
    /// not dest
    Not(Reg),
    /// cmp lhs, rhs
    Cmp {
        lhs: Operand,
        rhs: Operand,
    },
    /// test lhs, rhs
    Test {
        lhs: Reg,
        rhs: Reg,
    },
    /// setcc reg (8-bit)
    Setcc {
        cond: ConditionCode,
        dest: Reg,
    },
    /// movzx dest, byte_reg (zero-extend 8-bit to 64-bit)
    Movzx {
        dest: Reg,
        src: Reg,
    },
    /// lea dest, [base + offset]
    Lea {
        dest: Reg,
        src: Operand,
    },
    /// push reg
    Push(Reg),
    /// pop reg
    Pop(Reg),
    /// jmp label
    Jmp(String),
    /// jcc label
    Jcc {
        cond: ConditionCode,
        target: String,
    },
    /// call function_name
    Call(String),
    /// ret
    Ret,
    /// Local label
    Label(String),
}
