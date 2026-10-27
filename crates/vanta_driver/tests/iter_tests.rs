//! Comprehensive integration and differential tests for Milestone 12:
//! Iterator Protocols, `for` Loops, Range Iteration, and Collection Integration.

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
        "Expected diagnostic {} in output:\n{}",
        expected_code,
        json_diag
    );
}

#[test]
fn test_range_basic_iteration() {
    let src = r#"
import std::iter::range_i64;

fn main() -> i64 {
    for x in range_i64(0, 5) {
        print_i64(x);
    }
    0
}
"#;
    run_differential_stdout_test("range_basic", src, 0, b"01234");
}

#[test]
fn test_range_empty_cases() {
    let src = r#"
import std::iter::range_i64;

fn main() -> i64 {
    let mut count = 0;
    for x in range_i64(5, 5) {
        count = count + 1;
    }
    for x in range_i64(10, 2) {
        count = count + 1;
    }
    print_i64(count);
    0
}
"#;
    run_differential_stdout_test("range_empty", src, 0, b"0");
}

#[test]
fn test_range_negative_zero_crossing() {
    let src = r#"
import std::iter::range_i64;

fn main() -> i64 {
    for x in range_i64(-3, 3) {
        print_i64(x);
    }
    0
}
"#;
    run_differential_stdout_test("range_negative", src, 0, b"-3-2-1012");
}

#[test]
fn test_range_single_element() {
    let src = r#"
import std::iter::range_i64;

fn main() -> i64 {
    for x in range_i64(42, 43) {
        print_i64(x);
    }
    0
}
"#;
    run_differential_stdout_test("range_single", src, 0, b"42");
}

#[test]
fn test_range_near_i64_max() {
    let src = r#"
import std::iter::range_i64;

fn main() -> i64 {
    for x in range_i64(9223372036854775805, 9223372036854775807) {
        print_i64(x);
    }
    0
}
"#;
    run_differential_stdout_test(
        "range_near_max",
        src,
        0,
        b"92233720368547758059223372036854775806",
    );
}

#[test]
fn test_range_nested_loops() {
    let src = r#"
import std::iter::range_i64;

fn main() -> i64 {
    for i in range_i64(0, 3) {
        for j in range_i64(10, 12) {
            print_i64(i + j);
        }
    }
    0
}
"#;
    run_differential_stdout_test("range_nested", src, 0, b"101111121213");
}

#[test]
fn test_for_break_and_continue() {
    let src = r#"
import std::iter::range_i64;

fn main() -> i64 {
    for x in range_i64(0, 10) {
        if x == 2 {
            continue;
        }
        if x == 5 {
            break;
        }
        print_i64(x);
    }
    0
}
"#;
    run_differential_stdout_test("for_break_cont", src, 0, b"0134");
}

#[test]
fn test_user_defined_iterator_countdown() {
    let src = r#"
import std::iter::Iterator;
import std::option::Option;

struct Countdown {
    curr: i64,
}

impl Iterator<i64> for Countdown {
    fn next(self: &mut Countdown) -> Option<i64> {
        if self.curr > 0 {
            let v = self.curr;
            self.curr = self.curr - 1;
            Option::Some(v)
        } else {
            Option::None
        }
    }
}

fn main() -> i64 {
    let cd = Countdown { curr: 4 };
    for val in cd {
        print_i64(val);
    }
    0
}
"#;
    run_differential_stdout_test("custom_iter_countdown", src, 0, b"4321");
}

#[test]
fn test_user_defined_iterator_step() {
    let src = r#"
import std::iter::Iterator;
import std::option::Option;

struct StepRange {
    curr: i64,
    stop: i64,
    step: i64,
}

impl Iterator<i64> for StepRange {
    fn next(self: &mut StepRange) -> Option<i64> {
        if self.curr < self.stop {
            let v = self.curr;
            self.curr = self.curr + self.step;
            Option::Some(v)
        } else {
            Option::None
        }
    }
}

fn main() -> i64 {
    let sr = StepRange { curr: 10, stop: 25, step: 5 };
    for val in sr {
        print_i64(val);
    }
    0
}
"#;
    run_differential_stdout_test("custom_iter_step", src, 0, b"101520");
}

#[test]
fn test_single_evaluation_of_iterable() {
    let src = r#"
import std::iter::Iterator;
import std::iter::RangeI64;
import std::option::Option;

fn create_range() -> RangeI64 {
    print_i64(999);
    RangeI64 { current: 1, end: 4 }
}

fn main() -> i64 {
    for x in create_range() {
        print_i64(x);
    }
    0
}
"#;
    run_differential_stdout_test("single_eval", src, 0, b"999123");
}

#[test]
fn test_diagnostic_e0901_non_iterable_primitive() {
    let src = r#"
fn main() -> i64 {
    for x in 42 {
        print_i64(x);
    }
    0
}
"#;
    check_compile_error(src, "E0901");
}

#[test]
fn test_diagnostic_e0901_vec_deferred() {
    let src = r#"
fn main() -> i64 {
    let v: Vec<i64> = Vec::new();
    for x in v {
        print_i64(x);
    }
    0
}
"#;
    check_compile_error(src, "E0901");
}

#[test]
fn test_diagnostic_e0901_vec_ref_deferred() {
    let src = r#"
fn main() -> i64 {
    let v: Vec<i64> = Vec::new();
    for x in &v {
        print_i64(*x);
    }
    0
}
"#;
    check_compile_error(src, "E0901");
}
