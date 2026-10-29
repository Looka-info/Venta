//! Comprehensive integration and differential tests for Milestone 11:
//! Standard Library Foundation, Option<T>, Result<T, E>, and Error Propagation.

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
        "Diagnostic output missing code {}: {}",
        expected_code,
        json_diag
    );
}

// ============================================================================
// Phase 2: Option<T> Tests
// ============================================================================

#[test]
fn test_option_construction_and_matching() {
    let source = r#"
import std::option::Option;

fn describe_opt(opt: Option<i64>) -> i64 {
    match opt {
        Option::Some(val) => val,
        Option::None => -1,
    }
}

fn main() {
    let s = Option::Some(42);
    let n: Option<i64> = Option::None;
    print_i64(describe_opt(s));
    print_i64(describe_opt(n));
}
"#;
    run_differential_stdout_test("opt_construct_match", source, 0, b"42-1");
}

#[test]
fn test_option_builtin_methods() {
    let source = r#"
import std::option::Option;

fn main() {
    let s = Option::Some(100);
    let n: Option<i64> = Option::None;

    if s.is_some() {
        print_i64(1);
    } else {
        print_i64(0);
    }

    if s.is_none() {
        print_i64(1);
    } else {
        print_i64(0);
    }

    if n.is_some() {
        print_i64(1);
    } else {
        print_i64(0);
    }

    if n.is_none() {
        print_i64(1);
    } else {
        print_i64(0);
    }

    let v1 = s.unwrap_or(999);
    print_i64(v1);

    let v2 = n.unwrap_or(999);
    print_i64(v2);

    let v3 = s.unwrap();
    print_i64(v3);
}
"#;
    run_differential_stdout_test("opt_builtin_methods", source, 0, b"1001100999100");
}

#[test]
fn test_option_vector_payload_cleanup() {
    let source = r#"
import std::option::Option;

fn test_some_cleanup() {
    let mut v = Vec_new();
    v.push(10);
    v.push(20);
    let opt = Option::Some(v);
    print_i64(vanta_active_allocs());
}

fn test_none_cleanup() {
    let opt: Option<Vec<i64> > = Option::None;
    print_i64(vanta_active_allocs());
}

fn main() {
    test_some_cleanup();
    // Scope ended, vector in Some dropped -> 0 active allocs
    print_i64(vanta_active_allocs());

    test_none_cleanup();
    // Scope ended, None has no heap payload -> 0 active allocs
    print_i64(vanta_active_allocs());
}
"#;
    run_differential_stdout_test("opt_vec_cleanup", source, 0, b"1000");
}

// ============================================================================
// Phase 3: Result<T, E> Tests
// ============================================================================

#[test]
fn test_result_construction_and_matching() {
    let source = r#"
import std::result::Result;

fn check_status(res: Result<i64, i64>) -> i64 {
    match res {
        Result::Ok(val) => val,
        Result::Err(err) => -err,
    }
}

fn main() {
    let ok: Result<i64, i64> = Result::Ok(200);
    let err: Result<i64, i64> = Result::Err(404);
    print_i64(check_status(ok));
    print_i64(check_status(err));
}
"#;
    run_differential_stdout_test("res_construct_match", source, 0, b"200-404");
}

#[test]
fn test_result_builtin_methods() {
    let source = r#"
import std::result::Result;

fn main() {
    let ok: Result<i64, i64> = Result::Ok(77);
    let err: Result<i64, i64> = Result::Err(500);

    if ok.is_ok() {
        print_i64(1);
    } else {
        print_i64(0);
    }

    if ok.is_err() {
        print_i64(1);
    } else {
        print_i64(0);
    }

    if err.is_ok() {
        print_i64(1);
    } else {
        print_i64(0);
    }

    if err.is_err() {
        print_i64(1);
    } else {
        print_i64(0);
    }

    let val1 = ok.unwrap_or(0);
    print_i64(val1);

    let val2 = err.unwrap_or(0);
    print_i64(val2);

    let val3 = ok.unwrap();
    print_i64(val3);
}
"#;
    run_differential_stdout_test("res_builtin_methods", source, 0, b"100177077");
}

#[test]
fn test_result_vector_payload_cleanup() {
    let source = r#"
import std::result::Result;

fn test_ok_cleanup() {
    let mut v = Vec_new();
    v.push(1);
    v.push(2);
    v.push(3);
    let res: Result<Vec<i64>, i64> = Result::Ok(v);
    print_i64(vanta_active_allocs());
}

fn test_err_cleanup() {
    let res: Result<Vec<i64>, i64> = Result::Err(404);
    print_i64(vanta_active_allocs());
}

fn main() {
    test_ok_cleanup();
    print_i64(vanta_active_allocs());

    test_err_cleanup();
    print_i64(vanta_active_allocs());
}
"#;
    run_differential_stdout_test("res_vec_cleanup", source, 0, b"1000");
}

// ============================================================================
// Phase 4: ? Operator Propagation Tests
// ============================================================================

#[test]
fn test_try_operator_result_success_and_early_return() {
    let source = r#"
import std::result::Result;

fn compute_step(val: i64) -> Result<i64, i64> {
    if val > 0 {
        Result::Ok(val * 10)
    } else {
        Result::Err(-99)
    }
}

fn pipeline(val: i64) -> Result<i64, i64> {
    let step1 = compute_step(val)?;
    let step2 = compute_step(step1 + 5)?;
    Result::Ok(step2 + 1)
}

fn main() {
    // 1. Success path: val = 2 -> step1 = 20 -> compute_step(25) = 250 -> 251
    let r1 = pipeline(2);
    match r1 {
        Result::Ok(v) => print_i64(v),
        Result::Err(e) => print_i64(e),
    }

    // 2. Early return at step 1: val = -1 -> Err(-99)
    let r2 = pipeline(-1);
    match r2 {
        Result::Ok(v) => print_i64(v),
        Result::Err(e) => print_i64(e),
    }
}
"#;
    run_differential_stdout_test("try_result_pipeline", source, 0, b"251-99");
}

#[test]
fn test_try_operator_option_success_and_early_return() {
    let source = r#"
import std::option::Option;

fn lookup(key: i64) -> Option<i64> {
    if key == 1 {
        Option::Some(10)
    } else {
        if key == 2 {
            Option::Some(20)
        } else {
            Option::None
        }
    }
}

fn find_sum(k1: i64, k2: i64) -> Option<i64> {
    let v1 = lookup(k1)?;
    let v2 = lookup(k2)?;
    Option::Some(v1 + v2)
}

fn main() {
    let s1 = find_sum(1, 2);
    match s1 {
        Option::Some(v) => print_i64(v),
        Option::None => print_i64(-1),
    }

    let s2 = find_sum(1, 99);
    match s2 {
        Option::Some(v) => print_i64(v),
        Option::None => print_i64(-1),
    }
}
"#;
    run_differential_stdout_test("try_option_pipeline", source, 0, b"30-1");
}

#[test]
fn test_try_operator_in_expression() {
    let source = r#"
import std::result::Result;

fn get_val(code: i64) -> Result<i64, i64> {
    if code >= 0 {
        Result::Ok(code * 2)
    } else {
        Result::Err(code)
    }
}

fn add_three(c1: i64, c2: i64, c3: i64) -> Result<i64, i64> {
    let total = get_val(c1)? + get_val(c2)? + get_val(c3)?;
    Result::Ok(total)
}

fn main() {
    let ok = add_three(5, 10, 15);
    match ok {
        Result::Ok(v) => print_i64(v),
        Result::Err(e) => print_i64(e),
    }

    let err = add_three(5, -7, 15);
    match err {
        Result::Ok(v) => print_i64(v),
        Result::Err(e) => print_i64(e),
    }
}
"#;
    run_differential_stdout_test("try_in_expression", source, 0, b"60-7");
}

#[test]
fn test_try_operator_cleanup_on_early_return() {
    let source = r#"
import std::result::Result;

fn fail_op() -> Result<i64, i64> {
    Result::Err(404)
}

fn operation_with_allocations() -> Result<i64, i64> {
    let mut vec1 = Vec_new();
    vec1.push(100);
    vec1.push(200);

    let mut vec2 = Vec_new();
    vec2.push(300);

    // 2 active allocations before ? triggers early return
    print_i64(vanta_active_allocs());

    let _x = fail_op()?;

    Result::Ok(42)
}

fn main() {
    let res = operation_with_allocations();
    match res {
        Result::Ok(v) => print_i64(v),
        Result::Err(e) => print_i64(e),
    }

    // Both local vectors in operation_with_allocations must be dropped on early return
    print_i64(vanta_active_allocs());
}
"#;
    run_differential_stdout_test("try_cleanup_early_ret", source, 0, b"24040");
}

#[test]
fn test_try_operator_propagating_owned_error() {
    let source = r#"
import std::result::Result;

fn create_error() -> Result<i64, Vec<i64> > {
    let mut err_vec = Vec_new();
    err_vec.push(404);
    err_vec.push(500);
    Result::Err(err_vec)
}

fn caller() -> Result<i64, Vec<i64> > {
    let val = create_error()?;
    Result::Ok(val + 1)
}

fn main() {
    {
        let res = caller();
        match res {
            Result::Ok(v) => print_i64(v),
            Result::Err(e) => {
                print_i64(e.len());
                print_i64(e[0]);
                print_i64(e[1]);
                // Error payload active in arm scope
                print_i64(vanta_active_allocs());
            },
        }
        // Scope ended, e dropped
        print_i64(vanta_active_allocs());
    }
}
"#;
    run_differential_stdout_test("try_owned_error_prop", source, 0, b"240450010");
}

#[test]
fn test_generic_function_with_try_operator() {
    let source = r#"
import std::result::Result;

fn wrap<T>(val: T) -> Result<T, i64> {
    Result::Ok(val)
}

fn pass_through<T>(val: T) -> Result<T, i64> {
    let unwrapped = wrap(val)?;
    Result::Ok(unwrapped)
}

fn main() {
    let res = pass_through(888);
    match res {
        Result::Ok(v) => print_i64(v),
        Result::Err(e) => print_i64(e),
    }
}
"#;
    run_differential_stdout_test("generic_try", source, 0, b"888");
}

// ============================================================================
// Phase 5: Compile-Fail Diagnostic Tests (E0801 - E0805)
// ============================================================================

#[test]
fn test_compile_fail_e0801_invalid_try_operand() {
    let source = r#"
fn main() {
    let x = 42;
    let y = x?;
}
"#;
    check_compile_error(source, "E0801");
}

#[test]
fn test_compile_fail_e0802_incompatible_return_type() {
    let source = r#"
import std::result::Result;

fn compute() -> i64 {
    let r: Result<i64, i64> = Result::Ok(10);
    let val = r?;
    val
}

fn main() {
    compute();
}
"#;
    check_compile_error(source, "E0802");
}

#[test]
fn test_compile_fail_e0803_mismatched_result_error_type() {
    let source = r#"
import std::result::Result;

fn sub_step() -> Result<i64, bool> {
    Result::Err(true)
}

fn outer() -> Result<i64, i64> {
    let val = sub_step()?;
    Result::Ok(val)
}

fn main() {
    outer();
}
"#;
    check_compile_error(source, "E0803");
}

#[test]
fn test_compile_fail_e0805_mixing_option_and_result() {
    let source = r#"
import std::option::Option;
import std::result::Result;

fn sub_result() -> Result<i64, i64> {
    Result::Ok(100)
}

fn outer_option() -> Option<i64> {
    let val = sub_result()?;
    Option::Some(val)
}

fn main() {
    outer_option();
}
"#;
    check_compile_error(source, "E0805");
}
