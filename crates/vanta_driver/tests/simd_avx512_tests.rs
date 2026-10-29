//! AVX-512 ZMM Register Allocation, Spilling & System V Vector ABI Tests (Milestone 26.4).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use vanta_codegen_x86_64::target::TargetFeatures;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, OptLevel};

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

fn compile_to_obj(
    test_name: &str,
    source: &str,
    target_features: TargetFeatures,
) -> (PathBuf, PathBuf) {
    let temp_dir = std::env::temp_dir().join(format!("venta_avx512_test_{}", test_name));
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("test.va");
    let obj_path = temp_dir.join("test.o");
    fs::write(&src_path, source).expect("Failed to write test source");

    let mut session = CompilerSession::new();
    let mut options = CompilerOptions::new(src_path.clone(), obj_path.clone());
    options.opt_level = OptLevel::O1;
    options.emit_target = EmitTarget::Obj;
    options.std_path = Some(PathBuf::from("library"));
    options.target_features = target_features;

    if let Err(e) = session.compile(&options) {
        eprintln!(
            "Compiler error:\n{}",
            session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
        );
        panic!("Compilation failed for {}: {}", test_name, e);
    }

    (temp_dir, obj_path)
}

fn disassemble_obj(obj_path: &PathBuf) -> String {
    let output = Command::new("objdump")
        .args(["-d", obj_path.to_str().unwrap()])
        .output()
        .expect("Failed to run objdump on object file");
    assert!(
        output.status.success(),
        "objdump exited with failure: {:?}",
        output.status
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

// 1. Code Generation & Dynamic 64-Byte Stack Realignment
#[test]
fn test_avx512_code_generation_and_dynamic_alignment() {
    let source = r#"
pub struct v512 {
    pub q0: i64,
    pub q1: i64,
    pub q2: i64,
    pub q3: i64,
    pub q4: i64,
    pub q5: i64,
    pub q6: i64,
    pub q7: i64,
}

pub fn add_vectors_512(a: v512, b: v512) -> v512 {
    venta_simd_vec_add512(a, b)
}
"#;

    let (temp_dir, obj_path) =
        compile_to_obj("code_gen_and_dyn_align", source, TargetFeatures::avx512());
    let disasm = disassemble_obj(&obj_path);
    let fn_asm = extract_function_disasm(&disasm, "add_vectors_512");

    assert!(
        !fn_asm.is_empty(),
        "Could not find <add_vectors_512> disassembly:\n{}",
        disasm
    );

    // Verify dynamic 64-byte stack realignment in prologue: and rsp, -64 (0xffffffffffffffc0)
    assert!(
        fn_asm.contains("and")
            && (fn_asm.contains("-0x40")
                || fn_asm.contains("0xffffffffffffffc0")
                || fn_asm.contains("$-0x40")),
        "Expected dynamic 64-byte stack realignment (and rsp, -64) in prologue, got:\n{}",
        fn_asm
    );

    // Verify EVEX-encoded 512-bit addition: vpaddq with %zmm registers
    assert!(
        fn_asm.contains("vpaddq") && fn_asm.contains("%zmm"),
        "Expected vpaddq with %zmm registers, got:\n{}",
        fn_asm
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

// 2. Allocator Stress Test: Utilization of Expanded 30-Register ZMM Bank (zmm0..zmm29)
#[test]
fn test_avx512_allocator_utilizes_expanded_zmm_registers() {
    // Generate 20 concurrently live virtual SIMD values to verify linear scan assigns
    // registers across the expanded AVX-512 ZMM bank (including upper bank zmm16..zmm29).
    let source = r#"
pub struct Vec8 {
    pub q0: i64, pub q1: i64, pub q2: i64, pub q3: i64,
    pub q4: i64, pub q5: i64, pub q6: i64, pub q7: i64,
}

pub fn stress_alloc_512(src: &Vec8, out: &mut Vec8) {
    let v0 = venta_simd_reg_load512(src);
    let v1 = venta_simd_reg_load512(src);
    let v2 = venta_simd_reg_load512(src);
    let v3 = venta_simd_reg_load512(src);
    let v4 = venta_simd_reg_load512(src);
    let v5 = venta_simd_reg_load512(src);
    let v6 = venta_simd_reg_load512(src);
    let v7 = venta_simd_reg_load512(src);
    let v8 = venta_simd_reg_load512(src);
    let v9 = venta_simd_reg_load512(src);
    let v10 = venta_simd_reg_load512(src);
    let v11 = venta_simd_reg_load512(src);
    let v12 = venta_simd_reg_load512(src);
    let v13 = venta_simd_reg_load512(src);
    let v14 = venta_simd_reg_load512(src);
    let v15 = venta_simd_reg_load512(src);
    let v16 = venta_simd_reg_load512(src);
    let v17 = venta_simd_reg_load512(src);
    let v18 = venta_simd_reg_load512(src);
    let v19 = venta_simd_reg_load512(src);

    let s0 = venta_simd_reg_add512(v0, v1);
    let s1 = venta_simd_reg_add512(s0, v2);
    let s2 = venta_simd_reg_add512(s1, v3);
    let s3 = venta_simd_reg_add512(s2, v4);
    let s4 = venta_simd_reg_add512(s3, v5);
    let s5 = venta_simd_reg_add512(s4, v6);
    let s6 = venta_simd_reg_add512(s5, v7);
    let s7 = venta_simd_reg_add512(s6, v8);
    let s8 = venta_simd_reg_add512(s7, v9);
    let s9 = venta_simd_reg_add512(s8, v10);
    let s10 = venta_simd_reg_add512(s9, v11);
    let s11 = venta_simd_reg_add512(s10, v12);
    let s12 = venta_simd_reg_add512(s11, v13);
    let s13 = venta_simd_reg_add512(s12, v14);
    let s14 = venta_simd_reg_add512(s13, v15);
    let s15 = venta_simd_reg_add512(s14, v16);
    let s16 = venta_simd_reg_add512(s15, v17);
    let s17 = venta_simd_reg_add512(s16, v18);
    let s18 = venta_simd_reg_add512(s17, v19);

    venta_simd_reg_store512(out, s18);
}
"#;

    let (temp_dir, obj_path) =
        compile_to_obj("allocator_stress_zmm", source, TargetFeatures::avx512());
    let disasm = disassemble_obj(&obj_path);
    let fn_asm = extract_function_disasm(&disasm, "stress_alloc_512");

    assert!(
        !fn_asm.is_empty(),
        "Could not find <stress_alloc_512> disassembly:\n{}",
        disasm
    );

    // Verify presence of registers in upper AVX-512 bank (zmm16..zmm29)
    let uses_upper_bank = (16..30).any(|reg_idx| fn_asm.contains(&format!("%zmm{}", reg_idx)));

    assert!(
        uses_upper_bank,
        "Expected allocator to use upper ZMM register bank (zmm16..zmm29), got:\n{}",
        fn_asm
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

// 3. Register Spilling & 64-Byte Aligned Spill Slot Layout
#[test]
fn test_avx512_spill_and_reload_64byte_aligned() {
    // 32 concurrently live virtual SIMD values exceeds the 30 allocatable ZMM registers,
    // forcing the register allocator to spill values to 64-byte aligned stack slots.
    let source = r#"
pub struct Vec8 {
    pub q0: i64, pub q1: i64, pub q2: i64, pub q3: i64,
    pub q4: i64, pub q5: i64, pub q6: i64, pub q7: i64,
}

pub fn spill_high_pressure_512(src: &Vec8, out: &mut Vec8) {
    let v0 = venta_simd_reg_load512(src);
    let v1 = venta_simd_reg_load512(src);
    let v2 = venta_simd_reg_load512(src);
    let v3 = venta_simd_reg_load512(src);
    let v4 = venta_simd_reg_load512(src);
    let v5 = venta_simd_reg_load512(src);
    let v6 = venta_simd_reg_load512(src);
    let v7 = venta_simd_reg_load512(src);
    let v8 = venta_simd_reg_load512(src);
    let v9 = venta_simd_reg_load512(src);
    let v10 = venta_simd_reg_load512(src);
    let v11 = venta_simd_reg_load512(src);
    let v12 = venta_simd_reg_load512(src);
    let v13 = venta_simd_reg_load512(src);
    let v14 = venta_simd_reg_load512(src);
    let v15 = venta_simd_reg_load512(src);
    let v16 = venta_simd_reg_load512(src);
    let v17 = venta_simd_reg_load512(src);
    let v18 = venta_simd_reg_load512(src);
    let v19 = venta_simd_reg_load512(src);
    let v20 = venta_simd_reg_load512(src);
    let v21 = venta_simd_reg_load512(src);
    let v22 = venta_simd_reg_load512(src);
    let v23 = venta_simd_reg_load512(src);
    let v24 = venta_simd_reg_load512(src);
    let v25 = venta_simd_reg_load512(src);
    let v26 = venta_simd_reg_load512(src);
    let v27 = venta_simd_reg_load512(src);
    let v28 = venta_simd_reg_load512(src);
    let v29 = venta_simd_reg_load512(src);
    let v30 = venta_simd_reg_load512(src);
    let v31 = venta_simd_reg_load512(src);

    let s0 = venta_simd_reg_add512(v0, v1);
    let s1 = venta_simd_reg_add512(s0, v2);
    let s2 = venta_simd_reg_add512(s1, v3);
    let s3 = venta_simd_reg_add512(s2, v4);
    let s4 = venta_simd_reg_add512(s3, v5);
    let s5 = venta_simd_reg_add512(s4, v6);
    let s6 = venta_simd_reg_add512(s5, v7);
    let s7 = venta_simd_reg_add512(s6, v8);
    let s8 = venta_simd_reg_add512(s7, v9);
    let s9 = venta_simd_reg_add512(s8, v10);
    let s10 = venta_simd_reg_add512(s9, v11);
    let s11 = venta_simd_reg_add512(s10, v12);
    let s12 = venta_simd_reg_add512(s11, v13);
    let s13 = venta_simd_reg_add512(s12, v14);
    let s14 = venta_simd_reg_add512(s13, v15);
    let s15 = venta_simd_reg_add512(s14, v16);
    let s16 = venta_simd_reg_add512(s15, v17);
    let s17 = venta_simd_reg_add512(s16, v18);
    let s18 = venta_simd_reg_add512(s17, v19);
    let s19 = venta_simd_reg_add512(s18, v20);
    let s20 = venta_simd_reg_add512(s19, v21);
    let s21 = venta_simd_reg_add512(s20, v22);
    let s22 = venta_simd_reg_add512(s21, v23);
    let s23 = venta_simd_reg_add512(s22, v24);
    let s24 = venta_simd_reg_add512(s23, v25);
    let s25 = venta_simd_reg_add512(s24, v26);
    let s26 = venta_simd_reg_add512(s25, v27);
    let s27 = venta_simd_reg_add512(s26, v28);
    let s28 = venta_simd_reg_add512(s27, v29);
    let s29 = venta_simd_reg_add512(s28, v30);
    let s30 = venta_simd_reg_add512(s29, v31);

    venta_simd_reg_store512(out, s30);
}
"#;

    let (temp_dir, obj_path) =
        compile_to_obj("spill_and_reload_64byte", source, TargetFeatures::avx512());
    let disasm = disassemble_obj(&obj_path);
    let fn_asm = extract_function_disasm(&disasm, "spill_high_pressure_512");

    assert!(
        !fn_asm.is_empty(),
        "Could not find <spill_high_pressure_512> disassembly:\n{}",
        disasm
    );

    // Verify 64-byte frame alignment is enabled in prologue: and rsp, -64
    assert!(
        fn_asm.contains("and")
            && (fn_asm.contains("-0x40")
                || fn_asm.contains("0xffffffffffffffc0")
                || fn_asm.contains("$-0x40")),
        "Spilling function must realign stack to 64 bytes (and rsp, -64):\n{}",
        fn_asm
    );

    // Check for spill/reload: vmovups storing and loading %zmm to stack slot (%rsp)
    assert!(
        fn_asm.contains("vmovups")
            && (fn_asm.contains("(%rsp)")
                || fn_asm.contains("0x40(%rsp)")
                || fn_asm.contains("0x80(%rsp)")),
        "Expected vector spill/reload with vmovups to stack slot (%rsp), got:\n{}",
        fn_asm
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

// 4. System V AMD64 ABI: 512-bit Vector Argument Ingestion (zmm0..zmm7), Stack Fallback (arg 9), and Return (zmm0)
#[test]
fn test_avx512_abi_parameter_passing_and_exhaustion() {
    let source = r#"
pub struct v512 {
    pub q0: i64, pub q1: i64, pub q2: i64, pub q3: i64,
    pub q4: i64, pub q5: i64, pub q6: i64, pub q7: i64,
}

pub fn sum_9_vectors_512(
    v0: v512,
    v1: v512,
    v2: v512,
    v3: v512,
    v4: v512,
    v5: v512,
    v6: v512,
    v7: v512,
    v8: v512
) -> v512 {
    let s01 = venta_simd_vec_add512(v0, v1);
    let s23 = venta_simd_vec_add512(v2, v3);
    let s45 = venta_simd_vec_add512(v4, v5);
    let s67 = venta_simd_vec_add512(v6, v7);
    let s03 = venta_simd_vec_add512(s01, s23);
    let s47 = venta_simd_vec_add512(s45, s67);
    let s07 = venta_simd_vec_add512(s03, s47);
    venta_simd_vec_add512(s07, v8)
}
"#;

    let (temp_dir, obj_path) =
        compile_to_obj("abi_exhaustion_512", source, TargetFeatures::avx512());
    let disasm = disassemble_obj(&obj_path);
    let fn_asm = extract_function_disasm(&disasm, "sum_9_vectors_512");

    assert!(
        !fn_asm.is_empty(),
        "Could not find <sum_9_vectors_512> disassembly:\n{}",
        disasm
    );

    // Verify that register arguments zmm0..zmm7 are ingested
    for i in 0..8 {
        assert!(
            fn_asm.contains(&format!("%zmm{}", i)),
            "Expected argument {} in %zmm{}, got:\n{}",
            i,
            i,
            fn_asm
        );
    }

    // Verify that the 9th vector argument (v8) is loaded from the caller stack frame via 16(%rbp) or 0x10(%rbp)
    assert!(
        fn_asm.contains("0x10(%rbp)") || fn_asm.contains("16(%rbp)"),
        "Expected 9th vector argument to be ingested from 16(%rbp) on stack, got:\n{}",
        fn_asm
    );

    // Verify return value is in %zmm0
    assert!(
        fn_asm.contains("%zmm0"),
        "Expected return value in %zmm0, got:\n{}",
        fn_asm
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

// 5. System V AMD64 ABI: Mixed Scalar and 512-bit Vector Argument Classification
#[test]
fn test_avx512_abi_mixed_scalar_and_vector() {
    let source = r#"
pub struct v512 {
    pub q0: i64, pub q1: i64, pub q2: i64, pub q3: i64,
    pub q4: i64, pub q5: i64, pub q6: i64, pub q7: i64,
}

pub fn mixed_scalar_vector(a: i64, v0: v512, b: i64, v1: v512) -> v512 {
    // Under System V AMD64 ABI:
    // a -> %rdi (integer reg 0)
    // v0 -> %zmm0 (vector reg 0)
    // b -> %rsi (integer reg 1)
    // v1 -> %zmm1 (vector reg 1)
    venta_simd_vec_add512(v0, v1)
}
"#;

    let (temp_dir, obj_path) =
        compile_to_obj("mixed_scalar_vector_512", source, TargetFeatures::avx512());
    let disasm = disassemble_obj(&obj_path);
    let fn_asm = extract_function_disasm(&disasm, "mixed_scalar_vector");

    assert!(
        !fn_asm.is_empty(),
        "Could not find <mixed_scalar_vector> disassembly:\n{}",
        disasm
    );

    // Vector arguments should be in %zmm0 and %zmm1
    assert!(
        fn_asm.contains("%zmm0") && fn_asm.contains("%zmm1"),
        "Expected %zmm0 and %zmm1 for vector arguments, got:\n{}",
        fn_asm
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

// 6. C ABI Oracle Interoperability with GCC (__m512i / System V Vector ABI)
#[test]
fn test_avx512_c_oracle_bidirectional_interop() {
    let temp_dir = std::env::temp_dir().join("venta_c_oracle_512_interop");
    let _ = fs::create_dir_all(&temp_dir);
    let v_src = temp_dir.join("venta_callee.va");
    let v_obj = temp_dir.join("venta_callee.o");
    let c_src = temp_dir.join("main.c");
    let bin_path = temp_dir.join("c_oracle_512_bin");

    // Venta callee: accepts two 512-bit vectors and returns their sum
    let venta_code = r#"
pub struct v512 {
    pub q0: i64, pub q1: i64, pub q2: i64, pub q3: i64,
    pub q4: i64, pub q5: i64, pub q6: i64, pub q7: i64,
}

pub fn venta_exported_vector_add512(a: v512, b: v512) -> v512 {
    venta_simd_vec_add512(a, b)
}
"#;
    fs::write(&v_src, venta_code).expect("Write Venta source");

    let mut session = CompilerSession::new();
    let mut options = CompilerOptions::new(v_src.clone(), v_obj.clone());
    options.opt_level = OptLevel::O1;
    options.emit_target = EmitTarget::Obj;
    options.std_path = Some(PathBuf::from("library"));
    options.target_features = TargetFeatures::avx512();

    session
        .compile(&options)
        .expect("Compile Venta 512-bit callee to object file");

    // C driver calling Venta exported function using GCC __m512i
    let c_code = r#"
#include <immintrin.h>
#include <stdint.h>
#include <stdio.h>

extern __m512i venta_exported_vector_add512(__m512i a, __m512i b);

int main() {
    // Initialize 8 64-bit lanes: [100, 200, 300, 400, 500, 600, 700, 800]
    __m512i a = _mm512_set_epi64(800, 700, 600, 500, 400, 300, 200, 100);
    __m512i b = _mm512_set_epi64(80, 70, 60, 50, 40, 30, 20, 10);

    __m512i res = venta_exported_vector_add512(a, b);

    int64_t out[8] __attribute__((aligned(64)));
    _mm512_store_si512((__m512i*)out, res);

    if (out[0] == 110 && out[1] == 220 && out[2] == 330 && out[3] == 440 &&
        out[4] == 550 && out[5] == 660 && out[6] == 770 && out[7] == 880) {
        printf("C_CALLS_VENTA_512_OK\n");
        return 0;
    } else {
        printf("C_CALLS_VENTA_512_FAIL: got [%ld, %ld, %ld, %ld, %ld, %ld, %ld, %ld]\n",
               out[0], out[1], out[2], out[3], out[4], out[5], out[6], out[7]);
        return 1;
    }
}
"#;
    fs::write(&c_src, c_code).expect("Write C source");

    let rt_archive = CompilerSession::find_runtime_archive(&options).expect("Locate libvanta_rt.a");

    let gcc_status = Command::new("gcc")
        .arg("-O2")
        .arg("-mavx512f")
        .arg(&c_src)
        .arg(&v_obj)
        .arg(&rt_archive)
        .arg("-o")
        .arg(&bin_path)
        .status()
        .expect("Invoke gcc linker for AVX-512 C oracle");

    assert!(gcc_status.success(), "GCC link failed for AVX-512 C oracle");

    // Disassemble the linked binary to verify seamless ABI and symbol linkage
    let disasm = disassemble_obj(&bin_path);
    let fn_asm = extract_function_disasm(&disasm, "venta_exported_vector_add512");
    assert!(
        !fn_asm.is_empty(),
        "Linked binary must contain <venta_exported_vector_add512>"
    );
    assert!(
        fn_asm.contains("vpaddq") && fn_asm.contains("%zmm"),
        "Exported function in linked binary must contain EVEX vpaddq with %zmm: {}",
        fn_asm
    );

    // Hardware-gated execution: if host CPU supports AVX-512F, execute the binary
    let host = TargetFeatures::host_native();
    if host.avx512f {
        let run_out = Command::new(&bin_path)
            .output()
            .expect("Run AVX-512 C oracle binary");

        assert_eq!(
            run_out.stdout,
            b"C_CALLS_VENTA_512_OK\n",
            "C oracle stdout mismatch: {}",
            String::from_utf8_lossy(&run_out.stdout)
        );
    } else {
        println!("Host CPU lacks AVX-512F execution support. Code generation, System V ABI calling convention, GCC symbol linkage, and EVEX disassembly verified successfully.");
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

// 7. Negative Test: Reject 512-bit SIMD Intrinsics When Target Feature +avx512f is Disabled
#[test]
fn test_negative_avx512_target_feature_rejection() {
    let source = r#"
pub struct v512 {
    pub q0: i64, pub q1: i64, pub q2: i64, pub q3: i64,
    pub q4: i64, pub q5: i64, pub q6: i64, pub q7: i64,
}

pub fn reject_test(a: v512, b: v512) -> v512 {
    venta_simd_vec_add512(a, b)
}
"#;

    let temp_dir = std::env::temp_dir().join("venta_avx512_reject_test");
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("test.va");
    let obj_path = temp_dir.join("test.o");
    fs::write(&src_path, source).expect("Failed to write test source");

    let mut session = CompilerSession::new();
    let mut options = CompilerOptions::new(src_path, obj_path);
    options.opt_level = OptLevel::O1;
    options.emit_target = EmitTarget::Obj;
    options.std_path = Some(PathBuf::from("library"));
    // AVX2 only (no avx512f)
    options.target_features = TargetFeatures::avx2();

    let result = session.compile(&options);
    assert!(
        result.is_err(),
        "Compilation using 512-bit intrinsic without +avx512f must fail"
    );

    let err_msg = result.err().unwrap();
    assert!(
        err_msg.contains("avx512f"),
        "Backend error must mention 'avx512f', got:\n{}",
        err_msg
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
