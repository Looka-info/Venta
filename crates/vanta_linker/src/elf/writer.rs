//! Pure-Rust ELF64 Static Executable (ET_EXEC) binary writer.

use crate::section::MergedSection;

pub const ET_EXEC: u16 = 2;
pub const EM_X86_64: u16 = 62;

pub const PT_LOAD: u32 = 1;
pub const PT_GNU_STACK: u32 = 0x6474e551;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_NOBITS: u32 = 8;

pub const SHF_WRITE: u64 = 0x1;
pub const SHF_ALLOC: u64 = 0x2;
pub const SHF_EXECINSTR: u64 = 0x4;

pub struct ElfExecutableWriter;

impl ElfExecutableWriter {
    /// Writes a complete, loadable Linux ELF64 static executable image.
    pub fn write_executable(
        entry_point_va: u64,
        merged_sections: &[MergedSection],
        global_symbols: &[(String, u64, u64)], // (name, address, size)
    ) -> Vec<u8> {
        let mut out = Vec::new();

        // 1. Identify segments:
        // We generate:
        // - Load Segment 1: RX (Headers + .text)
        // - Load Segment 2: R (.rodata)
        // - Load Segment 3: RW (.data + .bss)
        // - PT_GNU_STACK: RW (non-exec)
        let text_sec = merged_sections.iter().find(|s| s.name == ".text");
        let rodata_sec = merged_sections.iter().find(|s| s.name == ".rodata");
        let data_sec = merged_sections.iter().find(|s| s.name == ".data");
        let bss_sec = merged_sections.iter().find(|s| s.name == ".bss");

        let mut num_phdrs = 1; // at least 1 PT_LOAD
        if rodata_sec.is_some() {
            num_phdrs += 1;
        }
        if data_sec.is_some() || bss_sec.is_some() {
            num_phdrs += 1;
        }
        num_phdrs += 1; // PT_GNU_STACK

        let phdr_size = 56;
        let ehdr_size = 64;
        let header_bytes = ehdr_size + num_phdrs * phdr_size;

        // Base VA: text section VA minus file_offset
        let base_va = if let Some(text) = text_sec {
            text.virtual_address.saturating_sub(text.file_offset)
        } else {
            0x400000
        };

        // Write ELF Header (64 bytes)
        out.extend_from_slice(&[
            0x7F, b'E', b'L', b'F', // e_ident[EI_MAG0..3]
            2,    // EI_CLASS = ELFCLASS64
            1,    // EI_DATA = ELFDATA2LSB
            1,    // EI_VERSION = EV_CURRENT
            0,    // EI_OSABI = ELFOSABI_NONE / SYSV
            0,    // EI_ABIVERSION
            0, 0, 0, 0, 0, 0, 0, // padding
        ]);
        out.extend_from_slice(&ET_EXEC.to_le_bytes()); // e_type
        out.extend_from_slice(&EM_X86_64.to_le_bytes()); // e_machine
        out.extend_from_slice(&1u32.to_le_bytes()); // e_version
        out.extend_from_slice(&entry_point_va.to_le_bytes()); // e_entry
        out.extend_from_slice(&(ehdr_size as u64).to_le_bytes()); // e_phoff
        let shoff_pos = out.len();
        out.extend_from_slice(&0u64.to_le_bytes()); // e_shoff (patched later)
        out.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        out.extend_from_slice(&(ehdr_size as u16).to_le_bytes()); // e_ehsize
        out.extend_from_slice(&(phdr_size as u16).to_le_bytes()); // e_phentsize
        out.extend_from_slice(&(num_phdrs as u16).to_le_bytes()); // e_phnum
        out.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize
        let shnum_pos = out.len();
        out.extend_from_slice(&0u16.to_le_bytes()); // e_shnum (patched later)
        let shstrndx_pos = out.len();
        out.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx (patched later)

        // Write Program Headers (56 bytes each)
        // 1. PT_LOAD for Text (RX)
        let text_filesz = if let Some(text) = text_sec {
            text.file_offset + text.data.len() as u64
        } else {
            header_bytes as u64
        };
        write_phdr(
            &mut out,
            PT_LOAD,
            PF_R | PF_X,
            0,
            base_va,
            base_va,
            text_filesz,
            text_filesz,
            4096,
        );

        // 2. PT_LOAD for Rodata (R)
        if let Some(rodata) = rodata_sec {
            let filesz = rodata.data.len() as u64;
            write_phdr(
                &mut out,
                PT_LOAD,
                PF_R,
                rodata.file_offset,
                rodata.virtual_address,
                rodata.virtual_address,
                filesz,
                filesz,
                4096,
            );
        }

        // 3. PT_LOAD for Data + BSS (RW)
        if data_sec.is_some() || bss_sec.is_some() {
            let file_off = data_sec
                .map(|d| d.file_offset)
                .unwrap_or_else(|| bss_sec.unwrap().file_offset);
            let vaddr = data_sec
                .map(|d| d.virtual_address)
                .unwrap_or_else(|| bss_sec.unwrap().virtual_address);
            let filesz = data_sec.map(|d| d.data.len() as u64).unwrap_or(0);
            let memsz = filesz + bss_sec.map(|b| b.size as u64).unwrap_or(0);
            write_phdr(
                &mut out,
                PT_LOAD,
                PF_R | PF_W,
                file_off,
                vaddr,
                vaddr,
                filesz,
                memsz,
                4096,
            );
        }

        // 4. PT_GNU_STACK (RW, non-executable stack)
        write_phdr(&mut out, PT_GNU_STACK, PF_R | PF_W, 0, 0, 0, 0, 0, 16);

        // Write section data onto their file offsets
        for sec in merged_sections {
            if sec.kind.is_zero_init() {
                continue;
            }
            if out.len() < sec.file_offset as usize {
                out.resize(sec.file_offset as usize, 0);
            }
            out.extend_from_slice(&sec.data);
        }

        // Section Headers & String Tables
        // Build .shstrtab
        let mut shstrtab = vec![0u8];
        let mut add_shstr = |name: &str| -> u32 {
            let off = shstrtab.len() as u32;
            shstrtab.extend_from_slice(name.as_bytes());
            shstrtab.push(0);
            off
        };

        // Build .strtab
        let mut strtab = vec![0u8];
        let mut add_str = |name: &str| -> u32 {
            let off = strtab.len() as u32;
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);
            off
        };

        // Build .symtab entries
        let mut symtab_bytes = Vec::new();
        // NULL symbol (index 0)
        symtab_bytes.resize(24, 0);

        for (sym_name, sym_addr, sym_size) in global_symbols {
            let st_name = add_str(sym_name);
            symtab_bytes.extend_from_slice(&st_name.to_le_bytes());
            symtab_bytes.push(0x12); // STB_GLOBAL | STT_FUNC (or STT_OBJECT)
            symtab_bytes.push(0); // st_other
            symtab_bytes.extend_from_slice(&1u16.to_le_bytes()); // st_shndx (1: .text)
            symtab_bytes.extend_from_slice(&sym_addr.to_le_bytes()); // st_value
            symtab_bytes.extend_from_slice(&sym_size.to_le_bytes()); // st_size
        }

        // Append tables to file
        let symtab_file_off = out.len() as u64;
        out.extend_from_slice(&symtab_bytes);

        let strtab_file_off = out.len() as u64;
        out.extend_from_slice(&strtab);

        // Name strings for all sections
        let mut section_header_data = Vec::new();
        // 0. NULL Section
        section_header_data.resize(64, 0);

        for sec in merged_sections {
            let name_off = add_shstr(&sec.name);
            let sh_type = if sec.kind.is_zero_init() {
                SHT_NOBITS
            } else {
                SHT_PROGBITS
            };
            let mut sh_flags = 0u64;
            if sec.flags.alloc {
                sh_flags |= SHF_ALLOC;
            }
            if sec.flags.write {
                sh_flags |= SHF_WRITE;
            }
            if sec.flags.execute {
                sh_flags |= SHF_EXECINSTR;
            }
            let size = if sec.kind.is_zero_init() {
                sec.size as u64
            } else {
                sec.data.len() as u64
            };

            write_shdr(
                &mut section_header_data,
                name_off,
                sh_type,
                sh_flags,
                sec.virtual_address,
                sec.file_offset,
                size,
                0,
                0,
                sec.align as u64,
                0,
            );
        }

        // Symtab section header
        let symtab_name_off = add_shstr(".symtab");
        let symtab_idx = 1 + merged_sections.len() as u32;
        let strtab_idx = symtab_idx + 1;
        let shstrtab_idx = symtab_idx + 2;

        write_shdr(
            &mut section_header_data,
            symtab_name_off,
            SHT_SYMTAB,
            0,
            0,
            symtab_file_off,
            symtab_bytes.len() as u64,
            strtab_idx,
            1, // first global symbol index
            8,
            24,
        );

        // Strtab section header
        let strtab_name_off = add_shstr(".strtab");
        write_shdr(
            &mut section_header_data,
            strtab_name_off,
            SHT_STRTAB,
            0,
            0,
            strtab_file_off,
            strtab.len() as u64,
            0,
            0,
            1,
            0,
        );

        // Shstrtab data
        let shstrtab_name_off = add_shstr(".shstrtab");
        let shstrtab_file_off = (out.len() + section_header_data.len() + 64) as u64; // right after section headers

        write_shdr(
            &mut section_header_data,
            shstrtab_name_off,
            SHT_STRTAB,
            0,
            0,
            shstrtab_file_off,
            shstrtab.len() as u64,
            0,
            0,
            1,
            0,
        );

        // Write Section Headers
        let shoff = out.len() as u64;
        out.extend_from_slice(&section_header_data);
        out.extend_from_slice(&shstrtab);

        let total_sections = 1 + merged_sections.len() + 3; // NULL + merged + symtab + strtab + shstrtab

        // Patch ELF header fields
        out[shoff_pos..shoff_pos + 8].copy_from_slice(&shoff.to_le_bytes());
        out[shnum_pos..shnum_pos + 2].copy_from_slice(&(total_sections as u16).to_le_bytes());
        out[shstrndx_pos..shstrndx_pos + 2].copy_from_slice(&(shstrtab_idx as u16).to_le_bytes());

        out
    }
}

fn write_phdr(
    out: &mut Vec<u8>,
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
) {
    out.extend_from_slice(&p_type.to_le_bytes());
    out.extend_from_slice(&p_flags.to_le_bytes());
    out.extend_from_slice(&p_offset.to_le_bytes());
    out.extend_from_slice(&p_vaddr.to_le_bytes());
    out.extend_from_slice(&p_paddr.to_le_bytes());
    out.extend_from_slice(&p_filesz.to_le_bytes());
    out.extend_from_slice(&p_memsz.to_le_bytes());
    out.extend_from_slice(&p_align.to_le_bytes());
}

fn write_shdr(
    out: &mut Vec<u8>,
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
    sh_offset: u64,
    sh_size: u64,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u64,
    sh_entsize: u64,
) {
    out.extend_from_slice(&sh_name.to_le_bytes());
    out.extend_from_slice(&sh_type.to_le_bytes());
    out.extend_from_slice(&sh_flags.to_le_bytes());
    out.extend_from_slice(&sh_addr.to_le_bytes());
    out.extend_from_slice(&sh_offset.to_le_bytes());
    out.extend_from_slice(&sh_size.to_le_bytes());
    out.extend_from_slice(&sh_link.to_le_bytes());
    out.extend_from_slice(&sh_info.to_le_bytes());
    out.extend_from_slice(&sh_addralign.to_le_bytes());
    out.extend_from_slice(&sh_entsize.to_le_bytes());
}
