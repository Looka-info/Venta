use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

/// Helper to compile and run a Vanta snippet under both -O0 and -O1, verifying identical behavior.
fn run_differential_test(name: &str, src: &str, expected_exit_code: i32) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", name));
    fs::write(&src_path, src).unwrap();

    // 1. Unoptimized build (-O0)
    let out_o0 = temp_dir.path().join(format!("{}_o0", name));
    let opts_o0 = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_o0.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };
    let mut session_o0 = CompilerSession::new();
    let res_o0 = session_o0.compile(&opts_o0);
    assert!(res_o0.is_ok(), "O0 Compilation failed: {:?}", res_o0.err());
    let status_o0 = Command::new(&out_o0).status().expect("Execute O0 binary");
    assert_eq!(
        status_o0.code(),
        Some(expected_exit_code),
        "O0 exit code mismatch for {}",
        name
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
        ..Default::default()
    };
    let mut session_o1 = CompilerSession::new();
    let res_o1 = session_o1.compile(&opts_o1);
    assert!(res_o1.is_ok(), "O1 Compilation failed: {:?}", res_o1.err());
    let status_o1 = Command::new(&out_o1).status().expect("Execute O1 binary");
    assert_eq!(
        status_o1.code(),
        Some(expected_exit_code),
        "O1 exit code mismatch for {}",
        name
    );

    // 3. Differential check: both must agree
    assert_eq!(
        status_o0.code(),
        status_o1.code(),
        "Differential mismatch between O0 and O1 for {}",
        name
    );
}

/// Helper to compile and run a Vanta snippet under both -O0 and -O1, verifying stdout bytes and exit code.
fn run_differential_stdout_test(
    name: &str,
    src: &str,
    expected_exit_code: i32,
    expected_stdout: &[u8],
) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", name));
    fs::write(&src_path, src).unwrap();

    // 1. Unoptimized build (-O0)
    let out_o0 = temp_dir.path().join(format!("{}_o0", name));
    let opts_o0 = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_o0.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };
    let mut session_o0 = CompilerSession::new();
    let res_o0 = session_o0.compile(&opts_o0);
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
        ..Default::default()
    };
    let mut session_o1 = CompilerSession::new();
    let res_o1 = session_o1.compile(&opts_o1);
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

    // 3. Differential check: both must agree exactly on stdout and exit status
    assert_eq!(
        output_o0.stdout, output_o1.stdout,
        "Differential stdout mismatch between O0 and O1 for {}",
        name
    );
    assert_eq!(
        output_o0.status.code(),
        output_o1.status.code(),
        "Differential exit code mismatch between O0 and O1 for {}",
        name
    );
}

/// Helper to compile and run a Vanta snippet under both -O0 and -O1, verifying stderr bytes and exit code.
fn run_differential_stderr_test(
    name: &str,
    src: &str,
    expected_exit_code: i32,
    expected_stderr: &[u8],
) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", name));
    fs::write(&src_path, src).unwrap();

    // 1. Unoptimized build (-O0)
    let out_o0 = temp_dir.path().join(format!("{}_o0", name));
    let opts_o0 = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_o0.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };
    let mut session_o0 = CompilerSession::new();
    let res_o0 = session_o0.compile(&opts_o0);
    assert!(res_o0.is_ok(), "O0 Compilation failed: {:?}", res_o0.err());
    let output_o0 = Command::new(&out_o0).output().expect("Execute O0 binary");
    assert_eq!(
        output_o0.status.code(),
        Some(expected_exit_code),
        "O0 exit code mismatch for {}",
        name
    );
    assert_eq!(
        output_o0.stderr.as_slice(),
        expected_stderr,
        "O0 stderr mismatch for {}: got {:?}, expected {:?}",
        name,
        String::from_utf8_lossy(&output_o0.stderr),
        String::from_utf8_lossy(expected_stderr)
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
        ..Default::default()
    };
    let mut session_o1 = CompilerSession::new();
    let res_o1 = session_o1.compile(&opts_o1);
    assert!(res_o1.is_ok(), "O1 Compilation failed: {:?}", res_o1.err());
    let output_o1 = Command::new(&out_o1).output().expect("Execute O1 binary");
    assert_eq!(
        output_o1.status.code(),
        Some(expected_exit_code),
        "O1 exit code mismatch for {}",
        name
    );
    assert_eq!(
        output_o1.stderr.as_slice(),
        expected_stderr,
        "O1 stderr mismatch for {}: got {:?}, expected {:?}",
        name,
        String::from_utf8_lossy(&output_o1.stderr),
        String::from_utf8_lossy(expected_stderr)
    );

    // 3. Differential check: both must agree exactly on stderr and exit status
    assert_eq!(
        output_o0.stderr, output_o1.stderr,
        "Differential stderr mismatch between O0 and O1 for {}",
        name
    );
    assert_eq!(
        output_o0.status.code(),
        output_o1.status.code(),
        "Differential exit code mismatch between O0 and O1 for {}",
        name
    );
}

#[test]
fn test_e2e_return_42() {
    run_differential_test(
        "return_42",
        r#"
        pub fn main() -> i64 {
            return 42;
        }
        "#,
        42,
    );
}

#[test]
fn test_e2e_arithmetic_constant_folding() {
    run_differential_test(
        "arithmetic",
        r#"
        pub fn main() -> i64 {
            let a: i64 = 15;
            let b: i64 = 27;
            return a + b;
        }
        "#,
        42,
    );
}

#[test]
fn test_e2e_while_loop() {
    run_differential_test(
        "while_loop",
        r#"
        pub fn main() -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 1;
            while i <= 10 {
                sum += i;
                i += 1;
            }
            return sum;
        }
        "#,
        55,
    );
}

#[test]
fn test_e2e_function_calls() {
    run_differential_test(
        "fn_call",
        r#"
        fn square(x: i64) -> i64 {
            return x * x;
        }

        pub fn main() -> i64 {
            let a: i64 = 7;
            return square(a);
        }
        "#,
        49,
    );
}

#[test]
fn test_e2e_pointers_and_references() {
    run_differential_test(
        "pointers",
        r#"
        fn add_ten(p: &mut i64) {
            *p += 10;
        }

        pub fn main() -> i64 {
            let mut val: i64 = 32;
            add_ten(&mut val);
            return val;
        }
        "#,
        42,
    );
}

#[test]
fn test_e2e_constant_branch_folding_and_dead_loop() {
    run_differential_test(
        "branch_folding",
        r#"
        pub fn main() -> i64 {
            let mut res: i64 = 0;
            if 10 > 2 {
                res = 37;
            } else {
                let mut dead_sum = 0;
                let mut i = 0;
                while i < 100 {
                    dead_sum += i;
                    i += 1;
                }
                res = dead_sum;
            }
            return res;
        }
        "#,
        37,
    );
}

#[test]
fn test_e2e_dead_computation_chain() {
    run_differential_test(
        "dead_chain",
        r#"
        pub fn main() -> i64 {
            let unused1: i64 = 100 + 200;
            let unused2: i64 = unused1 * 3;
            let useful: i64 = 25 + 5;
            return useful;
        }
        "#,
        30,
    );
}

#[test]
fn test_compile_fail_borrowcheck_mutation_when_borrowed() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("borrow_fail.va");
    let out_exe = temp_dir.path().join("borrow_fail");

    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            let mut x: i64 = 42;
            let r = &x;
            x = 99; // Error: cannot assign to x while borrowed
            return x;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe,
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err(), "Compilation should fail borrow checking");
    assert!(session.diagnostics.has_errors());
    assert!(session.diagnostics.diagnostics().iter().any(|d| d
        .message
        .contains("Cannot assign to 'x' because it is borrowed")));
}

// ============================================================================
// Milestone 3: Standard Output Intrinsics & String Slices Tests
// ============================================================================

#[test]
fn test_e2e_print_i64_zero() {
    run_differential_stdout_test(
        "print_i64_zero",
        r#"
        pub fn main() -> i64 {
            print_i64(0);
            return 0;
        }
        "#,
        0,
        b"0",
    );
}

#[test]
fn test_e2e_print_i64_positive_and_negative() {
    run_differential_stdout_test(
        "print_i64_pos_neg",
        r#"
        pub fn main() -> i64 {
            print_i64(42);
            print_i64(-17);
            print_i64(1);
            print_i64(-999);
            return 0;
        }
        "#,
        0,
        b"42-171-999",
    );
}

#[test]
fn test_e2e_print_i64_min_and_max() {
    run_differential_stdout_test(
        "print_i64_min_max",
        r#"
        pub fn main() -> i64 {
            let max_val: i64 = 9223372036854775807;
            let min_val: i64 = -9223372036854775807 - 1;
            print_i64(max_val);
            print_i64(min_val);
            return 0;
        }
        "#,
        0,
        b"9223372036854775807-9223372036854775808",
    );
}

#[test]
fn test_e2e_print_str_literals() {
    run_differential_stdout_test(
        "print_str_literals",
        r#"
        pub fn main() -> i64 {
            print_str("Hello, Vanta!");
            return 0;
        }
        "#,
        0,
        b"Hello, Vanta!",
    );
}

#[test]
fn test_e2e_print_str_empty() {
    run_differential_stdout_test(
        "print_str_empty",
        r#"
        pub fn main() -> i64 {
            print_str("");
            return 0;
        }
        "#,
        0,
        b"",
    );
}

#[test]
fn test_e2e_print_str_escapes() {
    run_differential_stdout_test(
        "print_str_escapes",
        r#"
        pub fn main() -> i64 {
            print_str("Line 1\nLine 2\t\"Quoted\" \\ Backslash\n");
            return 0;
        }
        "#,
        0,
        b"Line 1\nLine 2\t\"Quoted\" \\ Backslash\n",
    );
}

#[test]
fn test_e2e_print_str_utf8() {
    run_differential_stdout_test(
        "print_str_utf8",
        r#"
        pub fn main() -> i64 {
            print_str("Привет, мир! 🚀 🦀\n");
            return 0;
        }
        "#,
        0,
        "Привет, мир! 🚀 🦀\n".as_bytes(),
    );
}

#[test]
fn test_e2e_print_str_embedded_null() {
    run_differential_stdout_test(
        "print_str_embedded_null",
        r#"
        pub fn main() -> i64 {
            print_str("Hello\0World");
            return 0;
        }
        "#,
        0,
        b"Hello\0World",
    );
}

#[test]
fn test_e2e_print_str_variable_and_reassignment() {
    run_differential_stdout_test(
        "print_str_var",
        r#"
        pub fn main() -> i64 {
            let mut msg: &str = "first message\n";
            print_str(msg);
            msg = "second message\n";
            print_str(msg);
            return 0;
        }
        "#,
        0,
        b"first message\nsecond message\n",
    );
}

#[test]
fn test_e2e_print_str_helper_function() {
    run_differential_stdout_test(
        "print_str_helper",
        r#"
        fn log_info(s: &str) {
            print_str("[INFO] ");
            print_str(s);
        }

        pub fn main() -> i64 {
            log_info("Execution started\n");
            log_info("Execution finished\n");
            return 0;
        }
        "#,
        0,
        b"[INFO] Execution started\n[INFO] Execution finished\n",
    );
}

#[test]
fn test_e2e_print_loops_and_branches() {
    run_differential_stdout_test(
        "print_loops_branches",
        r#"
        pub fn main() -> i64 {
            let mut i: i64 = 0;
            while i < 4 {
                if i % 2 == 0 {
                    print_str("even:");
                } else {
                    print_str("odd:");
                }
                print_i64(i);
                print_str(" ");
                i += 1;
            }
            return 0;
        }
        "#,
        0,
        b"even:0 odd:1 even:2 odd:3 ",
    );
}

#[test]
fn test_compile_fail_redefine_intrinsic_print_i64() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("redefine_print_i64.va");
    let out_exe = temp_dir.path().join("redefine_print_i64");

    fs::write(
        &src_path,
        r#"
        fn print_i64(val: i64) {}
        pub fn main() -> i64 { return 0; }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe,
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err());
    assert!(session.diagnostics.diagnostics().iter().any(|d| d
        .message
        .contains("Cannot redefine built-in intrinsic 'print_i64'")));
}

#[test]
fn test_compile_fail_redefine_intrinsic_print_str() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("redefine_print_str.va");
    let out_exe = temp_dir.path().join("redefine_print_str");

    fs::write(
        &src_path,
        r#"
        fn print_str(val: &str) {}
        pub fn main() -> i64 { return 0; }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe,
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err());
    assert!(session.diagnostics.diagnostics().iter().any(|d| d
        .message
        .contains("Cannot redefine built-in intrinsic 'print_str'")));
}

#[test]
fn test_compile_fail_returning_str_slice() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("return_str_fail.va");
    let out_exe = temp_dir.path().join("return_str_fail");

    fs::write(
        &src_path,
        r#"
        fn get_msg() -> &str {
            return "msg";
        }
        pub fn main() -> i64 { return 0; }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe,
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err());
    assert!(session
        .diagnostics
        .diagnostics()
        .iter()
        .any(|d| d.message.contains(
            "returns a reference, but does not take any reference parameters to borrow from"
        ) || d.message.contains(
            "Returning '&str' from user-defined functions is not yet supported"
        )));
}

#[test]
fn test_compile_fail_type_mismatch_print_i64() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("type_mismatch_print_i64.va");
    let out_exe = temp_dir.path().join("type_mismatch_print_i64");

    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            print_i64("not an int");
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe,
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err());
    assert!(session
        .diagnostics
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("expects i64, found &str")));
}

#[test]
fn test_compile_fail_type_mismatch_print_str() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("type_mismatch_print_str.va");
    let out_exe = temp_dir.path().join("type_mismatch_print_str");

    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            print_str(12345);
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe,
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err());
    assert!(session
        .diagnostics
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("expects &str, found i64")));
}

#[test]
fn test_compile_fail_bare_str() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("bare_str.va");
    let out_exe = temp_dir.path().join("bare_str");

    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            let x: str;
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe,
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err());
    assert!(session.diagnostics.diagnostics().iter().any(|d| d
        .message
        .contains("cannot be used directly as a sized value")));
}

#[test]
fn test_compile_fail_mut_str() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("mut_str.va");
    let out_exe = temp_dir.path().join("mut_str");

    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            let x: &mut str;
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe,
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err());
    assert!(session.diagnostics.diagnostics().iter().any(|d| d
        .message
        .contains("Mutable string slices '&mut str' are not supported")));
}

#[test]
fn test_ai_first_check_clean() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("check_clean.va");
    fs::write(
        &src_path,
        r#"
        fn compute(a: i64, b: i64) -> i64 {
            let sum: i64 = a + b;
            return sum;
        }

        pub fn main() -> i64 {
            print_str("Hello AI\n");
            print_i64(compute(10, 32));
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_ok(), "Check should succeed on valid program");
    assert!(!session.diagnostics.has_errors());

    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"version\":\"1.0.0\""));
    assert!(json_report.contains("\"success\":true"));
    assert!(json_report.contains("\"error_count\":0"));
}

#[test]
fn test_ai_first_check_diagnostics_json_type_mismatch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("check_mismatch.va");
    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            print_i64("error string");
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err(), "Check should fail on type mismatch");
    assert!(session.diagnostics.has_errors());

    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"version\":\"1.0.0\""));
    assert!(json_report.contains("\"success\":false"));
    assert!(json_report.contains("\"error_count\":1"));
    assert!(json_report.contains("\"code\":\"E0201\""));
    assert!(json_report.contains("\"expected\":\"i64\""));
    assert!(json_report.contains("\"actual\":\"&str\""));
    assert!(json_report.contains("\"replacement_text\":\"print_str\""));
}

#[test]
fn test_ai_first_check_borrow_conflict_json() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("check_borrow.va");
    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            let mut x: i64 = 100;
            let r: &i64 = &x;
            x = 200;
            return *r;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err(), "Check should fail on borrow conflict");
    assert!(session.diagnostics.has_errors());

    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"version\":\"1.0.0\""));
    assert!(json_report.contains("\"success\":false"));
    assert!(json_report.contains("\"code\":\"E0306\""));
    assert!(json_report.contains("active shared borrow exists"));
}

// =========================================================================
// MILESTONE 4: Array Bounds Safety, Slice Bounds Safety & Runtime Panic
// =========================================================================

#[test]
fn test_e2e_array_first_and_last_valid_element() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 3] = [10, 20, 30];
            let first: i64 = arr[0];
            let last: i64 = arr[2];
            return first + last;
        }
    "#;
    run_differential_test("array_first_last", src, 40);
}

#[test]
fn test_e2e_array_single_element() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 1] = [42];
            return arr[0];
        }
    "#;
    run_differential_test("array_single", src, 42);
}

#[test]
fn test_e2e_array_indexed_write() {
    let src = r#"
        pub fn main() -> i64 {
            let mut arr: [i64; 3] = [10, 20, 30];
            arr[1] = 99;
            return arr[1];
        }
    "#;
    run_differential_test("array_indexed_write", src, 99);
}

#[test]
fn test_e2e_array_indexed_in_loop() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 3] = [100, 200, 300];
            let mut i: i64 = 0;
            while i < 3 {
                print_i64(arr[i]);
                print_str("\n");
                i += 1;
            }
            return 0;
        }
    "#;
    run_differential_stdout_test("array_loop", src, 0, b"100\n200\n300\n");
}

#[test]
fn test_e2e_array_expression_as_index() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 4] = [1, 2, 3, 4];
            let idx: i64 = 1 + 2;
            return arr[idx];
        }
    "#;
    run_differential_test("array_expr_idx", src, 4);
}

#[test]
fn test_e2e_array_reference_indexing() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 3] = [15, 25, 35];
            let r: &[i64; 3] = &arr;
            return r[1];
        }
    "#;
    run_differential_test("array_ref_idx", src, 25);
}

#[test]
fn test_e2e_string_slice_indexing() {
    let src = r#"
        pub fn main() -> i64 {
            let s: &str = "Vanta";
            let b0: u8 = s[0];
            let b1: u8 = s[1];
            print_i64(b0);
            print_str(" ");
            print_i64(b1);
            return 0;
        }
    "#;
    // 'V' is ASCII 86, 'a' is ASCII 97
    run_differential_stdout_test("str_slice_indexing", src, 0, b"86 97");
}

#[test]
fn test_e2e_panic_bounds_check_index_equals_len() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 3] = [10, 20, 30];
            let val: i64 = arr[3];
            return val;
        }
    "#;
    run_differential_stderr_test(
        "panic_idx_equals_len",
        src,
        101,
        b"Vanta panic: index out of bounds (index 3, length 3)\n",
    );
}

#[test]
fn test_e2e_panic_bounds_check_index_greater_than_len() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 3] = [10, 20, 30];
            let val: i64 = arr[10];
            return val;
        }
    "#;
    run_differential_stderr_test(
        "panic_idx_gt_len",
        src,
        101,
        b"Vanta panic: index out of bounds (index 10, length 3)\n",
    );
}

#[test]
fn test_e2e_panic_bounds_check_negative_index() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 3] = [10, 20, 30];
            let val: i64 = arr[-1];
            return val;
        }
    "#;
    run_differential_stderr_test(
        "panic_neg_idx",
        src,
        101,
        b"Vanta panic: index out of bounds (index -1, length 3)\n",
    );
}

#[test]
fn test_e2e_panic_bounds_check_empty_array() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 0] = [];
            let val: i64 = arr[0];
            return val;
        }
    "#;
    run_differential_stderr_test(
        "panic_empty_array",
        src,
        101,
        b"Vanta panic: index out of bounds (index 0, length 0)\n",
    );
}

#[test]
fn test_e2e_panic_bounds_check_write_out_of_bounds() {
    let src = r#"
        pub fn main() -> i64 {
            let mut arr: [i64; 3] = [1, 2, 3];
            arr[5] = 99;
            return 0;
        }
    "#;
    run_differential_stderr_test(
        "panic_write_oob",
        src,
        101,
        b"Vanta panic: index out of bounds (index 5, length 3)\n",
    );
}

#[test]
fn test_e2e_panic_bounds_check_in_helper_function() {
    let src = r#"
        fn get_at(arr: &[i64; 3], i: i64) -> i64 {
            return arr[i];
        }

        pub fn main() -> i64 {
            let arr: [i64; 3] = [100, 200, 300];
            return get_at(&arr, 4);
        }
    "#;
    run_differential_stderr_test(
        "panic_in_helper",
        src,
        101,
        b"Vanta panic: index out of bounds (index 4, length 3)\n",
    );
}

#[test]
fn test_e2e_panic_bounds_check_in_branch() {
    let src = r#"
        pub fn main() -> i64 {
            let arr: [i64; 3] = [1, 2, 3];
            let should_panic: bool = true;
            if should_panic {
                let _x: i64 = arr[7];
                return 1;
            } else {
                return 42;
            }
        }
    "#;
    run_differential_stderr_test(
        "panic_in_branch",
        src,
        101,
        b"Vanta panic: index out of bounds (index 7, length 3)\n",
    );
}

#[test]
fn test_e2e_panic_intrinsic_explicit() {
    let src = r#"
        pub fn main() -> i64 {
            panic("critical assertion failed: state is invalid");
            return 0;
        }
    "#;
    run_differential_stderr_test(
        "panic_intrinsic",
        src,
        101,
        b"Vanta panic: critical assertion failed: state is invalid\n",
    );
}

#[test]
fn test_compile_fail_mutating_immutable_array() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("immutable_arr.va");
    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            let arr: [i64; 3] = [1, 2, 3];
            arr[0] = 10;
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    assert!(session.diagnostics.has_errors());

    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0303\""));
    assert!(json_report.contains("Cannot assign to indexed element of immutable variable 'arr'"));
}

#[test]
fn test_compile_fail_mutating_string_slice_indexing() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("immutable_str_slice.va");
    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            let s: &str = "hello";
            s[0] = 65;
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    assert!(session.diagnostics.has_errors());

    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0303\""));
    assert!(json_report.contains("String slices are immutable"));
}

#[test]
fn test_compile_fail_redefining_panic_intrinsic() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("redefine_panic.va");
    fs::write(
        &src_path,
        r#"
        fn panic(msg: &str) {}
        pub fn main() -> i64 {
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    assert!(session.diagnostics.has_errors());

    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0103\""));
    assert!(json_report.contains("Cannot redefine built-in intrinsic 'panic'"));
}

#[test]
fn test_e2e_struct_declaration_and_field_access() {
    let src = r#"
    struct Point {
        x: i64,
        y: i64,
    }

    pub fn main() -> i64 {
        let p = Point { x: 10, y: 20 };
        print_i64(p.x);
        print_i64(p.y);
        return 0;
    }
    "#;
    run_differential_stdout_test("struct_access", src, 0, b"1020");
}

#[test]
fn test_e2e_struct_field_mutation() {
    let src = r#"
    struct Point {
        x: i64,
        y: i64,
    }

    pub fn main() -> i64 {
        let mut p = Point { x: 10, y: 20 };
        p.x = 42;
        p.y = p.y + 5;
        print_i64(p.x);
        print_i64(p.y);
        return 0;
    }
    "#;
    run_differential_stdout_test("struct_mutation", src, 0, b"4225");
}

#[test]
fn test_e2e_struct_layout_and_padding() {
    let src = r#"
    struct Mixed {
        a: u8,
        b: i64,
        c: u8,
    }

    pub fn main() -> i64 {
        let mut m = Mixed { a: 7, b: 1000, c: 99 };
        print_i64(m.b);
        m.b = 2000;
        print_i64(m.b);
        return 0;
    }
    "#;
    run_differential_stdout_test("struct_padding", src, 0, b"10002000");
}

#[test]
fn test_e2e_nested_structs() {
    let src = r#"
    struct Vec2 {
        x: i64,
        y: i64,
    }

    struct Player {
        pos: Vec2,
        health: i64,
    }

    pub fn main() -> i64 {
        let mut p = Player {
            pos: Vec2 { x: 15, y: 30 },
            health: 100,
        };
        print_i64(p.pos.x);
        print_i64(p.pos.y);
        print_i64(p.health);

        p.pos.x = 50;
        p.health = 80;
        print_i64(p.pos.x);
        print_i64(p.health);
        return 0;
    }
    "#;
    run_differential_stdout_test("nested_structs", src, 0, b"15301005080");
}

#[test]
fn test_e2e_inherent_methods_mut_and_ref() {
    let src = r#"
    struct Point {
        x: i64,
        y: i64,
    }

    impl Point {
        fn translate(self: &mut Self, dx: i64, dy: i64) {
            self.x = self.x + dx;
            self.y = self.y + dy;
        }

        fn sum(self: &Self) -> i64 {
            return self.x + self.y;
        }
    }

    pub fn main() -> i64 {
        let mut p = Point { x: 10, y: 20 };
        p.translate(5, 7);
        let s = p.sum();
        print_i64(s);
        return 0;
    }
    "#;
    run_differential_stdout_test("struct_methods", src, 0, b"42");
}

#[test]
fn test_e2e_dynamic_slice_borrow_and_len() {
    let src = r#"
    pub fn main() -> i64 {
        let arr: [i64; 4] = [10, 20, 30, 40];
        let view: &[i64] = &arr;
        print_i64(view.len());
        print_i64(view[0]);
        print_i64(view[1]);
        print_i64(view[2]);
        print_i64(view[3]);
        return 0;
    }
    "#;
    run_differential_stdout_test("slice_borrow_len", src, 0, b"410203040");
}

#[test]
fn test_e2e_dynamic_slice_passed_to_function() {
    let src = r#"
    fn sum_slice(s: &[i64]) -> i64 {
        let mut total = 0;
        let mut i = 0;
        let n = s.len();
        while i < n {
            total = total + s[i];
            i = i + 1;
        }
        return total;
    }

    pub fn main() -> i64 {
        let arr: [i64; 3] = [100, 200, 300];
        let total = sum_slice(&arr);
        print_i64(total);
        return 0;
    }
    "#;
    run_differential_stdout_test("slice_func_arg", src, 0, b"600");
}

#[test]
fn test_e2e_dynamic_slice_out_of_bounds_panic() {
    let src = r#"
    pub fn main() -> i64 {
        let arr: [i64; 3] = [1, 2, 3];
        let view: &[i64] = &arr;
        print_i64(view[3]);
        return 0;
    }
    "#;
    run_differential_test("slice_oob_panic", src, 101);
}

#[test]
fn test_compile_fail_recursive_struct() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("rec.va");
    fs::write(
        &src_path,
        r#"
        struct Node {
            val: i64,
            next: Node,
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0208\""));
    assert!(json_report.contains("infinite size due to recursive cycle"));
}

#[test]
fn test_compile_fail_missing_field() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("missing.va");
    fs::write(
        &src_path,
        r#"
        struct Point {
            x: i64,
            y: i64,
        }
        fn test() {
            let p = Point { x: 10 };
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0107\""));
    assert!(json_report.contains("Missing field 'y'"));
}

#[test]
fn test_compile_fail_duplicate_field() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("dup_field.va");
    fs::write(
        &src_path,
        r#"
        struct Point {
            x: i64,
            y: i64,
        }
        fn test() {
            let p = Point { x: 10, y: 20, x: 30 };
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0106\""));
    assert!(json_report.contains("Duplicate field 'x'"));
}

#[test]
fn test_compile_fail_unknown_method() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("unknown_m.va");
    fs::write(
        &src_path,
        r#"
        struct Point {
            x: i64,
            y: i64,
        }
        fn test() {
            let p = Point { x: 10, y: 20 };
            p.non_existent();
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0108\""));
    assert!(json_report.contains("No method named 'non_existent'"));
}

#[test]
fn test_compile_fail_invalid_receiver() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("bad_recv.va");
    fs::write(
        &src_path,
        r#"
        struct Point {
            x: i64,
            y: i64,
        }
        impl Point {
            fn bad(x: i64) {}
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0109\""));
    assert!(json_report.contains("must have 'self' as its first parameter"));
}

#[test]
fn test_compile_fail_mutating_through_immutable_ref() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("immut_ref.va");
    fs::write(
        &src_path,
        r#"
        struct Point {
            x: i64,
            y: i64,
        }
        impl Point {
            fn try_mutate(self: &Self) {
                self.x = 99;
            }
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0303\""));
    assert!(json_report.contains("Cannot assign through immutable reference 'self'"));
}

#[test]
fn test_compile_fail_mutating_array_while_borrowed_by_slice() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("borrow_conflict.va");
    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            let mut arr: [i64; 3] = [1, 2, 3];
            let view: &[i64] = &arr;
            arr = [4, 5, 6];
            return 0;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(result.is_err());
    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(json_report.contains("\"code\":\"E0306\""));
    assert!(json_report.contains("Cannot assign to 'arr' because it is borrowed"));
}
