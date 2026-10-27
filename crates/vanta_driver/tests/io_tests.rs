//! Integration tests for std::io (write_stdout, write_stderr, read_to_string).

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

#[test]
fn test_io_stdout_and_stderr() {
    let src = r#"
import std::io;

fn main() -> i64 {
    io::write_stdout("This goes to standard output\n");
    io::write_stderr("This goes to standard error\n");
    return 0;
}
"#;

    for &opt_level in &[OptLevel::None, OptLevel::O1] {
        let temp_dir = tempdir().unwrap();
        let src_file = temp_dir.path().join("test_io.va");
        let exe_file = temp_dir.path().join("test_io.bin");

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
        assert!(session.compile(&options).is_ok());

        let output = Command::new(&exe_file).output().expect("Execute binary");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"This goes to standard output\n");
        assert_eq!(output.stderr, b"This goes to standard error\n");
    }
}

#[test]
fn test_io_read_to_string_stdin() {
    let src = r#"
import std::io;
import std::result::Result;

fn main() -> i64 {
    let res = io::read_to_string();
    match res {
        Result::Ok(content) => {
            print_str("stdin read: ");
            print_str(content.as_str());
        }
        Result::Err(err) => {
            print_str("stdin err: ");
            print_i64(err);
            return 1;
        }
    }
    return 0;
}
"#;

    for &opt_level in &[OptLevel::None, OptLevel::O1] {
        let temp_dir = tempdir().unwrap();
        let src_file = temp_dir.path().join("test_stdin.va");
        let exe_file = temp_dir.path().join("test_stdin.bin");

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
        assert!(session.compile(&options).is_ok());

        let mut child = Command::new(&exe_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Spawn process");

        let input_text = "Echo through Vanta stdin!";
        {
            let stdin = child.stdin.as_mut().expect("Open stdin");
            stdin.write_all(input_text.as_bytes()).unwrap();
        }

        let output = child.wait_with_output().expect("Wait on child");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"stdin read: Echo through Vanta stdin!");
    }
}
