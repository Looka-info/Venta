use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use vanta_build::fingerprint::CompilerIdentity;
use vanta_build::source_digest::{
    compute_source_tree_digest, compute_source_tree_digest_with_reader, scan_compiler_source_files,
};

fn create_mock_compiler_tree() -> TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    // 1. Workspace manifests and lockfile
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n",
    )
    .expect("write Cargo.toml");
    fs::write(root.join("Cargo.lock"), "# lockfile content\n").expect("write Cargo.lock");

    // 2. Compiler crate: vanta_driver
    fs::create_dir_all(root.join("crates/vanta_driver/src")).expect("create vanta_driver");
    fs::write(
        root.join("crates/vanta_driver/Cargo.toml"),
        "[package]\nname = \"vanta_driver\"\nversion = \"0.1.0\"\n",
    )
    .expect("write driver Cargo.toml");
    fs::write(
        root.join("crates/vanta_driver/src/lib.rs"),
        "pub fn compile() {}\n",
    )
    .expect("write driver lib.rs");

    // 3. Nested backend crate: vanta_codegen_x86_64
    fs::create_dir_all(root.join("crates/vanta_codegen_x86_64/src"))
        .expect("create vanta_codegen_x86_64");
    fs::write(
        root.join("crates/vanta_codegen_x86_64/Cargo.toml"),
        "[package]\nname = \"vanta_codegen_x86_64\"\nversion = \"0.1.0\"\n",
    )
    .expect("write codegen Cargo.toml");
    fs::write(
        root.join("crates/vanta_codegen_x86_64/src/lib.rs"),
        "pub fn emit_x86() {}\n",
    )
    .expect("write codegen lib.rs");

    // 4. Runtime input
    fs::create_dir_all(root.join("runtime")).expect("create runtime");
    fs::write(root.join("runtime/vanta_rt.c"), "void rt_init() {}\n").expect("write runtime c");

    // 5. Standard library input
    fs::create_dir_all(root.join("library/std/src")).expect("create library");
    fs::write(
        root.join("library/std/src/lib.va"),
        "fn println(s: str) {}\n",
    )
    .expect("write std va");

    tmp
}

#[test]
fn test_1_same_tree_produces_same_digest() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    let digest1 = compute_source_tree_digest(root).expect("digest 1");
    let digest2 = compute_source_tree_digest(root).expect("digest 2");

    assert_eq!(
        digest1, digest2,
        "Two evaluations on the exact same tree must produce identical digests"
    );
    assert_eq!(digest1.to_hex(), digest2.to_hex());
    assert_eq!(digest1.to_hex().len(), 64);
}

#[test]
fn test_2_modifying_compiler_source_file_changes_digest() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    let base_digest = compute_source_tree_digest(root).expect("base digest");

    // Modify a compiler source file in crates/vanta_driver/src/lib.rs
    fs::write(
        root.join("crates/vanta_driver/src/lib.rs"),
        "pub fn compile() { /* modified */ }\n",
    )
    .expect("modify file");

    let modified_digest = compute_source_tree_digest(root).expect("modified digest");
    assert_ne!(
        base_digest, modified_digest,
        "Modifying a compiler source file must change the source tree digest"
    );
}

#[test]
fn test_3_modifying_nested_backend_source_changes_digest() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    let base_digest = compute_source_tree_digest(root).expect("base digest");

    // Modify nested backend file
    fs::write(
        root.join("crates/vanta_codegen_x86_64/src/lib.rs"),
        "pub fn emit_x86() { /* modified instruction selection */ }\n",
    )
    .expect("modify backend file");

    let modified_digest = compute_source_tree_digest(root).expect("modified digest");
    assert_ne!(
        base_digest, modified_digest,
        "Modifying a nested backend source file must change the source tree digest"
    );
}

#[test]
fn test_4_modifying_embedded_runtime_input_changes_digest() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    let base_digest = compute_source_tree_digest(root).expect("base digest");

    // 4a. Modify runtime C input
    fs::write(
        root.join("runtime/vanta_rt.c"),
        "void rt_init() { /* updated runtime allocator */ }\n",
    )
    .expect("modify runtime");

    let runtime_modified_digest = compute_source_tree_digest(root).expect("runtime modified");
    assert_ne!(
        base_digest, runtime_modified_digest,
        "Modifying embedded runtime C input must change the source tree digest"
    );

    // 4b. Modify standard library input
    fs::write(
        root.join("library/std/src/lib.va"),
        "fn println(s: str) { /* updated stdlib print */ }\n",
    )
    .expect("modify stdlib");

    let stdlib_modified_digest = compute_source_tree_digest(root).expect("stdlib modified");
    assert_ne!(
        runtime_modified_digest, stdlib_modified_digest,
        "Modifying standard library source input must change the source tree digest"
    );
}

#[test]
fn test_5_uncommitted_modification_changes_identity_without_git_commit() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    // Initialize git repository and commit all files
    let _ = Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.email", "test@venta.org"])
        .current_dir(root)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.name", "Venta Tester"])
        .current_dir(root)
        .output();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(root)
        .output();

    let id_clean = CompilerIdentity::from_source_tree("x86_64-unknown-linux-gnu", root)
        .expect("clean identity");

    // Modify a source file WITHOUT committing to git (working tree is now dirty)
    fs::write(
        root.join("crates/vanta_driver/src/lib.rs"),
        "pub fn compile() { /* uncommitted dirty changes */ }\n",
    )
    .expect("dirty edit");

    let id_dirty = CompilerIdentity::from_source_tree("x86_64-unknown-linux-gnu", root)
        .expect("dirty identity");

    // Verify git revision is unchanged (no commit was made)
    assert_eq!(
        id_clean.git_revision, id_dirty.git_revision,
        "Git revision is identical because no commit occurred"
    );

    // BUT the source_digest and fingerprint MUST change!
    assert_ne!(
        id_clean.source_digest, id_dirty.source_digest,
        "Uncommitted modification must change source digest without git commit"
    );
    assert_ne!(
        id_clean.fingerprint(),
        id_dirty.fingerprint(),
        "Uncommitted modification must change compiler fingerprint without git commit"
    );
}

#[test]
fn test_6_source_archive_without_git_produces_valid_identity() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    // Ensure no .git exists
    assert!(!root.join(".git").exists());

    let id = CompilerIdentity::from_source_tree("x86_64-unknown-linux-gnu", root)
        .expect("source archive identity");

    assert_eq!(
        id.source_digest.len(),
        64,
        "Source digest must be 64-hex chars"
    );
    assert_ne!(
        id.source_digest,
        "0000000000000000000000000000000000000000000000000000000000000000"
    );

    let fp = id.fingerprint();
    assert_ne!(fp.to_hex().len(), 0);
}

#[test]
fn test_7_modifying_target_or_cache_does_not_change_identity() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    let base_digest = compute_source_tree_digest(root).expect("base digest");

    // 1. Create files in target/ directory
    fs::create_dir_all(root.join("target/debug/incremental")).expect("create target");
    fs::write(
        root.join("target/debug/libvanta.rlib"),
        "compiled rlib artifact",
    )
    .expect("write target file");

    // 2. Create files in .venta_cache/ directory
    fs::create_dir_all(root.join(".venta_cache/objects")).expect("create cache");
    fs::write(
        root.join(".venta_cache/objects/test.o"),
        "cached object file",
    )
    .expect("write cache file");

    // 3. Create editor backup and temporary files
    fs::write(
        root.join("crates/vanta_driver/src/lib.rs~"),
        "backup editor copy",
    )
    .expect("write backup");
    fs::write(
        root.join("crates/vanta_driver/src/temp.tmp"),
        "temporary file",
    )
    .expect("write tmp");

    // 4. Create compiled binary files in runtime
    fs::write(root.join("runtime/libvanta_rt.a"), "compiled archive").expect("write a");
    fs::write(root.join("runtime/vanta_rt.o"), "compiled object").expect("write o");

    let post_digest = compute_source_tree_digest(root).expect("post digest");
    assert_eq!(
        base_digest, post_digest,
        "Modifying target, .venta_cache, temp files, or compiled binaries must NOT change compiler identity"
    );
}

#[test]
fn test_8_workspace_root_digest_evaluation() {
    let ws_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");

    let digest = compute_source_tree_digest(ws_root).expect("ws digest");
    let hex = digest.to_hex();
    assert_eq!(hex.len(), 64);

    let id = CompilerIdentity::current("x86_64-unknown-linux-gnu");
    assert_eq!(id.source_digest.len(), 64);
    assert_eq!(
        id.source_digest, hex,
        "Embedded compile-time digest matches current workspace evaluation"
    );
}

#[test]
fn test_failclosed_1_nonexistent_workspace_root_returns_error() {
    let non_existent = Path::new("nonexistent/path/for/venta/workspace/does/not/exist");
    assert!(
        scan_compiler_source_files(non_existent).is_err(),
        "Scanning a non-existent workspace root must return an io::Error"
    );
    assert!(
        compute_source_tree_digest(non_existent).is_err(),
        "Computing digest on a non-existent workspace root must return an io::Error"
    );
}

#[test]
fn test_failclosed_2_regular_file_instead_of_dir_returns_error() {
    let tmp_file = tempfile::NamedTempFile::new().expect("create named temp file");
    let file_path = tmp_file.path();

    assert!(
        scan_compiler_source_files(file_path).is_err(),
        "Supplying a regular file instead of a directory must return an io::Error"
    );
    assert!(
        compute_source_tree_digest(file_path).is_err(),
        "Computing digest on a file instead of a directory must return an io::Error"
    );
}

#[test]
fn test_failclosed_3_failed_source_file_read_returns_error_rather_than_partial_digest() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    // Use injectable file reader where reading runtime/vanta_rt.c fails deterministically
    let result = compute_source_tree_digest_with_reader(root, |path| {
        let path_str = path.to_string_lossy();
        if path_str.contains("vanta_rt.c") {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Deterministic simulated read failure on vanta_rt.c",
            ))
        } else {
            fs::read(path)
        }
    });

    assert!(
        result.is_err(),
        "A failed source-file read must immediately fail closed and return an Err, not a partial digest"
    );
}

#[test]
fn test_failclosed_4_discovery_failure_cannot_produce_valid_compiler_identity() {
    let non_existent = Path::new("nonexistent/path/for/compiler/identity");
    let result = CompilerIdentity::from_source_tree("x86_64-unknown-linux-gnu", non_existent);
    assert!(
        result.is_err(),
        "Discovery failure must return Err and prevent constructing CompilerIdentity"
    );

    let tree = create_mock_compiler_tree();
    let root = tree.path();
    let read_err_result =
        CompilerIdentity::from_source_tree_with_reader("x86_64-unknown-linux-gnu", root, |_| {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Injected read failure",
            ))
        });
    assert!(
        read_err_result.is_err(),
        "File read error during identity calculation must return Err"
    );

    // Validation checks on corrupt or missing source digests
    let bad_empty = CompilerIdentity {
        compiler_version: "2.1.0".into(),
        cache_schema_version: 1,
        git_revision: "rev".into(),
        target_triple: "x86_64-unknown-linux-gnu".into(),
        source_digest: String::new(),
    };
    assert!(bad_empty.validate().is_err());

    let bad_all_zero = CompilerIdentity {
        compiler_version: "2.1.0".into(),
        cache_schema_version: 1,
        git_revision: "rev".into(),
        target_triple: "x86_64-unknown-linux-gnu".into(),
        source_digest: "0000000000000000000000000000000000000000000000000000000000000000".into(),
    };
    assert!(bad_all_zero.validate().is_err());
}

#[test]
fn test_failclosed_5_valid_source_tree_produces_identical_digest_as_before() {
    let tree = create_mock_compiler_tree();
    let root = tree.path();

    let d1 = compute_source_tree_digest(root).expect("digest 1");
    let d2 = compute_source_tree_digest(root).expect("digest 2");

    assert_eq!(d1, d2);
    assert_eq!(d1.to_hex().len(), 64);
    assert!(vanta_build::source_digest::is_valid_digest(&d1.to_hex()));
}
