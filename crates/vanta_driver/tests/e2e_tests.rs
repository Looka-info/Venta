use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, OptLevel};

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
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err(), "Compilation should fail borrow checking");
    assert!(session.diagnostics.has_errors());
    assert!(session.diagnostics.diagnostics().iter().any(|d| d
        .message
        .contains("Cannot assign to 'x' because it is borrowed")));
}
