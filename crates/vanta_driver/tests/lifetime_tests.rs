//! Comprehensive integration and differential tests for Milestone 13:
//! Lifetime Parameters, Reference Safety, and Immutable Collection Iteration.

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
    let result = session.compile(&options);
    assert!(
        result.is_err(),
        "Expected compilation error with code {}",
        expected_code
    );

    let diags = session.diagnostics.diagnostics();
    let has_code = diags
        .iter()
        .any(|d| d.code.as_deref() == Some(expected_code));
    if !has_code {
        eprintln!("Expected diagnostic code: {}", expected_code);
        eprintln!("Actual diagnostics: {:?}", diags);
    }
    assert!(
        has_code,
        "Expected diagnostic code '{}' was not emitted",
        expected_code
    );
}

// ============================================================================
// PHASE 1 & 2: Compile-Fail Diagnostics
// ============================================================================

#[test]
fn test_diagnostic_e0310_duplicate_lifetime_in_fn() {
    let src = r#"
    fn bad<'a, 'a>(x: &'a i64) -> &'a i64 {
        x
    }
    pub fn main() -> i64 { 0 }
    "#;
    check_compile_error(src, "E0310");
}

#[test]
fn test_diagnostic_e0310_duplicate_lifetime_in_struct() {
    let src = r#"
    struct Holder<'a, 'a> {
        val: &'a i64,
    }
    pub fn main() -> i64 { 0 }
    "#;
    check_compile_error(src, "E0310");
}

#[test]
fn test_diagnostic_e0311_undeclared_lifetime_in_param() {
    let src = r#"
    fn bad(x: &'a i64) -> i64 {
        *x
    }
    pub fn main() -> i64 { 0 }
    "#;
    check_compile_error(src, "E0311");
}

#[test]
fn test_diagnostic_e0311_undeclared_lifetime_in_return() {
    let src = r#"
    fn bad<'a>(x: &'a i64) -> &'b i64 {
        x
    }
    pub fn main() -> i64 { 0 }
    "#;
    check_compile_error(src, "E0311");
}

#[test]
fn test_diagnostic_e0312_return_lifetime_outlives_inputs() {
    let src = r#"
    fn bad<'a, 'b>(x: &'a i64) -> &'b i64 {
        x
    }
    pub fn main() -> i64 { 0 }
    "#;
    check_compile_error(src, "E0312");
}

#[test]
fn test_diagnostic_e0313_ambiguous_lifetime_elision() {
    let src = r#"
    fn choose(a: &i64, b: &i64) -> &i64 {
        a
    }
    pub fn main() -> i64 { 0 }
    "#;
    check_compile_error(src, "E0313");
}

#[test]
fn test_diagnostic_e0314_return_reference_to_local_var() {
    let src = r#"
    fn escape_local() -> &i64 {
        let x = 42;
        &x
    }
    pub fn main() -> i64 { 0 }
    "#;
    check_compile_error(src, "E0314");
}

#[test]
fn test_diagnostic_e0314_return_ref_with_zero_ref_params() {
    let src = r#"
    fn escape_val(x: i64) -> &i64 {
        &x
    }
    pub fn main() -> i64 { 0 }
    "#;
    check_compile_error(src, "E0314");
}

#[test]
fn test_diagnostic_e0306_mutate_during_for_iteration() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(1);
        v.push(2);
        for x in v.iter() {
            v.push(3);
        }
        0
    }
    "#;
    check_compile_error(src, "E0306");
}

#[test]
fn test_diagnostic_e0306_move_vector_during_iteration() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(1);
        let it = v.iter();
        let v2 = v;
        0
    }
    "#;
    check_compile_error(src, "E0306");
}

#[test]
fn test_diagnostic_e0316_into_iter_deferred() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(1);
        v.into_iter();
        0
    }
    "#;
    check_compile_error(src, "E0316");
}

#[test]
fn test_diagnostic_e0316_iter_mut_deferred() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(1);
        v.iter_mut();
        0
    }
    "#;
    check_compile_error(src, "E0316");
}

// ============================================================================
// PHASE 3 & 4: Runtime Differential Execution Tests (-O0 and -O1)
// ============================================================================

#[test]
fn test_valid_lifetime_identity_function() {
    let src = r#"
    fn id<'a>(x: &'a i64) -> &'a i64 {
        x
    }

    pub fn main() -> i64 {
        let val = 42;
        let r = id(&val);
        print_i64(*r);
        0
    }
    "#;
    run_differential_stdout_test("lifetime_identity", src, 0, b"42");
}

#[test]
fn test_valid_lifetime_conditional_choice() {
    let src = r#"
    fn pick<'a>(a: &'a i64, b: &'a i64, choose_first: bool) -> &'a i64 {
        if choose_first {
            a
        } else {
            b
        }
    }

    pub fn main() -> i64 {
        let x = 100;
        let y = 200;
        let r1 = pick(&x, &y, true);
        print_i64(*r1);
        let r2 = pick(&x, &y, false);
        print_i64(*r2);
        0
    }
    "#;
    run_differential_stdout_test("lifetime_pick", src, 0, b"100200");
}

#[test]
fn test_valid_lifetime_elision_single_ref_param() {
    let src = r#"
    fn pass(val: &i64) -> &i64 {
        val
    }

    pub fn main() -> i64 {
        let x = 777;
        let r = pass(&x);
        print_i64(*r);
        0
    }
    "#;
    run_differential_stdout_test("lifetime_elision_single", src, 0, b"777");
}

#[test]
fn test_vector_empty_iter() {
    let src = r#"
    pub fn main() -> i64 {
        let v: Vec<i64> = Vec_new();
        for x in v.iter() {
            print_i64(*x);
        }
        print_i64(999);
        0
    }
    "#;
    run_differential_stdout_test("vec_empty_iter", src, 0, b"999");
}

#[test]
fn test_vector_single_element_iter() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(42);
        for x in v.iter() {
            print_i64(*x);
        }
        0
    }
    "#;
    run_differential_stdout_test("vec_single_iter", src, 0, b"42");
}

#[test]
fn test_vector_multi_element_iter() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(10);
        v.push(20);
        v.push(30);
        for x in v.iter() {
            print_i64(*x);
        }
        0
    }
    "#;
    run_differential_stdout_test("vec_multi_iter", src, 0, b"102030");
}

#[test]
fn test_vector_reallocation_before_iter() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        let mut i = 0;
        while i < 15 {
            v.push(i * 10);
            i = i + 1;
        }

        let mut sum = 0;
        for x in v.iter() {
            sum = sum + *x;
        }
        print_i64(sum);
        0
    }
    "#;
    // Sum of 0*10 + 1*10 + ... + 14*10 = 10 * (14 * 15 / 2) = 1050
    run_differential_stdout_test("vec_realloc_iter", src, 0, b"1050");
}

#[test]
fn test_vector_iter_break_and_continue() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(1);
        v.push(2);
        v.push(3);
        v.push(4);
        v.push(50);
        v.push(6);

        for x in v.iter() {
            if *x == 2 {
                continue;
            }
            if *x == 50 {
                break;
            }
            print_i64(*x);
        }
        0
    }
    "#;
    run_differential_stdout_test("vec_iter_break_continue", src, 0, b"134");
}

#[test]
fn test_vector_mutate_after_iteration() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(1);
        v.push(2);

        for x in v.iter() {
            print_i64(*x);
        }

        // Mutation is valid again after iteration finishes
        v.push(3);
        print_i64(v.len());
        0
    }
    "#;
    run_differential_stdout_test("vec_mutate_after_iter", src, 0, b"123");
}

#[test]
fn test_vector_multiple_consecutive_iterations() {
    let src = r#"
    pub fn main() -> i64 {
        let mut v = Vec_new();
        v.push(11);
        v.push(22);

        for a in v.iter() {
            print_i64(*a);
        }

        for b in v.iter() {
            print_i64(*b);
        }
        0
    }
    "#;
    run_differential_stdout_test("vec_multiple_consecutive_iters", src, 0, b"11221122");
}
