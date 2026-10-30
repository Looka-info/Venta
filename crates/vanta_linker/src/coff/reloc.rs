//! AMD64 PE/COFF relocation application engine.

use crate::error::{LinkResult, LinkerError};
use crate::relocation::RelocationRecord;
use std::path::Path;

pub const IMAGE_REL_AMD64_ABSOLUTE: u32 = 0;
pub const IMAGE_REL_AMD64_ADDR64: u32 = 1;
pub const IMAGE_REL_AMD64_ADDR32: u32 = 2;
pub const IMAGE_REL_AMD64_ADDR32NB: u32 = 3;
pub const IMAGE_REL_AMD64_REL32: u32 = 4;
pub const IMAGE_REL_AMD64_REL32_1: u32 = 5;
pub const IMAGE_REL_AMD64_REL32_2: u32 = 6;
pub const IMAGE_REL_AMD64_REL32_3: u32 = 7;
pub const IMAGE_REL_AMD64_REL32_4: u32 = 8;
pub const IMAGE_REL_AMD64_REL32_5: u32 = 9;

/// Applies a single AMD64 COFF relocation into section bytes at `patch_offset`.
pub fn apply_coff_relocation(
    path: &Path,
    section_name: &str,
    reloc: &RelocationRecord,
    patch_offset: usize,
    target_bytes: &mut [u8],
    symbol_va: u64,
    place_va: u64,
    image_base: u64,
) -> LinkResult<()> {
    match reloc.reloc_type {
        IMAGE_REL_AMD64_ABSOLUTE => Ok(()),
        IMAGE_REL_AMD64_ADDR64 => {
            if patch_offset + 8 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "Relocation offset + 8 exceeds section length".into(),
                });
            }
            let existing = u64::from_le_bytes([
                target_bytes[patch_offset],
                target_bytes[patch_offset + 1],
                target_bytes[patch_offset + 2],
                target_bytes[patch_offset + 3],
                target_bytes[patch_offset + 4],
                target_bytes[patch_offset + 5],
                target_bytes[patch_offset + 6],
                target_bytes[patch_offset + 7],
            ]);
            let final_val = symbol_va.wrapping_add(existing);
            target_bytes[patch_offset..patch_offset + 8].copy_from_slice(&final_val.to_le_bytes());
            Ok(())
        }
        IMAGE_REL_AMD64_ADDR32 => {
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "Relocation offset + 4 exceeds section length".into(),
                });
            }
            let existing = u32::from_le_bytes([
                target_bytes[patch_offset],
                target_bytes[patch_offset + 1],
                target_bytes[patch_offset + 2],
                target_bytes[patch_offset + 3],
            ]) as u64;
            let final_val = symbol_va.wrapping_add(existing);
            if final_val > u32::MAX as u64 {
                return Err(LinkerError::RelocationOverflow {
                    path: path.to_path_buf(),
                    section: section_name.to_string(),
                    offset: patch_offset,
                    symbol: reloc.symbol_name.clone(),
                    value: final_val as i64,
                    bits: 32,
                });
            }
            target_bytes[patch_offset..patch_offset + 4]
                .copy_from_slice(&(final_val as u32).to_le_bytes());
            Ok(())
        }
        IMAGE_REL_AMD64_ADDR32NB => {
            // Relative Virtual Address (RVA = VA - ImageBase)
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "Relocation offset + 4 exceeds section length".into(),
                });
            }
            let existing = u32::from_le_bytes([
                target_bytes[patch_offset],
                target_bytes[patch_offset + 1],
                target_bytes[patch_offset + 2],
                target_bytes[patch_offset + 3],
            ]) as u64;
            let full_va = symbol_va.wrapping_add(existing);
            let rva = full_va.saturating_sub(image_base);
            if rva > u32::MAX as u64 {
                return Err(LinkerError::RelocationOverflow {
                    path: path.to_path_buf(),
                    section: section_name.to_string(),
                    offset: patch_offset,
                    symbol: reloc.symbol_name.clone(),
                    value: rva as i64,
                    bits: 32,
                });
            }
            target_bytes[patch_offset..patch_offset + 4]
                .copy_from_slice(&(rva as u32).to_le_bytes());
            Ok(())
        }
        IMAGE_REL_AMD64_REL32
        | IMAGE_REL_AMD64_REL32_1
        | IMAGE_REL_AMD64_REL32_2
        | IMAGE_REL_AMD64_REL32_3
        | IMAGE_REL_AMD64_REL32_4
        | IMAGE_REL_AMD64_REL32_5 => {
            let k = match reloc.reloc_type {
                IMAGE_REL_AMD64_REL32_1 => 1i128,
                IMAGE_REL_AMD64_REL32_2 => 2i128,
                IMAGE_REL_AMD64_REL32_3 => 3i128,
                IMAGE_REL_AMD64_REL32_4 => 4i128,
                IMAGE_REL_AMD64_REL32_5 => 5i128,
                _ => 0i128,
            };
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "Relocation offset + 4 exceeds section length".into(),
                });
            }
            let existing = i32::from_le_bytes([
                target_bytes[patch_offset],
                target_bytes[patch_offset + 1],
                target_bytes[patch_offset + 2],
                target_bytes[patch_offset + 3],
            ]) as i128;
            let disp = (symbol_va as i128) + existing - (place_va as i128) - 4 - k;
            if disp < i32::MIN as i128 || disp > i32::MAX as i128 {
                return Err(LinkerError::RelocationOverflow {
                    path: path.to_path_buf(),
                    section: section_name.to_string(),
                    offset: patch_offset,
                    symbol: reloc.symbol_name.clone(),
                    value: disp as i64,
                    bits: 32,
                });
            }
            target_bytes[patch_offset..patch_offset + 4]
                .copy_from_slice(&(disp as i32).to_le_bytes());
            Ok(())
        }
        other => Err(LinkerError::UnsupportedRelocation {
            path: path.to_path_buf(),
            section: section_name.to_string(),
            reloc_type: other,
            reloc_name: format!("COFF_AMD64_{}", other),
        }),
    }
}
