//! Comprehensive integration tests for Venta Milestone 32:
//! Incremental compilation, content-addressed build graphs, and cache verification.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use vanta_driver::{
    ArtifactType, CacheManager, CompilerIdentity, CompilerOptions, CompilerSession,
    IncrementalBuildEngine, IncrementalOptions, LinkerKind, TargetTriple,
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

// =========================================================================
// Milestone 32.5 Stage 32.5.7: Tests A through J
// Multi-Module Project and Function-Level Invalidation Suite
// =========================================================================

fn setup_multi_module_project(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    let main_va = src_dir.join("main.va");
    let math_va = src_dir.join("math.va");
    let vector_va = src_dir.join("vector.va");
    let geometry_va = src_dir.join("geometry.va");

    fs::write(
        &math_va,
        r#"
pub fn add(a: i64, b: i64) -> i64 {
    return a + b;
}

fn private_math_helper(x: i64) -> i64 {
    return x * 3;
}

pub fn scaled_add(a: i64, b: i64) -> i64 {
    let p = private_math_helper(a);
    return p + b;
}
"#,
    )
    .unwrap();

    fs::write(
        &vector_va,
        r#"
pub struct Vec2 {
    pub x: i64,
    pub y: i64,
}

pub fn vec2_new(x: i64, y: i64) -> Vec2 {
    return Vec2 { x: x, y: y };
}

pub fn vec2_dot(v1: Vec2, v2: Vec2) -> i64 {
    return v1.x * v2.x + v1.y * v2.y;
}
"#,
    )
    .unwrap();

    fs::write(
        &geometry_va,
        r#"
pub trait Measurable {
    fn measure(self: &Self) -> i64;
}

pub struct Rect {
    pub width: i64,
    pub height: i64,
}

impl Measurable for Rect {
    fn measure(self: &Rect) -> i64 {
        return self.width * self.height;
    }
}

pub fn compute_area(r: &Rect) -> i64 {
    return r.measure();
}

pub fn generic_identity<T>(x: T) -> T {
    return x;
}
"#,
    )
    .unwrap();

    fs::write(
        &main_va,
        r#"
mod math;
mod vector;
mod geometry;

import math::scaled_add;
import vector::{Vec2, vec2_new, vec2_dot};
import geometry::{Rect, compute_area, generic_identity};

fn private_main_calc() -> i64 {
    return 100;
}

pub fn main() -> i64 {
    let s = scaled_add(10, 5); // 10 * 3 + 5 = 35
    let v1 = vec2_new(1, 2);
    let v2 = vec2_new(3, 4);
    let dot = vec2_dot(v1, v2); // 1*3 + 2*4 = 11
    let r = Rect { width: 4, height: 5 };
    let area = compute_area(&r); // 4 * 5 = 20
    let gid = generic_identity(s); // 35
    let p = private_main_calc(); // 100
    return gid + area + dot + p; // 35 + 20 + 11 + 100 = 166
}
"#,
    )
    .unwrap();

    let exe_path = dir.join("app.out");
    let cache_dir = dir.join(".venta_cache");
    (main_va, exe_path, cache_dir)
}

#[test]
fn test_stage32_5_test_a_multi_module_noop_rebuild() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    // 1. Initial cold build
    let mut session1 = CompilerSession::new();
    let res1 = session1.compile(&opts);
    assert!(res1.is_ok(), "Cold build failed: {:?}", res1.err());
    let (code1, _) = run_executable(&exe_path);
    assert_eq!(code1, 166);

    let stats1 = session1.last_build_stats.expect("stats1");
    assert_eq!(stats1.source_files_changed, 4);
    assert_eq!(stats1.objects_regenerated, 1);
    assert!(stats1.linking_performed);

    // 2. Test A: Warm No-Op Rebuild
    let mut session2 = CompilerSession::new();
    let res2 = session2.compile(&opts);
    assert!(res2.is_ok(), "Warm build failed: {:?}", res2.err());
    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 166);

    let stats2 = session2.last_build_stats.expect("stats2");
    assert_eq!(stats2.source_files_unchanged, 4);
    assert_eq!(stats2.source_files_changed, 0);
    assert_eq!(stats2.objects_reused, 1);
    assert_eq!(stats2.objects_regenerated, 0);
    assert!(!stats2.linking_performed, "No-op must skip relinking");
    assert_eq!(stats2.functions_recompiled, 0);
}

#[test]
fn test_stage32_5_test_b_private_function_edit_and_fragment_reuse() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());
    let (code1, _) = run_executable(&exe_path);
    assert_eq!(code1, 166);

    // Test B: Modify only the body of private_math_helper in math.va (x * 3 -> x * 5)
    let math_va = dir.path().join("src/math.va");
    fs::write(
        &math_va,
        r#"
pub fn add(a: i64, b: i64) -> i64 {
    return a + b;
}

fn private_math_helper(x: i64) -> i64 {
    return x * 5;
}

pub fn scaled_add(a: i64, b: i64) -> i64 {
    let p = private_math_helper(a);
    return p + b;
}
"#,
    )
    .unwrap();

    let mut session2 = CompilerSession::new();
    let res2 = session2.compile(&opts);
    assert!(res2.is_ok(), "Rebuild failed: {:?}", res2.err());

    // Exit code: scaled_add is now 10 * 5 + 5 = 55. Total is 55 + 20 + 11 + 100 = 186.
    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 186);

    let stats2 = session2.last_build_stats.expect("stats2");
    assert_eq!(stats2.source_files_changed, 1);
    assert_eq!(stats2.source_files_unchanged, 3);
    assert!(stats2.linking_performed);
    assert!(
        stats2.functions_cached >= 1,
        "Unmodified function fragments must be reused"
    );
}

#[test]
fn test_stage32_5_test_c_public_signature_change() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());

    // Test C: Change scaled_add signature to accept a third argument 'c: i64'
    let math_va = dir.path().join("src/math.va");
    fs::write(
        &math_va,
        r#"
pub fn add(a: i64, b: i64) -> i64 {
    return a + b;
}

fn private_math_helper(x: i64) -> i64 {
    return x * 3;
}

pub fn scaled_add(a: i64, b: i64, c: i64) -> i64 {
    let p = private_math_helper(a);
    return p + b + c;
}
"#,
    )
    .unwrap();

    // Update main.va caller to pass the 3rd argument
    fs::write(
        &main_va,
        r#"
mod math;
mod vector;
mod geometry;

import math::scaled_add;
import vector::{Vec2, vec2_new, vec2_dot};
import geometry::{Rect, compute_area, generic_identity};

fn private_main_calc() -> i64 {
    return 100;
}

pub fn main() -> i64 {
    let s = scaled_add(10, 5, 10); // 10 * 3 + 5 + 10 = 45
    let v1 = vec2_new(1, 2);
    let v2 = vec2_new(3, 4);
    let dot = vec2_dot(v1, v2); // 11
    let r = Rect { width: 4, height: 5 };
    let area = compute_area(&r); // 20
    let gid = generic_identity(s); // 45
    let p = private_main_calc(); // 100
    return gid + area + dot + p; // 45 + 20 + 11 + 100 = 176
}
"#,
    )
    .unwrap();

    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 176);
}

#[test]
fn test_stage32_5_test_d_generic_template_change() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());

    // Test D: Change generic function in geometry.va
    let geometry_va = dir.path().join("src/geometry.va");
    fs::write(
        &geometry_va,
        r#"
pub trait Measurable {
    fn measure(self: &Self) -> i64;
}

pub struct Rect {
    pub width: i64,
    pub height: i64,
}

impl Measurable for Rect {
    fn measure(self: &Rect) -> i64 {
        return self.width * self.height;
    }
}

pub fn compute_area(r: &Rect) -> i64 {
    return r.measure();
}

pub fn generic_identity<T>(x: T) -> T {
    // Replaced generic body
    let temp: T = x;
    return temp;
}
"#,
    )
    .unwrap();

    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 166);
}

#[test]
fn test_stage32_5_test_e_type_layout_change() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());

    // Test E: Change struct Rect layout in geometry.va by adding a 'depth' field
    let geometry_va = dir.path().join("src/geometry.va");
    fs::write(
        &geometry_va,
        r#"
pub trait Measurable {
    fn measure(self: &Self) -> i64;
}

pub struct Rect {
    pub width: i64,
    pub height: i64,
    pub depth: i64,
}

impl Measurable for Rect {
    fn measure(self: &Rect) -> i64 {
        return self.width * self.height * self.depth;
    }
}

pub fn compute_area(r: &Rect) -> i64 {
    return r.measure();
}

pub fn generic_identity<T>(x: T) -> T {
    return x;
}
"#,
    )
    .unwrap();

    // Update main.va to initialize the new field
    fs::write(
        &main_va,
        r#"
mod math;
mod vector;
mod geometry;

import math::scaled_add;
import vector::{Vec2, vec2_new, vec2_dot};
import geometry::{Rect, compute_area, generic_identity};

fn private_main_calc() -> i64 {
    return 100;
}

pub fn main() -> i64 {
    let s = scaled_add(10, 5); // 35
    let v1 = vec2_new(1, 2);
    let v2 = vec2_new(3, 4);
    let dot = vec2_dot(v1, v2); // 11
    let r = Rect { width: 4, height: 5, depth: 2 };
    let area = compute_area(&r); // 4 * 5 * 2 = 40
    let gid = generic_identity(s); // 35
    let p = private_main_calc(); // 100
    return gid + area + dot + p; // 35 + 40 + 11 + 100 = 186
}
"#,
    )
    .unwrap();

    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 186);
}

#[test]
fn test_stage32_5_test_f_trait_implementation_change() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());

    // Test F: Modify trait implementation logic in geometry.va
    let geometry_va = dir.path().join("src/geometry.va");
    fs::write(
        &geometry_va,
        r#"
pub trait Measurable {
    fn measure(self: &Self) -> i64;
}

pub struct Rect {
    pub width: i64,
    pub height: i64,
}

impl Measurable for Rect {
    fn measure(self: &Rect) -> i64 {
        // Perimeter instead of area
        return (self.width + self.height) * 2;
    }
}

pub fn compute_area(r: &Rect) -> i64 {
    return r.measure();
}

pub fn generic_identity<T>(x: T) -> T {
    return x;
}
"#,
    )
    .unwrap();

    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    // Area is now perimeter: (4 + 5) * 2 = 18. Total = 35 + 18 + 11 + 100 = 164.
    let (code2, _) = run_executable(&exe_path);
    assert_eq!(code2, 164);
}

#[test]
fn test_stage32_5_test_g_link_only_rebuild() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());
    assert_eq!(run_executable(&exe_path).0, 166);

    // Test G: Remove the executable binary while leaving the cache untouched
    fs::remove_file(&exe_path).unwrap();
    assert!(!exe_path.exists());

    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts).is_ok());
    assert!(exe_path.exists(), "Executable should be relinked");
    assert_eq!(run_executable(&exe_path).0, 166);

    let stats2 = session2.last_build_stats.expect("stats2");
    assert_eq!(stats2.objects_reused, 1, "Cached object reused");
    assert_eq!(stats2.objects_regenerated, 0);
    assert!(stats2.linking_performed, "Relink performed");
}

#[test]
fn test_stage32_5_test_h_same_size_archive_change() {
    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join(".venta_cache");
    let target = TargetTriple::default().spec().triple.to_string();

    let inc_opts = IncrementalOptions {
        enabled: true,
        cache_dir: cache_dir.clone(),
        target_triple: target,
        opt_level: "O0".into(),
        target_features: "default".into(),
        debug_info: false,
        explain_rebuild: true,
        build_stats: true,
        build_config: None,
    };
    let engine = IncrementalBuildEngine::new(inc_opts).unwrap();

    let obj_fp =
        vanta_driver::ContentFingerprint::for_bytes(vanta_driver::Domain::Object, b"sample_object");

    let archive_path = dir.path().join("libsample.a");
    // Initial archive of 32 bytes
    fs::write(&archive_path, b"INITIAL_ARCHIVE_DATA_SAME_LEN_32").unwrap();
    assert_eq!(fs::metadata(&archive_path).unwrap().len(), 32);

    let link_fp1 = engine.compute_link_fingerprint(&[obj_fp], &[archive_path.clone()], &[]);

    // Alter contents with exact same length of 32 bytes
    fs::write(&archive_path, b"ALTERED_ARCHIVE_DATA_SAME_LEN_32").unwrap();
    assert_eq!(fs::metadata(&archive_path).unwrap().len(), 32);

    let link_fp2 = engine.compute_link_fingerprint(&[obj_fp], &[archive_path], &[]);

    // Must detect content alteration and produce different fingerprint despite identical size!
    assert_ne!(
        link_fp1, link_fp2,
        "Same-size archive change must produce different link fingerprint"
    );
}

#[test]
fn test_stage32_5_test_i_compiler_identity_invalidation() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts.incremental = true;
    opts.cache_dir = Some(cache_dir.clone());
    opts.linker = LinkerKind::Internal;
    opts.build_stats = true;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts).is_ok());

    // Tamper manifest compiler_id to simulate an older compiler version
    let triple_str = TargetTriple::default().spec().triple.to_string();
    let manifest_path = cache_dir.join(&triple_str).join("manifest.json");
    if manifest_path.exists() {
        let content = fs::read_to_string(&manifest_path).unwrap();
        let tampered = content.replace(
            &CompilerIdentity::current(&triple_str)
                .fingerprint()
                .to_hex(),
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        fs::write(&manifest_path, tampered).unwrap();
    }

    let mut session2 = CompilerSession::new();
    let res2 = session2.compile(&opts);
    assert!(
        res2.is_ok(),
        "Rebuild after compiler ID mismatch should cleanly recompile: {:?}",
        res2.err()
    );
    assert_eq!(run_executable(&exe_path).0, 166);

    let stats2 = session2.last_build_stats.expect("stats2");
    assert_eq!(
        stats2.objects_regenerated, 1,
        "Object should be regenerated on compiler identity change"
    );
}

#[test]
fn test_stage32_5_test_j_target_feature_change() {
    let dir = tempdir().unwrap();
    let (main_va, exe_path, cache_dir) = setup_multi_module_project(dir.path());

    let mut opts1 = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts1.incremental = true;
    opts1.cache_dir = Some(cache_dir.clone());
    opts1.linker = LinkerKind::Internal;
    opts1.build_stats = true;
    opts1.target_features.avx2 = false;

    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts1).is_ok());
    assert_eq!(run_executable(&exe_path).0, 166);

    // Build for AVX2 enabled
    let mut opts2 = CompilerOptions::new(main_va.clone(), exe_path.clone());
    opts2.incremental = true;
    opts2.cache_dir = Some(cache_dir.clone());
    opts2.linker = LinkerKind::Internal;
    opts2.build_stats = true;
    opts2.target_features.avx2 = true;

    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts2).is_ok());
    assert_eq!(run_executable(&exe_path).0, 166);

    let stats2 = session2.last_build_stats.expect("stats2");
    assert_eq!(
        stats2.objects_regenerated, 1,
        "Target feature change (AVX2) must invalidate cached object and regenerate"
    );
}
