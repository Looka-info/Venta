//! Semantic types and layout for Vanta.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Unit,
    Ref {
        is_mut: bool,
        inner: Box<Type>,
    },
    Array {
        inner: Box<Type>,
        len: usize,
    },
    Struct(String),
}

impl Type {
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64
        )
    }

    pub fn is_signed_integer(&self) -> bool {
        matches!(self, Type::I8 | Type::I16 | Type::I32 | Type::I64)
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Type::Bool)
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Unit => true,
            Type::Ref { is_mut, .. } => !*is_mut, // Shared references &T are Copy; &mut T are Move
            Type::Array { inner, .. } => inner.is_copy(),
            Type::Struct(_) => false, // Default non-copy for structs unless Copy derived
        }
    }

    pub fn size_in_bytes(&self) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 => 8,
            Type::Unit => 0,
            Type::Ref { .. } => 8, // 64-bit pointers on x86-64
            Type::Array { inner, len } => inner.size_in_bytes() * len,
            Type::Struct(_) => 8, // Default pointer/slot size for initial structs
        }
    }

    pub fn alignment(&self) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 => 8,
            Type::Unit => 1,
            Type::Ref { .. } => 8,
            Type::Array { inner, .. } => inner.alignment(),
            Type::Struct(_) => 8,
        }
    }
}
