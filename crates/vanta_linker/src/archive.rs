//! Static archive (.a and .lib) parsing, symbol index extraction, and deterministic creation.

use crate::error::{LinkResult, LinkerError};
use crate::input::{detect_format, InputFormat};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A single member entry within a static archive.
#[derive(Debug, Clone)]
pub struct ArchiveMember {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub data: Vec<u8>,
}

/// In-memory representation of a parsed static archive with symbol index.
#[derive(Debug, Clone)]
pub struct Archive {
    pub path: PathBuf,
    pub members: Vec<ArchiveMember>,
    /// Map of symbol name -> index of the member in `members` that defines it.
    pub symbol_index: HashMap<String, usize>,
}

impl Archive {
    /// Parses an archive from raw bytes (supporting both Unix ar and Windows COFF .lib).
    pub fn parse<P: AsRef<Path>>(path: P, data: &[u8]) -> LinkResult<Self> {
        let p = path.as_ref().to_path_buf();
        if data.len() < 8 || !data.starts_with(b"!<arch>\n") {
            return Err(LinkerError::MalformedInput {
                path: p,
                offset: 0,
                message: "Archive magic '!<arch>\\n' missing".into(),
            });
        }

        let mut offset = 8usize;
        let mut raw_members: Vec<(String, usize, usize, &[u8])> = Vec::new();
        let mut gnu_strtab: Option<Vec<u8>> = None;
        let mut symbol_to_member_offset: HashMap<String, usize> = HashMap::new();

        while offset + 60 <= data.len() {
            let header = &data[offset..offset + 60];
            if header[58] != 0x60 || header[59] != 0x0A {
                return Err(LinkerError::MalformedInput {
                    path: p.clone(),
                    offset,
                    message: "Invalid archive member header magic".into(),
                });
            }

            let name_raw = std::str::from_utf8(&header[0..16]).unwrap_or("").trim_end();
            let size_str = std::str::from_utf8(&header[48..58]).unwrap_or("").trim();
            let size: usize = size_str.parse().map_err(|_| LinkerError::MalformedInput {
                path: p.clone(),
                offset: offset + 48,
                message: format!("Invalid archive member size: '{}'", size_str),
            })?;

            let content_start = offset + 60;
            let content_end =
                content_start
                    .checked_add(size)
                    .ok_or_else(|| LinkerError::MalformedInput {
                        path: p.clone(),
                        offset: content_start,
                        message: "Archive member size overflow".into(),
                    })?;

            if content_end > data.len() {
                return Err(LinkerError::MalformedInput {
                    path: p.clone(),
                    offset,
                    message: format!(
                        "Archive member '{}' extends beyond end of file (size {})",
                        name_raw, size
                    ),
                });
            }

            let member_bytes = &data[content_start..content_end];

            // Handle special members:
            // 1. GNU long filename string table: "//"
            if name_raw == "//" {
                gnu_strtab = Some(member_bytes.to_vec());
            }
            // 2. GNU or COFF First Linker Member: "/"
            else if name_raw == "/" {
                parse_gnu_or_coff_symbol_index(member_bytes, &mut symbol_to_member_offset);
            }
            // 3. BSD ranlib symbol table: "__.SYMDEF" or "__.SYMDEF SORTED"
            else if name_raw.starts_with("__.SYMDEF") {
                parse_bsd_symbol_index(member_bytes, &mut symbol_to_member_offset);
            } else {
                raw_members.push((name_raw.to_string(), offset, size, member_bytes));
            }

            // Move to next member (2-byte aligned)
            offset = content_end + (content_end % 2);
        }

        if offset < data.len() {
            return Err(LinkerError::MalformedInput {
                path: p.clone(),
                offset,
                message: "Truncated archive member header".into(),
            });
        }

        // Resolve member names (resolving GNU "/offset" and BSD "#1/len")
        let mut members = Vec::new();
        let mut offset_to_member_idx: HashMap<usize, usize> = HashMap::new();

        for (name_raw, member_offset, size, bytes) in raw_members {
            let (final_name, final_bytes) = if let Some(offset_str) = name_raw.strip_prefix('/') {
                // GNU long filename index
                if let (Ok(str_off), Some(ref strtab)) = (offset_str.parse::<usize>(), &gnu_strtab)
                {
                    if str_off < strtab.len() {
                        let name_slice = &strtab[str_off..];
                        let len = name_slice
                            .iter()
                            .position(|&b| b == b'/' || b == 0 || b == b'\n')
                            .unwrap_or(name_slice.len());
                        let name = String::from_utf8_lossy(&name_slice[..len]).to_string();
                        (name, bytes)
                    } else {
                        (name_raw.clone(), bytes)
                    }
                } else {
                    (name_raw.clone(), bytes)
                }
            } else if let Some(len_str) = name_raw.strip_prefix("#1/") {
                // BSD extended filename
                if let Ok(name_len) = len_str.parse::<usize>() {
                    if name_len <= bytes.len() {
                        let name = String::from_utf8_lossy(&bytes[..name_len])
                            .trim_matches('\0')
                            .to_string();
                        let actual_bytes = &bytes[name_len..];
                        (name, actual_bytes)
                    } else {
                        (name_raw.clone(), bytes)
                    }
                } else {
                    (name_raw.clone(), bytes)
                }
            } else {
                let clean_name = name_raw.trim_end_matches('/').to_string();
                (clean_name, bytes)
            };

            let member_idx = members.len();
            offset_to_member_idx.insert(member_offset, member_idx);
            members.push(ArchiveMember {
                name: final_name,
                offset: member_offset,
                size,
                data: final_bytes.to_vec(),
            });
        }

        // Map symbols to resolved member indices
        let mut symbol_index: HashMap<String, usize> = HashMap::new();
        for (sym, mem_offset) in symbol_to_member_offset {
            if let Some(&idx) = offset_to_member_idx.get(&mem_offset) {
                symbol_index.insert(sym, idx);
            }
        }

        // If archive lacked an explicit symbol index table, scan object members directly
        if symbol_index.is_empty() {
            for (idx, mem) in members.iter().enumerate() {
                scan_object_symbols(&mem.data, idx, &mut symbol_index);
            }
        }

        Ok(Self {
            path: p,
            members,
            symbol_index,
        })
    }
}

fn parse_gnu_or_coff_symbol_index(data: &[u8], symbols: &mut HashMap<String, usize>) {
    if data.len() < 4 {
        return;
    }
    // Big-endian count of symbols in GNU format
    let num_symbols = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let offsets_end = 4 + num_symbols * 4;
    if data.len() < offsets_end {
        return;
    }

    let mut member_offsets = Vec::with_capacity(num_symbols);
    for i in 0..num_symbols {
        let off_bytes = &data[4 + i * 4..4 + (i + 1) * 4];
        let off =
            u32::from_be_bytes([off_bytes[0], off_bytes[1], off_bytes[2], off_bytes[3]]) as usize;
        member_offsets.push(off);
    }

    // String table follows offsets
    let strtab = &data[offsets_end..];
    let mut str_pos = 0;
    for &mem_off in &member_offsets {
        if str_pos >= strtab.len() {
            break;
        }
        let sym_slice = &strtab[str_pos..];
        let len = sym_slice
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(sym_slice.len());
        let sym_name = String::from_utf8_lossy(&sym_slice[..len]).to_string();
        str_pos += len + 1;
        if !sym_name.is_empty() {
            symbols.insert(sym_name, mem_off);
        }
    }
}

fn parse_bsd_symbol_index(data: &[u8], symbols: &mut HashMap<String, usize>) {
    if data.len() < 4 {
        return;
    }
    let ranlibs_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let num_ranlibs = ranlibs_size / 8;
    let strtab_offset = 4 + ranlibs_size + 4; // size + ranlibs + strtab_size
    if data.len() < strtab_offset {
        return;
    }

    let strtab = &data[strtab_offset..];
    for i in 0..num_ranlibs {
        let entry = &data[4 + i * 8..4 + (i + 1) * 8];
        let str_off = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
        let mem_off = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]) as usize;

        if str_off < strtab.len() {
            let sym_slice = &strtab[str_off..];
            let len = sym_slice
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(sym_slice.len());
            let sym_name = String::from_utf8_lossy(&sym_slice[..len]).to_string();
            if !sym_name.is_empty() {
                symbols.insert(sym_name, mem_off);
            }
        }
    }
}

/// Fallback scanner: extracts exported symbol names directly from ELF/COFF/Mach-O bytes.
pub fn scan_object_symbols(
    data: &[u8],
    member_idx: usize,
    symbol_index: &mut HashMap<String, usize>,
) {
    match detect_format(data) {
        InputFormat::Elf64 => {
            if let Ok(elf) = crate::elf::reader::parse_elf64_object(
                &PathBuf::from("archive_member"),
                data,
                member_idx,
            ) {
                for sym in elf.symbols {
                    if sym.is_global() && sym.is_defined() {
                        symbol_index.insert(sym.name, member_idx);
                    }
                }
            }
        }
        InputFormat::CoffAmd64 => {
            if let Ok(coff) = crate::coff::reader::parse_coff_object(
                &PathBuf::from("archive_member"),
                data,
                member_idx,
            ) {
                for sym in coff.symbols {
                    if sym.is_global() && sym.is_defined() {
                        symbol_index.insert(sym.name, member_idx);
                    }
                }
            }
        }
        InputFormat::MachO64 => {
            if let Ok(macho) = crate::macho::reader::parse_macho_object(
                &PathBuf::from("archive_member"),
                data,
                member_idx,
            ) {
                for sym in macho.symbols {
                    if sym.is_global() && sym.is_defined() {
                        symbol_index.insert(sym.name, member_idx);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Pure-Rust deterministic static archive creator (.a and .lib).
pub struct ArchiveWriter {
    members: Vec<(String, Vec<u8>)>,
}

impl Default for ArchiveWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ArchiveWriter {
    pub fn new() -> Self {
        Self {
            members: Vec::new(),
        }
    }

    pub fn add_member(&mut self, name: String, data: Vec<u8>) {
        self.members.push((name, data));
    }

    /// Emits a deterministic Unix ar / Windows COFF archive byte buffer.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"!<arch>\n");

        // 1. Collect exported symbols across all members
        let mut sym_entries: Vec<(String, usize)> = Vec::new(); // (symbol_name, member_index)
        for (idx, (_name, data)) in self.members.iter().enumerate() {
            let mut symbols = HashMap::new();
            scan_object_symbols(data, idx, &mut symbols);
            for (sym, _) in symbols {
                sym_entries.push((sym, idx));
            }
        }

        // Sort symbols deterministically
        sym_entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        // 2. Build Symbol Index member "/"
        let mut symtab_member_data = Vec::new();
        // Number of symbols (4 bytes big-endian)
        symtab_member_data.extend_from_slice(&(sym_entries.len() as u32).to_be_bytes());

        // Placeholder for offsets (4 bytes each)
        let offsets_pos = symtab_member_data.len();
        symtab_member_data.resize(offsets_pos + sym_entries.len() * 4, 0);

        // String table follows
        for (sym, _) in &sym_entries {
            symtab_member_data.extend_from_slice(sym.as_bytes());
            symtab_member_data.push(0); // NUL terminator
        }

        // Compute where members will land
        let sym_header_offset = out.len();
        let sym_member_size = symtab_member_data.len();
        let sym_member_padding = sym_member_size % 2;
        let first_obj_offset = sym_header_offset + 60 + sym_member_size + sym_member_padding;

        // Compute offsets of each member object
        let mut member_offsets = Vec::with_capacity(self.members.len());
        let mut curr_offset = first_obj_offset;
        for (_name, data) in &self.members {
            member_offsets.push(curr_offset);
            let size = data.len();
            let pad = size % 2;
            curr_offset += 60 + size + pad;
        }

        // Fill in symbol table offsets
        for (i, (_sym, mem_idx)) in sym_entries.iter().enumerate() {
            let off = member_offsets[*mem_idx] as u32;
            let dest = &mut symtab_member_data[offsets_pos + i * 4..offsets_pos + (i + 1) * 4];
            dest.copy_from_slice(&off.to_be_bytes());
        }

        // Write Symbol Index member header
        write_ar_header(&mut out, "/", 0, sym_member_size);
        out.extend_from_slice(&symtab_member_data);
        if sym_member_padding > 0 {
            out.push(b'\n');
        }

        // 3. Write each member
        for (name, data) in &self.members {
            let clean_name = format!("{}/", name.trim_end_matches('/'));
            write_ar_header(&mut out, &clean_name, 0, data.len());
            out.extend_from_slice(data);
            if data.len() % 2 != 0 {
                out.push(b'\n');
            }
        }

        out
    }
}

fn write_ar_header(out: &mut Vec<u8>, name: &str, mtime: u64, size: usize) {
    let mut header = [b' '; 60];

    // Name (16 bytes)
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(16);
    header[0..name_len].copy_from_slice(&name_bytes[..name_len]);

    // Mtime (12 bytes)
    let mtime_str = format!("{}", mtime);
    let mtime_bytes = mtime_str.as_bytes();
    header[16..16 + mtime_bytes.len()].copy_from_slice(mtime_bytes);

    // UID (6 bytes)
    header[28..29].copy_from_slice(b"0");
    // GID (6 bytes)
    header[34..35].copy_from_slice(b"0");
    // Mode (8 bytes octal: 644)
    header[40..46].copy_from_slice(b"100644");

    // Size (10 bytes decimal)
    let size_str = format!("{}", size);
    let size_bytes = size_str.as_bytes();
    header[48..48 + size_bytes.len()].copy_from_slice(size_bytes);

    // Magic "\x60\n"
    header[58] = 0x60;
    header[59] = 0x0A;

    out.extend_from_slice(&header);
}

/// Lazily extracts required archive members to resolve missing symbols.
pub fn extract_required_members(
    archives: &[Archive],
    unresolved_symbols: &mut HashSet<String>,
    extracted_members: &mut Vec<ArchiveMember>,
) {
    let mut changed = true;
    while changed {
        changed = false;
        let current_unresolved: Vec<String> = unresolved_symbols.iter().cloned().collect();

        for sym in current_unresolved {
            for archive in archives {
                if let Some(&mem_idx) = archive.symbol_index.get(&sym) {
                    if mem_idx < archive.members.len() {
                        let member = &archive.members[mem_idx];
                        // If not already extracted
                        if !extracted_members.iter().any(|m| m.offset == member.offset) {
                            extracted_members.push(member.clone());
                            changed = true;
                            unresolved_symbols.remove(&sym);
                            // Find which symbols this new member defines and remove them
                            let mut defined_by_member = HashMap::new();
                            scan_object_symbols(&member.data, 0, &mut defined_by_member);
                            for (defined_sym, _) in defined_by_member {
                                unresolved_symbols.remove(&defined_sym);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
}
