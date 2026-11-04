//! Milestone: Verification of `std::collections` in the Module Loader.
//! Covers Priority 3 requirements:
//! 1. `import std::collections;`
//! 2. Importing and using `HashMap`.
//! 3. Importing and using `RingBuffer`.
//! 4. Nested module resolution.
//! 5. Diagnostics on invalid module access (E0603).
//! 6. Execution under unoptimized (-O0) and optimized (-O1) pipelines.

use std::fs;
use std::process::Command;
use tempfile::tempdir;
use vanta_driver::{
    CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, LinkerKind, OptLevel,
    TargetTriple,
};

fn run_collections_test(name: &str, source: &str, opt_level: OptLevel, expected_exit_code: i32) {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join(format!("{}.va", name));
    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };
    let exe_path = dir.path().join(format!("{}{}", name, exe_suffix));
    fs::write(&src_path, source).unwrap();

    let mut opts = CompilerOptions::new(src_path, exe_path.clone());
    opts.target = TargetTriple::host();
    opts.linker = match std::env::var("VANTA_LINKER").as_deref() {
        Ok("system") => LinkerKind::System,
        _ => LinkerKind::Internal,
    };
    opts.opt_level = opt_level;
    opts.emit_target = EmitTarget::Exe;

    let mut session = CompilerSession::new();
    let res = session.compile(&opts);
    if res.is_err() {
        eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
    }
    assert!(
        res.is_ok(),
        "Compilation failed for {} under {:?}: {:?}",
        name,
        opt_level,
        res.err()
    );
    assert!(exe_path.is_file(), "Executable was not produced");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&exe_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exe_path, perms).unwrap();
    }

    let status = Command::new(&exe_path)
        .status()
        .expect("Failed to execute test binary");
    assert_eq!(
        status.code(),
        Some(expected_exit_code),
        "Exit code mismatch for {} under {:?}",
        name,
        opt_level
    );
}

fn check_compile_failure(source: &str, expected_code: &str) {
    let dir = tempdir().unwrap();
    let src_path = dir.path().join("fail.va");
    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };
    let exe_path = dir.path().join(format!("fail{}", exe_suffix));
    fs::write(&src_path, source).unwrap();

    let mut opts = CompilerOptions::new(src_path, exe_path);
    opts.target = TargetTriple::host();
    opts.linker = match std::env::var("VANTA_LINKER").as_deref() {
        Ok("system") => LinkerKind::System,
        _ => LinkerKind::Internal,
    };
    opts.emit_target = EmitTarget::Obj;

    let mut session = CompilerSession::new();
    let res = session.compile(&opts);
    assert!(res.is_err(), "Expected compilation to fail");
    let json_diag = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(
        json_diag.contains(expected_code),
        "Expected error code {} in diagnostics, got:\n{}",
        expected_code,
        json_diag
    );
}

#[test]
fn test_minimal_main() {
    let source = "fn main() -> i64 { return 77; }";
    run_collections_test("min_main", source, OptLevel::None, 77);
}

#[test]
fn test_import_std_collections_reexports() {
    let source = r#"
import std::collections;
import std::collections::hash_map;
import std::collections::ring_buffer;

fn main() -> i64 {
    let map: collections::HashMap<i64, i64> = collections::new_hash_map(16);
    let rb: collections::RingBuffer<i64> = collections::new_ring_buffer(8);
    let sum = hash_map::len(&map) + ring_buffer::len(&rb);
    return sum;
}
"#;
    run_collections_test("std_coll_o0", source, OptLevel::None, 0);
    run_collections_test("std_coll_o1", source, OptLevel::O1, 0);
}

#[test]
fn test_import_hash_map_direct_and_operations() {
    let source = r#"
import std::collections::HashMap;
import std::collections::new_hash_map;
import std::collections::hash_map::len;
import std::collections::hash_map::is_empty;

fn main() -> i64 {
    let map: HashMap<i64, i64> = new_hash_map(32);
    if !is_empty(&map) {
        return 1;
    }
    if len(&map) != 0 {
        return 2;
    }
    return 42;
}
"#;
    run_collections_test("hash_map_o0", source, OptLevel::None, 42);
    run_collections_test("hash_map_o1", source, OptLevel::O1, 42);
}

#[test]
fn test_import_ring_buffer_direct_and_operations() {
    let source = r#"
import std::collections::RingBuffer;
import std::collections::new_ring_buffer;
import std::collections::ring_buffer::push;
import std::collections::ring_buffer::pop;
import std::collections::ring_buffer::len;
import std::collections::ring_buffer::is_empty;

fn main() -> i64 {
    let mut rb: RingBuffer<i64> = new_ring_buffer(4);
    if !is_empty(&rb) {
        return 1;
    }
    if len(&rb) != 0 {
        return 2;
    }
    return 77;
}
"#;
    run_collections_test("ring_buffer_o0", source, OptLevel::None, 77);
    run_collections_test("ring_buffer_o1", source, OptLevel::O1, 77);
}

#[test]
fn test_nested_module_resolution() {
    let source = r#"
import std::collections::hash_map;
import std::collections::ring_buffer;

fn main() -> i64 {
    let m: hash_map::HashMap<i64, i64> = hash_map::new_hash_map(8);
    let r: ring_buffer::RingBuffer<i64> = ring_buffer::new_ring_buffer(8);
    return hash_map::len(&m) + ring_buffer::len(&r);
}
"#;
    run_collections_test("nested_res_o0", source, OptLevel::None, 0);
    run_collections_test("nested_res_o1", source, OptLevel::O1, 0);
}

#[test]
fn test_invalid_module_item_access_fails() {
    let source = r#"
import std::collections::non_existent_symbol;

fn main() -> i64 {
    return 0;
}
"#;
    check_compile_failure(source, "E0603");
}

#[test]
fn test_ring_buffer_circular_wrap_push_pop_and_clear() {
    let source = r#"
import std::collections::RingBuffer;
import std::collections::new_ring_buffer;
import std::collections::ring_buffer::push;
import std::collections::ring_buffer::pop;
import std::collections::ring_buffer::peek;
import std::collections::ring_buffer::len;
import std::collections::ring_buffer::is_empty;
import std::collections::ring_buffer::is_full;
import std::collections::ring_buffer::clear;
import std::option::Option;

fn main() -> i64 {
    let mut rb: RingBuffer<i64> = new_ring_buffer(3);
    if !is_empty(&rb) || is_full(&rb) {
        return 1;
    }
    push(&mut rb, 100);
    push(&mut rb, 200);
    push(&mut rb, 300);
    if !is_full(&rb) || len(&rb) != 3 {
        return 2;
    }
    // Trying to push to full buffer fails
    if push(&mut rb, 400) {
        return 3;
    }
    // Pop first item (100)
    let p1 = match pop(&mut rb) {
        Option::Some(v) => v,
        Option::None => 0,
    };
    if p1 != 100 {
        return 4;
    }
    // Now push 400 - this wraps into slot 0
    if !push(&mut rb, 400) {
        return 5;
    }
    // Peek next item: should be 200
    let pk = match peek(&rb) {
        Option::Some(v) => v,
        Option::None => 0,
    };
    if pk != 200 {
        return 6;
    }
    // Pop remaining 3 items: 200, 300, 400
    let p2 = match pop(&mut rb) { Option::Some(v) => v, Option::None => 0 };
    let p3 = match pop(&mut rb) { Option::Some(v) => v, Option::None => 0 };
    let p4 = match pop(&mut rb) { Option::Some(v) => v, Option::None => 0 };
    if p2 != 200 || p3 != 300 || p4 != 400 {
        return 7;
    }
    if !is_empty(&rb) {
        return 8;
    }
    // Test clear
    push(&mut rb, 555);
    clear(&mut rb);
    if !is_empty(&rb) {
        return 9;
    }
    return 100;
}
"#;
    run_collections_test("ring_buffer_wrap_o0", source, OptLevel::None, 100);
    run_collections_test("ring_buffer_wrap_o1", source, OptLevel::O1, 100);
}

#[test]
fn test_hash_map_crud_and_reinsertion() {
    let source = r#"
import std::collections::HashMap;
import std::collections::new_hash_map;
import std::collections::insert;
import std::collections::get;
import std::collections::contains_key;
import std::collections::remove;
import std::collections::hash_map::clear;
import std::collections::hash_map::len;
import std::collections::hash_map::is_empty;
import std::option::Option;

fn main() -> i64 {
    let mut map: HashMap<i64, i64> = new_hash_map(8);
    if !is_empty(&map) || len(&map) != 0 {
        return 1;
    }

    // Insert keys 1, 2, 3
    if !insert(&mut map, 1, 100) { return 2; }
    if !insert(&mut map, 2, 200) { return 3; }
    if !insert(&mut map, 3, 300) { return 4; }
    if len(&map) != 3 { return 5; }

    // Updating existing key returns false
    if insert(&mut map, 2, 250) { return 6; }
    if len(&map) != 3 { return 7; }

    // Check contains_key
    if !contains_key(&map, 1) || !contains_key(&map, 2) || !contains_key(&map, 3) {
        return 8;
    }
    if contains_key(&map, 999) {
        return 9;
    }

    // Check get updated value
    let v2 = match get(&map, 2) {
        Option::Some(v) => v,
        Option::None => 0,
    };
    if v2 != 250 { return 10; }

    // Remove key 2
    let rem2 = match remove(&mut map, 2) {
        Option::Some(v) => v,
        Option::None => 0,
    };
    if rem2 != 250 || len(&map) != 2 { return 11; }
    if contains_key(&map, 2) { return 12; }

    // Remove non-existing key returns None
    let rem_none = match remove(&mut map, 999) {
        Option::Some(v) => v,
        Option::None => -1,
    };
    if rem_none != -1 { return 13; }

    // Re-inserting key 2 should reuse the deleted slot
    if !insert(&mut map, 2, 2222) { return 14; }
    if len(&map) != 3 { return 15; }
    let v2_new = match get(&map, 2) {
        Option::Some(v) => v,
        Option::None => 0,
    };
    if v2_new != 2222 { return 16; }

    // Clear map
    clear(&mut map);
    if !is_empty(&map) || len(&map) != 0 {
        return 17;
    }
    if contains_key(&map, 1) {
        return 18;
    }

    return 42;
}
"#;
    run_collections_test("hash_map_crud_o0", source, OptLevel::None, 42);
    run_collections_test("hash_map_crud_o1", source, OptLevel::O1, 42);
}
