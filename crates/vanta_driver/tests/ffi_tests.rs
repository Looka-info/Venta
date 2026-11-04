use std::fs;
use std::process::Command;
use vanta_driver::{
    CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, LinkerKind, OptLevel,
};

#[test]
fn test_ffi_libc_abs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_abs.va");
    let out_bin = temp_dir.path().join("test_abs_bin");

    let source = r#"
extern "C" {
    pub fn abs(x: i64) -> i64;
}

pub fn main() {
    let neg = -42;
    let pos = abs(neg);
    if pos == 42 {
        print_i64(pos);
    } else {
        panic("abs(-42) did not equal 42");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("42"),
        "Expected output to contain '42', got: {}",
        stdout
    );
}

#[test]
fn test_ffi_custom_c_object_integration() {
    // 1. Create and compile a C helper file into a native .o object
    let temp_dir = tempfile::tempdir().unwrap();
    let c_file = temp_dir.path().join("c_helper.c");
    let obj_file = temp_dir.path().join("c_helper.o");

    let c_code = r#"
long long c_add_and_scale(long long a, long long b, long long scale) {
    return (a + b) * scale;
}
"#;
    fs::write(&c_file, c_code).unwrap();

    let cc_status = Command::new("cc")
        .args(&["-c", c_file.to_str().unwrap(), "-o", obj_file.to_str().unwrap()])
        .status()
        .expect("Failed to invoke system C compiler 'cc'");
    assert!(cc_status.success(), "Compilation of C source failed");

    // 2. Write Venta program that declares and invokes the C function
    let src_path = temp_dir.path().join("test_c_ffi.va");
    let out_bin = temp_dir.path().join("test_c_ffi_bin");

    let venta_code = r#"
extern "C" {
    pub fn c_add_and_scale(a: i64, b: i64, scale: i64) -> i64;
}

pub fn main() {
    let result = c_add_and_scale(10, 20, 3);
    if result == 90 {
        print_i64(result);
    } else {
        panic("c_add_and_scale failed");
    }
}
"#;
    fs::write(&src_path, venta_code).unwrap();

    // 3. Compile Venta program passing the .o object in additional_inputs
    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        additional_inputs: vec![obj_file],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    // 4. Run binary and verify result
    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("90"),
        "Expected output to contain '90', got: {}",
        stdout
    );
}

#[test]
fn test_ffi_custom_rust_object_integration() {
    // 1. Create and compile a Rust helper file into a native .o object
    let temp_dir = tempfile::tempdir().unwrap();
    let rs_file = temp_dir.path().join("rust_helper.rs");
    let obj_file = temp_dir.path().join("rust_helper.o");

    let rs_code = r#"
#![crate_type = "lib"]
#![no_std]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn rust_compute_power(base: i64, exp: i64) -> i64 {
    let mut acc: i64 = 1;
    let mut i: i64 = 0;
    while i < exp {
        acc = acc.wrapping_mul(base);
        i = i.wrapping_add(1);
    }
    acc
}
"#;
    fs::write(&rs_file, rs_code).unwrap();

    let rustc_status = Command::new("rustc")
        .args(&[
            "-O",
            "-C",
            "panic=abort",
            "--emit=obj",
            rs_file.to_str().unwrap(),
            "-o",
            obj_file.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to invoke 'rustc'");
    assert!(rustc_status.success(), "Compilation of Rust source failed");

    // 2. Write Venta program that calls the Rust function
    let src_path = temp_dir.path().join("test_rust_ffi.va");
    let out_bin = temp_dir.path().join("test_rust_ffi_bin");

    let venta_code = r#"
extern "C" fn rust_compute_power(base: i64, exp: i64) -> i64;

pub fn main() {
    let p = rust_compute_power(2, 10);
    if p == 1024 {
        print_i64(p);
    } else {
        panic("rust_compute_power failed");
    }
}
"#;
    fs::write(&src_path, venta_code).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        additional_inputs: vec![obj_file],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1024"),
        "Expected output to contain '1024', got: {}",
        stdout
    );
}

#[test]
fn test_ffi_libc_labs_and_multi_params() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_labs.va");
    let out_bin = temp_dir.path().join("test_labs_bin");

    let source = r#"
extern "C" {
    pub fn labs(x: i64) -> i64;
}

pub fn main() {
    let large_neg = -123456789;
    let large_pos = labs(large_neg);
    if large_pos == 123456789 {
        print_i64(large_pos);
    } else {
        panic("labs failed");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("123456789"),
        "Expected output to contain '123456789', got: {}",
        stdout
    );
}

#[test]
fn test_cli_driver_ffi_invocation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let c_file = temp_dir.path().join("c_math.c");
    let obj_file = temp_dir.path().join("c_math.o");

    let c_code = r#"
long long compute_cube(long long x) {
    return x * x * x;
}
"#;
    fs::write(&c_file, c_code).unwrap();

    let cc_status = Command::new("cc")
        .args(&["-c", c_file.to_str().unwrap(), "-o", obj_file.to_str().unwrap()])
        .status()
        .expect("Failed to invoke system C compiler 'cc'");
    assert!(cc_status.success(), "Compilation of C source failed");

    let src_path = temp_dir.path().join("test_cube.va");
    let out_bin = temp_dir.path().join("test_cube_bin");

    let venta_code = r#"
extern "C" {
    pub fn compute_cube(x: i64) -> i64;
}

pub fn main() {
    let val = compute_cube(5);
    if val == 125 {
        print_i64(val);
    } else {
        panic("compute_cube failed");
    }
}
"#;
    fs::write(&src_path, venta_code).unwrap();

    // Use CompilerOptions with additional_inputs
    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        additional_inputs: vec![obj_file],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(res.is_ok(), "CLI FFI build failed: {:?}", res.err());

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("125"), "Expected 125, got: {}", stdout);
}
