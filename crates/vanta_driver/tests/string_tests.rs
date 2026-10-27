//! Comprehensive integration and differential tests for Milestone 14:
//! Owned Strings, UTF-8 Operations, Safe Slicing, and String Memory Safety.

use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

/// Helper to compile and run a single-file Vanta program, checking stdout and exit code under -O0 and -O1.
fn run_differential_stdout_test(
    name: &str,
    source: &str,
    expected_exit_code: i32,
    expected_stdout: &[u8],
) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", name));
    fs::write(&src_path, source).unwrap();

    // 1. Unoptimized build (-O0)
    let out_o0 = temp_dir.path().join(format!("{}_o0", name));
    let opts_o0 = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_o0.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        ..Default::default()
    };
    let mut session_o0 = CompilerSession::new();
    let res_o0 = session_o0.compile(&opts_o0);
    if res_o0.is_err() {
        eprintln!("{}", session_o0.render_diagnostics(DiagnosticFormat::Human));
    }
    assert!(res_o0.is_ok(), "O0 Compilation failed: {:?}", res_o0.err());
    let output_o0 = Command::new(&out_o0).output().expect("Execute O0 binary");
    eprintln!("[{}] O0 status: {:?}, stdout: {:?}, stderr: {:?}", name, output_o0.status, String::from_utf8_lossy(&output_o0.stdout), String::from_utf8_lossy(&output_o0.stderr));
    assert_eq!(
        output_o0.status.code(),
        Some(expected_exit_code),
        "O0 exit code mismatch for {}",
        name
    );
    assert_eq!(
        output_o0.stdout.as_slice(),
        expected_stdout,
        "O0 stdout mismatch for {}: got {:?}, expected {:?}",
        name,
        String::from_utf8_lossy(&output_o0.stdout),
        String::from_utf8_lossy(expected_stdout)
    );

    // 2. Optimized build (-O1)
    let out_o1 = temp_dir.path().join(format!("{}_o1", name));
    let opts_o1 = CompilerOptions {
        input_file: src_path,
        output_file: out_o1.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        ..Default::default()
    };
    let mut session_o1 = CompilerSession::new();
    let res_o1 = session_o1.compile(&opts_o1);
    if res_o1.is_err() {
        eprintln!("{}", session_o1.render_diagnostics(DiagnosticFormat::Human));
    }
    assert!(res_o1.is_ok(), "O1 Compilation failed: {:?}", res_o1.err());
    let output_o1 = Command::new(&out_o1).output().expect("Execute O1 binary");
    assert_eq!(
        output_o1.status.code(),
        Some(expected_exit_code),
        "O1 exit code mismatch for {}",
        name
    );
    assert_eq!(
        output_o1.stdout.as_slice(),
        expected_stdout,
        "O1 stdout mismatch for {}: got {:?}, expected {:?}",
        name,
        String::from_utf8_lossy(&output_o1.stdout),
        String::from_utf8_lossy(expected_stdout)
    );
}

/// Helper to compile a program expecting a specific diagnostic code.
fn check_compile_error(source: &str, expected_code: &str) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("main.va");
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        std_path: None,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_err(),
        "Expected compilation failure for code {}",
        expected_code
    );

    let json_diag = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(
        json_diag.contains(expected_code),
        "Diagnostic output missing code {}: {}",
        expected_code,
        json_diag
    );
}

// ============================================================================
// Phase 1: Compile-Fail Borrow Checker & Type Safety Tests
// ============================================================================

#[test]
fn test_compile_fail_push_str_while_borrowed() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let mut s = String::from_str("hello");
    let view = s.as_str();
    s.push_str(" world");
    print_str(view);
    return 0;
}
"#;
    check_compile_error(src, "E0306");
}

#[test]
fn test_compile_fail_clear_while_borrowed() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let mut s = String::from_str("hello");
    let view = s.as_str();
    s.clear();
    print_str(view);
    return 0;
}
"#;
    check_compile_error(src, "E0306");
}

#[test]
fn test_compile_fail_move_while_borrowed() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let s = String::from_str("hello");
    let view = s.as_str();
    let s2 = s;
    print_str(view);
    return 0;
}
"#;
    check_compile_error(src, "E0306");
}

#[test]
fn test_compile_fail_push_str_non_str_arg() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let mut s = String::new();
    s.push_str(42);
    return 0;
}
"#;
    check_compile_error(src, "E0201");
}

#[test]
fn test_compile_fail_slice_bytes_non_int_arg() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let s = String::from_str("hello");
    let opt = s.slice_bytes("start", 3);
    return 0;
}
"#;
    check_compile_error(src, "E0201");
}

// ============================================================================
// Phase 2: String Construction, Growth, and Basic Inspection
// ============================================================================

#[test]
fn test_string_new_empty() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let s = String::new();
    print_i64(s.len_bytes());
    print_i64(s.capacity());
    if s.is_empty() {
        print_i64(1);
    } else {
        print_i64(0);
    }
    print_i64(vanta_active_allocs());
    return 0;
}
"#;
    // Expected output: len=0, cap=0, is_empty=1, active_allocs=0 (no allocation for empty String)
    run_differential_stdout_test("string_new_empty", src, 0, b"0010");
}

#[test]
fn test_string_from_str_ascii() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let s = String::from_str("Hello, Vanta!");
    print_str(s.as_str());
    print_i64(s.len_bytes());
    if s.is_empty() {
        print_i64(1);
    } else {
        print_i64(0);
    }
    return 0;
}
"#;
    run_differential_stdout_test("string_from_str_ascii", src, 0, b"Hello, Vanta!130");
}

#[test]
fn test_string_from_str_utf8() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    // "Привет" is 6 Cyrillic characters = 12 bytes in UTF-8
    let s = String::from_str("Привет");
    print_str(s.as_str());
    print_i64(s.len_bytes());
    return 0;
}
"#;
    run_differential_stdout_test("string_from_str_utf8", src, 0, "Привет12".as_bytes());
}

#[test]
fn test_string_embedded_null() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    // "foo\0bar" contains an embedded NUL byte and has length 7
    let s = String::from_str("foo\0bar");
    print_i64(s.len_bytes());
    print_str(s.as_str());
    return 0;
}
"#;
    run_differential_stdout_test("string_embedded_null", src, 0, b"7foo\0bar");
}

#[test]
fn test_string_push_str_growth() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let mut s = String::new();
    s.push_str("Vanta");
    s.push_str(" ");
    s.push_str("Systems");
    s.push_str(" Programming");
    print_str(s.as_str());
    print_i64(s.len_bytes());
    if s.capacity() >= s.len_bytes() {
        print_i64(1);
    } else {
        print_i64(0);
    }
    return 0;
}
"#;
    run_differential_stdout_test(
        "string_push_str_growth",
        src,
        0,
        b"Vanta Systems Programming251",
    );
}

#[test]
fn test_string_clear_and_reuse() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let mut s = String::from_str("Initial buffer allocation");
    let init_cap = s.capacity();
    s.clear();
    print_i64(s.len_bytes());
    if s.is_empty() {
        print_i64(1);
    } else {
        print_i64(0);
    }
    if s.capacity() == init_cap {
        print_i64(1);
    } else {
        print_i64(0);
    }
    s.push_str("Reused");
    print_str(s.as_str());
    print_i64(s.len_bytes());
    return 0;
}
"#;
    run_differential_stdout_test(
        "string_clear_and_reuse",
        src,
        0,
        b"011Reused6",
    );
}

#[test]
fn test_string_clone_independence() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let mut s1 = String::from_str("Original");
    let s2 = s1.clone();
    s1.push_str(" modified");
    print_str(s1.as_str());
    print_str(s2.as_str());
    print_i64(s1.len_bytes());
    print_i64(s2.len_bytes());
    return 0;
}
"#;
    run_differential_stdout_test(
        "string_clone_independence",
        src,
        0,
        b"Original modifiedOriginal178",
    );
}

#[test]
fn test_string_equals() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let s = String::from_str("apple");
    if s.equals("apple") {
        print_i64(1);
    } else {
        print_i64(0);
    }
    if s.equals("app") {
        print_i64(1);
    } else {
        print_i64(0);
    }
    if s.equals("apples") {
        print_i64(1);
    } else {
        print_i64(0);
    }
    if s.equals("banana") {
        print_i64(1);
    } else {
        print_i64(0);
    }
    let empty = String::new();
    if empty.equals("") {
        print_i64(1);
    } else {
        print_i64(0);
    }
    return 0;
}
"#;
    run_differential_stdout_test("string_equals", src, 0, b"10001");
}

// ============================================================================
// Phase 3: Safe UTF-8 Slicing (slice_bytes)
// ============================================================================

#[test]
fn test_string_slice_bytes_valid_ascii() {
    let src = r#"
import std::string::String;
import std::option::Option;

fn main() -> i64 {
    let s = String::from_str("0123456789");
    let sub_opt = s.slice_bytes(2, 7);
    match sub_opt {
        Option::Some(sub) => {
            print_str(sub);
            print_i64(sub.len());
        }
        Option::None => {
            print_i64(-1);
        }
    }
    return 0;
}
"#;
    run_differential_stdout_test("string_slice_bytes_valid_ascii", src, 0, b"234565");
}

#[test]
fn test_str_slice_bytes_method() {
    let src = r#"
import std::option::Option;

fn main() -> i64 {
    let text = "Vanta Systems";
    let sub_opt = text.slice_bytes(0, 5);
    match sub_opt {
        Option::Some(sub) => {
            print_str(sub);
            print_i64(sub.len());
        }
        Option::None => {
            print_i64(-1);
        }
    }
    return 0;
}
"#;
    run_differential_stdout_test("str_slice_bytes_method", src, 0, b"Vanta5");
}

#[test]
fn test_string_slice_bytes_invalid_bounds() {
    let src = r#"
import std::string::String;
import std::option::Option;

fn main() -> i64 {
    let s = String::from_str("hello");
    // 1. start > end
    match s.slice_bytes(4, 2) {
        Option::Some(_) => print_i64(1),
        Option::None => print_i64(0),
    }
    // 2. start < 0
    match s.slice_bytes(-1, 3) {
        Option::Some(_) => print_i64(1),
        Option::None => print_i64(0),
    }
    // 3. end > len
    match s.slice_bytes(0, 10) {
        Option::Some(_) => print_i64(1),
        Option::None => print_i64(0),
    }
    return 0;
}
"#;
    run_differential_stdout_test("string_slice_bytes_invalid_bounds", src, 0, b"000");
}

#[test]
fn test_string_slice_bytes_utf8_continuation_rejection() {
    let src = r#"
import std::string::String;
import std::option::Option;

fn main() -> i64 {
    // "🦀" (Ferris crab) is 4 bytes: 0xF0 0x9F 0xA6 0x80
    let s = String::from_str("🦀");
    print_i64(s.len_bytes()); // 4

    // Valid full slice: [0..4]
    match s.slice_bytes(0, 4) {
        Option::Some(sub) => print_str(sub),
        Option::None => print_i64(-1),
    }

    // Invalid boundary: index 1 is a continuation byte
    match s.slice_bytes(1, 4) {
        Option::Some(_) => print_i64(100),
        Option::None => print_i64(0),
    }

    // Invalid boundary: index 2 is a continuation byte
    match s.slice_bytes(0, 2) {
        Option::Some(_) => print_i64(100),
        Option::None => print_i64(0),
    }

    // Invalid boundary: index 3 is a continuation byte
    match s.slice_bytes(0, 3) {
        Option::Some(_) => print_i64(100),
        Option::None => print_i64(0),
    }

    return 0;
}
"#;
    run_differential_stdout_test(
        "string_slice_bytes_utf8_continuation_rejection",
        src,
        0,
        "4🦀000".as_bytes(),
    );
}

// ============================================================================
// Phase 4: Memory Management, Nested Containers, and Drop Glue
// ============================================================================

#[test]
fn test_string_active_allocs_clean_drop() {
    let src = r#"
import std::string::String;

fn make_and_drop_strings() {
    let s1 = String::from_str("First allocated buffer");
    let mut s2 = String::from_str("Second buffer");
    s2.push_str(" which is grown");
    let s3 = s2.clone();
    print_i64(vanta_active_allocs()); // 3 active allocations
}

fn main() -> i64 {
    make_and_drop_strings();
    // After function return, all 3 strings must be dropped and active_allocs must be 0
    print_i64(vanta_active_allocs());
    return 0;
}
"#;
    run_differential_stdout_test(
        "string_active_allocs_clean_drop",
        src,
        0,
        b"30",
    );
}

#[test]
fn test_string_reassignment_drop_old() {
    let src = r#"
import std::string::String;

fn main() -> i64 {
    let mut s = String::from_str("old buffer");
    print_i64(vanta_active_allocs()); // 1
    s = String::from_str("new buffer");
    print_i64(vanta_active_allocs()); // still 1 (old buffer was freed before replacing)
    print_str(s.as_str());
    return 0;
}
"#;
    run_differential_stdout_test(
        "string_reassignment_drop_old",
        src,
        0,
        b"11new buffer",
    );
}

#[test]
fn test_option_string_drop() {
    let src = r#"
import std::string::String;
import std::option::Option;

fn test_opt() {
    let s = String::from_str("inside option");
    let opt = Option::Some(s);
    print_i64(vanta_active_allocs()); // 1
}

fn main() -> i64 {
    test_opt();
    print_i64(vanta_active_allocs()); // 0
    return 0;
}
"#;
    run_differential_stdout_test("option_string_drop", src, 0, b"10");
}

#[test]
fn test_vec_string_push_and_drop() {
    let src = r#"
import std::string::String;

fn run_vec_strings() {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from_str("Alpha"));
    v.push(String::from_str("Beta"));
    v.push(String::from_str("Gamma"));
    print_i64(v.len()); // 3
    // Active allocations: 1 for Vec data buffer + 3 for String buffers = 4
    print_i64(vanta_active_allocs());
}

fn main() -> i64 {
    run_vec_strings();
    // Vector drop must free strings first, then vector buffer -> 0
    print_i64(vanta_active_allocs());
    return 0;
}
"#;
    run_differential_stdout_test(
        "vec_string_push_and_drop",
        src,
        0,
        b"340",
    );
}

#[test]
fn test_struct_containing_string() {
    let src = r#"
import std::string::String;

struct Person {
    name: String,
    age: i64,
}

fn test_struct() {
    let p = Person {
        name: String::from_str("Ada Lovelace"),
        age: 36,
    };
    print_str(p.name.as_str());
    print_i64(p.age);
    print_i64(vanta_active_allocs()); // 1
}

fn main() -> i64 {
    test_struct();
    print_i64(vanta_active_allocs()); // 0
    return 0;
}
"#;
    run_differential_stdout_test(
        "struct_containing_string",
        src,
        0,
        b"Ada Lovelace3610",
    );
}
