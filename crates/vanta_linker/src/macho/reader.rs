//! Pure-Rust Apple Silicon ARM64 Mach-O 64-bit Relocatable Object (.o) Reader.

use crate::error::{LinkResult, LinkerError};
use crate::input::InputFormat;
use crate::object::ObjectFile;
use crate::relocation::RelocationRecord;
use crate::section::{InputSection, SectionFlags, SectionKind};
use crate::symbol::{Symbol, SymbolKind, SymbolVisibility};
use std::path::PathBuf;

pub const MH_MAGIC_64: u32 = 0xFEEDFACF;
pub const CPU_TYPE_ARM64: u32 = 0x0100000C;

pub const LC_SEGMENT_64: u32 = 0x19;
pub const LC_SYMTAB: u32 = 0x02;

pub const N_EXT: u8 = 0x01;
pub const N_TYPE: u8 = 0x0E;
pub const N_SECT: u8 = 0x0E;
pub const N_UNDF: u8 = 0x00;

pub fn parse_macho_object(
    path: &std::path::Path,
    data: &[u8],
    object_id: usize,
) -> LinkResult<ObjectFile> {
    read_macho_object(
        object_id,
        path.display().to_string(),
        path.to_path_buf(),
        data,
    )
}

/// Parses an ARM64 Mach-O 64-bit relocatable object (.o) into an `ObjectFile`.
pub fn read_macho_object(
    object_id: usize,
    name: String,
    path: PathBuf,
    data: &[u8],
) -> LinkResult<ObjectFile> {
    if data.len() < 32 {
        return Err(LinkerError::MalformedInput {
            path,
            offset: 0,
            message: "File too small for Mach-O 64-bit header".into(),
        });
    }

    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != MH_MAGIC_64 {
        return Err(LinkerError::MalformedInput {
            path,
            offset: 0,
            message: format!("Invalid Mach-O magic: {:#010x}", magic),
        });
    }

    let cputype = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if cputype != CPU_TYPE_ARM64 {
        return Err(LinkerError::IncompatibleArchitecture {
            path,
            expected: "CPU_TYPE_ARM64 (0x0100000C)".into(),
            found: format!("{:#010x}", cputype),
        });
    }

    let ncmds = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let sizeofcmds = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as usize;

    if 32 + sizeofcmds > data.len() {
        return Err(LinkerError::MalformedInput {
            path: path.clone(),
            offset: 32,
            message: "Mach-O load commands extend beyond file bounds".into(),
        });
    }

    let mut cursor = 32usize;
    let mut sections = Vec::new();
    let mut symtab_cmd = None;
    let mut raw_relocs_by_section: Vec<(usize, u32, u32)> = Vec::new(); // (sec_idx, reloff, nreloc)

    for _ in 0..ncmds {
        if cursor + 8 > data.len() {
            break;
        }
        let cmd = u32::from_le_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]);
        let cmdsize = u32::from_le_bytes([
            data[cursor + 4],
            data[cursor + 5],
            data[cursor + 6],
            data[cursor + 7],
        ]) as usize;

        if cmd == LC_SEGMENT_64 {
            if cursor + 72 > data.len() {
                return Err(LinkerError::MalformedInput {
                    path: path.clone(),
                    offset: cursor,
                    message: "LC_SEGMENT_64 command truncated".into(),
                });
            }
            let nsects = u32::from_le_bytes([
                data[cursor + 64],
                data[cursor + 65],
                data[cursor + 66],
                data[cursor + 67],
            ]) as usize;
            let mut sec_cursor = cursor + 72;

            for _ in 0..nsects {
                if sec_cursor + 80 > data.len() {
                    break;
                }
                let sect_hdr = &data[sec_cursor..sec_cursor + 80];
                let sectname_raw = &sect_hdr[0..16];
                let segname_raw = &sect_hdr[16..32];

                let sectname = String::from_utf8_lossy(sectname_raw)
                    .trim_matches('\0')
                    .to_string();
                let segname = String::from_utf8_lossy(segname_raw)
                    .trim_matches('\0')
                    .to_string();

                let _addr = u64::from_le_bytes([
                    sect_hdr[32],
                    sect_hdr[33],
                    sect_hdr[34],
                    sect_hdr[35],
                    sect_hdr[36],
                    sect_hdr[37],
                    sect_hdr[38],
                    sect_hdr[39],
                ]);
                let size = u64::from_le_bytes([
                    sect_hdr[40],
                    sect_hdr[41],
                    sect_hdr[42],
                    sect_hdr[43],
                    sect_hdr[44],
                    sect_hdr[45],
                    sect_hdr[46],
                    sect_hdr[47],
                ]) as usize;
                let offset =
                    u32::from_le_bytes([sect_hdr[48], sect_hdr[49], sect_hdr[50], sect_hdr[51]])
                        as usize;
                let reloff =
                    u32::from_le_bytes([sect_hdr[56], sect_hdr[57], sect_hdr[58], sect_hdr[59]]);
                let nreloc =
                    u32::from_le_bytes([sect_hdr[60], sect_hdr[61], sect_hdr[62], sect_hdr[63]]);

                let (kind, flags) = if sectname == "__text" {
                    (SectionKind::Code, SectionFlags::code())
                } else if sectname == "__const" || sectname == "__rodata" {
                    (SectionKind::ReadOnlyData, SectionFlags::readonly())
                } else if sectname == "__data" {
                    (SectionKind::Data, SectionFlags::readwrite())
                } else if sectname == "__bss" {
                    (SectionKind::Bss, SectionFlags::readwrite())
                } else {
                    (SectionKind::Other, SectionFlags::readonly())
                };

                let sec_data = if kind.is_zero_init() {
                    Vec::new()
                } else if offset > 0 && size > 0 && offset + size <= data.len() {
                    data[offset..offset + size].to_vec()
                } else {
                    Vec::new()
                };

                let sec_idx = sections.len() + 1; // 1-based Mach-O section index
                sections.push(InputSection {
                    object_id,
                    source_index: sec_idx,
                    name: format!("{}.{}", segname, sectname),
                    kind,
                    flags,
                    align: 16,
                    data: sec_data,
                    size,
                });

                raw_relocs_by_section.push((sec_idx, reloff, nreloc));
                sec_cursor += 80;
            }
        } else if cmd == LC_SYMTAB {
            if cursor + 24 <= data.len() {
                let symoff = u32::from_le_bytes([
                    data[cursor + 8],
                    data[cursor + 9],
                    data[cursor + 10],
                    data[cursor + 11],
                ]) as usize;
                let nsyms = u32::from_le_bytes([
                    data[cursor + 12],
                    data[cursor + 13],
                    data[cursor + 14],
                    data[cursor + 15],
                ]) as usize;
                let stroff = u32::from_le_bytes([
                    data[cursor + 16],
                    data[cursor + 17],
                    data[cursor + 18],
                    data[cursor + 19],
                ]) as usize;
                let strsize = u32::from_le_bytes([
                    data[cursor + 20],
                    data[cursor + 21],
                    data[cursor + 22],
                    data[cursor + 23],
                ]) as usize;
                symtab_cmd = Some((symoff, nsyms, stroff, strsize));
            }
        }

        cursor += cmdsize;
    }

    // Parse Symbol Table
    let mut symbols: Vec<Symbol> = Vec::new();
    if let Some((symoff, nsyms, stroff, strsize)) = symtab_cmd {
        let strtab = if stroff + strsize <= data.len() {
            &data[stroff..stroff + strsize]
        } else {
            &[]
        };

        let read_str = |off: usize| -> String {
            if off < strtab.len() {
                let slice = &strtab[off..];
                let len = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                String::from_utf8_lossy(&slice[..len]).to_string()
            } else {
                String::new()
            }
        };

        for i in 0..nsyms {
            let nlist_off = symoff + i * 16;
            if nlist_off + 16 > data.len() {
                break;
            }
            let n_strx = u32::from_le_bytes([
                data[nlist_off],
                data[nlist_off + 1],
                data[nlist_off + 2],
                data[nlist_off + 3],
            ]) as usize;
            let n_type = data[nlist_off + 4];
            let n_sect = data[nlist_off + 5];
            let n_value = u64::from_le_bytes([
                data[nlist_off + 8],
                data[nlist_off + 9],
                data[nlist_off + 10],
                data[nlist_off + 11],
                data[nlist_off + 12],
                data[nlist_off + 13],
                data[nlist_off + 14],
                data[nlist_off + 15],
            ]);

            let raw_name = read_str(n_strx);
            // Darwin convention uses leading '_'; normalize
            let sym_name = raw_name.clone();

            let visibility = if n_type & N_EXT != 0 {
                SymbolVisibility::Global
            } else {
                SymbolVisibility::Local
            };

            let sec_idx = if n_sect > 0 {
                Some(n_sect as usize)
            } else {
                None
            };

            let kind = if (n_type & N_TYPE) == N_SECT {
                SymbolKind::Function
            } else {
                SymbolKind::Other
            };

            symbols.push(Symbol {
                name: sym_name,
                visibility,
                kind,
                section_index: sec_idx,
                value: n_value,
                size: 0,
                object_id,
                final_virtual_address: None,
            });
        }
    }

    // Parse Relocations
    let mut relocations: Vec<RelocationRecord> = Vec::new();
    for (sec_idx, reloff, nreloc) in raw_relocs_by_section {
        if reloff == 0 || nreloc == 0 {
            continue;
        }
        let start = reloff as usize;
        let count = nreloc as usize;
        if start + count * 8 > data.len() {
            return Err(LinkerError::MalformedInput {
                path: path.clone(),
                offset: start,
                message: "Mach-O relocations extend beyond file bounds".into(),
            });
        }

        for r in 0..count {
            let r_off = start + r * 8;
            let r_address = u32::from_le_bytes([
                data[r_off],
                data[r_off + 1],
                data[r_off + 2],
                data[r_off + 3],
            ]) as usize;
            let r_info = u32::from_le_bytes([
                data[r_off + 4],
                data[r_off + 5],
                data[r_off + 6],
                data[r_off + 7],
            ]);

            let r_symbolnum = (r_info & 0x00FFFFFF) as usize;
            let _r_pcrel = (r_info >> 24) & 1 != 0;
            let r_type = ((r_info >> 28) & 0x0F) as u32;

            let sym_name = if let Some(sym) = symbols.get(r_symbolnum) {
                sym.name.clone()
            } else {
                format!("<macho_sym_{}>", r_symbolnum)
            };

            relocations.push(RelocationRecord {
                object_id,
                section_index: sec_idx,
                offset_in_section: r_address,
                symbol_name: sym_name.clone(),
                symbol_index: r_symbolnum,
                reloc_type: r_type,
                addend: 0,
            });
        }
    }

    let mut obj = ObjectFile::new(object_id, name, path, InputFormat::MachO64);
    obj.sections = sections;
    obj.symbols = symbols;
    obj.relocations = relocations;

    Ok(obj)
}
