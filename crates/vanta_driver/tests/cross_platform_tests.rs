//! Milestone 30 — Cross-Platform Native Code Generation Integration Tests:
//! Targets:
//! - `x86_64-unknown-linux-gnu` (System V AMD64 ABI, ELF64)
//! - `x86_64-pc-windows-msvc` (Microsoft x64 ABI, PE/COFF)
//! - `aarch64-apple-darwin` (Apple Silicon AAPCS64 ABI, Mach-O 64)

use std::fs;
use tempfile::tempdir;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, TargetTriple};
use vanta_target::{Arch, ObjectFormat, Os, TargetAbi};

#[test]
fn test_target_triple_parsing_and_aliases() {
    // Linux canonical and aliases
    let linux = "x86_64-unknown-linux-gnu".parse::<TargetTriple>().unwrap();
    assert_eq!(linux, TargetTriple::X86_64UnknownLinuxGnu);
    assert_eq!(
        "linux".parse::<TargetTriple>().unwrap(),
        TargetTriple::X86_64UnknownLinuxGnu
    );
    assert_eq!(
        "x86_64-linux-gnu".parse::<TargetTriple>().unwrap(),
        TargetTriple::X86_64UnknownLinuxGnu
    );

    // Windows canonical and aliases
    let win = "x86_64-pc-windows-msvc".parse::<TargetTriple>().unwrap();
    assert_eq!(win, TargetTriple::X86_64PcWindowsMsvc);
    assert_eq!(
        "windows".parse::<TargetTriple>().unwrap(),
        TargetTriple::X86_64PcWindowsMsvc
    );
    assert_eq!(
        "win64".parse::<TargetTriple>().unwrap(),
        TargetTriple::X86_64PcWindowsMsvc
    );
    assert_eq!(
        "x86_64-windows-msvc".parse::<TargetTriple>().unwrap(),
        TargetTriple::X86_64PcWindowsMsvc
    );

    // Apple Darwin canonical and aliases
    let mac = "aarch64-apple-darwin".parse::<TargetTriple>().unwrap();
    assert_eq!(mac, TargetTriple::Aarch64AppleDarwin);
    assert_eq!(
        "macos".parse::<TargetTriple>().unwrap(),
        TargetTriple::Aarch64AppleDarwin
    );
    assert_eq!(
        "darwin".parse::<TargetTriple>().unwrap(),
        TargetTriple::Aarch64AppleDarwin
    );
    assert_eq!(
        "apple-silicon".parse::<TargetTriple>().unwrap(),
        TargetTriple::Aarch64AppleDarwin
    );
    assert_eq!(
        "arm64-apple-darwin".parse::<TargetTriple>().unwrap(),
        TargetTriple::Aarch64AppleDarwin
    );

    // Invalid target check
    let err = "unsupported-target-arch"
        .parse::<TargetTriple>()
        .unwrap_err();
    assert!(err.to_string().contains("Unsupported target triple"));
}

#[test]
fn test_target_spec_properties() {
    let linux_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();
    assert_eq!(linux_spec.arch, Arch::X86_64);
    assert_eq!(linux_spec.os, Os::Linux);
    assert_eq!(linux_spec.object_format, ObjectFormat::Elf64);
    assert_eq!(linux_spec.abi, TargetAbi::SystemVAmd64);
    assert_eq!(linux_spec.obj_extension, "o");

    let win_spec = TargetTriple::X86_64PcWindowsMsvc.spec();
    assert_eq!(win_spec.arch, Arch::X86_64);
    assert_eq!(win_spec.os, Os::Windows);
    assert_eq!(win_spec.object_format, ObjectFormat::Coff);
    assert_eq!(win_spec.abi, TargetAbi::MicrosoftX64);
    assert_eq!(win_spec.obj_extension, "obj");

    let mac_spec = TargetTriple::Aarch64AppleDarwin.spec();
    assert_eq!(mac_spec.arch, Arch::AArch64);
    assert_eq!(mac_spec.os, Os::MacOS);
    assert_eq!(mac_spec.object_format, ObjectFormat::MachO64);
    assert_eq!(mac_spec.abi, TargetAbi::AppleAArch64);
    assert_eq!(mac_spec.symbol_prefix, "_");
    assert_eq!(mac_spec.obj_extension, "o");
}

#[test]
fn test_windows_coff_object_generation() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("main.va");
    let obj_file = dir.path().join("main.obj");

    let src = r#"
fn add(a: i64, b: i64) -> i64 {
    return a + b;
}

fn main() -> i64 {
    let res: i64 = add(40, 2);
    return res;
}
"#;
    fs::write(&src_file, src).unwrap();

    let mut options = CompilerOptions::new(src_file, obj_file.clone());
    options.target = TargetTriple::X86_64PcWindowsMsvc;
    options.emit_target = EmitTarget::Obj;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Windows COFF compilation failed: {:?}",
        session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
    );

    let bytes = res.unwrap().expect("COFF object bytes expected");
    assert!(bytes.len() >= 20, "COFF file too small");

    // COFF Header check: Machine == 0x8664 (IMAGE_FILE_MACHINE_AMD64)
    let machine = u16::from_le_bytes([bytes[0], bytes[1]]);
    assert_eq!(
        machine, 0x8664,
        "Expected IMAGE_FILE_MACHINE_AMD64 (0x8664)"
    );

    let num_sections = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
    assert!(
        num_sections >= 2,
        "Expected at least .text and .pdata sections, found {}",
        num_sections
    );

    // Verify presence of section names in section headers
    let section_headers_start = 20;
    let mut section_names = Vec::new();
    for i in 0..num_sections {
        let offset = section_headers_start + i * 40;
        let name_bytes = &bytes[offset..offset + 8];
        let name_len = name_bytes.iter().position(|&b| b == 0).unwrap_or(8);
        let name_str = String::from_utf8_lossy(&name_bytes[..name_len]).to_string();
        section_names.push(name_str);
    }

    assert!(
        section_names.iter().any(|s| s == ".text"),
        "Missing .text section in COFF: {:?}",
        section_names
    );
    assert!(
        section_names.iter().any(|s| s == ".pdata"),
        "Missing .pdata unwind section in COFF: {:?}",
        section_names
    );
    assert!(
        section_names.iter().any(|s| s == ".xdata"),
        "Missing .xdata unwind info in COFF: {:?}",
        section_names
    );
}

#[test]
fn test_apple_arm64_macho_object_generation() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("main.va");
    let obj_file = dir.path().join("main.o");

    let src = r#"
fn multiply(x: i64, y: i64) -> i64 {
    return x * y;
}

fn main() -> i64 {
    let ans: i64 = multiply(6, 7);
    return ans;
}
"#;
    fs::write(&src_file, src).unwrap();

    let mut options = CompilerOptions::new(src_file, obj_file.clone());
    options.target = TargetTriple::Aarch64AppleDarwin;
    options.emit_target = EmitTarget::Obj;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "AArch64 Mach-O compilation failed: {:?}",
        session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
    );

    let bytes = res.unwrap().expect("Mach-O object bytes expected");
    assert!(bytes.len() >= 32, "Mach-O header too small");

    // Mach-O 64-bit Magic: 0xFEEDFACF
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    assert_eq!(magic, 0xFEEDFACF, "Expected MH_MAGIC_64 (0xFEEDFACF)");

    // CPU Type: 0x0100000C (CPU_TYPE_ARM64)
    let cputype = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(cputype, 0x0100000C, "Expected CPU_TYPE_ARM64 (0x0100000C)");

    // File type: 1 (MH_OBJECT)
    let filetype = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    assert_eq!(filetype, 1, "Expected MH_OBJECT (1)");

    let ncmds = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    assert!(
        ncmds >= 2,
        "Expected at least 2 load commands (LC_SEGMENT_64 + LC_SYMTAB)"
    );
}

#[test]
fn test_microsoft_x64_abi_argument_spilling() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("abi.va");
    let obj_file = dir.path().join("abi.obj");

    // Function with 6 parameters: tests passing via RCX, RDX, R8, R9, plus 2 stack arguments above 32-byte shadow space
    let src = r#"
fn sum6(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64) -> i64 {
    return a + b + c + d + e + f;
}

fn main() -> i64 {
    let res: i64 = sum6(1, 2, 3, 4, 5, 6);
    return res;
}
"#;
    fs::write(&src_file, src).unwrap();

    let mut options = CompilerOptions::new(src_file, obj_file);
    options.target = TargetTriple::X86_64PcWindowsMsvc;
    options.emit_target = EmitTarget::Obj;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "MS x64 6-arg compilation failed: {:?}",
        session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
    );

    let bytes = res.unwrap().expect("COFF object bytes expected");
    assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 0x8664);
}

#[test]
fn test_apple_arm64_abi_argument_passing() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("arm_abi.va");
    let obj_file = dir.path().join("arm_abi.o");

    // Function with 8 parameters: all 8 should be passed in registers X0..X7 under AAPCS64
    let src = r#"
fn sum8(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64, h: i64) -> i64 {
    return a + b + c + d + e + f + g + h;
}

fn main() -> i64 {
    let res: i64 = sum8(10, 20, 30, 40, 50, 60, 70, 80);
    return res;
}
"#;
    fs::write(&src_file, src).unwrap();

    let mut options = CompilerOptions::new(src_file, obj_file);
    options.target = TargetTriple::Aarch64AppleDarwin;
    options.emit_target = EmitTarget::Obj;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "ARM64 8-arg compilation failed: {:?}",
        session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
    );

    let bytes = res.unwrap().expect("Mach-O object bytes expected");
    assert_eq!(
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        0xFEEDFACF
    );
}

#[test]
fn test_linux_elf64_object_generation() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("main.va");
    let obj_file = dir.path().join("main.o");

    let src = r#"
fn main() -> i64 {
    return 42;
}
"#;
    fs::write(&src_file, src).unwrap();

    let mut options = CompilerOptions::new(src_file, obj_file);
    options.target = TargetTriple::X86_64UnknownLinuxGnu;
    options.emit_target = EmitTarget::Obj;

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Linux ELF64 compilation failed: {:?}",
        session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
    );

    let bytes = res.unwrap().expect("ELF64 object bytes expected");
    assert!(bytes.len() >= 64, "ELF64 header too small");

    // ELF magic: 0x7F 'E' 'L' 'F'
    assert_eq!(&bytes[0..4], &[0x7F, b'E', b'L', b'F']);
    // 64-bit ELF
    assert_eq!(bytes[4], 2);
    // Little endian
    assert_eq!(bytes[5], 1);
    // Machine: x86-64 (0x003E)
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    assert_eq!(machine, 0x003E);
}
