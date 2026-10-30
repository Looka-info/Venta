//! Pure Rust ELF64 relocatable object file writer for Linux x86-64.

use crate::encoding::Relocation;
use std::collections::HashMap;

pub struct FunctionSymbol {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub is_global: bool,
}

pub struct RodataSymbol {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone, Default)]
pub struct DebugSections {
    pub debug_info: Vec<u8>,
    pub debug_abbrev: Vec<u8>,
    pub debug_line: Vec<u8>,
    pub rela_debug_info: Vec<Relocation>,
    pub rela_debug_line: Vec<Relocation>,
}

pub struct Elf64Writer {
    text_section: Vec<u8>,
    rodata_section: Vec<u8>,
    relocations: Vec<Relocation>,
    rodata_relocations: Vec<Relocation>,
    functions: Vec<FunctionSymbol>,
    rodata_symbols: Vec<RodataSymbol>,
    debug_sections: Option<DebugSections>,
}

impl Elf64Writer {
    pub fn new(
        text_section: Vec<u8>,
        rodata_section: Vec<u8>,
        relocations: Vec<Relocation>,
        functions: Vec<FunctionSymbol>,
        rodata_symbols: Vec<RodataSymbol>,
    ) -> Self {
        Self {
            text_section,
            rodata_section,
            relocations,
            rodata_relocations: Vec::new(),
            functions,
            rodata_symbols,
            debug_sections: None,
        }
    }

    pub fn with_rodata_relocations(mut self, rodata_relocations: Vec<Relocation>) -> Self {
        self.rodata_relocations = rodata_relocations;
        self
    }

    pub fn with_debug(mut self, debug_sections: Option<DebugSections>) -> Self {
        self.debug_sections = debug_sections;
        self
    }

    pub fn write(self) -> Vec<u8> {
        let mut out = Vec::new();

        // Section names for .shstrtab
        let mut shstrtab = vec![0u8];
        let mut add_shstr = |name: &str| -> u32 {
            let offset = shstrtab.len() as u32;
            shstrtab.extend_from_slice(name.as_bytes());
            shstrtab.push(0);
            offset
        };

        let sh_null_name = 0;
        let sh_text_name = add_shstr(".text");
        let sh_rodata_name = add_shstr(".rodata");
        let sh_rela_text_name = add_shstr(".rela.text");
        let sh_rela_rodata_name = add_shstr(".rela.rodata");
        let sh_debug_info_name = add_shstr(".debug_info");
        let sh_rela_debug_info_name = add_shstr(".rela.debug_info");
        let sh_debug_abbrev_name = add_shstr(".debug_abbrev");
        let sh_debug_line_name = add_shstr(".debug_line");
        let sh_rela_debug_line_name = add_shstr(".rela.debug_line");
        let sh_symtab_name = add_shstr(".symtab");
        let sh_strtab_name = add_shstr(".strtab");
        let sh_shstrtab_name = add_shstr(".shstrtab");

        // String table for symbols (.strtab)
        let mut strtab = vec![0u8];
        let mut add_str = |name: &str| -> u32 {
            let offset = strtab.len() as u32;
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
            offset
        };

        // Determine section indices
        let has_rela_text = !self.relocations.is_empty();
        let sec_text = 1u32;
        let sec_rodata = 2u32;
        let mut next_sec = 3u32;

        let sec_rela_text = if has_rela_text {
            let s = next_sec;
            next_sec += 1;
            Some(s)
        } else {
            None
        };

        let has_rela_rodata = !self.rodata_relocations.is_empty();
        let sec_rela_rodata = if has_rela_rodata {
            let s = next_sec;
            next_sec += 1;
            Some(s)
        } else {
            None
        };

        let (
            sec_debug_info,
            sec_rela_debug_info,
            sec_debug_abbrev,
            sec_debug_line,
            sec_rela_debug_line,
        ) = if let Some(ref dbg) = self.debug_sections {
            let s_info = next_sec;
            next_sec += 1;

            let s_rela_info = if !dbg.rela_debug_info.is_empty() {
                let s = next_sec;
                next_sec += 1;
                Some(s)
            } else {
                None
            };

            let s_abbrev = next_sec;
            next_sec += 1;

            let s_line = next_sec;
            next_sec += 1;

            let s_rela_line = if !dbg.rela_debug_line.is_empty() {
                let s = next_sec;
                next_sec += 1;
                Some(s)
            } else {
                None
            };

            (
                Some(s_info),
                s_rela_info,
                Some(s_abbrev),
                Some(s_line),
                s_rela_line,
            )
        } else {
            (None, None, None, None, None)
        };

        let sec_symtab = next_sec;
        next_sec += 1;
        let sec_strtab = next_sec;
        next_sec += 1;
        let sec_shstrtab = next_sec;
        next_sec += 1;

        let num_sections = next_sec as usize;
        let shstr_idx = sec_shstrtab as u16;

        // Construct symbols (.symtab)
        let mut symtab_entries = Vec::new();
        let mut sym_indices: HashMap<String, u32> = HashMap::new();

        // Symbol 0: NULL symbol (STB_LOCAL)
        symtab_entries.push(ElfSym {
            st_name: 0,
            st_info: 0,
            st_other: 0,
            st_shndx: 0,
            st_value: 0,
            st_size: 0,
        });

        // Symbol 1: section symbol for .text (STT_SECTION, STB_LOCAL)
        symtab_entries.push(ElfSym {
            st_name: 0,
            st_info: 0x03, // (STB_LOCAL << 4) | STT_SECTION
            st_other: 0,
            st_shndx: sec_text as u16,
            st_value: 0,
            st_size: 0,
        });
        sym_indices.insert(".text".to_string(), 1);

        // Symbol 2: section symbol for .rodata (STT_SECTION, STB_LOCAL)
        symtab_entries.push(ElfSym {
            st_name: 0,
            st_info: 0x03, // (STB_LOCAL << 4) | STT_SECTION
            st_other: 0,
            st_shndx: sec_rodata as u16,
            st_value: 0,
            st_size: 0,
        });
        sym_indices.insert(".rodata".to_string(), 2);

        // Section symbols for debug sections (if present)
        if let Some(s_info) = sec_debug_info {
            let sym_idx = symtab_entries.len() as u32;
            symtab_entries.push(ElfSym {
                st_name: 0,
                st_info: 0x03,
                st_other: 0,
                st_shndx: s_info as u16,
                st_value: 0,
                st_size: 0,
            });
            sym_indices.insert(".debug_info".to_string(), sym_idx);
        }

        if let Some(s_abbrev) = sec_debug_abbrev {
            let sym_idx = symtab_entries.len() as u32;
            symtab_entries.push(ElfSym {
                st_name: 0,
                st_info: 0x03,
                st_other: 0,
                st_shndx: s_abbrev as u16,
                st_value: 0,
                st_size: 0,
            });
            sym_indices.insert(".debug_abbrev".to_string(), sym_idx);
        }

        if let Some(s_line) = sec_debug_line {
            let sym_idx = symtab_entries.len() as u32;
            symtab_entries.push(ElfSym {
                st_name: 0,
                st_info: 0x03,
                st_other: 0,
                st_shndx: s_line as u16,
                st_value: 0,
                st_size: 0,
            });
            sym_indices.insert(".debug_line".to_string(), sym_idx);
        }

        // Local symbols for .rodata string literals (STB_LOCAL, STT_OBJECT)
        for sym in &self.rodata_symbols {
            let st_name = add_str(&sym.name);
            let sym_idx = symtab_entries.len() as u32;
            symtab_entries.push(ElfSym {
                st_name,
                st_info: 0x01, // (STB_LOCAL << 4) | STT_OBJECT
                st_other: 0,
                st_shndx: sec_rodata as u16,
                st_value: sym.offset as u64,
                st_size: sym.size as u64,
            });
            sym_indices.insert(sym.name.clone(), sym_idx);
        }

        // Local function symbols (if any)
        for func in &self.functions {
            if !func.is_global {
                let st_name = add_str(&func.name);
                let sym_idx = symtab_entries.len() as u32;
                symtab_entries.push(ElfSym {
                    st_name,
                    st_info: 0x02, // (STB_LOCAL << 4) | STT_FUNC
                    st_other: 0,
                    st_shndx: sec_text as u16,
                    st_value: func.offset as u64,
                    st_size: func.size as u64,
                });
                sym_indices.insert(func.name.clone(), sym_idx);
            }
        }

        // Index of the first global / non-local symbol for sh_info of .symtab
        let first_non_local_sym_idx = symtab_entries.len() as u32;

        // Global function symbols (STB_GLOBAL, STT_FUNC)
        for func in &self.functions {
            if func.is_global {
                let st_name = add_str(&func.name);
                let sym_idx = symtab_entries.len() as u32;
                symtab_entries.push(ElfSym {
                    st_name,
                    st_info: 0x12, // (STB_GLOBAL << 4) | STT_FUNC
                    st_other: 0,
                    st_shndx: sec_text as u16,
                    st_value: func.offset as u64,
                    st_size: func.size as u64,
                });
                sym_indices.insert(func.name.clone(), sym_idx);
            }
        }

        // External / undefined symbols referenced in text relocations
        for rel in &self.relocations {
            if !sym_indices.contains_key(&rel.symbol) {
                let st_name = add_str(&rel.symbol);
                let sym_idx = symtab_entries.len() as u32;
                symtab_entries.push(ElfSym {
                    st_name,
                    st_info: 0x10, // (STB_GLOBAL << 4) | STT_NOTYPE
                    st_other: 0,
                    st_shndx: 0, // SHN_UNDEF
                    st_value: 0,
                    st_size: 0,
                });
                sym_indices.insert(rel.symbol.clone(), sym_idx);
            }
        }

        // External / undefined symbols referenced in rodata relocations
        for rel in &self.rodata_relocations {
            if !sym_indices.contains_key(&rel.symbol) {
                let st_name = add_str(&rel.symbol);
                let sym_idx = symtab_entries.len() as u32;
                symtab_entries.push(ElfSym {
                    st_name,
                    st_info: 0x10, // (STB_GLOBAL << 4) | STT_NOTYPE
                    st_other: 0,
                    st_shndx: 0, // SHN_UNDEF
                    st_value: 0,
                    st_size: 0,
                });
                sym_indices.insert(rel.symbol.clone(), sym_idx);
            }
        }

        // Build .rela.text entries
        let mut rela_text_entries = Vec::new();
        for rel in &self.relocations {
            let sym_idx = *sym_indices.get(&rel.symbol).unwrap_or_else(|| {
                panic!(
                    "Relocation symbol '{}' must exist in symbol table",
                    rel.symbol
                );
            });
            let r_info = ((sym_idx as u64) << 32) | (rel.r_type as u64);
            rela_text_entries.push(ElfRela {
                r_offset: rel.offset as u64,
                r_info,
                r_addend: rel.addend,
            });
        }

        // Build .rela.rodata entries
        let mut rela_rodata_entries = Vec::new();
        for rel in &self.rodata_relocations {
            let sym_idx = *sym_indices.get(&rel.symbol).unwrap_or_else(|| {
                panic!(
                    "Rodata relocation symbol '{}' must exist in symbol table",
                    rel.symbol
                );
            });
            let r_info = ((sym_idx as u64) << 32) | (rel.r_type as u64);
            rela_rodata_entries.push(ElfRela {
                r_offset: rel.offset as u64,
                r_info,
                r_addend: rel.addend,
            });
        }

        // Build .rela.debug_info entries
        let mut rela_debug_info_entries = Vec::new();
        if let Some(ref dbg) = self.debug_sections {
            for rel in &dbg.rela_debug_info {
                let sym_idx = *sym_indices.get(&rel.symbol).unwrap_or_else(|| {
                    panic!("Debug info relocation symbol '{}' must exist", rel.symbol);
                });
                let r_info = ((sym_idx as u64) << 32) | (rel.r_type as u64);
                rela_debug_info_entries.push(ElfRela {
                    r_offset: rel.offset as u64,
                    r_info,
                    r_addend: rel.addend,
                });
            }
        }

        // Build .rela.debug_line entries
        let mut rela_debug_line_entries = Vec::new();
        if let Some(ref dbg) = self.debug_sections {
            for rel in &dbg.rela_debug_line {
                let sym_idx = *sym_indices.get(&rel.symbol).unwrap_or_else(|| {
                    panic!("Debug line relocation symbol '{}' must exist", rel.symbol);
                });
                let r_info = ((sym_idx as u64) << 32) | (rel.r_type as u64);
                rela_debug_line_entries.push(ElfRela {
                    r_offset: rel.offset as u64,
                    r_info,
                    r_addend: rel.addend,
                });
            }
        }

        // Layout offsets in the ELF file
        let header_size = 64usize;
        let mut current_offset = header_size;

        // .text (align 16)
        current_offset = (current_offset + 15) & !15;
        let text_offset = current_offset;
        let text_size = self.text_section.len();
        current_offset += text_size;

        // .rodata (align 16)
        current_offset = (current_offset + 15) & !15;
        let rodata_offset = current_offset;
        let rodata_size = self.rodata_section.len();
        current_offset += rodata_size;

        // .rela.text (align 8)
        let (rela_text_offset, rela_text_size) = if sec_rela_text.is_some() {
            current_offset = (current_offset + 7) & !7;
            let off = current_offset;
            let sz = rela_text_entries.len() * 24;
            current_offset += sz;
            (off, sz)
        } else {
            (0, 0)
        };

        // .rela.rodata (align 8)
        let (rela_rodata_offset, rela_rodata_size) = if sec_rela_rodata.is_some() {
            current_offset = (current_offset + 7) & !7;
            let off = current_offset;
            let sz = rela_rodata_entries.len() * 24;
            current_offset += sz;
            (off, sz)
        } else {
            (0, 0)
        };

        // Debug sections layout
        let (
            dbg_info_offset,
            dbg_info_size,
            rela_dbg_info_offset,
            rela_dbg_info_size,
            dbg_abbrev_offset,
            dbg_abbrev_size,
            dbg_line_offset,
            dbg_line_size,
            rela_dbg_line_offset,
            rela_dbg_line_size,
        ) = if let Some(ref dbg) = self.debug_sections {
            // .debug_info (align 1)
            let i_off = current_offset;
            let i_sz = dbg.debug_info.len();
            current_offset += i_sz;

            // .rela.debug_info (align 8)
            let (ri_off, ri_sz) = if sec_rela_debug_info.is_some() {
                current_offset = (current_offset + 7) & !7;
                let off = current_offset;
                let sz = rela_debug_info_entries.len() * 24;
                current_offset += sz;
                (off, sz)
            } else {
                (0, 0)
            };

            // .debug_abbrev (align 1)
            let a_off = current_offset;
            let a_sz = dbg.debug_abbrev.len();
            current_offset += a_sz;

            // .debug_line (align 1)
            let l_off = current_offset;
            let l_sz = dbg.debug_line.len();
            current_offset += l_sz;

            // .rela.debug_line (align 8)
            let (rl_off, rl_sz) = if sec_rela_debug_line.is_some() {
                current_offset = (current_offset + 7) & !7;
                let off = current_offset;
                let sz = rela_debug_line_entries.len() * 24;
                current_offset += sz;
                (off, sz)
            } else {
                (0, 0)
            };

            (
                i_off, i_sz, ri_off, ri_sz, a_off, a_sz, l_off, l_sz, rl_off, rl_sz,
            )
        } else {
            (0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
        };

        // .symtab (align 8)
        current_offset = (current_offset + 7) & !7;
        let symtab_offset = current_offset;
        let symtab_size = symtab_entries.len() * 24;
        current_offset += symtab_size;

        // .strtab (align 1)
        let strtab_offset = current_offset;
        let strtab_size = strtab.len();
        current_offset += strtab_size;

        // .shstrtab (align 1)
        let shstrtab_offset = current_offset;
        let shstrtab_size = shstrtab.len();
        current_offset += shstrtab_size;

        // Section Header Table (align 8)
        current_offset = (current_offset + 7) & !7;
        let sh_offset = current_offset;

        // --- Write ELF Header (64 bytes) ---
        out.extend_from_slice(&[0x7F, b'E', b'L', b'F']); // magic
        out.push(2); // 64-bit
        out.push(1); // little-endian
        out.push(1); // version 1
        out.push(0); // System V ABI
        out.extend_from_slice(&[0; 8]); // padding

        out.extend_from_slice(&1u16.to_le_bytes()); // e_type = ET_REL (1)
        out.extend_from_slice(&62u16.to_le_bytes()); // e_machine = EM_X86_64 (62)
        out.extend_from_slice(&1u32.to_le_bytes()); // e_version = 1
        out.extend_from_slice(&0u64.to_le_bytes()); // e_entry = 0
        out.extend_from_slice(&0u64.to_le_bytes()); // e_phoff = 0
        out.extend_from_slice(&(sh_offset as u64).to_le_bytes()); // e_shoff
        out.extend_from_slice(&0u32.to_le_bytes()); // e_flags = 0
        out.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize = 64
        out.extend_from_slice(&0u16.to_le_bytes()); // e_phentsize = 0
        out.extend_from_slice(&0u16.to_le_bytes()); // e_phnum = 0
        out.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize = 64
        out.extend_from_slice(&(num_sections as u16).to_le_bytes()); // e_shnum
        out.extend_from_slice(&shstr_idx.to_le_bytes()); // e_shstrndx

        // Pad to .text
        while out.len() < text_offset {
            out.push(0);
        }
        out.extend_from_slice(&self.text_section);

        // Pad to .rodata
        while out.len() < rodata_offset {
            out.push(0);
        }
        out.extend_from_slice(&self.rodata_section);

        // .rela.text
        if sec_rela_text.is_some() {
            while out.len() < rela_text_offset {
                out.push(0);
            }
            for rela in &rela_text_entries {
                out.extend_from_slice(&rela.r_offset.to_le_bytes());
                out.extend_from_slice(&rela.r_info.to_le_bytes());
                out.extend_from_slice(&rela.r_addend.to_le_bytes());
            }
        }

        // .rela.rodata
        if sec_rela_rodata.is_some() {
            while out.len() < rela_rodata_offset {
                out.push(0);
            }
            for rela in &rela_rodata_entries {
                out.extend_from_slice(&rela.r_offset.to_le_bytes());
                out.extend_from_slice(&rela.r_info.to_le_bytes());
                out.extend_from_slice(&rela.r_addend.to_le_bytes());
            }
        }

        // Debug sections
        if let Some(ref dbg) = self.debug_sections {
            // .debug_info
            while out.len() < dbg_info_offset {
                out.push(0);
            }
            out.extend_from_slice(&dbg.debug_info);

            // .rela.debug_info
            if sec_rela_debug_info.is_some() {
                while out.len() < rela_dbg_info_offset {
                    out.push(0);
                }
                for rela in &rela_debug_info_entries {
                    out.extend_from_slice(&rela.r_offset.to_le_bytes());
                    out.extend_from_slice(&rela.r_info.to_le_bytes());
                    out.extend_from_slice(&rela.r_addend.to_le_bytes());
                }
            }

            // .debug_abbrev
            while out.len() < dbg_abbrev_offset {
                out.push(0);
            }
            out.extend_from_slice(&dbg.debug_abbrev);

            // .debug_line
            while out.len() < dbg_line_offset {
                out.push(0);
            }
            out.extend_from_slice(&dbg.debug_line);

            // .rela.debug_line
            if sec_rela_debug_line.is_some() {
                while out.len() < rela_dbg_line_offset {
                    out.push(0);
                }
                for rela in &rela_debug_line_entries {
                    out.extend_from_slice(&rela.r_offset.to_le_bytes());
                    out.extend_from_slice(&rela.r_info.to_le_bytes());
                    out.extend_from_slice(&rela.r_addend.to_le_bytes());
                }
            }
        }

        // .symtab
        while out.len() < symtab_offset {
            out.push(0);
        }
        for sym in &symtab_entries {
            out.extend_from_slice(&sym.st_name.to_le_bytes());
            out.push(sym.st_info);
            out.push(sym.st_other);
            out.extend_from_slice(&sym.st_shndx.to_le_bytes());
            out.extend_from_slice(&sym.st_value.to_le_bytes());
            out.extend_from_slice(&sym.st_size.to_le_bytes());
        }

        // .strtab
        while out.len() < strtab_offset {
            out.push(0);
        }
        out.extend_from_slice(&strtab);

        // .shstrtab
        while out.len() < shstrtab_offset {
            out.push(0);
        }
        out.extend_from_slice(&shstrtab);

        // --- Section Header Table ---
        while out.len() < sh_offset {
            out.push(0);
        }

        let mut write_shdr = |name: u32,
                              ty: u32,
                              flags: u64,
                              off: u64,
                              size: u64,
                              link: u32,
                              info: u32,
                              align: u64,
                              entsize: u64| {
            out.extend_from_slice(&name.to_le_bytes());
            out.extend_from_slice(&ty.to_le_bytes());
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&0u64.to_le_bytes()); // sh_addr = 0
            out.extend_from_slice(&off.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&link.to_le_bytes());
            out.extend_from_slice(&info.to_le_bytes());
            out.extend_from_slice(&align.to_le_bytes());
            out.extend_from_slice(&entsize.to_le_bytes());
        };

        // Section 0: NULL
        write_shdr(sh_null_name, 0, 0, 0, 0, 0, 0, 0, 0);

        // Section 1: .text (SHT_PROGBITS = 1, SHF_ALLOC | SHF_EXECINSTR = 6, align 16)
        write_shdr(
            sh_text_name,
            1,
            0x06,
            text_offset as u64,
            text_size as u64,
            0,
            0,
            16,
            0,
        );

        // Section 2: .rodata (SHT_PROGBITS = 1, SHF_ALLOC = 2, align 16)
        write_shdr(
            sh_rodata_name,
            1,
            0x02,
            rodata_offset as u64,
            rodata_size as u64,
            0,
            0,
            16,
            0,
        );

        // Section 3 (optional): .rela.text (SHT_RELA = 4, SHF_INFO_LINK = 0x40, align 8)
        if sec_rela_text.is_some() {
            write_shdr(
                sh_rela_text_name,
                4,
                0x40,
                rela_text_offset as u64,
                rela_text_size as u64,
                sec_symtab, // sh_link: .symtab index
                sec_text,   // sh_info: target section (.text)
                8,
                24,
            );
        }

        // Section (optional): .rela.rodata (SHT_RELA = 4, SHF_INFO_LINK = 0x40, align 8)
        if sec_rela_rodata.is_some() {
            write_shdr(
                sh_rela_rodata_name,
                4,
                0x40,
                rela_rodata_offset as u64,
                rela_rodata_size as u64,
                sec_symtab, // sh_link: .symtab index
                sec_rodata, // sh_info: target section (.rodata)
                8,
                24,
            );
        }

        // Debug sections
        if self.debug_sections.is_some() {
            // .debug_info (SHT_PROGBITS = 1, flags = 0, align 1)
            write_shdr(
                sh_debug_info_name,
                1,
                0,
                dbg_info_offset as u64,
                dbg_info_size as u64,
                0,
                0,
                1,
                0,
            );

            // .rela.debug_info (SHT_RELA = 4, flags = 0x40, align 8)
            if sec_rela_debug_info.is_some() {
                write_shdr(
                    sh_rela_debug_info_name,
                    4,
                    0x40,
                    rela_dbg_info_offset as u64,
                    rela_dbg_info_size as u64,
                    sec_symtab,              // sh_link: .symtab
                    sec_debug_info.unwrap(), // sh_info: target section (.debug_info)
                    8,
                    24,
                );
            }

            // .debug_abbrev (SHT_PROGBITS = 1, flags = 0, align 1)
            write_shdr(
                sh_debug_abbrev_name,
                1,
                0,
                dbg_abbrev_offset as u64,
                dbg_abbrev_size as u64,
                0,
                0,
                1,
                0,
            );

            // .debug_line (SHT_PROGBITS = 1, flags = 0, align 1)
            write_shdr(
                sh_debug_line_name,
                1,
                0,
                dbg_line_offset as u64,
                dbg_line_size as u64,
                0,
                0,
                1,
                0,
            );

            // .rela.debug_line (SHT_RELA = 4, flags = 0x40, align 8)
            if sec_rela_debug_line.is_some() {
                write_shdr(
                    sh_rela_debug_line_name,
                    4,
                    0x40,
                    rela_dbg_line_offset as u64,
                    rela_dbg_line_size as u64,
                    sec_symtab,              // sh_link: .symtab
                    sec_debug_line.unwrap(), // sh_info: target section (.debug_line)
                    8,
                    24,
                );
            }
        }

        // .symtab (SHT_SYMTAB = 2, align 8)
        write_shdr(
            sh_symtab_name,
            2,
            0,
            symtab_offset as u64,
            symtab_size as u64,
            sec_strtab,              // sh_link: .strtab index
            first_non_local_sym_idx, // sh_info: first non-local symbol index
            8,
            24,
        );

        // .strtab (SHT_STRTAB = 3, align 1)
        write_shdr(
            sh_strtab_name,
            3,
            0,
            strtab_offset as u64,
            strtab_size as u64,
            0,
            0,
            1,
            0,
        );

        // .shstrtab (SHT_STRTAB = 3, align 1)
        write_shdr(
            sh_shstrtab_name,
            3,
            0,
            shstrtab_offset as u64,
            shstrtab_size as u64,
            0,
            0,
            1,
            0,
        );

        out
    }
}

struct ElfSym {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
}

struct ElfRela {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
}
