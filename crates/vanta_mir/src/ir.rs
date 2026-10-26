//! Vanta Mid-Level IR (MIR) definitions.

use vanta_sema::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone)]
pub struct LocalSlot {
    pub id: SlotId,
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    LoadIndirect {
        dest: ValueId,
        addr: ValueId,
        ty: Type,
    },
    StoreIndirect {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringLiteral {
    pub symbol: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct MirProgram {
    pub functions: Vec<MirFunction>,
    pub string_literals: Vec<StringLiteral>,
}
