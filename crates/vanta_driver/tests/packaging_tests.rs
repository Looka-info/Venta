//! Tests for standalone release packaging, installation, toolchain discovery, and uninstallation.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_release_package_install_and_uninstall() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let dist_tarball = repo_root
        .join("dist")
        .join("vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz");

    // Ensure package is built
    if !dist_tarball.is_file() {
        let pkg_status = Command::new("bash")
            .arg(repo_root.join("scripts").join("package_release.sh"))
            .current_dir(repo_root)
            .status()
            .expect("Run package_release.sh");
        assert!(pkg_status.success(), "package_release.sh failed");
    }

    assert!(dist_tarball.is_file(), "Distribution tarball exists");

    // 1. Unpack tarball in a clean isolated directory
    let test_dir = tempdir().unwrap();
    let unpack_status = Command::new("tar")
        .arg("-xzf")
        .arg(&dist_tarball)
        .current_dir(test_dir.path())
        .status()
        .expect("Unpack release tarball");
    assert!(unpack_status.success(), "Failed to unpack release tarball");

    let stage_dir = test_dir
        .path()
        .join("vanta-v1.0-RC2-x86_64-unknown-linux-gnu");
    assert!(stage_dir.is_dir());

    // 2. Install to custom prefix
    let install_prefix = test_dir.path().join("installed_prefix");
    let install_status = Command::new("bash")
        .arg(stage_dir.join("install.sh"))
        .arg(format!("--prefix={}", install_prefix.display()))
        .current_dir(&stage_dir)
        .status()
        .expect("Run install.sh");
    assert!(install_status.success(), "install.sh failed");

    let installed_vanta = install_prefix.join("bin").join("vanta");
    let installed_vantac = install_prefix.join("bin").join("vantac");
    let installed_rt = install_prefix
        .join("lib")
        .join("vanta")
        .join("libvanta_rt.a");
    let installed_std = install_prefix.join("lib").join("vanta").join("std");

    assert!(installed_vanta.is_file(), "vanta binary installed");
    assert!(installed_vantac.is_file(), "vantac binary installed");
    assert!(installed_rt.is_file(), "libvanta_rt.a installed");
    assert!(installed_std.join("mod.va").is_file(), "std::mod installed");
    assert!(installed_std.join("io.va").is_file(), "std::io installed");
    assert!(installed_std.join("fs.va").is_file(), "std::fs installed");
    assert!(installed_std.join("env.va").is_file(), "std::env installed");

    // 3. Test running compiler from installed location on standalone file outside repo
    let app_dir = tempdir().unwrap();
    let sample_va = app_dir.path().join("sample.va");
    fs::write(
        &sample_va,
        r#"
import std::io;
import std::fs;
import std::env;
import std::result::Result;

fn main() -> i64 {
    let args_res = env::args();
    match args_res {
        Result::Ok(args) => {
            io::write_stdout("argc: ");
            print_i64(args.len());
            io::write_stdout("\n");
            let mut i = 0;
            while i < args.len() {
                io::write_stdout(args[i].as_str());
                io::write_stdout("\n");
                i = i + 1;
            }
        }
        Result::Err(_) => {
            return 1;
        }
    }

    let w = fs::write("msg.txt", "Standalone success!\n");
    match w {
        Result::Ok(_) => {}
        Result::Err(_) => {
            return 2;
        }
    }

    let r = fs::read_to_string("msg.txt");
    match r {
        Result::Ok(txt) => {
            io::write_stdout(txt.as_str());
        }
        Result::Err(_) => {
            return 3;
        }
    }

    return 0;
}
"#,
    )
    .unwrap();

    // 4. Test `vanta run` with arguments
    let run_output = Command::new(&installed_vanta)
        .arg("run")
        .arg(&sample_va)
        .arg("--")
        .arg("arg_alpha")
        .arg("arg_beta")
        .current_dir(app_dir.path())
        .output()
        .expect("Run installed vanta run");

    assert_eq!(
        run_output.status.code(),
        Some(0),
        "vanta run exited with code {:?}. Stderr: {}",
        run_output.status.code(),
        String::from_utf8_lossy(&run_output.stderr)
    );

    let stdout_str = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout_str.contains("argc: 3\n"), "got: {}", stdout_str);
    assert!(stdout_str.contains("arg_alpha\n"), "got: {}", stdout_str);
    assert!(stdout_str.contains("arg_beta\n"), "got: {}", stdout_str);
    assert!(
        stdout_str.contains("Standalone success!\n"),
        "got: {}",
        stdout_str
    );

    // 5. Test `vanta compile` to native executable
    let standalone_bin = app_dir.path().join("sample_prog");
    let compile_output = Command::new(&installed_vanta)
        .arg("compile")
        .arg(&sample_va)
        .arg("-o")
        .arg(&standalone_bin)
        .current_dir(app_dir.path())
        .output()
        .expect("Run installed vanta compile");

    assert_eq!(
        compile_output.status.code(),
        Some(0),
        "vanta compile failed: {}",
        String::from_utf8_lossy(&compile_output.stderr)
    );
    assert!(standalone_bin.is_file(), "Compiled binary exists");

    let bin_output = Command::new(&standalone_bin)
        .arg("param1")
        .current_dir(app_dir.path())
        .output()
        .expect("Run compiled binary");

    assert_eq!(bin_output.status.code(), Some(0));
    let bin_stdout = String::from_utf8_lossy(&bin_output.stdout);
    assert!(bin_stdout.contains("argc: 2\n"));
    assert!(bin_stdout.contains("param1\n"));

    // 6. Test uninstall.sh
    let uninstall_status = Command::new("bash")
        .arg(stage_dir.join("uninstall.sh"))
        .arg(format!("--prefix={}", install_prefix.display()))
        .current_dir(&stage_dir)
        .status()
        .expect("Run uninstall.sh");

    assert!(uninstall_status.success(), "uninstall.sh failed");
    assert!(!installed_vanta.exists(), "vanta binary removed");
    assert!(!installed_vantac.exists(), "vantac binary removed");
    assert!(!installed_rt.exists(), "libvanta_rt.a removed");
    assert!(!installed_std.exists(), "std directory removed");
}
