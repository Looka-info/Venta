//! System V AMD64 ABI and CLI reliability regression tests (Milestone 15).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, OptLevel};

fn run_source_test(test_name: &str, source: &str, opt_level: OptLevel, expected_stdout: &[u8]) {
    let temp_dir = std::env::temp_dir().join(format!("vanta_abi_test_{}_{:?}", test_name, opt_level));
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("main.va");
    let exe_path = temp_dir.join("main_bin");
    fs::write(&src_path, source).expect("Failed to write test source");

    let mut session = CompilerSession::new();
    let mut options = CompilerOptions::new(src_path.clone(), exe_path.clone());
    options.opt_level = opt_level;
    options.emit_target = EmitTarget::Exe;
    options.std_path = Some(PathBuf::from("library"));

    let compile_res = session.compile(&options);
    if let Err(e) = &compile_res {
        eprintln!("Compiler error: {}", session.render_diagnostics(vanta_driver::DiagnosticFormat::Human));
        panic!("Compilation failed for {}: {}", test_name, e);
    }

    let output = Command::new(&exe_path)
        .output()
        .expect("Failed to execute compiled test binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Test {} exited with nonzero status: {:?}",
        test_name,
        output.status
    );

    assert_eq!(
        output.stdout,
        expected_stdout,
        "Test {} stdout mismatch!\nGot: {}\nExpected: {}",
        test_name,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(expected_stdout)
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_abi_unit_returning_main_exits_zero() {
    let source = r#"
fn main() {
    print_str("Hello, Vanta ABI!\n");
}
"#;
    run_source_test("unit_main_exit_0", source, OptLevel::None, b"Hello, Vanta ABI!\n");
    run_source_test("unit_main_exit_o1", source, OptLevel::O1, b"Hello, Vanta ABI!\n");
}

#[test]
fn test_abi_string_return_by_value() {
    let source = r#"
import std::string::String;

fn make_string() -> String {
    let s = String::from_str("Owned String via sret");
    return s;
}

fn main() {
    {
        let msg = make_string();
        print_str(msg.as_str());
        print_str("\n");
        // Active alloc inside scope must be exactly 1
        print_i64(vanta_active_allocs());
    }
    // Active alloc after scope must be exactly 0
    print_i64(vanta_active_allocs());
}
"#;
    run_source_test("sret_string_by_value", source, OptLevel::None, b"Owned String via sret\n10");
    run_source_test("sret_string_by_value_o1", source, OptLevel::O1, b"Owned String via sret\n10");
}

#[test]
fn test_abi_vector_return_by_value() {
    let source = r#"
fn make_vector() -> Vec<i64> {
    let mut v = Vec_new();
    v.push(100);
    v.push(200);
    v.push(300);
    return v;
}

fn main() {
    {
        let v = make_vector();
        print_i64(v.len());
        print_i64(v[0]);
        print_i64(v[1]);
        print_i64(v[2]);
        // Allocation active
        print_i64(vanta_active_allocs());
    }
    // Freed exactly once
    print_i64(vanta_active_allocs());
}
"#;
    run_source_test("sret_vector_by_value", source, OptLevel::None, b"310020030010");
    run_source_test("sret_vector_by_value_o1", source, OptLevel::O1, b"310020030010");
}

#[test]
fn test_abi_large_struct_return_by_value() {
    let source = r#"
struct Point3D {
    x: i64,
    y: i64,
    z: i64,
}

fn make_point(x: i64, y: i64, z: i64) -> Point3D {
    Point3D { x: x, y: y, z: z }
}

fn main() {
    let pt = make_point(10, 20, 30);
    print_i64(pt.x);
    print_i64(pt.y);
    print_i64(pt.z);
}
"#;
    run_source_test("sret_large_struct", source, OptLevel::None, b"102030");
    run_source_test("sret_large_struct_o1", source, OptLevel::O1, b"102030");
}

#[test]
fn test_abi_aggregate_return_with_ordinary_and_slice_args() {
    let source = r#"
import std::string::String;

fn build_greeting(prefix: &str, count: i64) -> String {
    let mut s = String::from_str(prefix);
    let mut i = 0;
    while i < count {
        s.push_str("!");
        i = i + 1;
    }
    return s;
}

fn main() {
    let greeting = build_greeting("Hello", 3);
    print_str(greeting.as_str());
    print_str("\n");
}
"#;
    run_source_test("sret_with_args", source, OptLevel::None, b"Hello!!!\n");
    run_source_test("sret_with_args_o1", source, OptLevel::O1, b"Hello!!!\n");
}

#[test]
fn test_abi_nested_aggregate_return_calls() {
    let source = r#"
import std::string::String;

fn step1() -> String {
    String::from_str("nested")
}

fn step2() -> String {
    let mut s = step1();
    s.push_str("->level2");
    return s;
}

fn step3() -> String {
    let mut s = step2();
    s.push_str("->level3");
    return s;
}

fn main() {
    {
        let res = step3();
        print_str(res.as_str());
        print_str("\n");
        print_i64(vanta_active_allocs());
    }
    print_i64(vanta_active_allocs());
}
"#;
    run_source_test("sret_nested_calls", source, OptLevel::None, b"nested->level2->level3\n10");
    run_source_test("sret_nested_calls_o1", source, OptLevel::O1, b"nested->level2->level3\n10");
}

#[test]
fn test_abi_nested_struct_containing_owned_string() {
    let source = r#"
import std::string::String;

struct Person {
    name: String,
    age: i64,
}

fn create_person(name: &str, age: i64) -> Person {
    Person {
        name: String::from_str(name),
        age: age,
    }
}

fn main() {
    {
        let p = create_person("Ada Lovelace", 36);
        print_str(p.name.as_str());
        print_str(", Age: ");
        print_i64(p.age);
        print_str("\n");
        print_i64(vanta_active_allocs());
    }
    print_i64(vanta_active_allocs());
}
"#;
    run_source_test("sret_nested_struct_owned", source, OptLevel::None, b"Ada Lovelace, Age: 36\n10");
    run_source_test("sret_nested_struct_owned_o1", source, OptLevel::O1, b"Ada Lovelace, Age: 36\n10");
}

#[test]
fn test_cli_version_and_help_commands() {
    // 1. --version returns 0 and prints version to stdout
    let out_ver = Command::new("cargo")
        .args(&["run", "--bin", "vantac", "--", "--version"])
        .output()
        .expect("Failed to run vantac --version");
    assert!(out_ver.status.success());
    let stdout_ver = String::from_utf8_lossy(&out_ver.stdout);
    assert!(stdout_ver.contains("vantac 1.0.0-rc.1"), "Unexpected version: {}", stdout_ver);

    // 2. -V returns 0
    let out_v = Command::new("cargo")
        .args(&["run", "--bin", "vantac", "--", "-V"])
        .output()
        .expect("Failed to run vantac -V");
    assert!(out_v.status.success());
    let stdout_v = String::from_utf8_lossy(&out_v.stdout);
    assert!(stdout_v.contains("vantac 1.0.0-rc.1"), "Unexpected -V: {}", stdout_v);

    // 3. --help returns 0 and prints help to stdout
    let out_help = Command::new("cargo")
        .args(&["run", "--bin", "vantac", "--", "--help"])
        .output()
        .expect("Failed to run vantac --help");
    assert!(out_help.status.success());
    let stdout_help = String::from_utf8_lossy(&out_help.stdout);
    assert!(stdout_help.contains("USAGE:"), "Help text missing USAGE");
    assert!(stdout_help.contains("COMMANDS:"), "Help text missing COMMANDS");
    assert!(stdout_help.contains("OPTIONS:"), "Help text missing OPTIONS");

    // 4. Unknown option returns exit code 2 and outputs error to stderr
    let out_unknown = Command::new("cargo")
        .args(&["run", "--bin", "vantac", "--", "--non-existent-option"])
        .output()
        .expect("Failed to run vantac --unknown");
    assert_eq!(out_unknown.status.code(), Some(2));
    let stderr_unknown = String::from_utf8_lossy(&out_unknown.stderr);
    assert!(stderr_unknown.contains("Error: Unknown option '--non-existent-option'"));
}
