//! Pure-Rust Apple Silicon macOS Mach-O 64-bit Relocatable Object File (.o) Writer.
//!
//! Emits valid Mach-O 64-bit object files (`CPU_TYPE_ARM64 = 0x0100000C`, `MH_OBJECT = 1`)
//! conforming to Apple Silicon macOS requirements:
//! - 64-bit Mach header (`mach_header_64`)
//! - Segment command `LC_SEGMENT_64` with sections `__text` and `__const` in segment `__TEXT`
//! - Symbol table command `LC_SYMTAB` with 64-bit `nlist_64` entries
//! - Mach-O relocations (`ARM64_RELOC_BRANCH26`, `ARM64_RELOC_PAGE21`, `ARM64_RELOC_PAGEOFF12`, `ARM64_RELOC_UNSIGNED`)
//! - Darwin symbol naming convention (leading underscore `_`)

use crate::encoding::{
    ArmRelocation, ARM64_RELOC_BRANCH26, ARM64_RELOC_PAGE21, ARM64_RELOC_PAGEOFF12,
    ARM64_RELOC_UNSIGNED,
};
use std::collections::HashMap;

pub const MH_MAGIC_64: u32 = 0xFEEDFACF;
pub const CPU_TYPE_ARM64: u32 = 0x0100000C;
pub const CPU_SUBTYPE_ARM64_ALL: u32 = 0x00000000;
pub const MH_OBJECT: u32 = 0x1;
pub const MH_SUBSECTIONS_VIA_SYMBOLS: u32 = 0x2000;

pub const LC_SEGMENT_64: u32 = 0x19;
pub const LC_SYMTAB: u32 = 0x02;

pub const S_ATTR_PURE_INSTRUCTIONS: u32 = 0x80000000;
pub const S_ATTR_SOME_INSTRUCTIONS: u32 = 0x00000400;

pub const N_EXT: u8 = 0x01;
pub const N_SECT: u8 = 0x0E;
pub const N_UNDF: u8 = 0x00;

#[derive(Debug, Clone)]
pub struct MachOFuncSymbol {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub is_global: bool,
}

#[derive(Debug, Clone)]
pub struct MachODataSymbol {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

pub struct MachOWriter {
    text: Vec<u8>,
    const_data: Vec<u8>,
    text_relocations: Vec<ArmRelocation>,
    const_relocations: Vec<ArmRelocation>,
    functions: Vec<MachOFuncSymbol>,
    data_symbols: Vec<MachODataSymbol>,
}

impl MachOWriter {
    pub fn new(
        text: Vec<u8>,
        const_data: Vec<u8>,
        text_relocations: Vec<ArmRelocation>,
        functions: Vec<MachOFuncSymbol>,
        data_symbols: Vec<MachODataSymbol>,
    ) -> Self {
        Self {
            text,
            const_data,
            text_relocations,
            const_relocations: Vec::new(),
            functions,
            data_symbols,
        }
    }

    pub fn with_const_relocations(mut self, relocs: Vec<ArmRelocation>) -> Self {
        self.const_relocations = relocs;
        self
    }

    /// Emits a valid Mach-O 64-bit object file (.o) byte buffer.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();

        // 1. Build Darwin symbols (prepend '_' if missing)
        let mut symbols: Vec<(String, u8, u8, u64)> = Vec::new(); // (name, n_type, n_sect, n_value)
        let mut symbol_indices: HashMap<String, u32> = HashMap::new();

        // Section 1: __text
        // Section 2: __const
        for func in &self.functions {
            let symbol_name = if !func.name.starts_with('_') {
                format!("_{}", func.name)
            } else {
                func.name.clone()
            };
            let idx = symbols.len() as u32;
            symbols.push((
                symbol_name.clone(),
                N_SECT | N_EXT,
                1, // section 1: __text
                func.offset as u64,
            ));
            symbol_indices.insert(symbol_name, idx);
            symbol_indices.insert(func.name.clone(), idx);
        }

        for data in &self.data_symbols {
            let symbol_name = if !data.name.starts_with('_') {
                format!("_{}", data.name)
            } else {
                data.name.clone()
            };
            let idx = symbols.len() as u32;
            symbols.push((
                symbol_name.clone(),
                N_SECT | N_EXT,
                2, // section 2: __const
                data.offset as u64,
            ));
            symbol_indices.insert(symbol_name, idx);
            symbol_indices.insert(data.name.clone(), idx);
        }

        // Undefined external references from relocations
        for r in self.text_relocations.iter().chain(&self.const_relocations) {
            let symbol_name = if !r.symbol.starts_with('_') {
                format!("_{}", r.symbol)
            } else {
                r.symbol.clone()
            };
            if !symbol_indices.contains_key(&symbol_name) && !symbol_indices.contains_key(&r.symbol)
            {
                let idx = symbols.len() as u32;
                symbols.push((
                    symbol_name.clone(),
                    N_UNDF | N_EXT,
                    0, // undefined
                    0,
                ));
                symbol_indices.insert(symbol_name, idx);
                symbol_indices.insert(r.symbol.clone(), idx);
            }
        }

        // 2. Prepare Mach-O Relocations
        let mut macho_text_relocs: Vec<u64> = Vec::new();
        for r in &self.text_relocations {
            let sym_idx = *symbol_indices
                .get(&r.symbol)
                .expect("all referenced symbols mapped");
            let pcrel: u32 = match r.r_type {
                ARM64_RELOC_BRANCH26 | ARM64_RELOC_PAGE21 | ARM64_RELOC_PAGEOFF12 => 1,
                _ => 0,
            };
            let length: u32 = 2; // 2 = 4 bytes (32-bit instruction)
            let is_extern: u32 = 1;
            let r_type: u32 = r.r_type as u32;

            let word2 = (sym_idx & 0x00FFFFFF)
                | (pcrel << 24)
                | (length << 25)
                | (is_extern << 27)
                | (r_type << 28);
            let reloc_entry = ((word2 as u64) << 32) | ((r.offset as u32) as u64);
            macho_text_relocs.push(reloc_entry);
        }

        let mut macho_const_relocs: Vec<u64> = Vec::new();
        for r in &self.const_relocations {
            let sym_idx = *symbol_indices
                .get(&r.symbol)
                .expect("all referenced symbols mapped");
            let pcrel: u32 = 0;
            let length: u32 = 3; // 3 = 8 bytes (64-bit pointer)
            let is_extern: u32 = 1;
            let r_type: u32 = ARM64_RELOC_UNSIGNED as u32;

            let word2 = (sym_idx & 0x00FFFFFF)
                | (pcrel << 24)
                | (length << 25)
                | (is_extern << 27)
                | (r_type << 28);
            let reloc_entry = ((word2 as u64) << 32) | ((r.offset as u32) as u64);
            macho_const_relocs.push(reloc_entry);
        }

        // 3. Layout calculation
        let num_sections = 2u32;
        let header_size = 32usize;
        let segment_cmd_size = 72 + (num_sections as usize) * 80; // 72 + 160 = 232 bytes
        let symtab_cmd_size = 24usize;
        let sizeofcmds = (segment_cmd_size + symtab_cmd_size) as u32;

        let raw_data_start = header_size + (sizeofcmds as usize);
        let text_size = self.text.len();
        let const_size = self.const_data.len();

        let text_file_offset = raw_data_start as u32;
        let const_file_offset = text_file_offset + (text_size as u32);

        let text_relocs_offset = const_file_offset + (const_size as u32);
        let text_relocs_size = (macho_text_relocs.len() * 8) as u32;

        let const_relocs_offset = text_relocs_offset + text_relocs_size;
        let const_relocs_size = (macho_const_relocs.len() * 8) as u32;

        let symtab_offset = const_relocs_offset + const_relocs_size;
        let symtab_size = (symbols.len() * 16) as u32;

        let stroff = symtab_offset + symtab_size;

        // Build string table
        let mut string_table = Vec::new();
        string_table.push(0); // string table index 0 is empty string

        let mut symbol_str_offsets = Vec::with_capacity(symbols.len());
        for (name, _, _, _) in &symbols {
            let off = string_table.len() as u32;
            string_table.extend_from_slice(name.as_bytes());
            string_table.push(0);
            symbol_str_offsets.push(off);
        }

        // 4. Write mach_header_64 (32 bytes)
        out.extend_from_slice(&MH_MAGIC_64.to_le_bytes()); // magic
        out.extend_from_slice(&CPU_TYPE_ARM64.to_le_bytes()); // cputype
        out.extend_from_slice(&CPU_SUBTYPE_ARM64_ALL.to_le_bytes()); // cpusubtype
        out.extend_from_slice(&MH_OBJECT.to_le_bytes()); // filetype
        out.extend_from_slice(&2u32.to_le_bytes()); // ncmds
        out.extend_from_slice(&sizeofcmds.to_le_bytes()); // sizeofcmds
        out.extend_from_slice(&MH_SUBSECTIONS_VIA_SYMBOLS.to_le_bytes()); // flags
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved

        // 5. Write LC_SEGMENT_64 command (232 bytes)
        out.extend_from_slice(&LC_SEGMENT_64.to_le_bytes()); // cmd
        out.extend_from_slice(&(segment_cmd_size as u32).to_le_bytes()); // cmdsize
        out.extend_from_slice(&[0u8; 16]); // segname (empty for MH_OBJECT)
        out.extend_from_slice(&0u64.to_le_bytes()); // vmaddr
        out.extend_from_slice(&((text_size + const_size) as u64).to_le_bytes()); // vmsize
        out.extend_from_slice(&(text_file_offset as u64).to_le_bytes()); // fileoff
        out.extend_from_slice(&((text_size + const_size) as u64).to_le_bytes()); // filesize
        out.extend_from_slice(&7u32.to_le_bytes()); // maxprot (rwx)
        out.extend_from_slice(&7u32.to_le_bytes()); // initprot (rwx)
        out.extend_from_slice(&num_sections.to_le_bytes()); // nsects
        out.extend_from_slice(&0u32.to_le_bytes()); // flags

        // Section 1: __text (80 bytes)
        let mut sect_text_name = [0u8; 16];
        sect_text_name[..6].copy_from_slice(b"__text");
        let mut seg_text_name = [0u8; 16];
        seg_text_name[..6].copy_from_slice(b"__TEXT");

        out.extend_from_slice(&sect_text_name); // sectname
        out.extend_from_slice(&seg_text_name); // segname
        out.extend_from_slice(&0u64.to_le_bytes()); // addr
        out.extend_from_slice(&(text_size as u64).to_le_bytes()); // size
        out.extend_from_slice(&text_file_offset.to_le_bytes()); // offset
        out.extend_from_slice(&4u32.to_le_bytes()); // align (2^4 = 16 bytes)
        out.extend_from_slice(
            &(if macho_text_relocs.is_empty() {
                0
            } else {
                text_relocs_offset
            })
            .to_le_bytes(),
        );
        out.extend_from_slice(&(macho_text_relocs.len() as u32).to_le_bytes());
        out.extend_from_slice(&(S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved1
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved2
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved3

        // Section 2: __const (80 bytes)
        let mut sect_const_name = [0u8; 16];
        sect_const_name[..7].copy_from_slice(b"__const");

        out.extend_from_slice(&sect_const_name); // sectname
        out.extend_from_slice(&seg_text_name); // segname
        out.extend_from_slice(&(text_size as u64).to_le_bytes()); // addr
        out.extend_from_slice(&(const_size as u64).to_le_bytes()); // size
        out.extend_from_slice(&const_file_offset.to_le_bytes()); // offset
        out.extend_from_slice(&3u32.to_le_bytes()); // align (2^3 = 8 bytes)
        out.extend_from_slice(
            &(if macho_const_relocs.is_empty() {
                0
            } else {
                const_relocs_offset
            })
            .to_le_bytes(),
        );
        out.extend_from_slice(&(macho_const_relocs.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // flags (S_REGULAR)
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved1
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved2
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved3

        // 6. Write LC_SYMTAB command (24 bytes)
        out.extend_from_slice(&LC_SYMTAB.to_le_bytes()); // cmd
        out.extend_from_slice(&(symtab_cmd_size as u32).to_le_bytes()); // cmdsize
        out.extend_from_slice(&symtab_offset.to_le_bytes()); // symoff
        out.extend_from_slice(&(symbols.len() as u32).to_le_bytes()); // nsyms
        out.extend_from_slice(&stroff.to_le_bytes()); // stroff
        out.extend_from_slice(&(string_table.len() as u32).to_le_bytes()); // strsize

        // 7. Write Raw Section Data
        out.extend_from_slice(&self.text);
        out.extend_from_slice(&self.const_data);

        // 8. Write Relocations
        for reloc in &macho_text_relocs {
            out.extend_from_slice(&reloc.to_le_bytes());
        }
        for reloc in &macho_const_relocs {
            out.extend_from_slice(&reloc.to_le_bytes());
        }

        // 9. Write Symbol Table (16 bytes per symbol)
        for (i, (_, n_type, n_sect, n_value)) in symbols.iter().enumerate() {
            let str_idx = symbol_str_offsets[i];
            out.extend_from_slice(&str_idx.to_le_bytes()); // n_strx
            out.push(*n_type);
            out.push(*n_sect);
            out.extend_from_slice(&0u16.to_le_bytes()); // n_desc
            out.extend_from_slice(&n_value.to_le_bytes()); // n_value
        }

        // 10. Write String Table
        out.extend_from_slice(&string_table);

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macho_writer_magic_and_structure() {
        let text = vec![0xC0, 0x03, 0x5F, 0xD6]; // ret x30
        let const_data = b"hello\0".to_vec();
        let funcs = vec![MachOFuncSymbol {
            name: "test_fn".into(),
            offset: 0,
            size: 4,
            is_global: true,
        }];
        let data = vec![MachODataSymbol {
            name: ".str0".into(),
            offset: 0,
            size: 6,
        }];

        let writer = MachOWriter::new(text, const_data, vec![], funcs, data);
        let bytes = writer.write();

        // 1. Verify Mach-O 64-bit Magic (0xFEEDFACF)
        assert!(bytes.len() >= 32);
        assert_eq!(
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            MH_MAGIC_64
        );

        // 2. Verify CPU Type ARM64 (0x0100000C)
        assert_eq!(
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            CPU_TYPE_ARM64
        );

        // 3. Verify Filetype MH_OBJECT (1)
        assert_eq!(
            u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            MH_OBJECT
        );

        // 4. Verify 2 Load Commands
        assert_eq!(
            u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            2
        );
    }
}
