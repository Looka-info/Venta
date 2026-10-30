//! x86-64 ELF relocation application engine.

use crate::error::{LinkResult, LinkerError};
use crate::relocation::RelocationRecord;
use std::path::Path;

pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_PC32: u32 = 2;
pub const R_X86_64_PLT32: u32 = 4;
pub const R_X86_64_32: u32 = 10;
pub const R_X86_64_32S: u32 = 11;
pub const R_X86_64_PC64: u32 = 24;

/// Applies a single x86-64 relocation into section bytes at `patch_offset`.
pub fn apply_relocation(
    path: &Path,
    section_name: &str,
    reloc: &RelocationRecord,
    patch_offset: usize,
    target_bytes: &mut [u8],
    symbol_va: u64,
    place_va: u64,
) -> LinkResult<()> {
    let s = symbol_va as i128;
    let a = reloc.addend as i128;
    let p = place_va as i128;

    match reloc.reloc_type {
        R_X86_64_NONE => Ok(()),
        R_X86_64_64 => {
            // S + A (64-bit absolute)
            let val = (s + a) as u64;
            if patch_offset + 8 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: format!(
                        "Relocation offset {} + 8 exceeds section length {}",
                        patch_offset,
                        target_bytes.len()
                    ),
                });
            }
            target_bytes[patch_offset..patch_offset + 8].copy_from_slice(&val.to_le_bytes());
            Ok(())
        }
        R_X86_64_PC32 | R_X86_64_PLT32 => {
            // S + A - P (32-bit signed PC-relative)
            let val = s + a - p;
            if val < i32::MIN as i128 || val > i32::MAX as i128 {
                return Err(LinkerError::RelocationOverflow {
                    path: path.to_path_buf(),
                    section: section_name.to_string(),
                    offset: patch_offset,
                    symbol: reloc.symbol_name.clone(),
                    value: val as i64,
                    bits: 32,
                });
            }
            let val32 = val as i32;
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: format!(
                        "Relocation offset {} + 4 exceeds section length {}",
                        patch_offset,
                        target_bytes.len()
                    ),
                });
            }
            target_bytes[patch_offset..patch_offset + 4].copy_from_slice(&val32.to_le_bytes());
            Ok(())
        }
        R_X86_64_32 => {
            // S + A (32-bit unsigned)
            let val = s + a;
            if val < 0 || val > u32::MAX as i128 {
                return Err(LinkerError::RelocationOverflow {
                    path: path.to_path_buf(),
                    section: section_name.to_string(),
                    offset: patch_offset,
                    symbol: reloc.symbol_name.clone(),
                    value: val as i64,
                    bits: 32,
                });
            }
            let val32 = val as u32;
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "Relocation offset out of bounds".into(),
                });
            }
            target_bytes[patch_offset..patch_offset + 4].copy_from_slice(&val32.to_le_bytes());
            Ok(())
        }
        R_X86_64_32S => {
            // S + A (32-bit signed)
            let val = s + a;
            if val < i32::MIN as i128 || val > i32::MAX as i128 {
                return Err(LinkerError::RelocationOverflow {
                    path: path.to_path_buf(),
                    section: section_name.to_string(),
                    offset: patch_offset,
                    symbol: reloc.symbol_name.clone(),
                    value: val as i64,
                    bits: 32,
                });
            }
            let val32 = val as i32;
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "Relocation offset out of bounds".into(),
                });
            }
            target_bytes[patch_offset..patch_offset + 4].copy_from_slice(&val32.to_le_bytes());
            Ok(())
        }
        R_X86_64_PC64 => {
            let val = (s + a - p) as u64;
            if patch_offset + 8 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "Relocation offset out of bounds".into(),
                });
            }
            target_bytes[patch_offset..patch_offset + 8].copy_from_slice(&val.to_le_bytes());
            Ok(())
        }
        other => Err(LinkerError::UnsupportedRelocation {
            path: path.to_path_buf(),
            section: section_name.to_string(),
            reloc_type: other,
            reloc_name: format!("R_X86_64_{}", other),
        }),
    }
}
