//! Integration tests for std::env (args) and unified vanta CLI.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

#[test]
fn test_env_args_basic() {
    let src = r#"
import std::env;
import std::result::Result;

fn main() -> i64 {
    let args_res = env::args();
    match args_res {
        Result::Ok(args) => {
            print_str("argc: ");
            print_i64(args.len());
            print_str("\n");
            let mut i = 0;
            while i < args.len() {
                print_str("arg: ");
                print_str(args[i].as_str());
                print_str("\n");
                i = i + 1;
            }
        }
        Result::Err(err) => {
            print_str("env err: ");
            print_i64(err);
            return 1;
        }
    }
    return 0;
}
"#;

    for &opt_level in &[OptLevel::None, OptLevel::O1] {
        let temp_dir = tempdir().unwrap();
        let src_file = temp_dir.path().join("test_env.va");
        let exe_file = temp_dir.path().join("test_env.bin");

        fs::write(&src_file, src).unwrap();

        let options = CompilerOptions {
            input_file: src_file.clone(),
            output_file: exe_file.clone(),
            emit_target: EmitTarget::Exe,
            opt_level,
            verbose: false,
            diagnostic_format: DiagnosticFormat::Human,
            std_path: None,
            runtime_path: None,
            ..Default::default()
        };

        let mut session = CompilerSession::new();
        let res = session.compile(&options);
        if res.is_err() {
            eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
        }
        assert!(res.is_ok());

        let output = Command::new(&exe_file)
            .args(["foo", "bar", "hello world"])
            .output()
            .expect("Execute binary");

        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("argc: 4\n"), "got: {}", stdout);
        assert!(stdout.contains("arg: foo\n"), "got: {}", stdout);
        assert!(stdout.contains("arg: bar\n"), "got: {}", stdout);
        assert!(stdout.contains("arg: hello world\n"), "got: {}", stdout);
    }
}

#[test]
fn test_env_args_memory_safety_and_drop() {
    let src = r#"
import std::env;
import std::result::Result;

fn run_args() {
    let args_res = env::args();
    match args_res {
        Result::Ok(args) => {
            // Args vector and strings are allocated
            let active = vanta_active_allocs();
            if active > 0 {
                print_str("allocs_active\n");
            }
        }
        Result::Err(_) => {
            print_str("error\n");
        }
    }
}

fn main() -> i64 {
    run_args();
    // After scope exit, all args and vector must be fully freed
    if vanta_active_allocs() == 0 {
        print_str("all_freed\n");
    } else {
        print_str("leak detected\n");
        return 1;
    }
    return 0;
}
"#;

    let temp_dir = tempdir().unwrap();
    let src_file = temp_dir.path().join("test_env_drop.va");
    let exe_file = temp_dir.path().join("test_env_drop.bin");

    fs::write(&src_file, src).unwrap();

    let options = CompilerOptions {
        input_file: src_file.clone(),
        output_file: exe_file.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    assert!(session.compile(&options).is_ok());

    let output = Command::new(&exe_file)
        .args(["a", "b", "c", "d", "e"])
        .output()
        .expect("Execute binary");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "allocs_active\nall_freed\n");
}

#[test]
fn test_unified_vanta_cli_run_command() {
    let temp_dir = tempdir().unwrap();
    let src_file = temp_dir.path().join("prog.va");

    fs::write(
        &src_file,
        r#"
import std::env;
import std::result::Result;

fn main() -> i64 {
    let res = env::args();
    match res {
        Result::Ok(args) => {
            if args.len() >= 2 {
                print_str(args[1].as_str());
            }
        }
        Result::Err(_) => {}
    }
    return 42;
}
"#,
    )
    .unwrap();

    let vanta_bin = PathBuf::from(env!("CARGO_BIN_EXE_vanta"));
    let output = Command::new(&vanta_bin)
        .arg("run")
        .arg(&src_file)
        .arg("--")
        .arg("passed_via_vanta_run")
        .output()
        .expect("Run vanta CLI");

    assert_eq!(output.status.code(), Some(42));
    assert_eq!(output.stdout, b"passed_via_vanta_run");
}
