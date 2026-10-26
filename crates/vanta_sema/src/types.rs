//! Semantic types and layout for Vanta.

use std::fmt;

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
    Str,
    Unit,
    Ref { is_mut: bool, inner: Box<Type> },
    Array { inner: Box<Type>, len: usize },
    Struct(String),
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::I8 => write!(f, "i8"),
            Type::I16 => write!(f, "i16"),
            Type::I32 => write!(f, "i32"),
            Type::I64 => write!(f, "i64"),
            Type::U8 => write!(f, "u8"),
            Type::U16 => write!(f, "u16"),
            Type::U32 => write!(f, "u32"),
            Type::U64 => write!(f, "u64"),
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Unit => write!(f, "()"),
            Type::Ref { is_mut, inner } => {
                if *is_mut {
                    write!(f, "&mut {}", inner)
                } else {
                    write!(f, "&{}", inner)
                }
            }
            Type::Array { inner, len } => write!(f, "[{}; {}]", inner, len),
            Type::Struct(name) => write!(f, "{}", name),
        }
    }
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

    pub fn is_str_slice(&self) -> bool {
        matches!(self, Type::Ref { is_mut: false, inner } if **inner == Type::Str)
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
            Type::Str => false,                   // Dynamically sized
            Type::Ref { is_mut, .. } => !*is_mut, // Shared references (&T and &str) are Copy; &mut T are Move
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
            Type::Unit | Type::Str => 0,
            Type::Ref { inner, .. } => {
                if **inner == Type::Str {
                    16 // 2-word slice descriptor (data pointer + byte length)
                } else {
                    8 // 64-bit pointers on x86-64
                }
            }
            Type::Array { inner, len } => inner.size_in_bytes() * len,
            Type::Struct(_) => 8, // Default pointer/slot size for initial structs
        }
    }

    pub fn alignment(&self) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool | Type::Str => 1,
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
