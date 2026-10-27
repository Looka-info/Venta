//! Vanta Mid-Level IR (MIR) definitions.

use std::collections::HashMap;
use vanta_sema::{EnumDef, StructDef, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone)]
pub struct LocalSlot {
    pub id: SlotId,
    pub name: String,
    pub ty: Type,
    pub size: usize,
    pub align: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MirBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MirUnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    ConstInt {
        dest: ValueId,
        val: i64,
        ty: Type,
    },
    ConstBool {
        dest: ValueId,
        val: bool,
    },
    SlotAddr {
        dest: ValueId,
        slot: SlotId,
    },
    Load {
        dest: ValueId,
        slot: SlotId,
        ty: Type,
    },
    Store {
        slot: SlotId,
        val: ValueId,
    },
    Binary {
        dest: ValueId,
        op: MirBinaryOp,
        lhs: ValueId,
        rhs: ValueId,
        ty: Type,
    },
    Unary {
        dest: ValueId,
        op: MirUnaryOp,
        operand: ValueId,
        ty: Type,
    },
    Call {
        dest: Option<ValueId>,
        callee: String,
        args: Vec<ValueId>,
        ret_type: Type,
    },
    IndexAddr {
        dest: ValueId,
        slot: SlotId,
        index: ValueId,
        elem_size: usize,
    },
    IndexPtrAddr {
        dest: ValueId,
        base_ptr: ValueId,
        index: ValueId,
        elem_size: usize,
    },
    FieldAddr {
        dest: ValueId,
        base_ptr: ValueId,
        offset: usize,
    },
    LoadIndirect {
        dest: ValueId,
        addr: ValueId,
        ty: Type,
    },
    StoreIndirect {
        addr: ValueId,
        val: ValueId,
    },
    StoreIndirect32 {
        addr: ValueId,
        val: ValueId,
    },
    StrSlice {
        dest_ptr: ValueId,
        dest_len: ValueId,
        symbol: String,
        len: usize,
    },
    StoreSlice {
        slot: SlotId,
        ptr: ValueId,
        len: ValueId,
    },
    LoadSlice {
        dest_ptr: ValueId,
        dest_len: ValueId,
        slot: SlotId,
    },
    Phi {
        dest: ValueId,
        inputs: Vec<(BlockId, ValueId)>,
        ty: Type,
    },
    Copy {
        dest: ValueId,
        src: ValueId,
        ty: Type,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    Branch(BlockId),
    CondBranch {
        cond: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
    Return(Option<ValueId>),
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub name: String,
    pub instructions: Vec<Instruction>,
    pub terminator: Option<Terminator>,
}

#[derive(Debug, Clone)]
pub struct MirFunction {
    pub name: String,
    pub params: Vec<(SlotId, Type)>,
    pub ret_type: Type,
    pub slots: Vec<LocalSlot>,
    pub blocks: Vec<BasicBlock>,
}

impl MirFunction {
    pub fn max_value_id(&self) -> u32 {
        let mut max = 0;
        for block in &self.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::ConstInt { dest, .. }
                    | Instruction::ConstBool { dest, .. }
                    | Instruction::SlotAddr { dest, .. }
                    | Instruction::Load { dest, .. }
                    | Instruction::Binary { dest, .. }
                    | Instruction::Unary { dest, .. }
                    | Instruction::IndexAddr { dest, .. }
                    | Instruction::IndexPtrAddr { dest, .. }
                    | Instruction::FieldAddr { dest, .. }
                    | Instruction::LoadIndirect { dest, .. }
                    | Instruction::Phi { dest, .. }
                    | Instruction::Copy { dest, .. } => {
                        max = max.max(dest.0);
                    }
                    Instruction::Call { dest, args, .. } => {
                        if let Some(d) = dest {
                            max = max.max(d.0);
                        }
                        for a in args {
                            max = max.max(a.0);
                        }
                    }
                    Instruction::StrSlice {
                        dest_ptr,
                        dest_len,
                        ..
                    }
                    | Instruction::LoadSlice {
                        dest_ptr,
                        dest_len,
                        ..
                    } => {
                        max = max.max(dest_ptr.0).max(dest_len.0);
                    }
                    Instruction::Store { val, .. } => {
                        max = max.max(val.0);
                    }
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
                        max = max.max(addr.0).max(val.0);
                    }
                    Instruction::StoreSlice { ptr, len, .. } => {
                        max = max.max(ptr.0).max(len.0);
                    }
                }
            }
            if let Some(term) = &block.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        max = max.max(cond.0);
                    }
                    Terminator::Return(Some(val)) => {
                        max = max.max(val.0);
                    }
                    _ => {}
                }
            }
        }
        max
    }

    pub fn max_block_id(&self) -> u32 {
        self.blocks.iter().map(|b| b.id.0).max().unwrap_or(0)
    }
}

impl Instruction {
    /// Returns the SSA `ValueId`s defined by this instruction.
    pub fn defs(&self) -> Vec<ValueId> {
        match self {
            Instruction::ConstInt { dest, .. }
            | Instruction::ConstBool { dest, .. }
            | Instruction::SlotAddr { dest, .. }
            | Instruction::Load { dest, .. }
            | Instruction::Binary { dest, .. }
            | Instruction::Unary { dest, .. }
            | Instruction::IndexAddr { dest, .. }
            | Instruction::IndexPtrAddr { dest, .. }
            | Instruction::FieldAddr { dest, .. }
            | Instruction::LoadIndirect { dest, .. }
            | Instruction::Copy { dest, .. }
            | Instruction::Phi { dest, .. } => vec![*dest],
            Instruction::Call { dest, .. } => dest.map(|d| vec![d]).unwrap_or_default(),
            Instruction::StrSlice {
                dest_ptr, dest_len, ..
            }
            | Instruction::LoadSlice {
                dest_ptr, dest_len, ..
            } => vec![*dest_ptr, *dest_len],
            Instruction::Store { .. }
            | Instruction::StoreIndirect { .. }
            | Instruction::StoreIndirect32 { .. }
            | Instruction::StoreSlice { .. } => Vec::new(),
        }
    }

    /// Returns the SSA `ValueId`s read/used by this instruction.
    pub fn uses(&self) -> Vec<ValueId> {
        match self {
            Instruction::ConstInt { .. }
            | Instruction::ConstBool { .. }
            | Instruction::SlotAddr { .. }
            | Instruction::Load { .. }
            | Instruction::StrSlice { .. }
            | Instruction::LoadSlice { .. } => Vec::new(),
            Instruction::Store { val, .. } => vec![*val],
            Instruction::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
            Instruction::Unary { operand, .. } => vec![*operand],
            Instruction::Call { args, .. } => args.clone(),
            Instruction::IndexAddr { index, .. } => vec![*index],
            Instruction::IndexPtrAddr {
                base_ptr, index, ..
            } => vec![*base_ptr, *index],
            Instruction::FieldAddr { base_ptr, .. } => vec![*base_ptr],
            Instruction::LoadIndirect { addr, .. } => vec![*addr],
            Instruction::StoreIndirect { addr, val }
            | Instruction::StoreIndirect32 { addr, val } => vec![*addr, *val],
            Instruction::StoreSlice { ptr, len, .. } => vec![*ptr, *len],
            Instruction::Copy { src, .. } => vec![*src],
            Instruction::Phi { inputs, .. } => inputs.iter().map(|(_, v)| *v).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLiteral {
    pub symbol: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct MirProgram {
    pub functions: Vec<MirFunction>,
    pub string_literals: Vec<StringLiteral>,
    pub structs: HashMap<String, StructDef>,
    pub enums: HashMap<String, EnumDef>,
}
