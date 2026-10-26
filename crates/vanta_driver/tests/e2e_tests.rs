use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget};

#[test]
fn test_compile_and_run_return_42() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("return_42.va");
    let out_exe = temp_dir.path().join("return_42");

    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            return 42;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe.clone(),
        emit_target: EmitTarget::Exe,
        verbose: false,
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

    // Execute the compiled native executable!
    let run_status = Command::new(&out_exe).status().expect("Execute native binary");
    assert_eq!(run_status.code(), Some(42));
}

#[test]
fn test_compile_and_run_arithmetic() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("arith.va");
    let out_exe = temp_dir.path().join("arith");

    fs::write(
        &src_path,
        r#"
        pub fn main() -> i64 {
            let a: i64 = 15;
            let b: i64 = 27;
            return a + b;
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe.clone(),
        emit_target: EmitTarget::Exe,
        verbose: false,
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

    let run_status = Command::new(&out_exe).status().expect("Execute native binary");
    assert_eq!(run_status.code(), Some(42));
}

#[test]
fn test_compile_and_run_while_loop() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("while_loop.va");
    let out_exe = temp_dir.path().join("while_loop");

    fs::write(
        &src_path,
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
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe.clone(),
        emit_target: EmitTarget::Exe,
        verbose: false,
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

    let run_status = Command::new(&out_exe).status().expect("Execute native binary");
    // sum(1..=10) = 55
    assert_eq!(run_status.code(), Some(55));
}

#[test]
fn test_compile_and_run_function_calls() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("fn_call.va");
    let out_exe = temp_dir.path().join("fn_call");

    fs::write(
        &src_path,
        r#"
        fn square(x: i64) -> i64 {
            return x * x;
        }

        pub fn main() -> i64 {
            let a: i64 = 7;
            return square(a);
        }
        "#,
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe.clone(),
        emit_target: EmitTarget::Exe,
        verbose: false,
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

    let run_status = Command::new(&out_exe).status().expect("Execute native binary");
    // 7 * 7 = 49
    assert_eq!(run_status.code(), Some(49));
}

#[test]
fn test_compile_and_run_pointers_and_references() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("pointers.va");
    let out_exe = temp_dir.path().join("pointers");

    fs::write(
        &src_path,
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
    )
    .unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_exe.clone(),
        emit_target: EmitTarget::Exe,
        verbose: false,
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

    let run_status = Command::new(&out_exe).status().expect("Execute native binary");
    // 32 + 10 = 42
    assert_eq!(run_status.code(), Some(42));
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
        verbose: false,
    };

    let mut session = CompilerSession::new();
    let result = session.compile(&options);
    assert!(result.is_err(), "Compilation should fail borrow checking");
    assert!(session.diagnostics.has_errors());
    assert!(session
        .diagnostics
        .diagnostics()
        .iter()
        .any(|d| d.message.contains("Cannot assign to 'x' because it is borrowed")));
}
