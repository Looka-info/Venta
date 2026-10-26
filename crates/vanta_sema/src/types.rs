//! Semantic types and layout for Vanta.

use std::collections::HashMap;
use std::fmt;
use vanta_source::Span;

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
    Slice(Box<Type>),
    Struct(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<StructField>,
    pub total_size: usize,
    pub alignment: usize,
    pub is_copy: bool,
    pub span: Span,
}

impl StructDef {
    pub fn compute_layout(
        name: String,
        raw_fields: Vec<(String, Type)>,
        struct_defs: &HashMap<String, StructDef>,
        span: Span,
    ) -> Self {
        let mut fields = Vec::with_capacity(raw_fields.len());
        let mut current_offset = 0;
        let mut max_align = 1;
        let mut all_copy = true;

        for (fname, fty) in raw_fields {
            let align = fty.alignment_with_structs(struct_defs).max(1);
            let size = fty.size_with_structs(struct_defs);
            if align > max_align {
                max_align = align;
            }
            if !fty.is_copy_with_structs(struct_defs) {
                all_copy = false;
            }
            // Natural alignment padding: align offset up to field's natural alignment
            current_offset = (current_offset + align - 1) & !(align - 1);
            let field_offset = current_offset;
            current_offset += size;

            fields.push(StructField {
                name: fname,
                ty: fty,
                offset: field_offset,
                size,
            });
        }

        // Round total size up to struct alignment
        let total_size = if max_align > 0 {
            (current_offset + max_align - 1) & !(max_align - 1)
        } else {
            current_offset
        };

        Self {
            name,
            fields,
            total_size,
            alignment: max_align,
            is_copy: all_copy,
            span,
        }
    }

    pub fn find_field(&self, name: &str) -> Option<&StructField> {
        self.fields.iter().find(|f| f.name == name)
    }
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
            Type::Slice(inner) => write!(f, "[{}]", inner),
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

    pub fn is_slice(&self) -> bool {
        matches!(self, Type::Ref { is_mut: false, inner } if matches!(**inner, Type::Slice(_)))
    }

    pub fn slice_elem_ty(&self) -> Option<&Type> {
        if let Type::Ref { inner, .. } = self {
            if let Type::Slice(elem) = &**inner {
                return Some(elem);
            }
        }
        None
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
            Type::Str | Type::Slice(_) => false,
            Type::Ref { is_mut, .. } => !*is_mut, // Shared references (&T and &str) are Copy; &mut T are Move
            Type::Array { inner, .. } => inner.is_copy(),
            Type::Struct(_) => false, // Default non-copy for structs unless known
        }
    }

    pub fn is_copy_with_structs(&self, structs: &HashMap<String, StructDef>) -> bool {
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
            Type::Str | Type::Slice(_) => false,
            Type::Ref { is_mut, .. } => !*is_mut,
            Type::Array { inner, .. } => inner.is_copy_with_structs(structs),
            Type::Struct(name) => structs.get(name).map(|s| s.is_copy).unwrap_or(false),
        }
    }

    pub fn size_in_bytes(&self) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 => 8,
            Type::Unit | Type::Str | Type::Slice(_) => 0,
            Type::Ref { inner, .. } => {
                if **inner == Type::Str || matches!(**inner, Type::Slice(_)) {
                    16 // 2-word slice descriptor (data pointer + byte/elem length)
                } else {
                    8 // 64-bit pointers on x86-64
                }
            }
            Type::Array { inner, len } => inner.size_in_bytes() * len,
            Type::Struct(_) => 8, // Default pointer/slot size for initial structs
        }
    }

    pub fn size_with_structs(&self, structs: &HashMap<String, StructDef>) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 => 8,
            Type::Unit | Type::Str | Type::Slice(_) => 0,
            Type::Ref { inner, .. } => {
                if **inner == Type::Str || matches!(**inner, Type::Slice(_)) {
                    16 // 2-word slice descriptor (data pointer + byte/elem length)
                } else {
                    8 // 64-bit pointers on x86-64
                }
            }
            Type::Array { inner, len } => inner.size_with_structs(structs) * len,
            Type::Struct(name) => structs.get(name).map(|s| s.total_size).unwrap_or(8),
        }
    }

    pub fn alignment(&self) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool | Type::Str | Type::Slice(_) => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 => 8,
            Type::Unit => 1,
            Type::Ref { .. } => 8,
            Type::Array { inner, .. } => inner.alignment(),
            Type::Struct(_) => 8,
        }
    }

    pub fn alignment_with_structs(&self, structs: &HashMap<String, StructDef>) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool | Type::Str | Type::Slice(_) => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 => 8,
            Type::Unit => 1,
            Type::Ref { .. } => 8,
            Type::Array { inner, .. } => inner.alignment_with_structs(structs),
            Type::Struct(name) => structs.get(name).map(|s| s.alignment).unwrap_or(8),
        }
    }
}
