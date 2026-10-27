//! System V AMD64 ABI classification and calling convention specification.
//!
//! Implements Section 3.2.3 of the System V AMD64 ABI for parameter passing
//! and aggregate return classification.

use std::collections::HashMap;
use vanta_sema::{EnumDef, StructDef, Type};

/// The ABI return and parameter classification of a Vanta type under System V AMD64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiClass {
    /// Zero-sized or unit return (no registers, no memory storage).
    Void,
    /// Up to 8 bytes, returned/passed in a single integer register (%rax for return).
    Integer1 { size: usize },
    /// 9 to 16 bytes, returned/passed in two integer registers (%rax low, %rdx high for return).
    Integer2 { size: usize },
    /// Memory class: returned via caller-provided storage passed as a hidden first
    /// pointer argument in %rdi (`sret`). Callee returns %rdi in %rax.
    Memory { size: usize, align: usize },
}

impl AbiClass {
    /// Whether this type requires a caller-allocated hidden return pointer (%rdi).
    #[inline]
    pub fn is_memory(&self) -> bool {
        matches!(self, AbiClass::Memory { .. })
    }

    /// Whether this type is void / unit.
    #[inline]
    pub fn is_void(&self) -> bool {
        matches!(self, AbiClass::Void)
    }

    /// Total byte size of the type.
    pub fn size(&self) -> usize {
        match self {
            AbiClass::Void => 0,
            AbiClass::Integer1 { size } => *size,
            AbiClass::Integer2 { size } => *size,
            AbiClass::Memory { size, .. } => *size,
        }
    }
}

/// Classify a Vanta type according to the System V AMD64 ABI rules.
///
/// Under System V AMD64:
/// 1. An aggregate with size > 16 bytes is classified as MEMORY.
/// 2. An aggregate containing unaligned fields is classified as MEMORY, even if size <= 16 bytes.
/// 3. An aggregate containing any field classified as MEMORY is classified as MEMORY.
/// 4. An aggregate of size <= 8 bytes without unaligned fields is classified as INTEGER (1 eightbyte).
/// 5. An aggregate of 9..16 bytes without unaligned fields is classified as INTEGER (2 eightbytes).
/// 6. Primitives (i8..i64, u8..u64, bool, refs) are classified as INTEGER.
/// 7. Slices and &str (16-byte fat pointers) are classified as INTEGER2 (ptr in 1st reg, len in 2nd reg).
pub fn classify_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> AbiClass {
    match ty {
        Type::Unit => AbiClass::Void,
        Type::I8 | Type::U8 | Type::Bool => AbiClass::Integer1 { size: 1 },
        Type::I16 | Type::U16 => AbiClass::Integer1 { size: 2 },
        Type::I32 | Type::U32 | Type::F32 => AbiClass::Integer1 { size: 4 },
        Type::I64 | Type::U64 | Type::F64 => AbiClass::Integer1 { size: 8 },
        Type::Ref { inner, .. } => {
            if **inner == Type::Str || matches!(**inner, Type::Slice(_)) {
                // 16-byte slice descriptor: ptr (8 bytes) + len (8 bytes)
                AbiClass::Integer2 { size: 16 }
            } else {
                AbiClass::Integer1 { size: 8 }
            }
        }
        Type::Str | Type::Slice(_) => AbiClass::Integer2 { size: 16 },
        Type::Array { inner, len } => {
            let elem_size = inner.size_with_context(structs, enums);
            let total_size = elem_size * len;
            let align = inner.alignment_with_context(structs, enums).max(1);

            if total_size == 0 || total_size > 16 {
                AbiClass::Memory {
                    size: total_size,
                    align,
                }
            } else {
                // Check if inner type itself is memory class
                let inner_class = classify_type(inner, structs, enums);
                if inner_class.is_memory() {
                    AbiClass::Memory {
                        size: total_size,
                        align,
                    }
                } else if total_size <= 8 {
                    AbiClass::Integer1 { size: total_size }
                } else {
                    AbiClass::Integer2 { size: total_size }
                }
            }
        }
        Type::Struct(name) => {
            if name == "String" || name == "Vec" || name.starts_with("Vec__") {
                // String and Vec descriptors are 24 bytes (3 words: ptr, len, cap)
                return AbiClass::Memory { size: 24, align: 8 };
            }

            let s_def = match structs.get(name) {
                Some(s) => s,
                None => {
                    return AbiClass::Integer1 { size: 8 };
                }
            };

            let total_size = s_def.total_size;
            let align = s_def.alignment.max(8);

            // Rule 1: Size > 16 bytes -> MEMORY
            if total_size > 16 || total_size == 0 {
                return AbiClass::Memory {
                    size: total_size,
                    align,
                };
            }

            // Rule 2: Unaligned field offsets -> MEMORY
            for field in &s_def.fields {
                let f_align = field.ty.alignment_with_context(structs, enums).max(1);
                if field.offset % f_align != 0 {
                    return AbiClass::Memory {
                        size: total_size,
                        align,
                    };
                }
                // Rule 3: Any field classified as MEMORY -> MEMORY
                let f_class = classify_type(&field.ty, structs, enums);
                if f_class.is_memory() {
                    return AbiClass::Memory {
                        size: total_size,
                        align,
                    };
                }
            }

            // Otherwise, fits in 1 or 2 integer registers
            if total_size <= 8 {
                AbiClass::Integer1 { size: total_size }
            } else {
                AbiClass::Integer2 { size: total_size }
            }
        }
        Type::Enum(name) => {
            let e_def = match enums.get(name) {
                Some(e) => e,
                None => {
                    return AbiClass::Integer1 { size: 8 };
                }
            };

            let total_size = e_def.total_size;
            let align = e_def.alignment.max(8);

            if total_size > 16 || total_size == 0 {
                AbiClass::Memory {
                    size: total_size,
                    align,
                }
            } else if total_size <= 8 {
                AbiClass::Integer1 { size: total_size }
            } else {
                AbiClass::Integer2 { size: total_size }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_sema::StructField;
    use vanta_source::{FileId, Span};

    #[test]
    fn test_classify_primitives() {
        let structs = HashMap::new();
        let enums = HashMap::new();

        assert_eq!(classify_type(&Type::Unit, &structs, &enums), AbiClass::Void);
        assert_eq!(
            classify_type(&Type::I64, &structs, &enums),
            AbiClass::Integer1 { size: 8 }
        );
        assert_eq!(
            classify_type(&Type::Bool, &structs, &enums),
            AbiClass::Integer1 { size: 1 }
        );
    }

    #[test]
    fn test_classify_string_and_vec_as_memory() {
        let structs = HashMap::new();
        let enums = HashMap::new();

        let str_ty = Type::Struct("String".into());
        assert_eq!(
            classify_type(&str_ty, &structs, &enums),
            AbiClass::Memory { size: 24, align: 8 }
        );

        let vec_ty = Type::Struct("Vec__i64".into());
        assert_eq!(
            classify_type(&vec_ty, &structs, &enums),
            AbiClass::Memory { size: 24, align: 8 }
        );
    }

    #[test]
    fn test_classify_small_struct() {
        let mut structs = HashMap::new();
        let enums = HashMap::new();

        // 16-byte struct: Point { x: i64, y: i64 }
        structs.insert(
            "Point".into(),
            StructDef {
                name: "Point".into(),
                fields: vec![
                    StructField {
                        name: "x".into(),
                        ty: Type::I64,
                        offset: 0,
                        size: 8,
                        is_pub: true,
                    },
                    StructField {
                        name: "y".into(),
                        ty: Type::I64,
                        offset: 8,
                        size: 8,
                        is_pub: true,
                    },
                ],
                total_size: 16,
                alignment: 8,
                is_copy: true,
                defining_module: String::new(),
                span: Span::new(FileId(0), 0, 0),
            },
        );

        assert_eq!(
            classify_type(&Type::Struct("Point".into()), &structs, &enums),
            AbiClass::Integer2 { size: 16 }
        );
    }

    #[test]
    fn test_classify_unaligned_struct_as_memory() {
        let mut structs = HashMap::new();
        let enums = HashMap::new();

        // Unaligned struct: Bad { a: u8, b: i64 } with unaligned b
        structs.insert(
            "Bad".into(),
            StructDef {
                name: "Bad".into(),
                fields: vec![
                    StructField {
                        name: "a".into(),
                        ty: Type::U8,
                        offset: 0,
                        size: 1,
                        is_pub: true,
                    },
                    StructField {
                        name: "b".into(),
                        ty: Type::I64,
                        offset: 1, // UNALIGNED!
                        size: 8,
                        is_pub: true,
                    },
                ],
                total_size: 9,
                alignment: 8,
                is_copy: true,
                defining_module: String::new(),
                span: Span::new(FileId(0), 0, 0),
            },
        );

        assert_eq!(
            classify_type(&Type::Struct("Bad".into()), &structs, &enums),
            AbiClass::Memory { size: 9, align: 8 }
        );
    }

    #[test]
    fn test_classify_large_struct_as_memory() {
        let mut structs = HashMap::new();
        let enums = HashMap::new();

        // 24-byte struct: Point3D { x: i64, y: i64, z: i64 }
        structs.insert(
            "Point3D".into(),
            StructDef {
                name: "Point3D".into(),
                fields: vec![
                    StructField {
                        name: "x".into(),
                        ty: Type::I64,
                        offset: 0,
                        size: 8,
                        is_pub: true,
                    },
                    StructField {
                        name: "y".into(),
                        ty: Type::I64,
                        offset: 8,
                        size: 8,
                        is_pub: true,
                    },
                    StructField {
                        name: "z".into(),
                        ty: Type::I64,
                        offset: 16,
                        size: 8,
                        is_pub: true,
                    },
                ],
                total_size: 24,
                alignment: 8,
                is_copy: true,
                defining_module: String::new(),
                span: Span::new(FileId(0), 0, 0),
            },
        );

        assert_eq!(
            classify_type(&Type::Struct("Point3D".into()), &structs, &enums),
            AbiClass::Memory { size: 24, align: 8 }
        );
    }
}
