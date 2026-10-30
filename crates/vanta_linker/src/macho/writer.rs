//! Pure-Rust Apple Silicon macOS Mach-O 64-bit Executable (`MH_EXECUTE`) Writer.

pub const MH_MAGIC_64: u32 = 0xFEEDFACF;
pub const CPU_TYPE_ARM64: u32 = 0x0100000C;
pub const CPU_SUBTYPE_ARM64_ALL: u32 = 0x00000000;
pub const MH_EXECUTE: u32 = 0x2;
pub const MH_FLAGS_EXEC: u32 = 0x00200085; // MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_PIE

pub const LC_SEGMENT_64: u32 = 0x19;
pub const LC_SYMTAB: u32 = 0x02;
pub const LC_LOAD_DYLINKER: u32 = 0x0E;
pub const LC_MAIN: u32 = 0x80000028;
pub const LC_LOAD_DYLIB: u32 = 0x0C;

pub const VM_PROT_NONE: u32 = 0;
pub const VM_PROT_READ: u32 = 1;
pub const VM_PROT_WRITE: u32 = 2;
pub const VM_PROT_EXECUTE: u32 = 4;

pub const S_ATTR_SOME_INSTRUCTIONS: u32 = 0x00000400;
pub const S_ATTR_PURE_INSTRUCTIONS: u32 = 0x80000000;
pub const S_ZEROFILL: u32 = 0x00000001;

pub const N_EXT: u8 = 0x01;
pub const N_SECT: u8 = 0x0E;

#[derive(Debug, Clone)]
pub struct MachOSectionDesc {
    pub segname: String,
    pub sectname: String,
    pub addr: u64,
    pub size: u64,
    pub data: Vec<u8>,
    pub is_zerofill: bool,
}

pub struct MachOExecutableWriter;

impl MachOExecutableWriter {
    pub const PAGE_SIZE: u64 = 0x4000; // 16KB standard Apple Silicon page size
    pub const PAGEZERO_SIZE: u64 = 0x100000000; // 4GB
    pub const BASE_TEXT_ADDR: u64 = 0x100000000;

    /// Emits a valid Apple Silicon macOS Mach-O executable image.
    pub fn write_executable(
        entry_offset_in_text: u64,
        text_sections: &[MachOSectionDesc],
        data_sections: &[MachOSectionDesc],
        symbols: &[(String, u64)], // (name, addr)
    ) -> Vec<u8> {
        let mut out = Vec::new();

        // 1. Calculate Load Commands layout
        // Commands:
        // 1. LC_SEGMENT_64 (__PAGEZERO) - 72 bytes
        // 2. LC_SEGMENT_64 (__TEXT) - 72 + num_text_sects * 80 bytes
        // 3. LC_SEGMENT_64 (__DATA) - 72 + num_data_sects * 80 bytes (if present)
        // 4. LC_SEGMENT_64 (__LINKEDIT) - 72 bytes
        // 5. LC_LOAD_DYLINKER (/usr/lib/dyld) - 32 bytes
        // 6. LC_MAIN - 24 bytes
        // 7. LC_LOAD_DYLIB (/usr/lib/libSystem.B.dylib) - 56 bytes
        // 8. LC_SYMTAB - 24 bytes
        let mut ncmds = 1 /* PAGEZERO */ + 1 /* TEXT */ + 1 /* LINKEDIT */ + 1 /* DYLINKER */ + 1 /* MAIN */ + 1 /* DYLIB */ + 1 /* SYMTAB */;
        if !data_sections.is_empty() {
            ncmds += 1;
        }

        let cmd_pagezero_size = 72;
        let cmd_text_size = 72 + text_sections.len() * 80;
        let cmd_data_size = if data_sections.is_empty() {
            0
        } else {
            72 + data_sections.len() * 80
        };
        let cmd_linkedit_size = 72;
        let cmd_dylinker_size = 32;
        let cmd_main_size = 24;
        let cmd_dylib_size = 56;
        let cmd_symtab_size = 24;

        let sizeofcmds = cmd_pagezero_size
            + cmd_text_size
            + cmd_data_size
            + cmd_linkedit_size
            + cmd_dylinker_size
            + cmd_main_size
            + cmd_dylib_size
            + cmd_symtab_size;

        let header_and_cmds_size = 32 + sizeofcmds;

        // Calculate __TEXT segment layout
        let text_filesize =
            header_and_cmds_size + text_sections.iter().map(|s| s.data.len()).sum::<usize>();
        let text_vmsize = align_up(text_filesize as u64, Self::PAGE_SIZE);

        // Calculate __DATA segment layout
        let (data_vmsize, data_filesize, data_fileoff, data_vmaddr) = if !data_sections.is_empty() {
            let d_filesize = data_sections.iter().map(|s| s.data.len()).sum::<usize>();
            let d_vmsize = align_up(
                data_sections.iter().map(|s| s.size).sum::<u64>(),
                Self::PAGE_SIZE,
            );
            let d_fileoff = text_vmsize;
            let d_vmaddr = Self::BASE_TEXT_ADDR + text_vmsize;
            (d_vmsize, d_filesize as u64, d_fileoff, d_vmaddr)
        } else {
            (0, 0, text_vmsize, Self::BASE_TEXT_ADDR + text_vmsize)
        };

        // Calculate Symbol Table & String Table for __LINKEDIT
        let mut strtab = Vec::new();
        strtab.push(0); // initial null byte

        let mut nlist_bytes = Vec::new();
        for (name, addr) in symbols {
            let str_off = strtab.len() as u32;
            let full_name = if !name.starts_with('_') {
                format!("_{}", name)
            } else {
                name.clone()
            };
            strtab.extend_from_slice(full_name.as_bytes());
            strtab.push(0);

            // nlist_64 (16 bytes): n_strx (4), n_type (1), n_sect (1), n_desc (2), n_value (8)
            nlist_bytes.extend_from_slice(&str_off.to_le_bytes());
            nlist_bytes.push(N_EXT | N_SECT); // n_type
            nlist_bytes.push(1); // n_sect (section 1: __text)
            nlist_bytes.extend_from_slice(&0u16.to_le_bytes()); // n_desc
            nlist_bytes.extend_from_slice(&addr.to_le_bytes()); // n_value
        }

        let linkedit_fileoff = if data_filesize > 0 {
            data_fileoff + align_up(data_filesize, Self::PAGE_SIZE)
        } else {
            text_vmsize
        };
        let linkedit_vmaddr = if data_vmsize > 0 {
            data_vmaddr + data_vmsize
        } else {
            Self::BASE_TEXT_ADDR + text_vmsize
        };

        let symtab_off = linkedit_fileoff;
        let strtab_off = symtab_off + nlist_bytes.len() as u64;
        let linkedit_filesize = (nlist_bytes.len() + strtab.len()) as u64;
        let linkedit_vmsize = align_up(linkedit_filesize, Self::PAGE_SIZE);

        // =====================================================================
        // 1. Mach Header (32 bytes)
        // =====================================================================
        out.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
        out.extend_from_slice(&CPU_TYPE_ARM64.to_le_bytes());
        out.extend_from_slice(&CPU_SUBTYPE_ARM64_ALL.to_le_bytes());
        out.extend_from_slice(&MH_EXECUTE.to_le_bytes());
        out.extend_from_slice(&(ncmds as u32).to_le_bytes());
        out.extend_from_slice(&(sizeofcmds as u32).to_le_bytes());
        out.extend_from_slice(&MH_FLAGS_EXEC.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved

        // =====================================================================
        // 2. Load Commands
        // =====================================================================

        // 2.1 LC_SEGMENT_64: __PAGEZERO
        write_segment_command(
            &mut out,
            "__PAGEZERO",
            0,
            Self::PAGEZERO_SIZE,
            0,
            0,
            VM_PROT_NONE,
            VM_PROT_NONE,
            0,
            0,
        );

        // 2.2 LC_SEGMENT_64: __TEXT
        let text_prot = VM_PROT_READ | VM_PROT_EXECUTE;
        write_segment_command(
            &mut out,
            "__TEXT",
            Self::BASE_TEXT_ADDR,
            text_vmsize,
            0,
            text_vmsize,
            text_prot | VM_PROT_WRITE,
            text_prot,
            text_sections.len() as u32,
            0,
        );

        // Write __TEXT sections
        let mut curr_sect_off = header_and_cmds_size as u64;
        for sec in text_sections {
            let flags = if sec.sectname == "__text" {
                S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS
            } else {
                0
            };
            write_section_header(
                &mut out,
                &sec.sectname,
                "__TEXT",
                sec.addr,
                sec.size,
                curr_sect_off,
                4,
                flags,
            );
            curr_sect_off += sec.data.len() as u64;
        }

        // 2.3 LC_SEGMENT_64: __DATA (if present)
        if !data_sections.is_empty() {
            let data_prot = VM_PROT_READ | VM_PROT_WRITE;
            write_segment_command(
                &mut out,
                "__DATA",
                data_vmaddr,
                data_vmsize,
                data_fileoff,
                data_filesize,
                data_prot,
                data_prot,
                data_sections.len() as u32,
                0,
            );
            let mut curr_d_off = data_fileoff;
            for sec in data_sections {
                let flags = if sec.is_zerofill { S_ZEROFILL } else { 0 };
                let off = if sec.is_zerofill { 0 } else { curr_d_off };
                write_section_header(
                    &mut out,
                    &sec.sectname,
                    "__DATA",
                    sec.addr,
                    sec.size,
                    off,
                    4,
                    flags,
                );
                if !sec.is_zerofill {
                    curr_d_off += sec.data.len() as u64;
                }
            }
        }

        // 2.4 LC_SEGMENT_64: __LINKEDIT
        write_segment_command(
            &mut out,
            "__LINKEDIT",
            linkedit_vmaddr,
            linkedit_vmsize,
            linkedit_fileoff,
            linkedit_filesize,
            VM_PROT_READ,
            VM_PROT_READ,
            0,
            0,
        );

        // 2.5 LC_LOAD_DYLINKER (/usr/lib/dyld)
        out.extend_from_slice(&LC_LOAD_DYLINKER.to_le_bytes());
        out.extend_from_slice(&(cmd_dylinker_size as u32).to_le_bytes());
        out.extend_from_slice(&12u32.to_le_bytes()); // offset to string
        out.extend_from_slice(b"/usr/lib/dyld\0\0\0\0"); // 16 bytes padded to 32 total

        // 2.6 LC_MAIN
        out.extend_from_slice(&LC_MAIN.to_le_bytes());
        out.extend_from_slice(&(cmd_main_size as u32).to_le_bytes());
        out.extend_from_slice(&entry_offset_in_text.to_le_bytes()); // entryoff
        out.extend_from_slice(&0u64.to_le_bytes()); // stacksize

        // 2.7 LC_LOAD_DYLIB (/usr/lib/libSystem.B.dylib)
        out.extend_from_slice(&LC_LOAD_DYLIB.to_le_bytes());
        out.extend_from_slice(&(cmd_dylib_size as u32).to_le_bytes());
        out.extend_from_slice(&24u32.to_le_bytes()); // offset to name
        out.extend_from_slice(&0u32.to_le_bytes()); // timestamp
        out.extend_from_slice(&0x05000000u32.to_le_bytes()); // current version (5.0.0)
        out.extend_from_slice(&0x01000000u32.to_le_bytes()); // compatibility version (1.0.0)
        out.extend_from_slice(b"/usr/lib/libSystem.B.dylib\0\0\0\0\0\0\0"); // 32 bytes

        // 2.8 LC_SYMTAB
        out.extend_from_slice(&LC_SYMTAB.to_le_bytes());
        out.extend_from_slice(&(cmd_symtab_size as u32).to_le_bytes());
        out.extend_from_slice(&(symtab_off as u32).to_le_bytes());
        out.extend_from_slice(&(symbols.len() as u32).to_le_bytes());
        out.extend_from_slice(&(strtab_off as u32).to_le_bytes());
        out.extend_from_slice(&(strtab.len() as u32).to_le_bytes());

        // =====================================================================
        // 3. Section Data
        // =====================================================================
        for sec in text_sections {
            out.extend_from_slice(&sec.data);
        }

        // Pad __TEXT to text_vmsize
        while out.len() < text_vmsize as usize {
            out.push(0);
        }

        // Write __DATA sections
        if !data_sections.is_empty() {
            for sec in data_sections {
                if !sec.is_zerofill {
                    out.extend_from_slice(&sec.data);
                }
            }
            while out.len() < (data_fileoff + align_up(data_filesize, Self::PAGE_SIZE)) as usize {
                out.push(0);
            }
        }

        // =====================================================================
        // 4. __LINKEDIT (Symbol Table & String Table)
        // =====================================================================
        while out.len() < linkedit_fileoff as usize {
            out.push(0);
        }
        out.extend_from_slice(&nlist_bytes);
        out.extend_from_slice(&strtab);

        out
    }
}

fn write_segment_command(
    out: &mut Vec<u8>,
    segname: &str,
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: u32,
    initprot: u32,
    nsects: u32,
    flags: u32,
) {
    let cmdsize = 72 + nsects * 80;
    out.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
    out.extend_from_slice(&cmdsize.to_le_bytes());

    let mut name_buf = [0u8; 16];
    let bytes = segname.as_bytes();
    let len = bytes.len().min(16);
    name_buf[..len].copy_from_slice(&bytes[..len]);
    out.extend_from_slice(&name_buf);

    out.extend_from_slice(&vmaddr.to_le_bytes());
    out.extend_from_slice(&vmsize.to_le_bytes());
    out.extend_from_slice(&fileoff.to_le_bytes());
    out.extend_from_slice(&filesize.to_le_bytes());
    out.extend_from_slice(&maxprot.to_le_bytes());
    out.extend_from_slice(&initprot.to_le_bytes());
    out.extend_from_slice(&nsects.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
}

fn write_section_header(
    out: &mut Vec<u8>,
    sectname: &str,
    segname: &str,
    addr: u64,
    size: u64,
    offset: u64,
    align: u32,
    flags: u32,
) {
    let mut sname_buf = [0u8; 16];
    let sbytes = sectname.as_bytes();
    let slen = sbytes.len().min(16);
    sname_buf[..slen].copy_from_slice(&sbytes[..slen]);
    out.extend_from_slice(&sname_buf);

    let mut gname_buf = [0u8; 16];
    let gbytes = segname.as_bytes();
    let glen = gbytes.len().min(16);
    gname_buf[..glen].copy_from_slice(&gbytes[..glen]);
    out.extend_from_slice(&gname_buf);

    out.extend_from_slice(&addr.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&align.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reloff
    out.extend_from_slice(&0u32.to_le_bytes()); // nreloc
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved1
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved2
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved3
}

fn align_up(val: u64, align: u64) -> u64 {
    if align == 0 {
        val
    } else {
        (val + align - 1) & !(align - 1)
    }
}
