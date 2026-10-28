//! Integration tests verifying version consistency, release gate validation, and CI version policies.

use std::path::Path;
use std::process::Command;

#[test]
fn test_version_consistency_and_gate_validation() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let script = repo_root.join("scripts").join("test_version_validation.py");
    assert!(script.is_file(), "test_version_validation.py must exist");

    let status = Command::new("python3")
        .arg(&script)
        .current_dir(repo_root.join("scripts"))
        .status()
        .expect("Failed to execute python3 test_version_validation.py");

    assert!(status.success(), "Version consistency regression tests failed");
}

#[test]
fn test_current_workspace_version_consistency_script() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let script = repo_root.join("scripts").join("verify_version_consistency.py");
    assert!(script.is_file(), "verify_version_consistency.py must exist");

    let output = Command::new("python3")
        .arg(&script)
        .current_dir(repo_root)
        .output()
        .expect("Failed to execute python3 verify_version_consistency.py");

    assert!(
        output.status.success(),
        "verify_version_consistency.py failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Version validation:      PASSED"), "Output: {}", stdout);
    assert!(stdout.contains("Cargo workspace version: 1.0.0-rc.2"), "Output: {}", stdout);
}
