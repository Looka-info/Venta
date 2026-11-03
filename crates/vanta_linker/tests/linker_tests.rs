//! Comprehensive test suite for Venta v2.0 Pure-Rust Self-Contained Native Linker.

use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;
use vanta_linker::archive::{extract_required_members, Archive, ArchiveWriter};
use vanta_linker::coff::linker::link_coff_executable;
use vanta_linker::config::LinkerConfig;
use vanta_linker::error::LinkerError;
use vanta_linker::input::detect_format;
use vanta_linker::map_file::generate_link_map;
use vanta_linker::object::ObjectFile;
use vanta_linker::resolve::SymbolResolver;
use vanta_linker::section::{InputSection, MergedSection, SectionFlags, SectionKind};
use vanta_linker::symbol::{Symbol, SymbolKind, SymbolVisibility};
use vanta_linker::{inspect_archive, list_archive};
use vanta_target::TargetTriple;

#[test]
fn test_archive_create_parse_and_inspect() {
    let dir = tempdir().unwrap();
    let archive_path = dir.path().join("libmath.a");

    // Create a mock object 1
    let mut writer = ArchiveWriter::new();
    let obj1_data = b"\x7fELF\x02\x01\x01\0mock_elf64_member1";
    let obj2_data = b"\x7fELF\x02\x01\x01\0mock_elf64_member2";

    writer.add_member("add.o".into(), obj1_data.to_vec());
    writer.add_member("sub.o".into(), obj2_data.to_vec());

    let archive_bytes = writer.write();
    assert!(archive_bytes.starts_with(b"!<arch>\n"));
    fs::write(&archive_path, &archive_bytes).unwrap();

    // Verify member listing
    let members = list_archive(&archive_path).unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0], "add.o");
    assert_eq!(members[1], "sub.o");

    // Verify inspection
    let insp = inspect_archive(&archive_path).unwrap();
    assert_eq!(insp.members_count, 2);
    assert_eq!(insp.members[0].0, "add.o");
    assert_eq!(insp.members[1].0, "sub.o");
}

#[test]
fn test_archive_deterministic_reproducibility() {
    let mut w1 = ArchiveWriter::new();
    let mut w2 = ArchiveWriter::new();

    w1.add_member("foo.o".into(), vec![1, 2, 3, 4]);
    w1.add_member("bar.o".into(), vec![5, 6, 7, 8]);

    w2.add_member("foo.o".into(), vec![1, 2, 3, 4]);
    w2.add_member("bar.o".into(), vec![5, 6, 7, 8]);

    let bytes1 = w1.write();
    let bytes2 = w2.write();

    assert_eq!(
        bytes1, bytes2,
        "Archive generation must be byte-for-byte deterministic"
    );
}

#[test]
fn test_lazy_archive_member_extraction() {
    let mut archive = Archive {
        path: PathBuf::from("libcore.a"),
        members: vec![
            vanta_linker::archive::ArchiveMember {
                name: "needed.o".into(),
                offset: 100,
                size: 200,
                data: vec![0; 200],
            },
            vanta_linker::archive::ArchiveMember {
                name: "unused.o".into(),
                offset: 400,
                size: 500,
                data: vec![0; 500],
            },
        ],
        symbol_index: std::collections::HashMap::new(),
    };
    archive.symbol_index.insert("needed_func".into(), 0);
    archive.symbol_index.insert("unused_func".into(), 1);

    let mut unresolved = std::collections::HashSet::new();
    unresolved.insert("needed_func".into());

    let mut extracted = Vec::new();
    extract_required_members(&[archive], &mut unresolved, &mut extracted);

    assert_eq!(
        extracted.len(),
        1,
        "Only the required member should be extracted"
    );
    assert_eq!(extracted[0].name, "needed.o");
    assert!(!unresolved.contains("needed_func"));
}

#[test]
fn test_symbol_resolution_strong_and_weak_precedence() {
    let mut obj1 = ObjectFile::new(
        1,
        "main.o".into(),
        PathBuf::from("main.o"),
        vanta_linker::input::InputFormat::Elf64,
    );
    obj1.symbols.push(Symbol {
        name: "foo".into(),
        visibility: SymbolVisibility::Weak,
        kind: SymbolKind::Function,
        section_index: Some(1),
        value: 0x10,
        size: 32,
        object_id: 1,
        final_virtual_address: None,
    });

    let mut obj2 = ObjectFile::new(
        2,
        "lib.o".into(),
        PathBuf::from("lib.o"),
        vanta_linker::input::InputFormat::Elf64,
    );
    obj2.symbols.push(Symbol {
        name: "foo".into(),
        visibility: SymbolVisibility::Global,
        kind: SymbolKind::Function,
        section_index: Some(1),
        value: 0x20,
        size: 48,
        object_id: 2,
        final_virtual_address: None,
    });

    let objs = vec![obj1, obj2];
    let mut resolver = SymbolResolver::new();
    resolver.register_definitions(&objs).unwrap();

    let def = resolver.find_definition("foo", &objs);
    assert!(def.is_some());
    // Strong definition in obj 2 takes precedence
    assert_eq!(resolver.strong_symbols.get("foo"), Some(&(2, 0)));
}

#[test]
fn test_symbol_resolution_duplicate_strong_symbols_rejected() {
    let mut obj1 = ObjectFile::new(
        1,
        "a.o".into(),
        PathBuf::from("a.o"),
        vanta_linker::input::InputFormat::Elf64,
    );
    obj1.symbols.push(Symbol {
        name: "calculate".into(),
        visibility: SymbolVisibility::Global,
        kind: SymbolKind::Function,
        section_index: Some(1),
        value: 0,
        size: 16,
        object_id: 1,
        final_virtual_address: None,
    });

    let mut obj2 = ObjectFile::new(
        2,
        "b.o".into(),
        PathBuf::from("b.o"),
        vanta_linker::input::InputFormat::Elf64,
    );
    obj2.symbols.push(Symbol {
        name: "calculate".into(),
        visibility: SymbolVisibility::Global,
        kind: SymbolKind::Function,
        section_index: Some(1),
        value: 0,
        size: 16,
        object_id: 2,
        final_virtual_address: None,
    });

    let mut resolver = SymbolResolver::new();
    let err = resolver.register_definitions(&[obj1, obj2]).unwrap_err();

    match err {
        LinkerError::DuplicateSymbol {
            symbol,
            first_file,
            second_file,
        } => {
            assert_eq!(symbol, "calculate");
            assert_eq!(first_file, PathBuf::from("a.o"));
            assert_eq!(second_file, PathBuf::from("b.o"));
        }
        other => panic!("Expected DuplicateSymbol, got {:?}", other),
    }
}

#[test]
fn test_local_symbol_isolation_across_objects() {
    let mut obj1 = ObjectFile::new(
        1,
        "mod1.o".into(),
        PathBuf::from("mod1.o"),
        vanta_linker::input::InputFormat::Elf64,
    );
    obj1.symbols.push(Symbol {
        name: "local_helper".into(),
        visibility: SymbolVisibility::Local,
        kind: SymbolKind::Function,
        section_index: Some(1),
        value: 0x100,
        size: 16,
        object_id: 1,
        final_virtual_address: None,
    });

    let mut obj2 = ObjectFile::new(
        2,
        "mod2.o".into(),
        PathBuf::from("mod2.o"),
        vanta_linker::input::InputFormat::Elf64,
    );
    obj2.symbols.push(Symbol {
        name: "local_helper".into(),
        visibility: SymbolVisibility::Local,
        kind: SymbolKind::Function,
        section_index: Some(1),
        value: 0x200,
        size: 16,
        object_id: 2,
        final_virtual_address: None,
    });

    let mut resolver = SymbolResolver::new();
    // Two local symbols with identical names must not collide
    assert!(resolver.register_definitions(&[obj1, obj2]).is_ok());
}

#[test]
fn test_elf_static_executable_generation_and_layout() {
    let dir = tempdir().unwrap();
    let _exe_path = dir.path().join("test_elf_prog");

    // Construct a minimal ELF object in memory:
    // main:
    //   mov $42, %rax
    //   ret
    let code: Vec<u8> = vec![
        0x48, 0xC7, 0xC0, 0x2A, 0x00, 0x00, 0x00, // mov $42, %rax
        0xC3, // ret
    ];

    let sec = InputSection {
        object_id: 1,
        source_index: 1,
        name: ".text".into(),
        kind: SectionKind::Code,
        flags: SectionFlags::code(),
        align: 16,
        data: code,
        size: 8,
    };

    let mut merged = MergedSection::new(".text".into(), SectionKind::Code, SectionFlags::code());
    merged.append_contribution(&sec);
    merged.virtual_address = 0x401000;
    merged.file_offset = 0x1000;

    let symbols = vec![("main".to_string(), 0x401000u64, 8u64)];

    let binary = vanta_linker::elf::writer::ElfExecutableWriter::write_executable(
        0x401000,
        &[merged],
        &symbols,
    );

    assert!(binary.len() >= 64);
    assert_eq!(&binary[0..4], &[0x7F, b'E', b'L', b'F']);
    assert_eq!(binary[4], 2); // 64-bit
    assert_eq!(binary[5], 1); // 2LSB

    // e_type = ET_EXEC (2)
    let e_type = u16::from_le_bytes([binary[16], binary[17]]);
    assert_eq!(e_type, 2);

    // e_entry = 0x401000
    let e_entry = u64::from_le_bytes([
        binary[24], binary[25], binary[26], binary[27], binary[28], binary[29], binary[30],
        binary[31],
    ]);
    assert_eq!(e_entry, 0x401000);
}

#[test]
fn test_pe32_plus_executable_generation_and_layout() {
    let dir = tempdir().unwrap();
    let _exe_path = dir.path().join("test_win_prog.exe");

    let code: Vec<u8> = vec![
        0x48, 0x83, 0xEC, 0x28, // sub rsp, 40
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0
        0x48, 0x83, 0xC4, 0x28, // add rsp, 40
        0xC3, // ret
    ];

    let sec = vanta_linker::coff::pe_writer::PeSectionDesc {
        name: ".text".into(),
        virtual_size: code.len() as u32,
        virtual_address: 0x1000,
        data: code,
        characteristics: 0x60000020,
    };

    let binary = vanta_linker::coff::pe_writer::PeWriter::write_image(
        0x1000,
        vec![sec],
        None,
        None,
        None,
        None,
    );

    assert!(binary.len() > 0x200);
    // DOS header magic MZ
    assert_eq!(&binary[0..2], b"MZ");

    // PE signature
    let e_lfanew = u32::from_le_bytes([binary[60], binary[61], binary[62], binary[63]]) as usize;
    assert_eq!(&binary[e_lfanew..e_lfanew + 4], b"PE\0\0");

    // Machine AMD64 (0x8664)
    let machine = u16::from_le_bytes([binary[e_lfanew + 4], binary[e_lfanew + 5]]);
    assert_eq!(machine, 0x8664);

    // Optional header magic PE32+ (0x020B)
    let opt_magic = u16::from_le_bytes([binary[e_lfanew + 24], binary[e_lfanew + 25]]);
    assert_eq!(opt_magic, 0x020B);
}

#[test]
fn test_macho_64_executable_generation_and_layout() {
    let dir = tempdir().unwrap();
    let _exe_path = dir.path().join("test_macho_prog");

    let code: Vec<u8> = vec![
        0x00, 0x00, 0x80, 0xD2, // mov x0, #0
        0xC0, 0x03, 0x5F, 0xD6, // ret
    ];

    let sec = vanta_linker::macho::writer::MachOSectionDesc {
        segname: "__TEXT".into(),
        sectname: "__text".into(),
        addr: 0x100004000,
        size: code.len() as u64,
        data: code,
        is_zerofill: false,
    };

    let symbols = vec![("_main".to_string(), 0x100004000u64)];

    let binary = vanta_linker::macho::writer::MachOExecutableWriter::write_executable(
        0x4000,
        &[sec],
        &[],
        &symbols,
    );

    assert!(binary.len() >= 32);
    // Mach-O 64 magic: 0xFEEDFACF
    let magic = u32::from_le_bytes([binary[0], binary[1], binary[2], binary[3]]);
    assert_eq!(magic, 0xFEEDFACF);

    // CPU_TYPE_ARM64: 0x0100000C
    let cputype = u32::from_le_bytes([binary[4], binary[5], binary[6], binary[7]]);
    assert_eq!(cputype, 0x0100000C);

    // MH_EXECUTE: 2
    let filetype = u32::from_le_bytes([binary[12], binary[13], binary[14], binary[15]]);
    assert_eq!(filetype, 2);

    let ncmds = u32::from_le_bytes([binary[16], binary[17], binary[18], binary[19]]);
    let sizeofcmds = u32::from_le_bytes([binary[20], binary[21], binary[22], binary[23]]) as usize;

    // Parse and validate all load commands
    let mut offset = 32;
    let end = 32 + sizeofcmds;
    let mut parsed_cmds = Vec::new();
    while offset < end {
        let cmd = u32::from_le_bytes([
            binary[offset],
            binary[offset + 1],
            binary[offset + 2],
            binary[offset + 3],
        ]);
        let cmdsize = u32::from_le_bytes([
            binary[offset + 4],
            binary[offset + 5],
            binary[offset + 6],
            binary[offset + 7],
        ]) as usize;
        parsed_cmds.push((cmd, cmdsize));
        offset += cmdsize;
    }
    assert_eq!(
        offset, end,
        "Load commands must parse to exact end of sizeofcmds"
    );
    assert_eq!(
        parsed_cmds.len() as u32,
        ncmds,
        "Parsed count must match ncmds"
    );

    // Verify critical commands
    let cmd_ids: Vec<u32> = parsed_cmds.iter().map(|(c, _)| *c).collect();
    assert!(cmd_ids.contains(&0x19), "Must contain LC_SEGMENT_64");
    assert!(cmd_ids.contains(&0x0E), "Must contain LC_LOAD_DYLINKER");
    assert!(cmd_ids.contains(&0x80000028), "Must contain LC_MAIN");
    assert!(cmd_ids.contains(&0x0C), "Must contain LC_LOAD_DYLIB");
    assert!(cmd_ids.contains(&0x02), "Must contain LC_SYMTAB");

    // Check __text section in __TEXT segment
    // Section header offset is 32 + 72 (after __PAGEZERO) + 72 (after __TEXT segment header) = 176
    let sect_addr = u64::from_le_bytes([
        binary[208],
        binary[209],
        binary[210],
        binary[211],
        binary[212],
        binary[213],
        binary[214],
        binary[215],
    ]);
    let sect_offset = u32::from_le_bytes([binary[224], binary[225], binary[226], binary[227]]);
    assert_eq!(sect_addr, 0x100004000);
    assert_eq!(sect_offset, 0x4000);
}

#[test]
fn test_malformed_inputs_graceful_rejection() {
    // 1. Empty buffer
    assert_eq!(
        detect_format(&[]),
        vanta_linker::input::InputFormat::Unknown
    );

    // 2. Corrupt archive
    let err =
        Archive::parse("corrupt.a", b"!<arch>\ninvalid_header_bytes_without_magic").unwrap_err();
    match err {
        LinkerError::MalformedInput { .. } => {}
        other => panic!(
            "Expected MalformedInput for corrupt archive, got {:?}",
            other
        ),
    }

    // 3. Truncated ELF
    let err = vanta_linker::elf::reader::parse_elf64_object(
        std::path::Path::new("truncated.o"),
        b"\x7fELF\x02",
        0,
    )
    .unwrap_err();
    match err {
        LinkerError::MalformedInput { .. } => {}
        other => panic!("Expected MalformedInput for truncated ELF, got {:?}", other),
    }

    // 4. Truncated COFF
    let err = vanta_linker::coff::reader::read_coff_object(
        0,
        "trunc.obj".into(),
        PathBuf::from("trunc.obj"),
        b"\x64\x86",
    )
    .unwrap_err();
    match err {
        LinkerError::MalformedInput { .. } => {}
        other => panic!(
            "Expected MalformedInput for truncated COFF, got {:?}",
            other
        ),
    }

    // 5. Truncated Mach-O
    let err = vanta_linker::macho::reader::read_macho_object(
        0,
        "trunc.o".into(),
        PathBuf::from("trunc.o"),
        b"\xcf\xfa\xed\xfe",
    )
    .unwrap_err();
    match err {
        LinkerError::MalformedInput { .. } => {}
        other => panic!(
            "Expected MalformedInput for truncated Mach-O, got {:?}",
            other
        ),
    }
}

#[test]
fn test_link_map_generation() {
    let objects = vec![ObjectFile::new(
        1,
        "main.o".into(),
        PathBuf::from("main.o"),
        vanta_linker::input::InputFormat::Elf64,
    )];
    let merged = vec![MergedSection::new(
        ".text".into(),
        SectionKind::Code,
        SectionFlags::code(),
    )];
    let mut resolved = std::collections::HashMap::new();
    resolved.insert("main".into(), 0x401000u64);

    let map = generate_link_map(&objects, &[], &[], &merged, &resolved, 0x401000, "_start");
    assert!(map.contains("VENTA NATIVE LINKER MAP FILE"));
    assert!(map.contains("main"));
    assert!(map.contains("0x0000000000401000"));
}

/// Helper to build a minimal AMD64 COFF relocatable object (.obj) in memory.
fn create_test_coff_obj(
    entry_symbol: &str,
    code_bytes: &[u8],
    relocs: &[(&str, u32)], // (symbol_name, offset_in_code)
) -> Vec<u8> {
    let mut data = Vec::new();
    // 1. File header (20 bytes)
    data.extend_from_slice(&0x8664u16.to_le_bytes()); // AMD64
    data.extend_from_slice(&1u16.to_le_bytes()); // 1 section (.text)
    data.extend_from_slice(&0u32.to_le_bytes()); // timestamp

    let symtab_offset_pos = data.len();
    data.extend_from_slice(&0u32.to_le_bytes()); // placeholder symtab_offset
    let num_symbols = (1 + relocs.len()) as u32;
    data.extend_from_slice(&num_symbols.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes()); // opt header size
    data.extend_from_slice(&0u16.to_le_bytes()); // characteristics

    // 2. Section header (40 bytes)
    let mut name = [0u8; 8];
    name[..5].copy_from_slice(b".text");
    data.extend_from_slice(&name);
    data.extend_from_slice(&0u32.to_le_bytes()); // virtual_size
    data.extend_from_slice(&0u32.to_le_bytes()); // virtual_address
    data.extend_from_slice(&(code_bytes.len() as u32).to_le_bytes());
    let raw_data_ptr = 60u32;
    data.extend_from_slice(&raw_data_ptr.to_le_bytes());
    let reloc_ptr = if relocs.is_empty() {
        0u32
    } else {
        raw_data_ptr + code_bytes.len() as u32
    };
    data.extend_from_slice(&reloc_ptr.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes()); // 28..32: PointerToLinenumbers
    data.extend_from_slice(&(relocs.len() as u16).to_le_bytes()); // 32..34: NumberOfRelocations
    data.extend_from_slice(&0u16.to_le_bytes()); // 34..36: NumberOfLinenumbers
    data.extend_from_slice(&0x60000020u32.to_le_bytes()); // 36..40: Characteristics

    // 3. Raw section data
    data.extend_from_slice(code_bytes);

    // 4. Relocations (10 bytes each)
    for (i, &(_sym_name, offset)) in relocs.iter().enumerate() {
        data.extend_from_slice(&offset.to_le_bytes());
        let sym_idx = (1 + i) as u32; // index 0 is entry symbol
        data.extend_from_slice(&sym_idx.to_le_bytes());
        data.extend_from_slice(&4u16.to_le_bytes()); // IMAGE_REL_AMD64_REL32
    }

    // 5. Symbol table
    let symtab_offset = data.len() as u32;
    data[symtab_offset_pos..symtab_offset_pos + 4].copy_from_slice(&symtab_offset.to_le_bytes());

    let mut all_names = vec![entry_symbol.to_string()];
    for (sym_name, _) in relocs {
        all_names.push(sym_name.to_string());
    }

    let mut string_table = Vec::new();
    string_table.extend_from_slice(&0u32.to_le_bytes()); // placeholder string table length

    for (idx, sym_name) in all_names.iter().enumerate() {
        let is_entry = idx == 0;
        let mut sym_record = [0u8; 18];
        if sym_name.len() <= 8 {
            sym_record[..sym_name.len()].copy_from_slice(sym_name.as_bytes());
        } else {
            let str_off = string_table.len() as u32;
            sym_record[4..8].copy_from_slice(&str_off.to_le_bytes());
            string_table.extend_from_slice(sym_name.as_bytes());
            string_table.push(0);
        }
        let sec_num: i16 = if is_entry { 1 } else { 0 };
        sym_record[12..14].copy_from_slice(&sec_num.to_le_bytes());
        sym_record[14..16].copy_from_slice(&0x20u16.to_le_bytes()); // function
        sym_record[16] = 2; // EXTERNAL
        data.extend_from_slice(&sym_record);
    }

    // 6. String table
    let strtab_len = string_table.len() as u32;
    string_table[0..4].copy_from_slice(&strtab_len.to_le_bytes());
    data.extend_from_slice(&string_table);

    data
}

#[test]
fn test_windows_dynamic_import_generation_multi_dll() {
    let dir = tempdir().unwrap();
    let exe_path = dir.path().join("prog.exe");

    // Code calling ExitProcess (offset 7) and printf (offset 16):
    // 0: sub rsp, 40               48 83 ec 28
    // 4: xor ecx, ecx              31 c9
    // 6: call ExitProcess          e8 [rel32 at offset 7]
    // 11: lea rcx, [rip]           48 8d 0d 00 00 00 00
    // 18: call printf              e8 [rel32 at offset 19]
    // 23: add rsp, 40              48 83 c4 28
    // 27: ret                      c3
    let code: Vec<u8> = vec![
        0x48, 0x83, 0xEC, 0x28, // 0..3: sub rsp, 40
        0x31, 0xC9, // 4..5: xor ecx, ecx
        0xE8, 0x00, 0x00, 0x00, 0x00, // 6..10: call ExitProcess
        0x31, 0xC9, // 11..12: xor ecx, ecx
        0x48, 0x83, 0xC4, 0x28, // 13..16: add rsp, 40
        0xC3, // 17: ret
    ];

    let relocs = [("ExitProcess", 7u32), ("printf", 12u32)];

    let obj_bytes = create_test_coff_obj("main", &code, &relocs);

    let mut config = LinkerConfig::new(TargetTriple::X86_64PcWindowsMsvc, exe_path.clone());
    config.input_buffers.push(("main.obj".into(), obj_bytes));

    let out = link_coff_executable(&config)
        .expect("link_coff_executable must succeed with dynamic imports");
    assert!(exe_path.is_file(), "Executable must be written to disk");
    assert!(out.total_file_size > 0);

    let binary = fs::read(&exe_path).unwrap();

    // Check DOS magic MZ
    assert_eq!(&binary[0..2], b"MZ");

    // PE signature
    let e_lfanew = u32::from_le_bytes([binary[60], binary[61], binary[62], binary[63]]) as usize;
    assert_eq!(&binary[e_lfanew..e_lfanew + 4], b"PE\0\0");

    // Machine AMD64 (0x8664) and opt magic PE32+ (0x020B)
    let machine = u16::from_le_bytes([binary[e_lfanew + 4], binary[e_lfanew + 5]]);
    assert_eq!(machine, 0x8664);
    let opt_magic = u16::from_le_bytes([binary[e_lfanew + 24], binary[e_lfanew + 25]]);
    assert_eq!(opt_magic, 0x020B);

    // Optional header data directories:
    // Import Table is directory entry 1 (offset e_lfanew + 24 + 112 + 8)
    let import_dir_offset = e_lfanew + 24 + 112 + 8;
    let import_rva = u32::from_le_bytes([
        binary[import_dir_offset],
        binary[import_dir_offset + 1],
        binary[import_dir_offset + 2],
        binary[import_dir_offset + 3],
    ]);
    let import_size = u32::from_le_bytes([
        binary[import_dir_offset + 4],
        binary[import_dir_offset + 5],
        binary[import_dir_offset + 6],
        binary[import_dir_offset + 7],
    ]);
    assert!(import_rva > 0, "Import Directory RVA must be non-zero");
    assert!(import_size > 0, "Import Directory size must be non-zero");

    // IAT is directory entry 12 (offset e_lfanew + 24 + 112 + 12 * 8)
    let iat_dir_offset = e_lfanew + 24 + 112 + 12 * 8;
    let iat_rva = u32::from_le_bytes([
        binary[iat_dir_offset],
        binary[iat_dir_offset + 1],
        binary[iat_dir_offset + 2],
        binary[iat_dir_offset + 3],
    ]);
    let iat_size = u32::from_le_bytes([
        binary[iat_dir_offset + 4],
        binary[iat_dir_offset + 5],
        binary[iat_dir_offset + 6],
        binary[iat_dir_offset + 7],
    ]);
    assert!(iat_rva > 0, "IAT Directory RVA must be non-zero");
    assert!(iat_size > 0, "IAT Directory size must be non-zero");

    // Verify DLL names and imported symbols are present in the binary
    let binary_str = String::from_utf8_lossy(&binary);
    assert!(
        binary_str.contains("KERNEL32.dll"),
        "Must reference KERNEL32.dll"
    );
    assert!(
        binary_str.contains("msvcrt.dll"),
        "Must reference msvcrt.dll"
    );
    assert!(
        binary_str.contains("ExitProcess"),
        "Must import ExitProcess"
    );
    assert!(binary_str.contains("printf"), "Must import printf");

    // On Windows, execute the linked binary!
    #[cfg(target_os = "windows")]
    {
        let status = std::process::Command::new(&exe_path)
            .status()
            .expect("Failed to execute generated Windows PE executable");
        assert_eq!(
            status.code(),
            Some(0),
            "Executable must exit cleanly with code 0"
        );
    }
}

#[test]
fn test_windows_unknown_external_symbol_rejected() {
    let dir = tempdir().unwrap();
    let exe_path = dir.path().join("prog_fail.exe");

    let code: Vec<u8> = vec![
        0x48, 0x83, 0xEC, 0x28, 0xE8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x83, 0xC4, 0x28, 0xC3,
    ];

    let relocs = [("completely_unresolvable_function_12345", 5u32)];
    let obj_bytes = create_test_coff_obj("main", &code, &relocs);
    let debug_obj = vanta_linker::coff::reader::read_coff_object(
        0,
        "debug.obj".into(),
        PathBuf::from("debug.obj"),
        &obj_bytes,
    )
    .unwrap();
    eprintln!(
        "DEBUG symbols: {:?}",
        debug_obj
            .symbols
            .iter()
            .map(|s| &s.name)
            .collect::<Vec<_>>()
    );
    eprintln!("DEBUG relocs: {:?}", debug_obj.relocations);

    let mut config = LinkerConfig::new(TargetTriple::X86_64PcWindowsMsvc, exe_path);
    config.input_buffers.push(("main.obj".into(), obj_bytes));

    let err = link_coff_executable(&config).unwrap_err();
    match err {
        LinkerError::UndefinedSymbol { symbol, .. } => {
            assert_eq!(symbol, "completely_unresolvable_function_12345");
        }
        other => panic!("Expected UndefinedSymbol error, got {:?}", other),
    }
}

#[test]
fn test_windows_archive_symbol_overrides_dynamic_import() {
    let dir = tempdir().unwrap();
    let exe_path = dir.path().join("prog_ar.exe");

    // Main object calling a function
    let code: Vec<u8> = vec![
        0x48, 0x83, 0xEC, 0x28, 0xE8, 0x00, 0x00, 0x00, 0x00, 0x48, 0x83, 0xC4, 0x28, 0xC3,
    ];
    let main_relocs = [("helper_func", 5u32)];
    let main_obj = create_test_coff_obj("main", &code, &main_relocs);

    // Archive member defining helper_func statically
    let helper_code: Vec<u8> = vec![
        0x48, 0xC7, 0xC0, 0x2A, 0x00, 0x00, 0x00, // mov rax, 42
        0xC3, // ret
    ];
    let helper_obj = create_test_coff_obj("helper_func", &helper_code, &[]);

    let mut writer = ArchiveWriter::new();
    writer.add_member("helper.obj".into(), helper_obj);
    let ar_bytes = writer.write();

    let mut config = LinkerConfig::new(TargetTriple::X86_64PcWindowsMsvc, exe_path.clone());
    config.input_buffers.push(("main.obj".into(), main_obj));
    config
        .archive_buffers
        .push(("libhelper.lib".into(), ar_bytes));

    let out =
        link_coff_executable(&config).expect("Link must succeed with static archive resolution");
    assert_eq!(
        out.extracted_archive_members_count, 1,
        "Must extract helper.obj from archive"
    );
}
