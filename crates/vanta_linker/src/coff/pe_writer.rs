//! Pure-Rust 64-bit Portable Executable (PE32+) Image Writer for Windows x86-64.

pub const IMAGE_DOS_SIGNATURE: u16 = 0x5A4D; // MZ
pub const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550; // PE\0\0
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;

pub const IMAGE_FILE_EXECUTABLE_IMAGE: u16 = 0x0002;
pub const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020;

pub const PE32_PLUS_MAGIC: u16 = 0x020B;
pub const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3; // Console

pub const IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA: u16 = 0x0020;
pub const IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE: u16 = 0x0040;
pub const IMAGE_DLLCHARACTERISTICS_NX_COMPAT: u16 = 0x0100;
pub const IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE: u16 = 0x8000;

pub const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
pub const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
pub const IMAGE_SCN_CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;
pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
pub const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;
pub const IMAGE_SCN_MEM_DISCARDABLE: u32 = 0x0200_0000;

pub const IMAGE_REL_BASED_DIR64: u16 = 10;

#[derive(Debug, Clone)]
pub struct PeSectionDesc {
    pub name: String,
    pub virtual_size: u32,
    pub virtual_address: u32, // RVA
    pub data: Vec<u8>,
    pub characteristics: u32,
}

pub struct PeWriter;

impl PeWriter {
    pub const DEFAULT_IMAGE_BASE: u64 = 0x140000000;
    pub const SECTION_ALIGNMENT: u32 = 0x1000; // 4096
    pub const FILE_ALIGNMENT: u32 = 0x200; // 512

    /// Emits a complete, valid Windows PE32+ console executable.
    pub fn write_image(
        entry_point_rva: u32,
        mut sections: Vec<PeSectionDesc>,
        pdata_rva_and_size: Option<(u32, u32)>,
        idata_rva_and_size: Option<(u32, u32)>,
        iat_rva_and_size: Option<(u32, u32)>,
        reloc_rva_and_size: Option<(u32, u32)>,
    ) -> Vec<u8> {
        let mut out = Vec::new();

        // 1. DOS Header (64 bytes)
        let mut dos_header = [0u8; 64];
        dos_header[0..2].copy_from_slice(&IMAGE_DOS_SIGNATURE.to_le_bytes()); // e_magic
        dos_header[60..64].copy_from_slice(&0x80u32.to_le_bytes()); // e_lfanew = 0x80
        out.extend_from_slice(&dos_header);

        // 2. DOS Stub (64 bytes from 0x40 to 0x80)
        let dos_stub = [
            0x0e, 0x1f, 0xba, 0x0e, 0x00, 0xb4, 0x09, 0xcd, 0x21, 0xb8, 0x01, 0x4c, 0xcd, 0x21,
            b'T', b'h', b'i', b's', b' ', b'p', b'r', b'o', b'g', b'r', b'a', b'm', b' ', b'c',
            b'a', b'n', b'n', b'o', b't', b' ', b'b', b'e', b' ', b'r', b'u', b'n', b' ', b'i',
            b'n', b' ', b'D', b'O', b'S', b' ', b'm', b'o', b'd', b'e', b'.', b'\r', b'\r', b'\n',
            b'$', 0, 0, 0, 0, 0, 0, 0,
        ];
        out.extend_from_slice(&dos_stub);

        // Ensure we are at 0x80
        while out.len() < 0x80 {
            out.push(0);
        }

        // 3. PE Signature (4 bytes)
        out.extend_from_slice(&IMAGE_NT_SIGNATURE.to_le_bytes()); // "PE\0\0"

        // 4. COFF File Header (20 bytes)
        let num_sections = sections.len() as u16;
        let opt_header_size = 240u16;
        let file_characteristics = IMAGE_FILE_EXECUTABLE_IMAGE | IMAGE_FILE_LARGE_ADDRESS_AWARE;

        out.extend_from_slice(&IMAGE_FILE_MACHINE_AMD64.to_le_bytes()); // Machine
        out.extend_from_slice(&num_sections.to_le_bytes()); // NumberOfSections
        out.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp (deterministic)
        out.extend_from_slice(&0u32.to_le_bytes()); // PointerToSymbolTable
        out.extend_from_slice(&0u32.to_le_bytes()); // NumberOfSymbols
        out.extend_from_slice(&opt_header_size.to_le_bytes()); // SizeOfOptionalHeader
        out.extend_from_slice(&file_characteristics.to_le_bytes()); // Characteristics

        // Calculate Header Sizes and Section Layout
        let header_raw_size = 0x80 + 4 + 20 + 240 + (num_sections as usize * 40);
        let size_of_headers = align_up_u32(header_raw_size as u32, Self::FILE_ALIGNMENT);

        // Compute RVAs and file offsets for each section
        let mut current_rva = align_up_u32(size_of_headers, Self::SECTION_ALIGNMENT);
        for sec in &mut sections {
            sec.virtual_address = current_rva;
            current_rva = align_up_u32(current_rva + sec.virtual_size, Self::SECTION_ALIGNMENT);
        }

        let size_of_image = current_rva;

        // 5. Optional Header (PE32+, 240 bytes)
        out.extend_from_slice(&PE32_PLUS_MAGIC.to_le_bytes()); // Magic = 0x020B
        out.push(2); // MajorLinkerVersion
        out.push(0); // MinorLinkerVersion

        let size_of_code = sections
            .iter()
            .find(|s| s.characteristics & IMAGE_SCN_CNT_CODE != 0)
            .map(|s| align_up_u32(s.data.len() as u32, Self::FILE_ALIGNMENT))
            .unwrap_or(0);
        let size_of_init_data = sections
            .iter()
            .filter(|s| s.characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA != 0)
            .map(|s| align_up_u32(s.data.len() as u32, Self::FILE_ALIGNMENT))
            .sum::<u32>();

        out.extend_from_slice(&size_of_code.to_le_bytes()); // SizeOfCode
        out.extend_from_slice(&size_of_init_data.to_le_bytes()); // SizeOfInitializedData
        out.extend_from_slice(&0u32.to_le_bytes()); // SizeOfUninitializedData
        out.extend_from_slice(&entry_point_rva.to_le_bytes()); // AddressOfEntryPoint

        let base_of_code = sections
            .iter()
            .find(|s| s.characteristics & IMAGE_SCN_CNT_CODE != 0)
            .map(|s| s.virtual_address)
            .unwrap_or(0x1000);
        out.extend_from_slice(&base_of_code.to_le_bytes()); // BaseOfCode

        out.extend_from_slice(&Self::DEFAULT_IMAGE_BASE.to_le_bytes()); // ImageBase (64-bit)
        out.extend_from_slice(&Self::SECTION_ALIGNMENT.to_le_bytes()); // SectionAlignment
        out.extend_from_slice(&Self::FILE_ALIGNMENT.to_le_bytes()); // FileAlignment

        out.extend_from_slice(&6u16.to_le_bytes()); // MajorOperatingSystemVersion
        out.extend_from_slice(&0u16.to_le_bytes()); // MinorOperatingSystemVersion
        out.extend_from_slice(&0u16.to_le_bytes()); // MajorImageVersion
        out.extend_from_slice(&0u16.to_le_bytes()); // MinorImageVersion
        out.extend_from_slice(&6u16.to_le_bytes()); // MajorSubsystemVersion
        out.extend_from_slice(&0u16.to_le_bytes()); // MinorSubsystemVersion
        out.extend_from_slice(&0u32.to_le_bytes()); // Win32VersionValue

        out.extend_from_slice(&size_of_image.to_le_bytes()); // SizeOfImage
        out.extend_from_slice(&size_of_headers.to_le_bytes()); // SizeOfHeaders
        out.extend_from_slice(&0u32.to_le_bytes()); // CheckSum
        out.extend_from_slice(&IMAGE_SUBSYSTEM_WINDOWS_CUI.to_le_bytes()); // Subsystem (Console)

        let dll_chars = IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA
            | IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE
            | IMAGE_DLLCHARACTERISTICS_NX_COMPAT
            | IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE;
        out.extend_from_slice(&dll_chars.to_le_bytes()); // DllCharacteristics

        out.extend_from_slice(&0x100000u64.to_le_bytes()); // SizeOfStackReserve (1MB)
        out.extend_from_slice(&0x1000u64.to_le_bytes()); // SizeOfStackCommit (4KB)
        out.extend_from_slice(&0x100000u64.to_le_bytes()); // SizeOfHeapReserve (1MB)
        out.extend_from_slice(&0x1000u64.to_le_bytes()); // SizeOfHeapCommit (4KB)
        out.extend_from_slice(&0u32.to_le_bytes()); // LoaderFlags
        out.extend_from_slice(&16u32.to_le_bytes()); // NumberOfRvaAndSizes

        // Data Directories (16 entries * 8 bytes each = 128 bytes)
        let mut data_dirs = [(0u32, 0u32); 16];
        if let Some((rva, sz)) = idata_rva_and_size {
            data_dirs[1] = (rva, sz); // Import Directory
        }
        if let Some((rva, sz)) = pdata_rva_and_size {
            data_dirs[3] = (rva, sz); // Exception Directory
        }
        if let Some((rva, sz)) = reloc_rva_and_size {
            data_dirs[5] = (rva, sz); // Base Relocation Directory
        }
        if let Some((rva, sz)) = iat_rva_and_size {
            data_dirs[12] = (rva, sz); // IAT Directory
        }

        for (rva, sz) in data_dirs {
            out.extend_from_slice(&rva.to_le_bytes());
            out.extend_from_slice(&sz.to_le_bytes());
        }

        // 6. Section Headers (40 bytes each)
        let mut file_offset_cursor = size_of_headers;
        for sec in &sections {
            let mut sec_hdr = [0u8; 40];
            let name_bytes = sec.name.as_bytes();
            let n_len = name_bytes.len().min(8);
            sec_hdr[0..n_len].copy_from_slice(&name_bytes[..n_len]);

            sec_hdr[8..12].copy_from_slice(&sec.virtual_size.to_le_bytes());
            sec_hdr[12..16].copy_from_slice(&sec.virtual_address.to_le_bytes());

            let raw_size = align_up_u32(sec.data.len() as u32, Self::FILE_ALIGNMENT);
            sec_hdr[16..20].copy_from_slice(&raw_size.to_le_bytes());
            sec_hdr[20..24].copy_from_slice(&file_offset_cursor.to_le_bytes());

            sec_hdr[36..40].copy_from_slice(&sec.characteristics.to_le_bytes());

            out.extend_from_slice(&sec_hdr);
            file_offset_cursor += raw_size;
        }

        // Pad headers up to size_of_headers
        while out.len() < size_of_headers as usize {
            out.push(0);
        }

        // 7. Write Section Data (aligned to FILE_ALIGNMENT)
        for sec in &sections {
            let start = out.len();
            out.extend_from_slice(&sec.data);
            let raw_size = align_up_u32(sec.data.len() as u32, Self::FILE_ALIGNMENT) as usize;
            while out.len() < start + raw_size {
                out.push(0);
            }
        }

        out
    }
}

fn align_up_u32(value: u32, align: u32) -> u32 {
    if align == 0 {
        value
    } else {
        (value + align - 1) & !(align - 1)
    }
}
