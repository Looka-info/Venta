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
    Enum(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
    pub offset: usize,
    pub size: usize,
    pub is_pub: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<StructField>,
    pub total_size: usize,
    pub alignment: usize,
    pub is_copy: bool,
    pub defining_module: String,
    pub span: Span,
}

impl StructDef {
    pub fn compute_layout(
        name: String,
        raw_fields: Vec<(String, Type)>,
        struct_defs: &HashMap<String, StructDef>,
        span: Span,
    ) -> Self {
        Self::compute_layout_with_enums(name, raw_fields, struct_defs, &HashMap::new(), span)
    }

    pub fn compute_layout_with_enums(
        name: String,
        raw_fields: Vec<(String, Type)>,
        struct_defs: &HashMap<String, StructDef>,
        enum_defs: &HashMap<String, EnumDef>,
        span: Span,
    ) -> Self {
        let fields_with_pub = raw_fields.into_iter().map(|(n, t)| (n, t, true)).collect();
        Self::compute_layout_with_enums_and_module(
            name,
            fields_with_pub,
            struct_defs,
            enum_defs,
            String::new(),
            span,
        )
    }

    pub fn compute_layout_with_enums_and_module(
        name: String,
        raw_fields: Vec<(String, Type, bool)>,
        struct_defs: &HashMap<String, StructDef>,
        enum_defs: &HashMap<String, EnumDef>,
        defining_module: String,
        span: Span,
    ) -> Self {
        let mut fields = Vec::with_capacity(raw_fields.len());
        let mut current_offset = 0;
        let mut max_align = 1;
        let mut all_copy = true;

        if name == "v256"
            || name == "Simd256"
            || name.ends_with("__v256")
            || name.ends_with("__Simd256")
        {
            max_align = max_align.max(32);
        }

        for (fname, fty, is_pub) in raw_fields {
            let align = fty.alignment_with_context(struct_defs, enum_defs).max(1);
            let size = fty.size_with_context(struct_defs, enum_defs);
            if align > max_align {
                max_align = align;
            }
            if !fty.is_copy_with_context(struct_defs, enum_defs) {
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
                is_pub,
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
            defining_module,
            span,
        }
    }

    pub fn find_field(&self, name: &str) -> Option<&StructField> {
        self.fields.iter().find(|f| f.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDef {
    pub name: String,
    pub tag: u32,
    pub payload: Vec<Type>,
    pub payload_offsets: Vec<usize>,
    pub payload_size: usize,
    pub payload_align: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<VariantDef>,
    pub total_size: usize,
    pub alignment: usize,
    pub is_copy: bool,
    pub payload_offset: usize,
    pub defining_module: String,
    pub span: Span,
}

impl EnumDef {
    pub fn compute_layout(
        name: String,
        raw_variants: Vec<(String, Vec<Type>)>,
        struct_defs: &HashMap<String, StructDef>,
        enum_defs: &HashMap<String, EnumDef>,
        span: Span,
    ) -> Result<Self, String> {
        Self::compute_layout_with_module(
            name,
            raw_variants,
            struct_defs,
            enum_defs,
            String::new(),
            span,
        )
    }

    pub fn compute_layout_with_module(
        name: String,
        raw_variants: Vec<(String, Vec<Type>)>,
        struct_defs: &HashMap<String, StructDef>,
        enum_defs: &HashMap<String, EnumDef>,
        defining_module: String,
        span: Span,
    ) -> Result<Self, String> {
        let tag_size = 4usize;
        let tag_align = 4usize;

        let mut max_payload_align = 1usize;
        for (_, payload_tys) in &raw_variants {
            for ty in payload_tys {
                let a = ty.alignment_with_context(struct_defs, enum_defs);
                if a > max_payload_align {
                    max_payload_align = a;
                }
            }
        }

        let payload_offset = (tag_size + max_payload_align - 1) & !(max_payload_align - 1);

        let mut variants = Vec::with_capacity(raw_variants.len());
        let mut max_variant_size = 0usize;
        let mut all_copy = true;

        for (tag_idx, (vname, payload_tys)) in raw_variants.into_iter().enumerate() {
            let mut offsets = Vec::with_capacity(payload_tys.len());
            let mut curr_offset = payload_offset;
            let mut var_align = 1usize;

            for ty in &payload_tys {
                let align = ty.alignment_with_context(struct_defs, enum_defs);
                let size = ty.size_with_context(struct_defs, enum_defs);
                if align > var_align {
                    var_align = align;
                }
                if !ty.is_copy_with_context(struct_defs, enum_defs) {
                    all_copy = false;
                }

                curr_offset = (curr_offset + align - 1) & !(align - 1);
                offsets.push(curr_offset);
                curr_offset = curr_offset
                    .checked_add(size)
                    .ok_or_else(|| "Layout overflow in enum variant".to_string())?;
            }

            let p_size = curr_offset - payload_offset;
            if p_size > max_variant_size {
                max_variant_size = p_size;
            }

            variants.push(VariantDef {
                name: vname,
                tag: tag_idx as u32,
                payload: payload_tys,
                payload_offsets: offsets,
                payload_size: p_size,
                payload_align: var_align,
            });
        }

        let overall_align = tag_align.max(max_payload_align);
        let unaligned_total = if max_variant_size == 0 {
            tag_size
        } else {
            payload_offset + max_variant_size
        };
        let total_size = (unaligned_total + overall_align - 1) & !(overall_align - 1);

        Ok(Self {
            name,
            variants,
            total_size,
            alignment: overall_align,
            is_copy: all_copy,
            payload_offset,
            defining_module,
            span,
        })
    }

    pub fn find_variant(&self, name: &str) -> Option<&VariantDef> {
        self.variants.iter().find(|v| v.name == name)
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
            Type::Enum(name) => write!(f, "{}", name),
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
            Type::Struct(_) | Type::Enum(_) => false,
        }
    }

    pub fn is_copy_with_context(
        &self,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
    ) -> bool {
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
            Type::Array { inner, .. } => inner.is_copy_with_context(structs, enums),
            Type::Struct(name) => {
                if name == "v256"
                    || name == "Simd256"
                    || name.ends_with("__v256")
                    || name.ends_with("__Simd256")
                {
                    true
                } else if name == "Vec" || name.starts_with("Vec__") || self.is_string() {
                    false
                } else {
                    structs.get(name).map(|s| s.is_copy).unwrap_or(false)
                }
            }
            Type::Enum(name) => enums.get(name).map(|e| e.is_copy).unwrap_or(false),
        }
    }

    pub fn is_copy_with_structs(&self, structs: &HashMap<String, StructDef>) -> bool {
        self.is_copy_with_context(structs, &HashMap::new())
    }

    pub fn is_vec(&self) -> bool {
        matches!(self, Type::Struct(name) if name == "Vec" || name.starts_with("Vec__"))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Type::Struct(name) if name == "String" || name == "std__string__String" || name.ends_with("::String") || name.ends_with("__String"))
    }

    pub fn vec_elem_type(&self) -> Option<Type> {
        if let Type::Struct(name) = self {
            if let Some(suffix) = name.strip_prefix("Vec__") {
                return Some(Self::from_mangled_string(suffix));
            }
        }
        None
    }

    pub fn from_mangled_string(s: &str) -> Type {
        match s {
            "i8" => Type::I8,
            "i16" => Type::I16,
            "i32" => Type::I32,
            "i64" => Type::I64,
            "u8" => Type::U8,
            "u16" => Type::U16,
            "u32" => Type::U32,
            "u64" => Type::U64,
            "f32" => Type::F32,
            "f64" => Type::F64,
            "bool" => Type::Bool,
            "str" => Type::Str,
            "unit" => Type::Unit,
            other => Type::Struct(other.to_string()),
        }
    }

    pub fn needs_drop(
        &self,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
    ) -> bool {
        match self {
            Type::Struct(name) => {
                if name == "Vec" || name.starts_with("Vec__") || self.is_string() {
                    true
                } else if let Some(s) = structs.get(name) {
                    s.fields.iter().any(|f| f.ty.needs_drop(structs, enums))
                } else {
                    false
                }
            }
            Type::Enum(name) => {
                if let Some(e) = enums.get(name) {
                    e.variants
                        .iter()
                        .any(|v| v.payload.iter().any(|p| p.needs_drop(structs, enums)))
                } else {
                    false
                }
            }
            Type::Array { inner, .. } => inner.needs_drop(structs, enums),
            _ => false,
        }
    }

    pub fn size_with_context(
        &self,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
    ) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 => 8,
            Type::Unit | Type::Str | Type::Slice(_) => 0,
            Type::Ref { inner, .. } => {
                if **inner == Type::Str || matches!(**inner, Type::Slice(_)) {
                    16
                } else {
                    8
                }
            }
            Type::Array { inner, len } => inner.size_with_context(structs, enums) * len,
            Type::Struct(name) => {
                if name == "v256"
                    || name == "Simd256"
                    || name.ends_with("__v256")
                    || name.ends_with("__Simd256")
                {
                    32
                } else if name == "Vec" || name.starts_with("Vec__") || self.is_string() {
                    24
                } else {
                    structs.get(name).map(|s| s.total_size).unwrap_or(8)
                }
            }
            Type::Enum(name) => enums.get(name).map(|e| e.total_size).unwrap_or(8),
        }
    }

    pub fn alignment_with_context(
        &self,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
    ) -> usize {
        match self {
            Type::I8 | Type::U8 | Type::Bool | Type::Str | Type::Slice(_) => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 => 8,
            Type::Unit => 1,
            Type::Ref { .. } => 8,
            Type::Array { inner, .. } => inner.alignment_with_context(structs, enums),
            Type::Struct(name) => {
                if name == "v256"
                    || name == "Simd256"
                    || name.ends_with("__v256")
                    || name.ends_with("__Simd256")
                {
                    32
                } else if name == "Vec" || name.starts_with("Vec__") || self.is_string() {
                    8
                } else {
                    structs.get(name).map(|s| s.alignment).unwrap_or(8)
                }
            }
            Type::Enum(name) => enums.get(name).map(|e| e.alignment).unwrap_or(4),
        }
    }

    pub fn to_mangled_string(&self) -> String {
        match self {
            Type::I8 => "i8".into(),
            Type::I16 => "i16".into(),
            Type::I32 => "i32".into(),
            Type::I64 => "i64".into(),
            Type::U8 => "u8".into(),
            Type::U16 => "u16".into(),
            Type::U32 => "u32".into(),
            Type::U64 => "u64".into(),
            Type::F32 => "f32".into(),
            Type::F64 => "f64".into(),
            Type::Bool => "bool".into(),
            Type::Str => "str".into(),
            Type::Unit => "unit".into(),
            Type::Ref { is_mut, inner } => {
                if *is_mut {
                    format!("ref_mut_{}", inner.to_mangled_string())
                } else {
                    format!("ref_{}", inner.to_mangled_string())
                }
            }
            Type::Array { inner, len } => format!("arr_{}_{}", inner.to_mangled_string(), len),
            Type::Slice(inner) => format!("slice_{}", inner.to_mangled_string()),
            Type::Struct(name) => name.clone(),
            Type::Enum(name) => name.clone(),
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
            Type::Struct(_) | Type::Enum(_) => {
                if self.is_vec() || self.is_string() {
                    24
                } else {
                    8
                }
            }
        }
    }

    pub fn size_with_structs(&self, structs: &HashMap<String, StructDef>) -> usize {
        self.size_with_context(structs, &HashMap::new())
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
            Type::Struct(_) | Type::Enum(_) => 8,
        }
    }

    pub fn alignment_with_structs(&self, structs: &HashMap<String, StructDef>) -> usize {
        self.alignment_with_context(structs, &HashMap::new())
    }
}
