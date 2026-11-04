//! Integration tests for std::net (TCP client connect, server listen, accept, read, write, close).

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

fn execute_with_retry(bin_path: &Path) -> std::process::Output {
    let mut attempts = 0;
    loop {
        match Command::new(bin_path).output() {
            Ok(out) => return out,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 5 => {
                attempts += 1;
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("Failed to execute binary {:?}: {}", bin_path, e),
        }
    }
}

fn compile_net_test(
    name: &str,
    src: &str,
    opt_level: OptLevel,
    temp_dir: &tempfile::TempDir,
) -> PathBuf {
    let src_file = temp_dir.path().join(format!("{}.va", name));
    let exe_file = temp_dir.path().join(format!("{}.bin", name));

    fs::write(&src_file, src).unwrap();

    let options = CompilerOptions {
        input_file: src_file,
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
    assert!(
        compile_res.is_ok(),
        "Compilation failed for {}: {:?}",
        name,
        compile_res.err()
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&exe_file).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exe_file, perms).unwrap();
    }

    exe_file
}

#[test]
fn test_tcp_client_connect_and_transfer() {
    for &opt_level in &[OptLevel::None, OptLevel::O1] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Bind ephemeral test listener");
        let port = listener.local_addr().unwrap().port();

        // Spawn mock server thread
        let handle = thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 128];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let received = String::from_utf8_lossy(&buf[..n]);
                assert_eq!(received, "HELLO_FROM_VANTA_CLIENT");

                stream.write_all(b"REPLY_FROM_MOCK_SERVER").unwrap();
                stream.flush().unwrap();
                break;
            }
        });

        let source = format!(
            r#"
import std::net;
import std::string::String;

fn main() -> i64 {{
    let client_fd = std::net::connect("127.0.0.1", {port});
    if client_fd < 0 {{
        return 1;
    }}

    let written = std::net::write(client_fd, "HELLO_FROM_VANTA_CLIENT");
    if written <= 0 {{
        std::net::close(client_fd);
        return 2;
    }}

    let mut response = String::new();
    let bytes_read = std::net::read(client_fd, &mut response);
    if bytes_read <= 0 {{
        std::net::close(client_fd);
        return 3;
    }}

    if response.len != 22 {{
        std::net::close(client_fd);
        return 4;
    }}

    std::net::close(client_fd);
    return 42;
}}
"#
        );

        let temp_dir = tempdir().unwrap();
        let exe = compile_net_test(
            &format!("client_transfer_{:?}", opt_level),
            &source,
            opt_level,
            &temp_dir,
        );

        let output = execute_with_retry(&exe);
        assert_eq!(
            output.status.code(),
            Some(42),
            "Expected exit code 42, got {:?}. Stderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );

        handle.join().unwrap();
    }
}

#[test]
fn test_tcp_client_connect_failure_returns_negative() {
    for &opt_level in &[OptLevel::None, OptLevel::O1] {
        let source = r#"
import std::net;

fn main() -> i64 {
    // Attempt connecting to a closed port
    let client_fd = std::net::connect("127.0.0.1", 1);
    if client_fd < 0 {
        return 77;
    }
    std::net::close(client_fd);
    return 1;
}
"#;
        let temp_dir = tempdir().unwrap();
        let exe = compile_net_test(
            &format!("client_fail_{:?}", opt_level),
            source,
            opt_level,
            &temp_dir,
        );

        let output = execute_with_retry(&exe);
        assert_eq!(
            output.status.code(),
            Some(77),
            "Expected exit code 77 on connection failure, got {:?}",
            output.status.code()
        );
    }
}

#[test]
fn test_tcp_server_invalid_listen_port() {
    for &opt_level in &[OptLevel::None, OptLevel::O1] {
        let source = r#"
import std::net;

fn main() -> i64 {
    let server_fd = std::net::listen(-1);
    if server_fd < 0 {
        return 88;
    }
    std::net::close(server_fd);
    return 1;
}
"#;
        let temp_dir = tempdir().unwrap();
        let exe = compile_net_test(
            &format!("server_fail_{:?}", opt_level),
            source,
            opt_level,
            &temp_dir,
        );

        let output = execute_with_retry(&exe);
        assert_eq!(
            output.status.code(),
            Some(88),
            "Expected exit code 88 on invalid listen port, got {:?}",
            output.status.code()
        );
    }
}

#[test]
fn test_tcp_server_listen_accept_and_echo() {
    for &opt_level in &[OptLevel::None, OptLevel::O1] {
        // Pick an ephemeral port by binding and immediately dropping
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };

        let source = format!(
            r#"
import std::net;
import std::io;
import std::string::String;

fn main() -> i64 {{
    let server_fd = std::net::listen({port});
    if server_fd < 0 {{
        return 1;
    }}

    io::write_stdout("READY\n");

    let client_fd = std::net::accept(server_fd);
    if client_fd < 0 {{
        std::net::close(server_fd);
        return 2;
    }}

    let mut msg = String::new();
    let n = std::net::read(client_fd, &mut msg);
    if n <= 0 {{
        std::net::close(client_fd);
        std::net::close(server_fd);
        return 3;
    }}

    let written = std::net::write(client_fd, "PONG_FROM_VANTA_SERVER");
    if written <= 0 {{
        std::net::close(client_fd);
        std::net::close(server_fd);
        return 4;
    }}

    std::net::close(client_fd);
    std::net::close(server_fd);
    return 100;
}}
"#
        );

        let temp_dir = tempdir().unwrap();
        let exe = compile_net_test(
            &format!("server_echo_{:?}", opt_level),
            &source,
            opt_level,
            &temp_dir,
        );

        // Spawn child process running server
        let mut child = Command::new(&exe)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Spawn Vanta TCP server");

        // Wait until server prints READY\n
        let stdout = child.stdout.as_mut().expect("Child stdout");
        let mut line_buf = [0u8; 6];
        stdout.read_exact(&mut line_buf).expect("Read READY banner");
        assert_eq!(&line_buf, b"READY\n");

        // Connect as client from Rust test
        let mut client_stream =
            TcpStream::connect(("127.0.0.1", port)).expect("Connect to Vanta TCP server");
        client_stream.write_all(b"PING_FROM_RUST").unwrap();
        client_stream.flush().unwrap();

        let mut reply = [0u8; 128];
        let n = client_stream.read(&mut reply).expect("Read server reply");
        assert_eq!(&reply[..n], b"PONG_FROM_VANTA_SERVER");

        let status = child.wait().expect("Wait on child");
        assert_eq!(status.code(), Some(100));
    }
}
