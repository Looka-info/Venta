//! ELF64 relocatable object file parser.

use crate::error::{LinkResult, LinkerError};
use crate::input::InputFormat;
use crate::object::ObjectFile;
use crate::relocation::RelocationRecord;
use crate::section::{InputSection, SectionFlags, SectionKind};
use crate::symbol::{Symbol, SymbolKind, SymbolVisibility};
use std::path::Path;

pub const EM_X86_64: u16 = 62;
pub const ET_REL: u16 = 1;

pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_NOBITS: u32 = 8;

pub const SHF_WRITE: u64 = 0x1;
pub const SHF_ALLOC: u64 = 0x2;
pub const SHF_EXECINSTR: u64 = 0x4;

pub fn read_elf_object(
    object_id: usize,
    _name: String,
    path: std::path::PathBuf,
    data: &[u8],
) -> LinkResult<ObjectFile> {
    parse_elf64_object(&path, data, object_id)
}

pub fn parse_elf64_object(path: &Path, data: &[u8], object_id: usize) -> LinkResult<ObjectFile> {
    if data.len() < 64 {
        return Err(LinkerError::MalformedInput {
            path: path.to_path_buf(),
            offset: 0,
            message: "ELF file smaller than 64-byte header".into(),
        });
    }

    if !data.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return Err(LinkerError::MalformedInput {
            path: path.to_path_buf(),
            offset: 0,
            message: "Missing ELF magic".into(),
        });
    }

    // Class 2 = 64-bit, Data 1 = Little-endian
    if data[4] != 2 || data[5] != 1 {
        return Err(LinkerError::MalformedInput {
            path: path.to_path_buf(),
            offset: 4,
            message: "Only 64-bit little-endian ELF objects are supported".into(),
        });
    }

    let e_type = u16::from_le_bytes([data[16], data[17]]);
    if e_type != ET_REL {
        return Err(LinkerError::MalformedInput {
            path: path.to_path_buf(),
            offset: 16,
            message: format!("Expected relocatable ELF object (ET_REL), found {}", e_type),
        });
    }

    let e_machine = u16::from_le_bytes([data[18], data[19]]);
    if e_machine != EM_X86_64 {
        return Err(LinkerError::IncompatibleArchitecture {
            path: path.to_path_buf(),
            expected: "EM_X86_64 (62)".into(),
            found: format!("{}", e_machine),
        });
    }

    let e_shoff = u64::from_le_bytes(data[40..48].try_into().unwrap()) as usize;
    let e_shentsize = u16::from_le_bytes([data[58], data[59]]) as usize;
    let e_shnum = u16::from_le_bytes([data[60], data[61]]) as usize;
    let e_shstrndx = u16::from_le_bytes([data[62], data[63]]) as usize;

    if e_shentsize < 64 || e_shoff + e_shnum * e_shentsize > data.len() {
        return Err(LinkerError::MalformedInput {
            path: path.to_path_buf(),
            offset: e_shoff,
            message: "Section header table out of bounds".into(),
        });
    }

    // 1. First locate .shstrtab to read section names
    let shstr_header_offset = e_shoff + e_shstrndx * e_shentsize;
    let shstr_offset = u64::from_le_bytes(
        data[shstr_header_offset + 24..shstr_header_offset + 32]
            .try_into()
            .unwrap(),
    ) as usize;
    let shstr_size = u64::from_le_bytes(
        data[shstr_header_offset + 32..shstr_header_offset + 40]
            .try_into()
            .unwrap(),
    ) as usize;
    if shstr_offset + shstr_size > data.len() {
        return Err(LinkerError::MalformedInput {
            path: path.to_path_buf(),
            offset: shstr_offset,
            message: ".shstrtab out of bounds".into(),
        });
    }
    let shstrtab = &data[shstr_offset..shstr_offset + shstr_size];

    let get_sh_name = |name_offset: usize| -> String {
        if name_offset < shstrtab.len() {
            let slice = &shstrtab[name_offset..];
            let len = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
            String::from_utf8_lossy(&slice[..len]).to_string()
        } else {
            String::new()
        }
    };

    // 2. Read Section Headers
    struct RawElfSection {
        idx: usize,
        name: String,
        sh_type: u32,
        sh_flags: u64,
        sh_offset: usize,
        sh_size: usize,
        _sh_link: u32,
        sh_info: u32,
        sh_addralign: usize,
    }

    let mut raw_sections = Vec::with_capacity(e_shnum);
    let mut symtab_idx = None;
    let mut strtab_idx = None;

    for i in 0..e_shnum {
        let off = e_shoff + i * e_shentsize;
        let sh_name_off = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        let sh_type = u32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap());
        let sh_flags = u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
        let sh_offset = u64::from_le_bytes(data[off + 24..off + 32].try_into().unwrap()) as usize;
        let sh_size = u64::from_le_bytes(data[off + 32..off + 40].try_into().unwrap()) as usize;
        let sh_link = u32::from_le_bytes(data[off + 40..off + 44].try_into().unwrap());
        let sh_info = u32::from_le_bytes(data[off + 44..off + 48].try_into().unwrap());
        let sh_addralign =
            u64::from_le_bytes(data[off + 48..off + 56].try_into().unwrap()) as usize;

        let name = get_sh_name(sh_name_off);
        if sh_type == SHT_SYMTAB {
            symtab_idx = Some(i);
            strtab_idx = Some(sh_link as usize);
        }

        raw_sections.push(RawElfSection {
            idx: i,
            name,
            sh_type,
            sh_flags,
            sh_offset,
            sh_size,
            _sh_link: sh_link,
            sh_info,
            sh_addralign,
        });
    }

    let mut object_file = ObjectFile::new(
        object_id,
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("object")
            .to_string(),
        path.to_path_buf(),
        InputFormat::Elf64,
    );

    // 3. Populate InputSections (ignoring relocation/symtab/strtab sections as data sections)
    for s in &raw_sections {
        if s.sh_type == SHT_PROGBITS || s.sh_type == SHT_NOBITS {
            let kind = match s.name.as_str() {
                ".text" | ".text.startup" => SectionKind::Code,
                ".rodata" | ".rodata.str1.1" => SectionKind::ReadOnlyData,
                ".data" => SectionKind::Data,
                ".bss" => SectionKind::Bss,
                ".debug_info" => SectionKind::DebugInfo,
                ".debug_abbrev" => SectionKind::DebugAbbrev,
                ".debug_line" => SectionKind::DebugLine,
                _ => {
                    if (s.sh_flags & SHF_EXECINSTR) != 0 {
                        SectionKind::Code
                    } else if (s.sh_flags & SHF_WRITE) != 0 {
                        SectionKind::Data
                    } else if (s.sh_flags & SHF_ALLOC) != 0 {
                        SectionKind::ReadOnlyData
                    } else {
                        SectionKind::Other
                    }
                }
            };

            let flags = SectionFlags {
                read: true,
                write: (s.sh_flags & SHF_WRITE) != 0,
                execute: (s.sh_flags & SHF_EXECINSTR) != 0,
                alloc: (s.sh_flags & SHF_ALLOC) != 0,
            };

            let data_slice = if s.sh_type == SHT_NOBITS {
                Vec::new()
            } else {
                if s.sh_offset + s.sh_size > data.len() {
                    return Err(LinkerError::MalformedInput {
                        path: path.to_path_buf(),
                        offset: s.sh_offset,
                        message: format!("Section '{}' data out of bounds", s.name),
                    });
                }
                data[s.sh_offset..s.sh_offset + s.sh_size].to_vec()
            };

            object_file.sections.push(InputSection {
                object_id,
                source_index: s.idx,
                name: s.name.clone(),
                kind,
                flags,
                align: s.sh_addralign.max(1),
                data: data_slice,
                size: s.sh_size,
            });
        }
    }

    // 4. Parse Symbol Table
    if let (Some(sym_idx), Some(str_idx)) = (symtab_idx, strtab_idx) {
        let sym_sec = &raw_sections[sym_idx];
        let str_sec = &raw_sections[str_idx];

        if str_sec.sh_offset + str_sec.sh_size > data.len() {
            return Err(LinkerError::MalformedInput {
                path: path.to_path_buf(),
                offset: str_sec.sh_offset,
                message: "ELF .strtab out of bounds".into(),
            });
        }
        let strtab = &data[str_sec.sh_offset..str_sec.sh_offset + str_sec.sh_size];

        let get_symbol_name = |name_offset: usize| -> String {
            if name_offset < strtab.len() {
                let slice = &strtab[name_offset..];
                let len = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                String::from_utf8_lossy(&slice[..len]).to_string()
            } else {
                String::new()
            }
        };

        let sym_count = sym_sec.sh_size / 24;
        for i in 0..sym_count {
            let off = sym_sec.sh_offset + i * 24;
            let st_name_off = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
            let st_info = data[off + 4];
            let st_shndx = u16::from_le_bytes([data[off + 6], data[off + 7]]);
            let st_value = u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
            let st_size = u64::from_le_bytes(data[off + 16..off + 24].try_into().unwrap());

            let mut name = get_symbol_name(st_name_off);
            let binding = st_info >> 4;
            let sym_type = st_info & 0xF;

            let visibility = match binding {
                0 => SymbolVisibility::Local,
                1 => SymbolVisibility::Global,
                2 => SymbolVisibility::Weak,
                _ => SymbolVisibility::Local,
            };

            let kind = match sym_type {
                1 => SymbolKind::Object,
                2 => SymbolKind::Function,
                3 => SymbolKind::Section,
                _ => SymbolKind::Notype,
            };

            let section_index = if st_shndx == 0 {
                None // SHN_UNDEF
            } else {
                Some(st_shndx as usize)
            };

            if kind == SymbolKind::Section && name.is_empty() {
                if let Some(sec_idx) = section_index {
                    if let Some(rs) = raw_sections.get(sec_idx) {
                        name = rs.name.clone();
                    }
                }
            }

            object_file.symbols.push(Symbol {
                name,
                visibility,
                kind,
                section_index,
                value: st_value,
                size: st_size,
                object_id,
                final_virtual_address: None,
            });
        }
    }

    // 5. Parse Relocations (.rela.*)
    for s in &raw_sections {
        if s.sh_type == SHT_RELA {
            let target_sec_idx = s.sh_info as usize;
            let reloc_count = s.sh_size / 24;

            for i in 0..reloc_count {
                let off = s.sh_offset + i * 24;
                let r_offset = u64::from_le_bytes(data[off..off + 8].try_into().unwrap()) as usize;
                let r_info = u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
                let r_addend = i64::from_le_bytes(data[off + 16..off + 24].try_into().unwrap());

                let sym_idx = (r_info >> 32) as usize;
                let r_type = (r_info & 0xFFFFFFFF) as u32;

                let sym_name = if sym_idx < object_file.symbols.len() {
                    let sym = &object_file.symbols[sym_idx];
                    if sym.kind == SymbolKind::Section {
                        // Section symbol: name of the section it refers to
                        if let Some(sec_idx) = sym.section_index {
                            raw_sections
                                .get(sec_idx)
                                .map(|rs| rs.name.clone())
                                .unwrap_or_default()
                        } else {
                            sym.name.clone()
                        }
                    } else {
                        sym.name.clone()
                    }
                } else {
                    String::new()
                };

                object_file.relocations.push(RelocationRecord {
                    object_id,
                    section_index: target_sec_idx,
                    offset_in_section: r_offset,
                    symbol_name: sym_name,
                    symbol_index: sym_idx,
                    reloc_type: r_type,
                    addend: r_addend,
                });
            }
        }
    }

    Ok(object_file)
}
