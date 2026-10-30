//! Comprehensive integration tests for Venta Milestone 32:
//! Incremental compilation, content-addressed build graphs, and cache verification.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use vanta_driver::{
    ArtifactType, CacheManager, CompilerOptions, CompilerSession, LinkerKind, TargetTriple,
};

fn run_executable(exe_path: &Path) -> (i32, String) {
    let output = Command::new(exe_path)
        .output()
        .unwrap_or_else(|e| panic!("Failed to execute {:?}: {}", exe_path, e));
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    (code, stdout)
}

#[test]
fn test_incremental_cold_build_and_warm_noop() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main.out");
    let cache_dir = dir.path().join(".venta_cache");

    let source = r#"
fn main() -> i64 {
    return 42;
}
"#;
    fs::write(&src_path, source).unwrap();

    // 1. Cold build with incremental enabled
    let mut opts1 = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts1.incremental = true;
    opts1.cache_dir = Some(cache_dir.clone());
    opts1.linker = LinkerKind::Internal;
    opts1.build_stats = true;

    let mut session1 = CompilerSession::new();
    let res1 = session1.compile(&opts1);
    assert!(res1.is_ok(), "Cold build failed: {:?}", res1.err());
    assert!(exe_path.exists(), "Executable should exist");

    let stats1 = session1.last_build_stats.expect("stats from cold build");
    assert_eq!(stats1.source_files_changed, 1);
    assert_eq!(stats1.objects_regenerated, 1);
    assert!(stats1.linking_performed, "Cold build must perform link");

    let (code1, _) = run_executable(&exe_path);
    assert_eq!(code1, 42);

    // 2. Warm build (no source change) -> must be a complete no-op hit
    let mut opts2 = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts2.incremental = true;
    opts2.cache_dir = Some(cache_dir.clone());
    opts2.linker = LinkerKind::Internal;
    opts2.build_stats = true;

    let mut session2 = CompilerSession::new();
    let res2 = session2.compile(&opts2);
    assert!(res2.is_ok(), "Warm build failed: {:?}", res2.err());

    let stats2 = session2.last_build_stats.expect("stats from warm build");
    assert_eq!(stats2.source_files_unchanged, 1, "Source file unchanged");
    assert_eq!(stats2.objects_reused, 1, "Cached object should be reused");
    assert_eq!(stats2.objects_regenerated, 0, "No objects regenerated");
    assert!(
        !stats2.linking_performed,
        "Linking must be skipped on no-op build"
    );
    assert!(
        stats2.cache_hits >= 2,
        "Both object and link must hit cache"
    );

    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 42);
}

#[test]
fn test_incremental_source_change_rebuilds_and_relinks() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main.out");
    let cache_dir = dir.path().join(".venta_cache");

    // Initial version returns 10
    fs::write(
        &src_path,
        r#"
fn helper() -> i64 {
    return 10;
}
fn main() -> i64 {
    return helper();
}
"#,
    )
    .unwrap();

    let mut opts = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session = CompilerSession::new();
    assert!(session.compile(&opts).is_ok());
    let (code, _) = run_executable(&exe_path);
    assert_eq!(code, 10);

    // Modify helper() to return 99
    fs::write(
        &src_path,
        r#"
fn helper() -> i64 {
    return 99;
}
fn main() -> i64 {
    return helper();
}
"#,
    )
    .unwrap();

    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    let stats = session2.last_build_stats.expect("stats");
    assert_eq!(stats.source_files_changed, 1);
    assert_eq!(stats.objects_regenerated, 1);
    assert!(stats.linking_performed);

    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 99);
}

#[test]
fn test_incremental_link_only_rebuild() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main.out");
    let cache_dir = dir.path().join(".venta_cache");

    fs::write(
        &src_path,
        r#"
fn main() -> i64 {
    return 77;
}
"#,
    )
    .unwrap();

    let mut opts = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());

    // Delete the final executable output file while keeping cache intact
    fs::remove_file(&exe_path).unwrap();
    assert!(!exe_path.exists());

    // Rebuild: should reuse the cached object artifact and run ONLY the linker!
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    assert!(exe_path.exists(), "Executable should be relinked");

    let stats = session2.last_build_stats.expect("stats");
    assert_eq!(stats.objects_reused, 1, "Object must be reused from cache");
    assert_eq!(stats.objects_regenerated, 0, "No compilation should occur");
    assert!(
        stats.linking_performed,
        "Relinking must occur because executable was missing"
    );

    let (code, _) = run_executable(&exe_path);
    assert_eq!(code, 77);
}

#[test]
fn test_cache_corruption_recovery() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main.out");
    let cache_dir = dir.path().join(".venta_cache");

    fs::write(
        &src_path,
        r#"
fn main() -> i64 {
    return 55;
}
"#,
    )
    .unwrap();

    let mut opts = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());
    let (code1, _) = run_executable(&exe_path);
    assert_eq!(code1, 55);

    // Corrupt all cached object files in the cache directory
    let triple_str = TargetTriple::default().spec().triple.to_string();
    let triple_dir = cache_dir.join(&triple_str);
    let objects_dir = triple_dir.join("objects");
    if let Ok(entries) = fs::read_dir(&objects_dir) {
        for entry in entries.flatten() {
            fs::write(entry.path(), b"TRUNCATED AND CORRUPTED DATA").unwrap();
        }
    }

    // Incremental rebuild must detect corrupted artifact, recover, and rebuild cleanly
    let mut session2 = CompilerSession::new();
    let res = session2.compile(&opts);
    assert!(
        res.is_ok(),
        "Rebuild after corruption should succeed: {:?}",
        res.err()
    );

    let stats = session2.last_build_stats.expect("stats");
    assert!(
        stats.cache_corruptions >= 1,
        "Corruption recovery should be recorded"
    );
    assert_eq!(stats.objects_regenerated, 1, "Object should be regenerated");

    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 55);
}

#[test]
fn test_clean_vs_incremental_output_equivalence() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_clean = dir.path().join("clean.out");
    let exe_inc = dir.path().join("inc.out");
    let cache_dir = dir.path().join(".venta_cache");

    let source = r#"
struct Point {
    x: i64,
    y: i64,
}

fn add_points(p1: Point, p2: Point) -> Point {
    return Point { x: p1.x + p2.x, y: p1.y + p2.y };
}

fn main() -> i64 {
    let a = Point { x: 10, y: 20 };
    let b = Point { x: 3, y: 4 };
    let c = add_points(a, b);
    return c.x + c.y;
}
"#;
    fs::write(&src_path, source).unwrap();

    // 1. Clean build (non-incremental oracle)
    let mut opts_clean = CompilerOptions::new(src_path.clone(), exe_clean.clone());
    opts_clean.incremental = false;
    opts_clean.linker = LinkerKind::Internal;

    let mut session_clean = CompilerSession::new();
    assert!(session_clean.compile(&opts_clean).is_ok());
    let (code_clean, _) = run_executable(&exe_clean);

    // 2. Incremental build
    let mut opts_inc = CompilerOptions::new(src_path.clone(), exe_inc.clone());
    opts_inc.incremental = true;
    opts_inc.cache_dir = Some(cache_dir);
    opts_inc.linker = LinkerKind::Internal;

    let mut session_inc = CompilerSession::new();
    assert!(session_inc.compile(&opts_inc).is_ok());
    let (code_inc, _) = run_executable(&exe_inc);

    // Both outputs must be identically correct
    assert_eq!(code_clean, 37);
    assert_eq!(code_inc, 37);
    assert_eq!(code_clean, code_inc);
}

#[test]
fn test_cache_cli_commands_and_stats() {
    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join(".venta_cache");
    let target = TargetTriple::default().spec().triple.to_string();

    let cache = CacheManager::new(cache_dir.clone(), &target).unwrap();

    let key = vanta_driver::ContentFingerprint::for_bytes(
        vanta_driver::Domain::Object,
        b"sample_object_code",
    );
    cache
        .store_artifact(
            &key,
            ArtifactType::Object,
            b"binary machine code payload",
            &[],
        )
        .unwrap();

    // Verify stats
    let stats = cache.stats();
    assert_eq!(stats.object_count, 1);
    assert!(stats.total_bytes > 0);

    // Verify cache integrity
    let verify = cache.verify_cache();
    assert_eq!(verify.valid_entries, 1);
    assert_eq!(verify.corrupted_entries, 0);

    // Clean cache
    let clean = cache.clean_cache(None);
    assert!(clean.files_removed >= 1);
    assert!(clean.bytes_freed > 0);

    let stats_after = cache.stats();
    assert_eq!(stats_after.object_count, 0);
    assert_eq!(stats_after.total_bytes, 0);
}

#[test]
fn test_incremental_generics_monomorphization() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main.out");
    let cache_dir = dir.path().join(".venta_cache");

    let source = r#"
fn identity<T>(x: T) -> T {
    return x;
}

fn main() -> i64 {
    let a: i64 = identity(40);
    let b: i64 = identity(2);
    return a + b;
}
"#;
    fs::write(&src_path, source).unwrap();

    let mut opts = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    // Cold build with generics
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());
    let (code1, _) = run_executable(&exe_path);
    assert_eq!(code1, 42);

    // Warm no-op build with generics
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    let stats = session2.last_build_stats.expect("stats");
    assert_eq!(stats.objects_reused, 1);
    assert!(!stats.linking_performed);
    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 42);
}

#[test]
fn test_incremental_trait_dispatch() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main.out");
    let cache_dir = dir.path().join(".venta_cache");

    let source = r#"
trait Adder {
    fn add_ten(self: &Self) -> i64;
}

struct Value {
    val: i64,
}

impl Adder for Value {
    fn add_ten(self: &Value) -> i64 {
        return self.val + 10;
    }
}

fn main() -> i64 {
    let v = Value { val: 50 };
    return v.add_ten();
}
"#;
    fs::write(&src_path, source).unwrap();

    let mut opts = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    // Cold build
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());
    let (code1, _) = run_executable(&exe_path);
    assert_eq!(code1, 60);

    // Warm no-op build
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    let stats = session2.last_build_stats.expect("stats");
    assert_eq!(stats.objects_reused, 1);
    assert!(!stats.linking_performed);
    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 60);
}

#[test]
fn test_target_cache_namespace_isolation() {
    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join(".venta_cache");

    let target_linux = "x86_64-unknown-linux-gnu";
    let target_windows = "x86_64-pc-windows-msvc";

    let cache_linux = CacheManager::new(cache_dir.clone(), target_linux).unwrap();
    let cache_win = CacheManager::new(cache_dir.clone(), target_windows).unwrap();

    let key = vanta_driver::ContentFingerprint::for_bytes(
        vanta_driver::Domain::Object,
        b"shared_key_different_targets",
    );

    // Store linux artifact
    cache_linux
        .store_artifact(&key, ArtifactType::Object, b"linux ELF object", &[])
        .unwrap();

    // Verify linux cache has it, windows cache does NOT
    let linux_art = cache_linux
        .read_artifact(&key, ArtifactType::Object)
        .unwrap();
    assert_eq!(linux_art, Some(b"linux ELF object".to_vec()));

    let win_art = cache_win.read_artifact(&key, ArtifactType::Object).unwrap();
    assert_eq!(win_art, None, "Target caches must be strictly isolated!");
}

#[test]
fn test_explain_rebuild_and_stats_reporting() {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main.out");
    let cache_dir = dir.path().join(".venta_cache");

    fs::write(&src_path, "fn main() -> i64 { return 1; }").unwrap();

    let mut opts = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;
    opts.explain_rebuild = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());
    assert!(
        !session1.rebuild_explanations.is_empty(),
        "Cold build must explain rebuild"
    );

    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    assert!(
        session2.rebuild_explanations.is_empty(),
        "Warm no-op build should have no rebuild explanations"
    );
}
