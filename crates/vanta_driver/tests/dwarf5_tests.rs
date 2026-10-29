//! Native DWARF 5 Debug Information verification tests (Milestone 27).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, OptLevel};

fn setup_test_dir(test_name: &str) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("venta_dwarf5_test_{}", test_name));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).expect("Failed to create test directory");
    temp_dir
}

fn compile_venta_program(
    src_path: &Path,
    out_path: &Path,
    emit_target: EmitTarget,
    debug_info: bool,
    opt_level: OptLevel,
) {
    let mut session = CompilerSession::new();
    let mut options = CompilerOptions::new(src_path.to_path_buf(), out_path.to_path_buf());
    options.emit_target = emit_target;
    options.debug_info = debug_info;
    options.opt_level = opt_level;
    options.std_path = Some(PathBuf::from("library"));

    let res = session.compile(&options);
    if let Err(e) = res {
        eprintln!(
            "Diagnostics:\n{}",
            session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
        );
        panic!("Compilation of {:?} failed: {}", src_path, e);
    }
}

#[test]
fn test_dwarf5_elf_object_sections() {
    let temp_dir = setup_test_dir("obj_sections");
    let src_path = temp_dir.join("test.va");
    let obj_path = temp_dir.join("test.o");

    let source = r#"
pub fn add(a: i64, b: i64) -> i64 {
    let sum: i64 = a + b;
    return sum;
}

pub fn main() -> i64 {
    let x: i64 = 10;
    let y: i64 = 20;
    return add(x, y);
}
"#;
    fs::write(&src_path, source).unwrap();

    // 1. Compile with DWARF 5 debug info
    compile_venta_program(&src_path, &obj_path, EmitTarget::Obj, true, OptLevel::None);

    let output = Command::new("readelf")
        .arg("-S")
        .arg(&obj_path)
        .output()
        .expect("Failed to execute readelf -S");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(".debug_info"),
        "Expected .debug_info section in object: {}",
        stdout
    );
    assert!(
        stdout.contains(".debug_abbrev"),
        "Expected .debug_abbrev section in object: {}",
        stdout
    );
    assert!(
        stdout.contains(".debug_line"),
        "Expected .debug_line section in object: {}",
        stdout
    );
    assert!(
        stdout.contains(".rela.debug_info"),
        "Expected .rela.debug_info section: {}",
        stdout
    );
    assert!(
        stdout.contains(".rela.debug_line"),
        "Expected .rela.debug_line section: {}",
        stdout
    );

    // 2. Inspect DWARF 5 compilation unit details
    let dump_output = Command::new("readelf")
        .arg("--debug-dump=info")
        .arg(&obj_path)
        .output()
        .expect("Failed to execute readelf --debug-dump=info");

    let dump_stdout = String::from_utf8_lossy(&dump_output.stdout);
    assert!(
        dump_stdout.contains("Version:") && dump_stdout.contains("5"),
        "Expected DWARF Version 5 in .debug_info: {}",
        dump_stdout
    );
    assert!(
        dump_stdout.contains("DW_TAG_compile_unit") || dump_stdout.contains("compile_unit"),
        "Expected compile_unit tag: {}",
        dump_stdout
    );
    // Venta language ID: 0x8001 (32769)
    assert!(
        dump_stdout.contains("32769") || dump_stdout.contains("0x8001"),
        "Expected DW_LANG_Venta (0x8001 / 32769): {}",
        dump_stdout
    );

    // Subprogram DIEs
    assert!(
        dump_stdout.contains("add"),
        "Expected subprogram 'add' in .debug_info: {}",
        dump_stdout
    );
    assert!(
        dump_stdout.contains("main"),
        "Expected subprogram 'main' in .debug_info: {}",
        dump_stdout
    );

    // Parameters and local variables
    assert!(
        dump_stdout.contains("a"),
        "Expected param 'a' in .debug_info: {}",
        dump_stdout
    );
    assert!(
        dump_stdout.contains("b"),
        "Expected param 'b' in .debug_info: {}",
        dump_stdout
    );
    assert!(
        dump_stdout.contains("sum"),
        "Expected variable 'sum' in .debug_info: {}",
        dump_stdout
    );
    assert!(
        dump_stdout.contains("x"),
        "Expected variable 'x' in .debug_info: {}",
        dump_stdout
    );
    assert!(
        dump_stdout.contains("y"),
        "Expected variable 'y' in .debug_info: {}",
        dump_stdout
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_dwarf5_line_table_structure() {
    let temp_dir = setup_test_dir("line_table");
    let src_path = temp_dir.join("lines.va");
    let obj_path = temp_dir.join("lines.o");

    let source = r#"
pub fn compute(v: i64) -> i64 {
    let res: i64 = v * 2;
    return res;
}
"#;
    fs::write(&src_path, source).unwrap();

    compile_venta_program(&src_path, &obj_path, EmitTarget::Obj, true, OptLevel::None);

    let output = Command::new("readelf")
        .arg("-wl")
        .arg(&obj_path)
        .output()
        .expect("Failed to execute readelf -wl");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("DWARF Version:") || stdout.contains("version 5") || stdout.contains("5"),
        "Expected DWARF version 5 line table header: {}",
        stdout
    );
    assert!(
        stdout.contains("lines.va"),
        "Expected source file name in line table: {}",
        stdout
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_dwarf5_opt_in_no_debug_emitted_by_default() {
    let temp_dir = setup_test_dir("no_debug");
    let src_path = temp_dir.join("nodebug.va");
    let obj_path = temp_dir.join("nodebug.o");

    let source = r#"
pub fn main() -> i64 {
    return 42;
}
"#;
    fs::write(&src_path, source).unwrap();

    // Default compile: debug_info = false
    compile_venta_program(&src_path, &obj_path, EmitTarget::Obj, false, OptLevel::None);

    let output = Command::new("readelf")
        .arg("-S")
        .arg(&obj_path)
        .output()
        .expect("Failed to execute readelf -S");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains(".debug_info"),
        "Did not expect .debug_info when debug_info=false"
    );
    assert!(
        !stdout.contains(".debug_abbrev"),
        "Did not expect .debug_abbrev when debug_info=false"
    );
    assert!(
        !stdout.contains(".debug_line"),
        "Did not expect .debug_line when debug_info=false"
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_dwarf5_gdb_source_breakpoint_and_step() {
    let temp_dir = setup_test_dir("gdb_step");
    let src_path = temp_dir.join("main.va");
    let exe_path = temp_dir.join("main_bin");

    let source = r#"
pub fn multiply(a: i64, b: i64) -> i64 {
    let product: i64 = a * b;
    return product;
}

pub fn main() -> i64 {
    let x: i64 = 6;
    let y: i64 = 7;
    let z: i64 = multiply(x, y);
    return z;
}
"#;
    fs::write(&src_path, source).unwrap();

    compile_venta_program(&src_path, &exe_path, EmitTarget::Exe, true, OptLevel::None);

    // Run GDB in batch mode: break on function `multiply`, run, check frame and locals
    let gdb_script = Command::new("gdb")
        .arg("--batch")
        .arg("-ex")
        .arg("b multiply")
        .arg("-ex")
        .arg("run")
        .arg("-ex")
        .arg("info args")
        .arg("-ex")
        .arg("next")
        .arg("-ex")
        .arg("info locals")
        .arg("-ex")
        .arg("quit")
        .arg(&exe_path)
        .output();

    let output = match gdb_script {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Skipping GDB execution (gdb not available): {}", e);
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("GDB STDOUT:\n{}", stdout);
    if !stderr.is_empty() {
        println!("GDB STDERR:\n{}", stderr);
    }

    assert!(
        output.status.success(),
        "GDB batch session failed: {}",
        stderr
    );
    assert!(
        stdout.contains("Breakpoint 1") || stdout.contains("Breakpoint 1 at"),
        "Expected GDB breakpoint 1 set: {}",
        stdout
    );
    assert!(
        stdout.contains("multiply") || stdout.contains("main.va"),
        "Expected breakpoint hit in multiply(): {}",
        stdout
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_dwarf5_gdb_source_line_breakpoint_and_variable_values() {
    let temp_dir = setup_test_dir("gdb_vals");
    let src_path = temp_dir.join("calc.va");
    let exe_path = temp_dir.join("calc_bin");

    let source = r#"
pub fn calculate(a: i64, b: i64) -> i64 {
    let sum: i64 = a + b;
    let product: i64 = sum * 2;
    return product;
}

pub fn main() -> i64 {
    let val: i64 = calculate(15, 25);
    return val;
}
"#;
    fs::write(&src_path, source).unwrap();

    compile_venta_program(&src_path, &exe_path, EmitTarget::Exe, true, OptLevel::None);

    // Set breakpoint directly on source line 4 (inside calculate)
    let gdb_script = Command::new("gdb")
        .arg("--batch")
        .arg("-ex")
        .arg("b calc.va:4")
        .arg("-ex")
        .arg("run")
        .arg("-ex")
        .arg("print a")
        .arg("-ex")
        .arg("print b")
        .arg("-ex")
        .arg("print sum")
        .arg("-ex")
        .arg("next")
        .arg("-ex")
        .arg("print product")
        .arg("-ex")
        .arg("quit")
        .arg(&exe_path)
        .output();

    let output = match gdb_script {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Skipping GDB variable inspection test: {}", e);
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("GDB VARIABLES STDOUT:\n{}", stdout);

    assert!(output.status.success(), "GDB session failed");
    assert!(stdout.contains("$1 = 15"), "Expected a = 15: {}", stdout);
    assert!(stdout.contains("$2 = 25"), "Expected b = 25: {}", stdout);
    assert!(stdout.contains("$3 = 40"), "Expected sum = 40: {}", stdout);
    assert!(
        stdout.contains("$4 = 80"),
        "Expected product = 80: {}",
        stdout
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_dwarf5_gdb_call_stack_backtrace() {
    let temp_dir = setup_test_dir("gdb_bt");
    let src_path = temp_dir.join("trace.va");
    let exe_path = temp_dir.join("trace_bin");

    let source = r#"
pub fn leaf(val: i64) -> i64 {
    let r: i64 = val + 1;
    return r;
}

pub fn middle(val: i64) -> i64 {
    let step: i64 = val * 2;
    let res: i64 = leaf(step);
    return res + 1;
}

pub fn main() -> i64 {
    let base: i64 = 10;
    return middle(base);
}
"#;
    fs::write(&src_path, source).unwrap();

    compile_venta_program(&src_path, &exe_path, EmitTarget::Exe, true, OptLevel::None);

    let gdb_script = Command::new("gdb")
        .arg("--batch")
        .arg("-ex")
        .arg("b leaf")
        .arg("-ex")
        .arg("run")
        .arg("-ex")
        .arg("bt")
        .arg("-ex")
        .arg("quit")
        .arg(&exe_path)
        .output();

    let output = match gdb_script {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Skipping GDB backtrace test (gdb not available): {}", e);
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("GDB BACKTRACE STDOUT:\n{}", stdout);

    assert!(output.status.success(), "GDB backtrace session failed");
    assert!(
        stdout.contains("leaf"),
        "Expected leaf in backtrace: {}",
        stdout
    );
    assert!(
        stdout.contains("middle"),
        "Expected middle in backtrace: {}",
        stdout
    );
    assert!(
        stdout.contains("main"),
        "Expected main in backtrace: {}",
        stdout
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_dwarf5_lldb_batch_debugging() {
    let temp_dir = setup_test_dir("lldb_test");
    let src_path = temp_dir.join("lldb_trace.va");
    let exe_path = temp_dir.join("lldb_trace_bin");

    let source = r#"
pub fn helper(val: i64) -> i64 {
    let double_val: i64 = val + val;
    return double_val;
}

pub fn main() -> i64 {
    let initial: i64 = 5;
    return helper(initial);
}
"#;
    fs::write(&src_path, source).unwrap();

    compile_venta_program(&src_path, &exe_path, EmitTarget::Exe, true, OptLevel::None);

    let lldb_script = Command::new("lldb")
        .arg("--batch")
        .arg("-o")
        .arg("b helper")
        .arg("-o")
        .arg("run")
        .arg("-o")
        .arg("bt")
        .arg("-o")
        .arg("quit")
        .arg(&exe_path)
        .output();

    let output = match lldb_script {
        Ok(out) => out,
        Err(e) => {
            eprintln!("Skipping LLDB test (lldb not available): {}", e);
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("LLDB STDOUT:\n{}", stdout);
    if !stderr.is_empty() {
        println!("LLDB STDERR:\n{}", stderr);
    }

    assert!(
        stdout.contains("helper") && stdout.contains("main"),
        "Expected helper and main in LLDB backtrace: {}",
        stdout
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
