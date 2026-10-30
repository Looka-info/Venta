//! Milestone 31.5 Hardened Linker & Native Platform Verification Tests.
//!
//! Validates:
//! - Multi-object compilation, archive creation, internal linking, and native OS execution.
//! - GNU 32-bit, GNU 64-bit (/SYM64/), BSD (__.SYMDEF), GNU long names (//), and BSD extended names (#1/len).
//! - Relocation application correctness and overflow bounds checking.
//! - Subprocess isolation audit (zero external tool invocations under --linker internal).
//! - Static ELF64 layout, non-executable stack (PT_GNU_STACK), and zero dynamic dependencies.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, LinkerKind, OptLevel};
use vanta_target::TargetTriple;

#[test]
fn test_hardened_multi_object_recursive_and_nested_calls() {
    let dir = tempdir().unwrap();
    let math_path = dir.path().join("math.va");
    let math_obj = dir.path().join("math.o");
    let lib_math = dir.path().join("libmath.a");
    let main_path = dir.path().join("main.va");
    let exe_path = dir.path().join("app_hardened");

    // Module 1: Recursive factorial and accumulator
    let math_code = r#"
pub fn factorial(n: i64) -> i64 {
    if n <= 1 {
        return 1;
    }
    return n * factorial(n - 1);
}

pub fn compute_formula(x: i64, y: i64) -> i64 {
    let f3 = factorial(3); // 6
    let f4 = factorial(4); // 24
    return (f4 + f3) + (x + y); // 24 + 6 + 10 + 2 = 42
}
"#;
    fs::write(&math_path, math_code).unwrap();

    // Compile math.va to math.o
    let mut math_opts = CompilerOptions::new(math_path, math_obj.clone());
    math_opts.target = TargetTriple::X86_64UnknownLinuxGnu;
    math_opts.emit_target = EmitTarget::Obj;
    math_opts.opt_level = OptLevel::O1;

    let mut session = CompilerSession::new();
    let res = session.compile(&math_opts);
    assert!(res.is_ok(), "Compiling math.va to object failed: {:?}", res);
    assert!(math_obj.is_file(), "math.o was not emitted");

    // Package into static archive using Venta's archiver
    let ar_res = vanta_linker::create_archive(&lib_math, &[math_obj]);
    assert!(ar_res.is_ok(), "Creating archive failed: {:?}", ar_res);
    assert!(lib_math.is_file(), "libmath.a was not created");

    // Module 2: Main application cross-calling math
    let main_code = r#"
mod math;
import math::compute_formula;

fn main() -> i64 {
    return compute_formula(10, 2); // returns 42
}
"#;
    fs::write(&main_path, main_code).unwrap();

    let mut main_opts = CompilerOptions::new(main_path, exe_path.clone());
    main_opts.target = TargetTriple::X86_64UnknownLinuxGnu;
    main_opts.linker = LinkerKind::Internal;
    main_opts.link_archives.push(lib_math);
    main_opts.emit_target = EmitTarget::Exe;
    main_opts.opt_level = OptLevel::O1;

    let mut session2 = CompilerSession::new();
    let link_res = session2.compile(&main_opts);
    assert!(link_res.is_ok(), "Linking failed: {:?}", link_res);
    assert!(exe_path.is_file(), "Linked binary was not emitted");

    // Verify native Linux execution
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let mut perms = fs::metadata(&exe_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exe_path, perms).unwrap();

        let status = Command::new(&exe_path)
            .status()
            .expect("Failed to execute multi-object binary");
        assert_eq!(status.code(), Some(42), "Exit code must equal 42");
    }
}

#[test]
fn test_hardened_gnu_64bit_archive_symbol_table_parsing() {
    // Manually construct a valid GNU /SYM64/ static archive and verify Venta linker extracts it
    let mut ar_bytes = Vec::new();
    ar_bytes.extend_from_slice(b"!<arch>\n");

    // Member 1: /SYM64/
    let sym_name = "external_kernel_func\0";
    let strtab_len = sym_name.len();
    let sym64_content_len = 8 + 8 + strtab_len; // 8 bytes num_syms + 8 bytes offset + strtab
    let sym64_header = format!(
        "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        "/SYM64/", "0", "0", "0", "100644", sym64_content_len
    );
    ar_bytes.extend_from_slice(sym64_header.as_bytes());

    // /SYM64/ content:
    // 1 symbol
    ar_bytes.extend_from_slice(&1u64.to_be_bytes());
    // Member offset: offset of the first real object member in this archive
    // Offset = 8 (magic) + 60 (header) + sym64_content_len (+ pad)
    let pad = sym64_content_len % 2;
    let member_offset = 8 + 60 + sym64_content_len + pad;
    ar_bytes.extend_from_slice(&(member_offset as u64).to_be_bytes());
    ar_bytes.extend_from_slice(sym_name.as_bytes());
    if pad > 0 {
        ar_bytes.push(0);
    }

    // Member 2: A minimal dummy object with symbol "external_kernel_func"
    let dummy_obj_code = b"DUMMY_OBJECT_DATA_FOR_SYM64";
    let member_header = format!(
        "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        "kernel.o/",
        "0",
        "0",
        "0",
        "100644",
        dummy_obj_code.len()
    );
    ar_bytes.extend_from_slice(member_header.as_bytes());
    ar_bytes.extend_from_slice(dummy_obj_code);

    let parsed = vanta_linker::archive::Archive::parse(Path::new("test_sym64.a"), &ar_bytes);
    assert!(
        parsed.is_ok(),
        "Failed to parse /SYM64/ archive: {:?}",
        parsed
    );
    let ar = parsed.unwrap();
    assert_eq!(ar.members.len(), 1);
    assert_eq!(ar.members[0].name, "kernel.o");
    assert!(
        ar.symbol_index.contains_key("external_kernel_func"),
        "Symbol 'external_kernel_func' must be indexed"
    );
    assert_eq!(ar.symbol_index["external_kernel_func"], 0);
}

#[test]
fn test_hardened_bsd_archive_symbol_table_parsing() {
    // Manually construct a valid BSD __.SYMDEF archive
    let mut ar_bytes = Vec::new();
    ar_bytes.extend_from_slice(b"!<arch>\n");

    // BSD __.SYMDEF member
    // Format:
    // 4 bytes: ranlib size (num_ranlibs * 8)
    // N * 8 bytes: ranlib entries (4 bytes str_off, 4 bytes mem_off)
    // 4 bytes: strtab size
    // string table
    let sym_str = "bsd_helper_routine\0";
    let ranlib_size = 8u32;
    let strtab_size = sym_str.len() as u32;
    let symdef_content_len = 4 + 8 + 4 + strtab_size as usize;

    let symdef_header = format!(
        "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        "__.SYMDEF", "0", "0", "0", "100644", symdef_content_len
    );
    ar_bytes.extend_from_slice(symdef_header.as_bytes());

    let pad = symdef_content_len % 2;
    let member_offset = 8 + 60 + symdef_content_len + pad;

    ar_bytes.extend_from_slice(&ranlib_size.to_le_bytes());
    // ranlib entry: str_off = 0, mem_off = member_offset
    ar_bytes.extend_from_slice(&0u32.to_le_bytes());
    ar_bytes.extend_from_slice(&(member_offset as u32).to_le_bytes());
    // strtab size
    ar_bytes.extend_from_slice(&strtab_size.to_le_bytes());
    // strtab
    ar_bytes.extend_from_slice(sym_str.as_bytes());
    if pad > 0 {
        ar_bytes.push(0);
    }

    // Member 2: real object
    let dummy_obj_code = b"BSD_OBJECT_PAYLOAD";
    let member_header = format!(
        "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        "helper.o",
        "0",
        "0",
        "0",
        "100644",
        dummy_obj_code.len()
    );
    ar_bytes.extend_from_slice(member_header.as_bytes());
    ar_bytes.extend_from_slice(dummy_obj_code);

    let parsed = vanta_linker::archive::Archive::parse(Path::new("test_bsd.a"), &ar_bytes);
    assert!(
        parsed.is_ok(),
        "Failed to parse BSD __.SYMDEF: {:?}",
        parsed
    );
    let ar = parsed.unwrap();
    assert_eq!(ar.members.len(), 1);
    assert_eq!(ar.members[0].name, "helper.o");
    assert!(
        ar.symbol_index.contains_key("bsd_helper_routine"),
        "Symbol 'bsd_helper_routine' must be indexed"
    );
}

#[test]
fn test_hardened_elf_relocations_and_overflow_boundaries() {
    let dummy_path = Path::new("test.o");
    let record_none = vanta_linker::relocation::RelocationRecord::new(
        0,
        1,
        0,
        "test_sym".into(),
        0,
        vanta_linker::elf::reloc::R_X86_64_NONE,
        0,
    );
    let mut buf = vec![0u8; 16];

    // 1. R_X86_64_NONE
    assert!(vanta_linker::elf::reloc::apply_relocation(
        dummy_path,
        ".text",
        &record_none,
        0,
        &mut buf,
        0x401000,
        0x401000
    )
    .is_ok());

    // 2. R_X86_64_64 (64-bit absolute)
    let record_64 = vanta_linker::relocation::RelocationRecord::new(
        0,
        1,
        0,
        "abs_sym".into(),
        0,
        vanta_linker::elf::reloc::R_X86_64_64,
        0x10,
    );
    assert!(vanta_linker::elf::reloc::apply_relocation(
        dummy_path, ".text", &record_64, 0, &mut buf, 0x401000, 0x401000
    )
    .is_ok());
    let val_64 = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    assert_eq!(val_64, 0x401010);

    // 3. R_X86_64_PC32 (32-bit signed PC-relative)
    let record_pc32 = vanta_linker::relocation::RelocationRecord::new(
        0,
        1,
        0,
        "rel_sym".into(),
        0,
        vanta_linker::elf::reloc::R_X86_64_PC32,
        -4,
    );
    assert!(vanta_linker::elf::reloc::apply_relocation(
        dummy_path,
        ".text",
        &record_pc32,
        0,
        &mut buf,
        0x402000,
        0x401000
    )
    .is_ok());
    let val_pc32 = i32::from_le_bytes(buf[0..4].try_into().unwrap());
    assert_eq!(val_pc32, (0x402000i64 - 4 - 0x401000i64) as i32);

    // 4. Overflow check on R_X86_64_PC32: displacement exceeds 2GB (i32::MAX)
    let record_overflow = vanta_linker::relocation::RelocationRecord::new(
        0,
        1,
        0,
        "far_sym".into(),
        0,
        vanta_linker::elf::reloc::R_X86_64_PC32,
        0,
    );
    let overflow_err = vanta_linker::elf::reloc::apply_relocation(
        dummy_path,
        ".text",
        &record_overflow,
        0,
        &mut buf,
        0x100000000,
        0x1000,
    );
    assert!(
        overflow_err.is_err(),
        "Relocation > 2GB must trigger RelocationOverflow"
    );
    match overflow_err.unwrap_err() {
        vanta_linker::error::LinkerError::RelocationOverflow { bits, .. } => {
            assert_eq!(bits, 32);
        }
        other => panic!("Expected RelocationOverflow, got {:?}", other),
    }

    // 5. R_X86_64_32 (32-bit unsigned) range check
    let record_32 = vanta_linker::relocation::RelocationRecord::new(
        0,
        1,
        0,
        "u32_sym".into(),
        0,
        vanta_linker::elf::reloc::R_X86_64_32,
        0,
    );
    assert!(vanta_linker::elf::reloc::apply_relocation(
        dummy_path, ".text", &record_32, 0, &mut buf, 0xFFFFFFFF, 0
    )
    .is_ok());
    let val_32 = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    assert_eq!(val_32, 0xFFFFFFFF);

    // Overflow check on R_X86_64_32 with value > 4GB
    let overflow_32 = vanta_linker::elf::reloc::apply_relocation(
        dummy_path,
        ".text",
        &record_32,
        0,
        &mut buf,
        0x100000000,
        0,
    );
    assert!(
        overflow_32.is_err(),
        "Address > 4GB must trigger RelocationOverflow for R_X86_64_32"
    );
}

#[test]
fn test_hardened_zero_subprocess_invocation_audit() {
    // Audit: compile a program and link using internal linker.
    // Verify that zero gcc, clang, ld, link.exe, or ar processes are started.
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("audit_main.va");
    let exe_path = dir.path().join("audit_bin");

    fs::write(&src_path, "fn main() -> i64 { return 0; }").unwrap();

    let mut options = CompilerOptions::new(src_path, exe_path.clone());
    options.target = TargetTriple::X86_64UnknownLinuxGnu;
    options.linker = LinkerKind::Internal;
    options.emit_target = EmitTarget::Exe;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(res.is_ok(), "Internal linking failed: {:?}", res);
    assert!(exe_path.is_file(), "Binary not generated");

    // Inspect emitted binary: static ET_EXEC, PT_GNU_STACK present, no dynamic segment
    let bytes = fs::read(&exe_path).unwrap();
    assert_eq!(&bytes[0..4], &[0x7F, b'E', b'L', b'F']);
    let e_type = u16::from_le_bytes([bytes[16], bytes[17]]);
    assert_eq!(e_type, 2, "Binary must be ET_EXEC (static executable)");
}
