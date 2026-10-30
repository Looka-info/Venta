//! Apple Silicon ARM64 Mach-O relocation application engine.

use crate::error::{LinkResult, LinkerError};
use crate::relocation::RelocationRecord;
use std::path::Path;

pub const ARM64_RELOC_UNSIGNED: u32 = 0;
pub const ARM64_RELOC_SUBTRACTOR: u32 = 1;
pub const ARM64_RELOC_BRANCH26: u32 = 2;
pub const ARM64_RELOC_PAGE21: u32 = 3;
pub const ARM64_RELOC_PAGEOFF12: u32 = 4;
pub const ARM64_RELOC_GOT_LOAD_PAGE21: u32 = 5;
pub const ARM64_RELOC_GOT_LOAD_PAGEOFF12: u32 = 6;

/// Applies a single Apple ARM64 Mach-O relocation into section bytes at `patch_offset`.
pub fn apply_macho_relocation(
    path: &Path,
    section_name: &str,
    reloc: &RelocationRecord,
    patch_offset: usize,
    target_bytes: &mut [u8],
    symbol_va: u64,
    place_va: u64,
) -> LinkResult<()> {
    match reloc.reloc_type {
        ARM64_RELOC_UNSIGNED => {
            if patch_offset + 8 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "ARM64_RELOC_UNSIGNED exceeds section length".into(),
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
            let val = symbol_va.wrapping_add(existing);
            target_bytes[patch_offset..patch_offset + 8].copy_from_slice(&val.to_le_bytes());
            Ok(())
        }
        ARM64_RELOC_BRANCH26 => {
            // B / BL: displacement = (symbol_va - place_va) >> 2
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "ARM64_RELOC_BRANCH26 exceeds section length".into(),
                });
            }
            let disp = (symbol_va as i64).wrapping_sub(place_va as i64);
            if disp % 4 != 0 {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "ARM64 branch displacement not 4-byte aligned".into(),
                });
            }
            let imm26 = disp >> 2;
            if imm26 < -(1 << 25) || imm26 >= (1 << 25) {
                return Err(LinkerError::RelocationOverflow {
                    path: path.to_path_buf(),
                    section: section_name.to_string(),
                    offset: patch_offset,
                    symbol: reloc.symbol_name.clone(),
                    value: disp,
                    bits: 26,
                });
            }

            let mut insn = u32::from_le_bytes([
                target_bytes[patch_offset],
                target_bytes[patch_offset + 1],
                target_bytes[patch_offset + 2],
                target_bytes[patch_offset + 3],
            ]);

            // Clear bottom 26 bits and inject imm26
            insn = (insn & 0xFC000000) | ((imm26 as u32) & 0x03FFFFFF);
            target_bytes[patch_offset..patch_offset + 4].copy_from_slice(&insn.to_le_bytes());
            Ok(())
        }
        ARM64_RELOC_PAGE21 | ARM64_RELOC_GOT_LOAD_PAGE21 => {
            // ADRP: page displacement = (symbol_page - place_page) >> 12
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "ARM64_RELOC_PAGE21 exceeds section length".into(),
                });
            }
            let sym_page = symbol_va & !0xFFF;
            let place_page = place_va & !0xFFF;
            let page_disp = (sym_page as i64).wrapping_sub(place_page as i64) >> 12;

            if page_disp < -(1 << 20) || page_disp >= (1 << 20) {
                return Err(LinkerError::RelocationOverflow {
                    path: path.to_path_buf(),
                    section: section_name.to_string(),
                    offset: patch_offset,
                    symbol: reloc.symbol_name.clone(),
                    value: page_disp,
                    bits: 21,
                });
            }

            let imm21 = (page_disp as u32) & 0x1FFFFF;
            let immlo = (imm21 & 0x3) << 29;
            let immhi = ((imm21 >> 2) & 0x7FFFF) << 5;

            let mut insn = u32::from_le_bytes([
                target_bytes[patch_offset],
                target_bytes[patch_offset + 1],
                target_bytes[patch_offset + 2],
                target_bytes[patch_offset + 3],
            ]);

            // Clear immlo (bits 30..29) and immhi (bits 23..5)
            insn &= !((0x3 << 29) | (0x7FFFF << 5));
            insn |= immlo | immhi;

            target_bytes[patch_offset..patch_offset + 4].copy_from_slice(&insn.to_le_bytes());
            Ok(())
        }
        ARM64_RELOC_PAGEOFF12 | ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
            // ADD / LDR / STR immediate: offset within page = symbol_va & 0xFFF
            if patch_offset + 4 > target_bytes.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.to_path_buf(),
                    offset: patch_offset,
                    message: "ARM64_RELOC_PAGEOFF12 exceeds section length".into(),
                });
            }
            let page_offset = (symbol_va & 0xFFF) as u32;

            let mut insn = u32::from_le_bytes([
                target_bytes[patch_offset],
                target_bytes[patch_offset + 1],
                target_bytes[patch_offset + 2],
                target_bytes[patch_offset + 3],
            ]);

            // For ADD: bits 21..10 are imm12
            // For 64-bit LDR/STR (unsigned imm): scaled by 8, bits 21..10 are imm12 >> 3
            // Check if it's LDR/STR (opc 01 or size 11 in bits 31..30)
            if (insn >> 30) == 3 && ((insn >> 22) & 0x3) == 1 {
                // 64-bit load/store unsigned offset
                let scaled_imm = (page_offset >> 3) & 0xFFF;
                insn = (insn & !(0xFFF << 10)) | (scaled_imm << 10);
            } else {
                // ADD immediate
                insn = (insn & !(0xFFF << 10)) | ((page_offset & 0xFFF) << 10);
            }

            target_bytes[patch_offset..patch_offset + 4].copy_from_slice(&insn.to_le_bytes());
            Ok(())
        }
        other => Err(LinkerError::UnsupportedRelocation {
            path: path.to_path_buf(),
            section: section_name.to_string(),
            reloc_type: other,
            reloc_name: format!("MACHO_ARM64_{}", other),
        }),
    }
}
