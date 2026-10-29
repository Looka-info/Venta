//! Loop Auto-Vectorization & Strip-Mining Tests (Milestone 26.3).
//!
//! Tests conservative loop auto-vectorization over contiguous array operations,
//! strip-mining into 256-bit YMM SIMD operations, and correct scalar remainder handling.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use vanta_codegen_x86_64::target::TargetFeatures;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, OptLevel};

fn run_vec_test(
    test_name: &str,
    source: &str,
    target_features: TargetFeatures,
    enable_vectorize: bool,
    expected_exit_code: i32,
) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("venta_vec_{}", test_name));
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("main.va");
    let exe_path = temp_dir.join("test_bin");
    fs::write(&src_path, source).expect("Failed to write test source");

    let mut session = CompilerSession::new();
    let mut options = CompilerOptions::new(src_path, exe_path.clone());
    options.opt_level = OptLevel::O1;
    options.emit_target = EmitTarget::Exe;
    options.std_path = Some(PathBuf::from("library"));
    options.target_features = target_features;
    options.enable_vectorize = enable_vectorize;

    let compile_res = session.compile(&options);
    if let Err(e) = &compile_res {
        eprintln!(
            "Compiler error: {}",
            session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
        );
        panic!("Compilation failed for {}: {}", test_name, e);
    }

    let output = Command::new(&exe_path)
        .output()
        .expect("Failed to execute compiled binary");

    assert_eq!(
        output.status.code(),
        Some(expected_exit_code),
        "Test {} exited with code {:?}, expected {}",
        test_name,
        output.status.code(),
        expected_exit_code
    );

    exe_path
}

fn extract_function_disasm(full_asm: &str, fn_name: &str) -> String {
    let target = format!("<{}>:", fn_name);
    let mut collecting = false;
    let mut result = String::new();
    for line in full_asm.lines() {
        if line.contains(&target) {
            collecting = true;
        } else if collecting && line.starts_with("00000000") && line.contains(">:") {
            break;
        }
        if collecting {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

// 1. Exact multiple of 4: Length 16
#[test]
fn test_auto_vectorize_execution_exact_16() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 16], b: &[i64; 16], out: &mut [i64; 16]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let b: [i64; 16] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];
    let mut out: [i64; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(16, &a, &b, &mut out);
    
    if out[0] != 11 { return 1; }
    if out[1] != 22 { return 2; }
    if out[2] != 33 { return 3; }
    if out[3] != 44 { return 4; }
    if out[7] != 88 { return 8; }
    if out[15] != 176 { return 16; }
    0
}
"#;
    run_vec_test("exact_16", source, TargetFeatures::avx2(), true, 0);
}

// 2. Exact single vector iteration: Length 4
#[test]
fn test_auto_vectorize_execution_exact_4() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 8], b: &[i64; 8], out: &mut [i64; 8]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 8] = [10, 20, 30, 40, 50, 60, 70, 80];
    let b: [i64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let mut out: [i64; 8] = [-1, -1, -1, -1, -1, -1, -1, -1];
    vec_add(4, &a, &b, &mut out);
    
    // First 4 elements must be updated by vector loop
    if out[0] != 11 { return 1; }
    if out[1] != 22 { return 2; }
    if out[2] != 33 { return 3; }
    if out[3] != 44 { return 4; }
    // Remaining elements must be untouched
    if out[4] != -1 { return 5; }
    if out[7] != -1 { return 8; }
    0
}
"#;
    run_vec_test("exact_4", source, TargetFeatures::avx2(), true, 0);
}

// 3. Remainder 1: Length 5 (1 vector iteration of 4 + 1 scalar remainder)
#[test]
fn test_auto_vectorize_execution_remainder_1() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 8], b: &[i64; 8], out: &mut [i64; 8]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 8] = [100, 200, 300, 400, 500, 600, 700, 800];
    let b: [i64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let mut out: [i64; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(5, &a, &b, &mut out);
    
    if out[0] != 101 { return 1; }
    if out[1] != 202 { return 2; }
    if out[2] != 303 { return 3; }
    if out[3] != 404 { return 4; }
    // Remainder element processed by scalar tail
    if out[4] != 505 { return 5; }
    // Elements beyond 5 must be untouched
    if out[5] != 0 { return 6; }
    0
}
"#;
    run_vec_test("remainder_1", source, TargetFeatures::avx2(), true, 0);
}

// 4. Remainder 2: Length 6 (1 vector iteration + 2 scalar remainder)
#[test]
fn test_auto_vectorize_execution_remainder_2() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 8], b: &[i64; 8], out: &mut [i64; 8]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let b: [i64; 8] = [10, 20, 30, 40, 50, 60, 70, 80];
    let mut out: [i64; 8] = [99, 99, 99, 99, 99, 99, 99, 99];
    vec_add(6, &a, &b, &mut out);
    
    if out[0] != 11 { return 1; }
    if out[3] != 44 { return 4; }
    if out[4] != 55 { return 5; }
    if out[5] != 66 { return 6; }
    if out[6] != 99 { return 7; }
    if out[7] != 99 { return 8; }
    0
}
"#;
    run_vec_test("remainder_2", source, TargetFeatures::avx2(), true, 0);
}

// 5. Remainder 3: Length 7 (1 vector iteration + 3 scalar remainder)
#[test]
fn test_auto_vectorize_execution_remainder_3() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 8], b: &[i64; 8], out: &mut [i64; 8]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 8] = [1, 1, 1, 1, 1, 1, 1, 1];
    let b: [i64; 8] = [2, 2, 2, 2, 2, 2, 2, 2];
    let mut out: [i64; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(7, &a, &b, &mut out);
    
    if out[0] != 3 { return 1; }
    if out[1] != 3 { return 2; }
    if out[2] != 3 { return 3; }
    if out[3] != 3 { return 4; }
    if out[4] != 3 { return 5; }
    if out[5] != 3 { return 6; }
    if out[6] != 3 { return 7; }
    if out[7] != 0 { return 8; }
    0
}
"#;
    run_vec_test("remainder_3", source, TargetFeatures::avx2(), true, 0);
}

// 6. Zero length: Length 0 (vector loop and scalar loop skipped, no accesses)
#[test]
fn test_auto_vectorize_execution_short_0() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 4], b: &[i64; 4], out: &mut [i64; 4]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 4] = [1, 2, 3, 4];
    let b: [i64; 4] = [10, 20, 30, 40];
    let mut out: [i64; 4] = [77, 77, 77, 77];
    vec_add(0, &a, &b, &mut out);
    
    if out[0] != 77 { return 1; }
    if out[1] != 77 { return 2; }
    if out[2] != 77 { return 3; }
    if out[3] != 77 { return 4; }
    0
}
"#;
    run_vec_test("short_0", source, TargetFeatures::avx2(), true, 0);
}

// 7. Short length 1: Handled purely by scalar remainder loop
#[test]
fn test_auto_vectorize_execution_short_1() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 4], b: &[i64; 4], out: &mut [i64; 4]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 4] = [5, 6, 7, 8];
    let b: [i64; 4] = [50, 60, 70, 80];
    let mut out: [i64; 4] = [0, 0, 0, 0];
    vec_add(1, &a, &b, &mut out);
    
    if out[0] != 55 { return 1; }
    if out[1] != 0 { return 2; }
    0
}
"#;
    run_vec_test("short_1", source, TargetFeatures::avx2(), true, 0);
}

// 8. Short length 3: Handled purely by scalar remainder loop
#[test]
fn test_auto_vectorize_execution_short_3() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 4], b: &[i64; 4], out: &mut [i64; 4]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 4] = [1, 2, 3, 4];
    let b: [i64; 4] = [10, 20, 30, 40];
    let mut out: [i64; 4] = [0, 0, 0, 0];
    vec_add(3, &a, &b, &mut out);
    
    if out[0] != 11 { return 1; }
    if out[1] != 22 { return 2; }
    if out[2] != 33 { return 3; }
    if out[3] != 0 { return 4; }
    0
}
"#;
    run_vec_test("short_3", source, TargetFeatures::avx2(), true, 0);
}

// 9. In-place vector addition: out[i] == a[i]
#[test]
fn test_auto_vectorize_in_place_addition() {
    let source = r#"
fn vec_add_inplace(n: i64, a: &mut [i64; 8], b: &[i64; 8]) {
    let mut i = 0;
    while i < n {
        a[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let mut a: [i64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let b: [i64; 8] = [10, 20, 30, 40, 50, 60, 70, 80];
    vec_add_inplace(8, &mut a, &b);
    
    if a[0] != 11 { return 1; }
    if a[3] != 44 { return 4; }
    if a[7] != 88 { return 8; }
    0
}
"#;
    run_vec_test("in_place", source, TargetFeatures::avx2(), true, 0);
}

// 10. Negative values and two's complement boundary values
#[test]
fn test_auto_vectorize_negative_and_boundary_values() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 8], b: &[i64; 8], out: &mut [i64; 8]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 8] = [-100, -200, -300, -400, 500, -600, 700, -800];
    let b: [i64; 8] = [100, 50, -50, 400, -500, 1000, -700, 800];
    let mut out: [i64; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(8, &a, &b, &mut out);
    
    if out[0] != 0 { return 1; }
    if out[1] != -150 { return 2; }
    if out[2] != -350 { return 3; }
    if out[3] != 0 { return 4; }
    if out[4] != 0 { return 5; }
    if out[5] != 400 { return 6; }
    if out[6] != 0 { return 7; }
    if out[7] != 0 { return 8; }
    0
}
"#;
    run_vec_test("negative_values", source, TargetFeatures::avx2(), true, 0);
}

// 11. Disable vectorization flag (--no-vectorize): executes through pure scalar path
#[test]
fn test_auto_vectorize_disabled_with_flag() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 8], b: &[i64; 8], out: &mut [i64; 8]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let b: [i64; 8] = [10, 20, 30, 40, 50, 60, 70, 80];
    let mut out: [i64; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(8, &a, &b, &mut out);
    
    if out[0] != 11 || out[7] != 88 { return 1; }
    0
}
"#;
    let exe = run_vec_test("disabled_flag", source, TargetFeatures::avx2(), false, 0);

    // Verify machine code of vec_add does NOT contain vpaddq or vmovups when vectorization is disabled
    let objdump_output = Command::new("objdump")
        .args(["-d", exe.to_str().unwrap()])
        .output()
        .expect("Failed to run objdump");
    let asm = String::from_utf8_lossy(&objdump_output.stdout);
    let vec_add_asm = extract_function_disasm(&asm, "vec_add");
    assert!(
        !vec_add_asm.is_empty(),
        "Could not find <vec_add> disassembly in binary"
    );
    assert!(
        !vec_add_asm.contains("vpaddq"),
        "vec_add compiled with vectorization disabled must not contain vpaddq"
    );
    assert!(
        !vec_add_asm.contains("vmovups"),
        "vec_add compiled with vectorization disabled must not contain vmovups"
    );
}

// 12. Baseline CPU target: executes through pure scalar path without requiring AVX2
#[test]
fn test_auto_vectorize_baseline_target() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 8], b: &[i64; 8], out: &mut [i64; 8]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 8] = [2, 4, 6, 8, 10, 12, 14, 16];
    let b: [i64; 8] = [1, 1, 1, 1, 1, 1, 1, 1];
    let mut out: [i64; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(8, &a, &b, &mut out);
    
    if out[0] != 3 || out[7] != 17 { return 1; }
    0
}
"#;
    let exe = run_vec_test(
        "baseline_target",
        source,
        TargetFeatures::baseline(),
        true,
        0,
    );

    // Baseline build of vec_add must NOT emit AVX2 instructions
    let objdump_output = Command::new("objdump")
        .args(["-d", exe.to_str().unwrap()])
        .output()
        .expect("Failed to run objdump");
    let asm = String::from_utf8_lossy(&objdump_output.stdout);
    let vec_add_asm = extract_function_disasm(&asm, "vec_add");
    assert!(
        !vec_add_asm.is_empty(),
        "Could not find <vec_add> disassembly in binary"
    );
    assert!(
        !vec_add_asm.contains("vpaddq"),
        "Baseline vec_add build must not emit vpaddq instruction"
    );
    assert!(
        !vec_add_asm.contains("vmovups"),
        "Baseline vec_add build must not emit vmovups instruction"
    );
}

// 13. Disassembly proof: machine code inspection proving vmovups and vpaddq emission
#[test]
fn test_auto_vectorize_machine_code_inspection() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 16], b: &[i64; 16], out: &mut [i64; 16]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let b: [i64; 16] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];
    let mut out: [i64; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(16, &a, &b, &mut out);
    0
}
"#;
    let exe = run_vec_test("disasm_proof", source, TargetFeatures::avx2(), true, 0);

    let objdump_output = Command::new("objdump")
        .args(["-d", exe.to_str().unwrap()])
        .output()
        .expect("Failed to run objdump");
    let asm = String::from_utf8_lossy(&objdump_output.stdout);
    let vec_add_asm = extract_function_disasm(&asm, "vec_add");

    // Verify presence of 256-bit SIMD instructions in vec_add
    assert!(
        vec_add_asm.contains("vmovups"),
        "Disassembly of vec_add must contain vmovups unaligned packed vector load/store"
    );
    assert!(
        vec_add_asm.contains("vpaddq"),
        "Disassembly of vec_add must contain vpaddq packed 64-bit integer addition"
    );
}
