//! System V AMD64 ABI classification and calling convention specification.
//!
//! Implements Section 3.2.3 of the System V AMD64 ABI for parameter passing
//! and aggregate return classification.

use std::collections::HashMap;
use vanta_sema::{EnumDef, StructDef, Type};
use crate::register::{Reg, SimdReg};

/// The ABI return and parameter classification of a Vanta type under System V AMD64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiClass {
    /// Zero-sized or unit return (no registers, no memory storage).
    Void,
    /// Up to 8 bytes, returned/passed in a single integer register (%rax for return).
    Integer1 { size: usize },
    /// 9 to 16 bytes, returned/passed in two integer registers (%rax low, %rdx high for return).
    Integer2 { size: usize },
    /// 256-bit vector (32 bytes, 1 SSE + 3 SSEUP eightbytes), returned in %ymm0, passed in %ymm0..%ymm7.
    Vector256,
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

    /// Whether this type is a 256-bit vector.
    #[inline]
    pub fn is_vector(&self) -> bool {
        matches!(self, AbiClass::Vector256)
    }

    /// Total byte size of the type.
    pub fn size(&self) -> usize {
        match self {
            AbiClass::Void => 0,
            AbiClass::Integer1 { size } => *size,
            AbiClass::Integer2 { size } => *size,
            AbiClass::Vector256 => 32,
            AbiClass::Memory { size, .. } => *size,
        }
    }

    /// Required alignment in bytes.
    pub fn align(&self) -> usize {
        match self {
            AbiClass::Void => 1,
            AbiClass::Integer1 { size } => (*size).min(8).max(1),
            AbiClass::Integer2 { .. } => 8,
            AbiClass::Vector256 => 32,
            AbiClass::Memory { align, .. } => *align,
        }
    }
}

/// The physical location of a function argument under System V AMD64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgLocation {
    /// Passed in a single 64-bit general-purpose integer register.
    Reg(Reg),
    /// Passed in two 64-bit integer registers (low 8 bytes, high 8 bytes).
    Reg2(Reg, Reg),
    /// Passed in a 256-bit YMM vector register (%ymm0..%ymm7).
    VectorReg(SimdReg),
    /// Passed on the stack at [rbp + 16 + offset] in callee (or [rsp + offset] in caller).
    Stack {
        offset: i32,
        size: usize,
        align: usize,
    },
}

/// General-purpose integer registers used for argument passing in order (%rdi, %rsi, %rdx, %rcx, %r8, %r9).
pub const INT_ARG_REGS: [Reg; 6] = [
    Reg::Rdi,
    Reg::Rsi,
    Reg::Rdx,
    Reg::Rcx,
    Reg::R8,
    Reg::R9,
];

/// Physical YMM vector registers used for vector argument passing in order (%ymm0..%ymm7).
pub const VECTOR_ARG_REGS: [SimdReg; 8] = [
    SimdReg(0),
    SimdReg(1),
    SimdReg(2),
    SimdReg(3),
    SimdReg(4),
    SimdReg(5),
    SimdReg(6),
    SimdReg(7),
];

/// Physical layout of a function call boundary under System V AMD64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnCallLayout {
    /// Assigned location for each parameter in order.
    pub arg_locations: Vec<ArgLocation>,
    /// Total stack bytes allocated for stack-passed arguments (16-byte aligned).
    pub stack_arg_bytes: usize,
    /// Whether the return value requires a hidden first pointer argument in %rdi.
    pub has_sret: bool,
    /// ABI classification of the return value.
    pub ret_class: AbiClass,
}

/// Computes the exact System V AMD64 argument and return layout for a function signature.
///
/// Follows Section 3.2.3 of the System V AMD64 ABI:
/// - 256-bit vector arguments are classified as Vector256 (1 SSE + 3 SSEUP eightbytes)
///   and assigned to %ymm0..%ymm7.
/// - Integer arguments are assigned to %rdi, %rsi, %rdx, %rcx, %r8, %r9.
/// - Integer and vector register banks are tracked independently: an integer argument does
///   not consume a vector register, and a vector argument does not consume a GPR.
/// - When vector registers are exhausted (>8 vectors), remaining vectors are passed on the
///   stack aligned to 32 bytes.
/// - If a multi-register argument cannot fit completely in remaining registers (e.g. Integer2
///   when only 1 GPR remains), it rolls back and is passed entirely on the stack.
/// - Hidden sret return pointer occupies %rdi (int_reg_idx 0), shifting integer arguments.
pub fn compute_fn_call_layout(
    param_types: &[Type],
    ret_type: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> FnCallLayout {
    let ret_class = classify_type(ret_type, structs, enums);
    let has_sret = ret_class.is_memory();

    let mut int_reg_idx = if has_sret { 1 } else { 0 };
    let mut sse_reg_idx = 0;
    let mut current_stack_offset = 0usize;

    let mut arg_locations = Vec::with_capacity(param_types.len());

    for param_ty in param_types {
        let class = classify_type(param_ty, structs, enums);
        match class {
            AbiClass::Void => {
                arg_locations.push(ArgLocation::Stack {
                    offset: 0,
                    size: 0,
                    align: 1,
                });
            }
            AbiClass::Vector256 => {
                if sse_reg_idx < VECTOR_ARG_REGS.len() {
                    let reg = VECTOR_ARG_REGS[sse_reg_idx];
                    sse_reg_idx += 1;
                    arg_locations.push(ArgLocation::VectorReg(reg));
                } else {
                    // Argument register exhaustion: passed on stack.
                    // Under System V ABI, 256-bit vector stack arguments are 32-byte aligned.
                    current_stack_offset = (current_stack_offset + 31) & !31;
                    let off = current_stack_offset as i32;
                    current_stack_offset += 32;
                    arg_locations.push(ArgLocation::Stack {
                        offset: off,
                        size: 32,
                        align: 32,
                    });
                }
            }
            AbiClass::Integer1 { size } => {
                if int_reg_idx < INT_ARG_REGS.len() {
                    let reg = INT_ARG_REGS[int_reg_idx];
                    int_reg_idx += 1;
                    arg_locations.push(ArgLocation::Reg(reg));
                } else {
                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let off = current_stack_offset as i32;
                    current_stack_offset += 8;
                    arg_locations.push(ArgLocation::Stack {
                        offset: off,
                        size,
                        align: 8,
                    });
                }
            }
            AbiClass::Integer2 { size } => {
                if int_reg_idx + 1 < INT_ARG_REGS.len() {
                    let r1 = INT_ARG_REGS[int_reg_idx];
                    let r2 = INT_ARG_REGS[int_reg_idx + 1];
                    int_reg_idx += 2;
                    arg_locations.push(ArgLocation::Reg2(r1, r2));
                } else {
                    // System V AMD64 Rollback Rule:
                    // If an argument requires multiple registers and insufficient registers remain,
                    // it cannot be split across registers and stack: the ENTIRE argument is passed on the stack.
                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let off = current_stack_offset as i32;
                    current_stack_offset += 16;
                    arg_locations.push(ArgLocation::Stack {
                        offset: off,
                        size,
                        align: 8,
                    });
                }
            }
            AbiClass::Memory { size, align } => {
                let align_req = align.max(8);
                current_stack_offset = (current_stack_offset + align_req - 1) & !(align_req - 1);
                let off = current_stack_offset as i32;
                current_stack_offset += (size + 7) & !7;
                arg_locations.push(ArgLocation::Stack {
                    offset: off,
                    size,
                    align: align_req,
                });
            }
        }
    }

    // System V AMD64 requires the stack pointer to be 16-byte aligned before the call instruction
    let stack_arg_bytes = (current_stack_offset + 15) & !15;

    FnCallLayout {
        arg_locations,
        stack_arg_bytes,
        has_sret,
        ret_class,
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

            // 256-bit SIMD Vector classification (Milestone 26.2):
            // Under System V AMD64 ABI, an object of 256 bits (32 bytes) with SSE/SSEUP eightbytes
            // is classified as Vector256 (returned in %ymm0, passed in %ymm0..%ymm7).
            if name == "v256"
                || name == "Simd256"
                || name == "v256i"
                || name == "v256d"
                || name.ends_with("__v256")
                || name.ends_with("__Simd256")
            {
                return AbiClass::Vector256;
            }

            let s_def = match structs.get(name) {
                Some(s) => s,
                None => {
                    return AbiClass::Integer1 { size: 8 };
                }
            };

            let total_size = s_def.total_size;
            let align = s_def.alignment.max(8);

            // If a struct is exactly 32 bytes and 16/32-byte aligned, classify as Vector256
            if total_size == 32 && align >= 16 {
                return AbiClass::Vector256;
            }

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

    #[test]
    fn test_classify_vector256() {
        let structs = HashMap::new();
        let enums = HashMap::new();

        assert_eq!(
            classify_type(&Type::Struct("v256".into()), &structs, &enums),
            AbiClass::Vector256
        );
        assert_eq!(
            classify_type(&Type::Struct("Simd256".into()), &structs, &enums),
            AbiClass::Vector256
        );
        assert_eq!(AbiClass::Vector256.size(), 32);
        assert_eq!(AbiClass::Vector256.align(), 32);
        assert!(AbiClass::Vector256.is_vector());
        assert!(!AbiClass::Vector256.is_memory());
    }

    #[test]
    fn test_fn_call_layout_single_vector() {
        let structs = HashMap::new();
        let enums = HashMap::new();

        let layout = compute_fn_call_layout(
            &[Type::Struct("v256".into())],
            &Type::Struct("v256".into()),
            &structs,
            &enums,
        );

        assert_eq!(layout.arg_locations.len(), 1);
        assert_eq!(
            layout.arg_locations[0],
            ArgLocation::VectorReg(SimdReg(0))
        );
        assert_eq!(layout.stack_arg_bytes, 0);
        assert_eq!(layout.ret_class, AbiClass::Vector256);
        assert!(!layout.has_sret);
    }

    #[test]
    fn test_fn_call_layout_multiple_vectors_and_exhaustion() {
        let structs = HashMap::new();
        let enums = HashMap::new();

        // 9 vector arguments: 8 in registers (ymm0..ymm7), 9th on stack
        let params = vec![Type::Struct("v256".into()); 9];
        let layout = compute_fn_call_layout(&params, &Type::Unit, &structs, &enums);

        assert_eq!(layout.arg_locations.len(), 9);
        for i in 0..8 {
            assert_eq!(
                layout.arg_locations[i],
                ArgLocation::VectorReg(SimdReg(i as u8))
            );
        }
        assert_eq!(
            layout.arg_locations[8],
            ArgLocation::Stack {
                offset: 0,
                size: 32,
                align: 32,
            }
        );
        assert_eq!(layout.stack_arg_bytes, 32);
    }

    #[test]
    fn test_fn_call_layout_mixed_integer_and_vector() {
        let structs = HashMap::new();
        let enums = HashMap::new();

        // fn mixed(x: i64, v: v256, y: i64) -> v256
        let params = vec![
            Type::I64,
            Type::Struct("v256".into()),
            Type::I64,
        ];
        let layout = compute_fn_call_layout(
            &params,
            &Type::Struct("v256".into()),
            &structs,
            &enums,
        );

        assert_eq!(layout.arg_locations.len(), 3);
        assert_eq!(layout.arg_locations[0], ArgLocation::Reg(Reg::Rdi));
        assert_eq!(layout.arg_locations[1], ArgLocation::VectorReg(SimdReg(0)));
        assert_eq!(layout.arg_locations[2], ArgLocation::Reg(Reg::Rsi));
        assert_eq!(layout.stack_arg_bytes, 0);
        assert_eq!(layout.ret_class, AbiClass::Vector256);
    }

    #[test]
    fn test_fn_call_layout_rollback_rule() {
        let structs = HashMap::new();
        let enums = HashMap::new();

        // 5 i64 arguments occupy rdi, rsi, rdx, rcx, r8 (only 1 integer register r9 remains)
        // 6th argument is Type::Str (16-byte Integer2 fat pointer)
        // System V rollback rule: cannot split, must put entire Integer2 on stack!
        let params = vec![
            Type::I64,
            Type::I64,
            Type::I64,
            Type::I64,
            Type::I64,
            Type::Str,
        ];
        let layout = compute_fn_call_layout(&params, &Type::Unit, &structs, &enums);

        assert_eq!(layout.arg_locations.len(), 6);
        assert_eq!(layout.arg_locations[0], ArgLocation::Reg(Reg::Rdi));
        assert_eq!(layout.arg_locations[1], ArgLocation::Reg(Reg::Rsi));
        assert_eq!(layout.arg_locations[2], ArgLocation::Reg(Reg::Rdx));
        assert_eq!(layout.arg_locations[3], ArgLocation::Reg(Reg::Rcx));
        assert_eq!(layout.arg_locations[4], ArgLocation::Reg(Reg::R8));
        assert_eq!(
            layout.arg_locations[5],
            ArgLocation::Stack {
                offset: 0,
                size: 16,
                align: 8,
            }
        );
        assert_eq!(layout.stack_arg_bytes, 16);
    }
}
