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

pub struct Elf64Writer {
    text_section: Vec<u8>,
    rodata_section: Vec<u8>,
    relocations: Vec<Relocation>,
    functions: Vec<FunctionSymbol>,
    rodata_symbols: Vec<RodataSymbol>,
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
            functions,
            rodata_symbols,
        }
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

        // Construct symbols (.symtab)
        let mut symtab_entries = Vec::new();
        let mut sym_indices: HashMap<String, u32> = HashMap::new();

        // Symbol 0: always undefined NULL symbol (STB_LOCAL)
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
            st_shndx: 1, // .text section index
            st_value: 0,
            st_size: 0,
        });

        // Symbol 2: section symbol for .rodata (STT_SECTION, STB_LOCAL)
        symtab_entries.push(ElfSym {
            st_name: 0,
            st_info: 0x03, // (STB_LOCAL << 4) | STT_SECTION
            st_other: 0,
            st_shndx: 2, // .rodata section index
            st_value: 0,
            st_size: 0,
        });

        // Local symbols for .rodata string literals (STB_LOCAL, STT_OBJECT)
        for sym in &self.rodata_symbols {
            let st_name = add_str(&sym.name);
            let sym_idx = symtab_entries.len() as u32;
            symtab_entries.push(ElfSym {
                st_name,
                st_info: 0x01, // (STB_LOCAL << 4) | STT_OBJECT
                st_other: 0,
                st_shndx: 2, // .rodata section index
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
                    st_shndx: 1, // .text section index
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
                    st_shndx: 1, // .text section index
                    st_value: func.offset as u64,
                    st_size: func.size as u64,
                });
                sym_indices.insert(func.name.clone(), sym_idx);
            }
        }

        // Undefined / external symbols referenced in relocations (STB_GLOBAL, STT_NOTYPE)
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

        // Relocations (.rela.text)
        let mut rela_entries = Vec::new();
        for rel in &self.relocations {
            let sym_idx = *sym_indices.get(&rel.symbol).unwrap_or_else(|| {
                panic!(
                    "Relocation symbol '{}' must exist in symbol table",
                    rel.symbol
                );
            });
            let r_info = ((sym_idx as u64) << 32) | (rel.r_type as u64);
            rela_entries.push(ElfRela {
                r_offset: rel.offset as u64,
                r_info,
                r_addend: rel.addend,
            });
        }

        // Layout the ELF file sections:
        // 0: Header (64 bytes)
        // 1: .text
        // 2: .rodata
        // 3: .rela.text (if any relocations)
        // 4: .symtab
        // 5: .strtab
        // 6: .shstrtab
        // 7: Section Header Table

        let has_rela = !rela_entries.is_empty();
        let num_sections = if has_rela { 7 } else { 6 };
        let shstr_idx = if has_rela { 6u16 } else { 5u16 };

        let header_size = 64usize;
        let mut current_offset = header_size;

        // .text section (align 16)
        current_offset = (current_offset + 15) & !15;
        let text_offset = current_offset;
        let text_size = self.text_section.len();
        current_offset += text_size;

        // .rodata section (align 16)
        current_offset = (current_offset + 15) & !15;
        let rodata_offset = current_offset;
        let rodata_size = self.rodata_section.len();
        current_offset += rodata_size;

        // .rela.text section (align 8)
        let (rela_offset, rela_size) = if has_rela {
            current_offset = (current_offset + 7) & !7;
            let off = current_offset;
            let sz = rela_entries.len() * 24;
            current_offset += sz;
            (off, sz)
        } else {
            (0, 0)
        };

        // .symtab section (align 8)
        current_offset = (current_offset + 7) & !7;
        let symtab_offset = current_offset;
        let symtab_size = symtab_entries.len() * 24;
        current_offset += symtab_size;

        // .strtab section (align 1)
        let strtab_offset = current_offset;
        let strtab_size = strtab.len();
        current_offset += strtab_size;

        // .shstrtab section (align 1)
        let shstrtab_offset = current_offset;
        let shstrtab_size = shstrtab.len();
        current_offset += shstrtab_size;

        // Section header table (align 8)
        current_offset = (current_offset + 7) & !7;
        let sh_offset = current_offset;

        // Write ELF Header
        // e_ident
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
        if has_rela {
            while out.len() < rela_offset {
                out.push(0);
            }
            for rela in &rela_entries {
                out.extend_from_slice(&rela.r_offset.to_le_bytes());
                out.extend_from_slice(&rela.r_info.to_le_bytes());
                out.extend_from_slice(&rela.r_addend.to_le_bytes());
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

        // Section Header Table
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

        // 0: NULL section
        write_shdr(sh_null_name, 0, 0, 0, 0, 0, 0, 0, 0);

        // 1: .text section (SHT_PROGBITS = 1, SHF_ALLOC | SHF_EXECINSTR = 6)
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

        // 2: .rodata section (SHT_PROGBITS = 1, SHF_ALLOC = 2)
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

        // 3: .rela.text section (if present) (SHT_RELA = 4, SHF_INFO_LINK = 0x40)
        let (_symtab_sec_idx, strtab_sec_idx) = if has_rela {
            write_shdr(
                sh_rela_text_name,
                4,
                0x40,
                rela_offset as u64,
                rela_size as u64,
                4, // sh_link: symbol table index (.symtab is 4)
                1, // sh_info: target section index (.text is 1)
                8,
                24,
            );
            (4, 5)
        } else {
            (3, 4)
        };

        // .symtab (SHT_SYMTAB = 2)
        write_shdr(
            sh_symtab_name,
            2,
            0,
            symtab_offset as u64,
            symtab_size as u64,
            strtab_sec_idx,          // sh_link: .strtab index
            first_non_local_sym_idx, // sh_info: first non-local symbol index
            8,
            24,
        );

        // .strtab (SHT_STRTAB = 3)
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

        // .shstrtab (SHT_STRTAB = 3)
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
