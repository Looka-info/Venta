use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=VENTA_GIT_REV");
    println!("cargo:rerun-if-changed=src");

    // Check if explicitly provided via env
    if let Ok(rev) = std::env::var("VENTA_GIT_REV") {
        if !rev.trim().is_empty() {
            println!("cargo:rustc-env=VENTA_BUILD_GIT_REV={}", rev.trim());
            return;
        }
    }

    // Try git rev-parse HEAD
    let git_rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        });

    if let Some(rev) = git_rev {
        if !rev.is_empty() {
            println!("cargo:rustc-env=VENTA_BUILD_GIT_REV={}", rev);
            return;
        }
    }

    // Fallback: compute a deterministic digest of source files in src/
    let mut files: Vec<_> = std::fs::read_dir("src")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    files.sort();

    let mut combined_src = Vec::new();
    for f in files {
        if let Ok(bytes) = std::fs::read(&f) {
            combined_src.extend_from_slice(&bytes);
        }
    }

    // Simple FNV-1a 64-bit digest if no git available
    let mut hash = 0xcbf29ce484222325u64;
    for &b in &combined_src {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let digest = format!("src-digest-{:016x}", hash);
    println!("cargo:rustc-env=VENTA_BUILD_GIT_REV={}", digest);
}
