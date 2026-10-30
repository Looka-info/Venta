//! Pure-Rust AMD64 PE/COFF Relocatable Object (.obj) Reader.

use crate::error::{LinkResult, LinkerError};
use crate::input::InputFormat;
use crate::object::ObjectFile;
use crate::relocation::RelocationRecord;
use crate::section::{InputSection, SectionFlags, SectionKind};
use crate::symbol::{Symbol, SymbolKind, SymbolVisibility};
use std::path::PathBuf;

pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;

// Section characteristics
pub const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
pub const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
pub const IMAGE_SCN_CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;
pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
pub const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;

// Storage classes
pub const IMAGE_SYM_CLASS_EXTERNAL: u8 = 2;
pub const IMAGE_SYM_CLASS_STATIC: u8 = 3;

pub fn parse_coff_object(
    path: &std::path::Path,
    data: &[u8],
    object_id: usize,
) -> LinkResult<ObjectFile> {
    read_coff_object(
        object_id,
        path.display().to_string(),
        path.to_path_buf(),
        data,
    )
}

/// Parses a 64-bit AMD64 COFF relocatable object (.obj) into an `ObjectFile`.
pub fn read_coff_object(
    object_id: usize,
    name: String,
    path: PathBuf,
    data: &[u8],
) -> LinkResult<ObjectFile> {
    if data.len() < 20 {
        return Err(LinkerError::MalformedInput {
            path,
            offset: 0,
            message: "File too small for COFF header (expected at least 20 bytes)".into(),
        });
    }

    // 1. Read COFF File Header
    let machine = u16::from_le_bytes([data[0], data[1]]);
    if machine != IMAGE_FILE_MACHINE_AMD64 {
        return Err(LinkerError::IncompatibleArchitecture {
            path,
            expected: "IMAGE_FILE_MACHINE_AMD64 (0x8664)".into(),
            found: format!("{:#06x}", machine),
        });
    }

    let num_sections = u16::from_le_bytes([data[2], data[3]]) as usize;
    let symtab_offset = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    let num_symbols = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let optional_header_size = u16::from_le_bytes([data[16], data[17]]) as usize;

    let section_headers_offset = 20 + optional_header_size;
    let section_header_size = 40;
    let total_sec_headers_bytes =
        num_sections
            .checked_mul(section_header_size)
            .ok_or_else(|| LinkerError::MalformedInput {
                path: path.clone(),
                offset: 20,
                message: "Section count overflow".into(),
            })?;

    if section_headers_offset + total_sec_headers_bytes > data.len() {
        return Err(LinkerError::MalformedInput {
            path: path.clone(),
            offset: section_headers_offset,
            message: "COFF section headers extend beyond file bounds".into(),
        });
    }

    // 2. Parse String Table (immediately follows symbol table)
    let string_table_offset = symtab_offset + num_symbols * 18;
    let string_table = if string_table_offset + 4 <= data.len() {
        let strtab_len = u32::from_le_bytes([
            data[string_table_offset],
            data[string_table_offset + 1],
            data[string_table_offset + 2],
            data[string_table_offset + 3],
        ]) as usize;
        let strtab_end = (string_table_offset + strtab_len).min(data.len());
        &data[string_table_offset..strtab_end]
    } else {
        &[]
    };

    let read_string = |offset: usize| -> String {
        if offset < string_table.len() {
            let slice = &string_table[offset..];
            let len = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
            String::from_utf8_lossy(&slice[..len]).to_string()
        } else {
            String::new()
        }
    };

    // 3. Parse Sections
    let mut sections = Vec::with_capacity(num_sections);
    let mut raw_relocs_by_section: Vec<(usize, u32, u16)> = Vec::new(); // (sec_idx, reloc_ptr, num_relocs)

    for i in 0..num_sections {
        let off = section_headers_offset + i * section_header_size;
        let sec_hdr = &data[off..off + 40];

        // Section Name (8 bytes)
        let name_raw = &sec_hdr[0..8];
        let name = if name_raw.starts_with(b"/") {
            // String table reference "/offset"
            let str_off_str = std::str::from_utf8(&name_raw[1..])
                .unwrap_or("")
                .trim_matches('\0')
                .trim();
            if let Ok(str_off) = str_off_str.parse::<usize>() {
                read_string(str_off)
            } else {
                String::from_utf8_lossy(name_raw)
                    .trim_matches('\0')
                    .to_string()
            }
        } else {
            String::from_utf8_lossy(name_raw)
                .trim_matches('\0')
                .to_string()
        };

        let raw_data_size =
            u32::from_le_bytes([sec_hdr[16], sec_hdr[17], sec_hdr[18], sec_hdr[19]]) as usize;
        let raw_data_ptr =
            u32::from_le_bytes([sec_hdr[20], sec_hdr[21], sec_hdr[22], sec_hdr[23]]) as usize;
        let reloc_ptr = u32::from_le_bytes([sec_hdr[24], sec_hdr[25], sec_hdr[26], sec_hdr[27]]);
        let num_relocs = u16::from_le_bytes([sec_hdr[28], sec_hdr[29]]);
        let characteristics =
            u32::from_le_bytes([sec_hdr[36], sec_hdr[37], sec_hdr[38], sec_hdr[39]]);

        let kind = if name == ".text" || (characteristics & IMAGE_SCN_CNT_CODE != 0) {
            SectionKind::Code
        } else if name == ".rdata"
            || (characteristics & IMAGE_SCN_MEM_READ != 0
                && characteristics & IMAGE_SCN_MEM_WRITE == 0
                && characteristics & IMAGE_SCN_CNT_CODE == 0)
        {
            SectionKind::ReadOnlyData
        } else if name == ".data"
            || (characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA != 0
                && characteristics & IMAGE_SCN_MEM_WRITE != 0)
        {
            SectionKind::Data
        } else if name == ".bss" || (characteristics & IMAGE_SCN_CNT_UNINITIALIZED_DATA != 0) {
            SectionKind::Bss
        } else if name == ".pdata" {
            SectionKind::UnwindPdata
        } else if name == ".xdata" {
            SectionKind::UnwindXdata
        } else {
            SectionKind::Other
        };

        let flags = SectionFlags {
            read: characteristics & IMAGE_SCN_MEM_READ != 0,
            write: characteristics & IMAGE_SCN_MEM_WRITE != 0,
            execute: characteristics & IMAGE_SCN_MEM_EXECUTE != 0,
            alloc: kind.is_allocatable(),
        };

        let sec_data = if kind.is_zero_init() {
            Vec::new()
        } else if raw_data_ptr > 0 && raw_data_size > 0 {
            if raw_data_ptr + raw_data_size > data.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.clone(),
                    offset: raw_data_ptr,
                    message: format!("Section '{}' data extends beyond file bounds", name),
                });
            }
            data[raw_data_ptr..raw_data_ptr + raw_data_size].to_vec()
        } else {
            Vec::new()
        };

        sections.push(InputSection {
            object_id,
            source_index: i + 1, // COFF 1-based index
            name,
            kind,
            flags,
            align: 16,
            data: sec_data,
            size: raw_data_size,
        });

        raw_relocs_by_section.push((i + 1, reloc_ptr, num_relocs));
    }

    // 4. Parse Symbol Table
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut i = 0;
    while i < num_symbols {
        let sym_off = symtab_offset + i * 18;
        if sym_off + 18 > data.len() {
            break;
        }

        let sym_bytes = &data[sym_off..sym_off + 18];
        let name =
            if sym_bytes[0] == 0 && sym_bytes[1] == 0 && sym_bytes[2] == 0 && sym_bytes[3] == 0 {
                // String table offset in bytes 4..8
                let str_off =
                    u32::from_le_bytes([sym_bytes[4], sym_bytes[5], sym_bytes[6], sym_bytes[7]])
                        as usize;
                read_string(str_off)
            } else {
                String::from_utf8_lossy(&sym_bytes[0..8])
                    .trim_matches('\0')
                    .to_string()
            };

        let value =
            u32::from_le_bytes([sym_bytes[8], sym_bytes[9], sym_bytes[10], sym_bytes[11]]) as u64;
        let sec_num = i16::from_le_bytes([sym_bytes[12], sym_bytes[13]]);
        let sym_type = u16::from_le_bytes([sym_bytes[14], sym_bytes[15]]);
        let storage_class = sym_bytes[16];
        let num_aux = sym_bytes[17] as usize;

        let visibility = if storage_class == IMAGE_SYM_CLASS_EXTERNAL {
            SymbolVisibility::Global
        } else {
            SymbolVisibility::Local
        };

        let kind = if sym_type == 0x20 {
            SymbolKind::Function
        } else if sec_num > 0 {
            SymbolKind::Data
        } else {
            SymbolKind::Other
        };

        let sec_idx = if sec_num > 0 {
            Some(sec_num as usize)
        } else {
            None
        };

        symbols.push(Symbol {
            name,
            visibility,
            kind,
            section_index: sec_idx,
            value,
            size: 0,
            object_id,
            final_virtual_address: None,
        });

        // Skip auxiliary symbol records (each is 18 bytes)
        i += 1 + num_aux;
    }

    // 5. Parse Relocations
    let mut relocations: Vec<RelocationRecord> = Vec::new();
    for (sec_idx, reloc_ptr, num_relocs) in raw_relocs_by_section {
        if reloc_ptr == 0 || num_relocs == 0 {
            continue;
        }
        let start = reloc_ptr as usize;
        let count = num_relocs as usize;
        if start + count * 10 > data.len() {
            return Err(LinkerError::MalformedInput {
                path: path.clone(),
                offset: start,
                message: "COFF relocations extend beyond file bounds".into(),
            });
        }

        for r in 0..count {
            let r_off = start + r * 10;
            let virt_addr = u32::from_le_bytes([
                data[r_off],
                data[r_off + 1],
                data[r_off + 2],
                data[r_off + 3],
            ]) as usize;
            let sym_idx = u32::from_le_bytes([
                data[r_off + 4],
                data[r_off + 5],
                data[r_off + 6],
                data[r_off + 7],
            ]) as usize;
            let reloc_type = u16::from_le_bytes([data[r_off + 8], data[r_off + 9]]) as u32;

            let sym_name = if let Some(sym) = symbols.get(sym_idx) {
                sym.name.clone()
            } else {
                format!("<sym_{}>", sym_idx)
            };

            relocations.push(RelocationRecord {
                object_id,
                section_index: sec_idx,
                offset_in_section: virt_addr,
                symbol_name: sym_name.clone(),
                symbol_index: sym_idx,
                reloc_type,
                addend: 0,
            });
        }
    }

    let mut obj = ObjectFile::new(object_id, name, path, InputFormat::CoffAmd64);
    obj.sections = sections;
    obj.symbols = symbols;
    obj.relocations = relocations;

    Ok(obj)
}
