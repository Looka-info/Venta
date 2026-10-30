//! Apple Silicon (AArch64 / macOS) ABI classification and calling convention.
//!
//! Follows the ARM 64-bit Architecture Procedure Call Standard (AAPCS64) with
//! Apple Darwin specifics:
//! - Up to 8 integer/pointer arguments passed in X0..X7.
//! - Scalar returns up to 8 bytes in X0.
//! - 16-byte wide returns (slices, &str, trait objects) in X0 and X1.
//! - Large aggregates (>16 bytes) returned via hidden pointer in X8.
//! - Arguments beyond X7 passed on the stack (16-byte aligned stack pointer).

use crate::register::AArch64Reg;
use std::collections::HashMap;
use vanta_sema::{EnumDef, StructDef, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmAbiClass {
    Void,
    Integer1 { size: usize },
    Integer2 { size: usize },
    Memory { size: usize, align: usize },
}

impl ArmAbiClass {
    #[inline]
    pub fn is_memory(&self) -> bool {
        matches!(self, ArmAbiClass::Memory { .. })
    }

    #[inline]
    pub fn is_void(&self) -> bool {
        matches!(self, ArmAbiClass::Void)
    }

    pub fn size(&self) -> usize {
        match self {
            ArmAbiClass::Void => 0,
            ArmAbiClass::Integer1 { size } => *size,
            ArmAbiClass::Integer2 { size } => *size,
            ArmAbiClass::Memory { size, .. } => *size,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmArgLocation {
    Reg(AArch64Reg),
    Reg2(AArch64Reg, AArch64Reg),
    Stack { offset: i32, size: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmFnCallLayout {
    pub arg_locations: Vec<ArmArgLocation>,
    pub stack_arg_bytes: usize,
    pub has_sret: bool,
    pub ret_class: ArmAbiClass,
}

pub fn classify_arm_type(
    ty: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> ArmAbiClass {
    match ty {
        Type::Unit => ArmAbiClass::Void,
        Type::I8 | Type::U8 | Type::Bool => ArmAbiClass::Integer1 { size: 1 },
        Type::I16 | Type::U16 => ArmAbiClass::Integer1 { size: 2 },
        Type::I32 | Type::U32 | Type::F32 => ArmAbiClass::Integer1 { size: 4 },
        Type::I64 | Type::U64 | Type::F64 => ArmAbiClass::Integer1 { size: 8 },
        Type::Ref { inner, .. } => {
            if **inner == Type::Str || matches!(**inner, Type::Slice(_) | Type::TraitObject(_)) {
                ArmAbiClass::Integer2 { size: 16 }
            } else {
                ArmAbiClass::Integer1 { size: 8 }
            }
        }
        Type::Str | Type::Slice(_) | Type::TraitObject(_) => ArmAbiClass::Integer2 { size: 16 },
        Type::TypeParam(_) => ArmAbiClass::Integer1 { size: 8 },
        Type::Array { inner, len } => {
            let elem_size = inner.size_with_context(structs, enums);
            let total = elem_size * len;
            let align = inner.alignment_with_context(structs, enums).max(1);
            if total > 16 || total == 0 {
                ArmAbiClass::Memory { size: total, align }
            } else if total <= 8 {
                ArmAbiClass::Integer1 { size: total }
            } else {
                ArmAbiClass::Integer2 { size: total }
            }
        }
        Type::Struct(name) => {
            if name == "String" || name == "Vec" || name.starts_with("Vec__") {
                return ArmAbiClass::Memory { size: 24, align: 8 };
            }
            let s_def = match structs.get(name) {
                Some(s) => s,
                None => return ArmAbiClass::Integer1 { size: 8 },
            };
            let total = s_def.total_size;
            let align = s_def.alignment.max(8);
            if total > 16 || total == 0 {
                ArmAbiClass::Memory { size: total, align }
            } else if total <= 8 {
                ArmAbiClass::Integer1 { size: total }
            } else {
                ArmAbiClass::Integer2 { size: total }
            }
        }
        Type::Enum(name) => {
            let e_def = match enums.get(name) {
                Some(e) => e,
                None => return ArmAbiClass::Integer1 { size: 8 },
            };
            let total = e_def.total_size;
            let align = e_def.alignment.max(8);
            if total > 16 || total == 0 {
                ArmAbiClass::Memory { size: total, align }
            } else if total <= 8 {
                ArmAbiClass::Integer1 { size: total }
            } else {
                ArmAbiClass::Integer2 { size: total }
            }
        }
    }
}

pub fn compute_arm_fn_call_layout(
    param_types: &[Type],
    ret_type: &Type,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> ArmFnCallLayout {
    let ret_class = classify_arm_type(ret_type, structs, enums);
    let has_sret = ret_class.is_memory();

    let mut reg_idx = 0usize;
    let mut current_stack_offset = 0usize;
    let mut arg_locations = Vec::with_capacity(param_types.len());

    for param_ty in param_types {
        let class = classify_arm_type(param_ty, structs, enums);
        match class {
            ArmAbiClass::Void => {
                arg_locations.push(ArmArgLocation::Stack { offset: 0, size: 0 });
            }
            ArmAbiClass::Integer1 { size } => {
                if reg_idx < AArch64Reg::ARG_REGS.len() {
                    let reg = AArch64Reg::ARG_REGS[reg_idx];
                    reg_idx += 1;
                    arg_locations.push(ArmArgLocation::Reg(reg));
                } else {
                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let off = current_stack_offset as i32;
                    current_stack_offset += 8;
                    arg_locations.push(ArmArgLocation::Stack { offset: off, size });
                }
            }
            ArmAbiClass::Integer2 { size } => {
                if reg_idx + 1 < AArch64Reg::ARG_REGS.len() {
                    let r1 = AArch64Reg::ARG_REGS[reg_idx];
                    let r2 = AArch64Reg::ARG_REGS[reg_idx + 1];
                    reg_idx += 2;
                    arg_locations.push(ArmArgLocation::Reg2(r1, r2));
                } else {
                    current_stack_offset = (current_stack_offset + 7) & !7;
                    let off = current_stack_offset as i32;
                    current_stack_offset += 16;
                    arg_locations.push(ArmArgLocation::Stack { offset: off, size });
                }
            }
            ArmAbiClass::Memory { size, align } => {
                let align_req = align.max(8);
                current_stack_offset = (current_stack_offset + align_req - 1) & !(align_req - 1);
                let off = current_stack_offset as i32;
                current_stack_offset += (size + 7) & !7;
                arg_locations.push(ArmArgLocation::Stack { offset: off, size });
            }
        }
    }

    let stack_arg_bytes = (current_stack_offset + 15) & !15;

    ArmFnCallLayout {
        arg_locations,
        stack_arg_bytes,
        has_sret,
        ret_class,
    }
}
