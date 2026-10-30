//! Milestone 31 — Venta v2.0 Pure-Rust Self-Contained Native Linker & Static Archive E2E Tests.

use std::fs;
use std::process::Command;
use tempfile::tempdir;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, LinkerKind, TargetTriple};

#[test]
fn test_internal_linker_single_object_elf_static_exec() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main_elf");

    let source = r#"
fn main() -> i64 {
    return 42;
}
"#;
    fs::write(&src_path, source).unwrap();

    let mut options = CompilerOptions::new(src_path, exe_path.clone());
    options.target = TargetTriple::X86_64UnknownLinuxGnu;
    options.linker = LinkerKind::Internal;
    options.emit_target = EmitTarget::Exe;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(res.is_ok(), "Internal linking failed: {:?}", res);
    assert!(exe_path.is_file(), "Executable was not produced");

    let bytes = fs::read(&exe_path).unwrap();
    assert!(bytes.len() >= 64);
    assert_eq!(&bytes[0..4], &[0x7F, b'E', b'L', b'F']); // ELF magic

    // If on Linux x86-64, execute the generated static binary directly!
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&exe_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exe_path, perms).unwrap();

        let status = Command::new(&exe_path)
            .status()
            .expect("Failed to execute native ELF binary");
        assert_eq!(
            status.code(),
            Some(42),
            "Exit code must match returned value"
        );
    }
}

#[test]
fn test_internal_linker_windows_pe32_plus_generation() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("win_main.va");
    let exe_path = dir.path().join("win_main.exe");

    let source = r#"
fn main() -> i64 {
    return 0;
}
"#;
    fs::write(&src_path, source).unwrap();

    let mut options = CompilerOptions::new(src_path, exe_path.clone());
    options.target = TargetTriple::X86_64PcWindowsMsvc;
    options.linker = LinkerKind::Internal;
    options.emit_target = EmitTarget::Exe;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(res.is_ok(), "Internal Windows linking failed: {:?}", res);
    assert!(exe_path.is_file(), "Windows PE executable was not produced");

    let bytes = fs::read(&exe_path).unwrap();
    // Validate DOS header
    assert_eq!(&bytes[0..2], b"MZ");

    // Validate PE signature and AMD64 machine
    let e_lfanew = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]) as usize;
    assert_eq!(&bytes[e_lfanew..e_lfanew + 4], b"PE\0\0");
    let machine = u16::from_le_bytes([bytes[e_lfanew + 4], bytes[e_lfanew + 5]]);
    assert_eq!(machine, 0x8664);

    // Validate PE32+ Optional Header Magic (0x020B)
    let opt_magic = u16::from_le_bytes([bytes[e_lfanew + 24], bytes[e_lfanew + 25]]);
    assert_eq!(opt_magic, 0x020B);
}

#[test]
fn test_internal_linker_apple_silicon_macho_generation() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("mac_main.va");
    let exe_path = dir.path().join("mac_main");

    let source = r#"
fn main() -> i64 {
    return 0;
}
"#;
    fs::write(&src_path, source).unwrap();

    let mut options = CompilerOptions::new(src_path, exe_path.clone());
    options.target = TargetTriple::Aarch64AppleDarwin;
    options.linker = LinkerKind::Internal;
    options.emit_target = EmitTarget::Exe;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(res.is_ok(), "Internal macOS linking failed: {:?}", res);
    assert!(exe_path.is_file(), "Mach-O executable was not produced");

    let bytes = fs::read(&exe_path).unwrap();
    // Validate Mach-O 64 magic (0xFEEDFACF)
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    assert_eq!(magic, 0xFEEDFACF);

    // Validate CPU_TYPE_ARM64 (0x0100000C)
    let cputype = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(cputype, 0x0100000C);

    // Validate MH_EXECUTE (2)
    let filetype = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    assert_eq!(filetype, 2);
}

#[test]
fn test_internal_linker_link_map_output() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("map_test.va");
    let exe_path = dir.path().join("map_test");
    let map_path = dir.path().join("map_test.map");

    let source = r#"
fn main() -> i64 {
    return 7;
}
"#;
    fs::write(&src_path, source).unwrap();

    let mut options = CompilerOptions::new(src_path, exe_path);
    options.target = TargetTriple::X86_64UnknownLinuxGnu;
    options.linker = LinkerKind::Internal;
    options.emit_target = EmitTarget::Exe;
    options.map_file = Some(map_path.clone());

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(res.is_ok(), "Linking with map file failed: {:?}", res);
    assert!(map_path.is_file(), "Map file was not generated");

    let map_content = fs::read_to_string(&map_path).unwrap();
    assert!(map_content.contains("VENTA NATIVE LINKER MAP FILE"));
    assert!(map_content.contains("Output Sections:"));
    assert!(map_content.contains(".text"));
    assert!(map_content.contains("main"));
}

#[test]
fn test_internal_linker_multi_object_archive_cross_call() {
    let dir = tempdir().unwrap();
    let math_src = dir.path().join("math.va");
    let math_obj = dir.path().join("math.o");
    let libmath_a = dir.path().join("libmath.a");
    let main_src = dir.path().join("caller.va");
    let exe_path = dir.path().join("caller_exe");

    // 1. Math module
    let math_code = r#"
pub fn compute_sum(a: i64, b: i64) -> i64 {
    return a + b;
}
"#;
    fs::write(&math_src, math_code).unwrap();

    let mut math_opts = CompilerOptions::new(math_src, math_obj.clone());
    math_opts.target = TargetTriple::X86_64UnknownLinuxGnu;
    math_opts.emit_target = EmitTarget::Obj;

    let mut session1 = CompilerSession::new();
    let res1 = session1.compile(&math_opts);
    assert!(res1.is_ok(), "Failed to compile math.o: {:?}", res1);
    assert!(math_obj.is_file());

    // 2. Create archive libmath.a using vanta_linker
    let ar_bytes = vanta_linker::create_archive(&libmath_a, &[math_obj]).unwrap();
    assert!(ar_bytes > 0);
    assert!(libmath_a.is_file());

    // 3. Main caller importing math module
    let caller_code = r#"
mod math;
import math::compute_sum;

fn main() -> i64 {
    return compute_sum(25, 17);
}
"#;
    fs::write(&main_src, caller_code).unwrap();

    let mut caller_opts = CompilerOptions::new(main_src, exe_path.clone());
    caller_opts.target = TargetTriple::X86_64UnknownLinuxGnu;
    caller_opts.linker = LinkerKind::Internal;
    caller_opts.emit_target = EmitTarget::Exe;
    caller_opts.link_archives.push(libmath_a);

    let mut session2 = CompilerSession::new();
    let res2 = session2.compile(&caller_opts);
    assert!(res2.is_ok(), "Failed to link with archive: {:?}", res2);
    assert!(exe_path.is_file());

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&exe_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exe_path, perms).unwrap();

        let status = Command::new(&exe_path)
            .status()
            .expect("Failed to execute multi-object binary");
        assert_eq!(status.code(), Some(42), "25 + 17 must equal 42");
    }
}
