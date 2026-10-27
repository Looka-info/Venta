//! Integration tests for std::fs (read_to_string, write).

use std::fs;
use std::process::Command;
use tempfile::tempdir;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

fn run_fs_test(name: &str, src: &str, expected_exit_code: i32, expected_stdout: &[u8]) {
    for &opt_level in &[OptLevel::None, OptLevel::O1] {
        let temp_dir = tempdir().unwrap();
        let src_file = temp_dir.path().join(format!("{}.va", name));
        let exe_file = temp_dir.path().join(format!("{}.bin", name));

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
        let compile_res = session.compile(&options);
        if compile_res.is_err() {
            eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
        }
        assert!(compile_res.is_ok(), "Compilation failed: {:?}", compile_res.err());

        let output = Command::new(&exe_file)
            .current_dir(temp_dir.path())
            .output()
            .expect("Execute binary");

        assert_eq!(
            output.status.code(),
            Some(expected_exit_code),
            "Exit code mismatch for {} (opt_level={:?})",
            name,
            opt_level
        );
        assert_eq!(
            output.stdout,
            expected_stdout,
            "Stdout mismatch for {} (opt_level={:?}): got '{}', expected '{}'",
            name,
            opt_level,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(expected_stdout)
        );
    }
}

#[test]
fn test_fs_write_and_read_roundtrip() {
    let src = r#"
import std::fs;
import std::result::Result;

fn main() -> i64 {
    let path = "test_roundtrip.txt";
    let content = "Hello, Vanta Systems Filesystem!";
    let w_res = fs::write(path, content);
    match w_res {
        Result::Ok(bytes) => {
            print_str("written: ");
            print_i64(bytes);
            print_str("\n");
        }
        Result::Err(e) => {
            print_str("write err: ");
            print_i64(e);
            print_str("\n");
            return 1;
        }
    }

    let r_res = fs::read_to_string(path);
    match r_res {
        Result::Ok(text) => {
            print_str("read: ");
            print_str(text.as_str());
            print_str("\n");
        }
        Result::Err(e) => {
            print_str("read err: ");
            print_i64(e);
            print_str("\n");
            return 2;
        }
    }

    return 0;
}
"#;
    let expected = b"written: 32\nread: Hello, Vanta Systems Filesystem!\n";
    run_fs_test("fs_roundtrip", src, 0, expected);
}

#[test]
fn test_fs_read_nonexistent_file() {
    let src = r#"
import std::fs;
import std::result::Result;

fn main() -> i64 {
    let path = "does_not_exist_file_12345.txt";
    let r_res = fs::read_to_string(path);
    match r_res {
        Result::Ok(_) => {
            print_str("unexpected success\n");
            return 1;
        }
        Result::Err(err_code) => {
            // Negative error code (-ENOENT)
            if err_code < 0 {
                print_str("file not found handled\n");
                return 0;
            } else {
                return 2;
            }
        }
    }
}
"#;
    run_fs_test("fs_nonexistent", src, 0, b"file not found handled\n");
}

#[test]
fn test_fs_read_invalid_utf8_rejected() {
    let temp_dir = tempdir().unwrap();
    let bad_file = temp_dir.path().join("invalid_utf8.bin");
    // Invalid UTF-8 bytes: 0xFF, 0xFE
    fs::write(&bad_file, [0xFF, 0xFE, 0x80, 0x00]).unwrap();

    let src = format!(
        r#"
import std::fs;
import std::result::Result;

fn main() -> i64 {{
    let path = "{path}";
    let r_res = fs::read_to_string(path);
    match r_res {{
        Result::Ok(_) => {{
            print_str("unexpected ok on invalid utf8\n");
            return 1;
        }}
        Result::Err(err) => {{
            if err == -1001 {{
                print_str("utf8 error caught\n");
                return 0;
            }} else {{
                print_i64(err);
                return 2;
            }}
        }}
    }}
}}
"#,
        path = bad_file.to_str().unwrap()
    );

    run_fs_test("fs_invalid_utf8", &src, 0, b"utf8 error caught\n");
}
