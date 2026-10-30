use std::path::PathBuf;

#[path = "src/source_digest.rs"]
mod source_digest;

fn find_workspace_root() -> Option<PathBuf> {
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(manifest_dir);
        if let Some(ws) = p.parent().and_then(|p| p.parent()) {
            if ws.join("Cargo.toml").exists() {
                return Some(ws.to_path_buf());
            }
        }
    }
    if let Ok(mut curr) = std::env::current_dir() {
        loop {
            if curr.join("Cargo.toml").exists() && curr.join("crates").is_dir() {
                return Some(curr);
            }
            if !curr.pop() {
                break;
            }
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-env-changed=VENTA_GIT_REV");

    let ws_root = find_workspace_root().unwrap_or_else(|| PathBuf::from("../.."));

    // 1. Tell Cargo to rerun this build script whenever ANY compiler source file changes!
    if let Ok(files) = source_digest::scan_compiler_source_files(&ws_root) {
        for rel in &files {
            let full = ws_root.join(rel);
            println!("cargo:rerun-if-changed={}", full.display());
        }
    }
    // Also explicitly monitor key manifests and root directories
    println!(
        "cargo:rerun-if-changed={}",
        ws_root.join("Cargo.toml").display()
    );
    if ws_root.join("Cargo.lock").exists() {
        println!(
            "cargo:rerun-if-changed={}",
            ws_root.join("Cargo.lock").display()
        );
    }
    if ws_root.join("crates").is_dir() {
        println!(
            "cargo:rerun-if-changed={}",
            ws_root.join("crates").display()
        );
    }
    if ws_root.join("runtime").is_dir() {
        println!(
            "cargo:rerun-if-changed={}",
            ws_root.join("runtime").display()
        );
    }
    if ws_root.join("library").is_dir() {
        println!(
            "cargo:rerun-if-changed={}",
            ws_root.join("library").display()
        );
    }

    // 2. Compute the deterministic source-tree digest
    let source_digest_hex = match source_digest::compute_source_tree_digest(&ws_root) {
        Ok(d) => d.to_hex(),
        Err(e) => {
            eprintln!("Warning: failed to compute source tree digest: {}", e);
            "0000000000000000000000000000000000000000000000000000000000000000".to_string()
        }
    };
    println!(
        "cargo:rustc-env=VENTA_COMPILER_SOURCE_DIGEST={}",
        source_digest_hex
    );

    // 3. Supplementary Git revision (if available, e.g. commit hash)
    let git_rev = std::env::var("VENTA_GIT_REV").ok().or_else(|| {
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&ws_root)
            .output()
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    String::from_utf8(out.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
    });

    if let Some(rev) = git_rev {
        if !rev.is_empty() {
            println!("cargo:rustc-env=VENTA_BUILD_GIT_REV={}", rev);
        }
    }
}
