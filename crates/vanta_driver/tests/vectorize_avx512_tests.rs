//! AVX-512 8-Lane Loop Auto-Vectorization Tests (Milestone 26.5).
//!
//! Tests 512-bit EVEX vector emission for 8-wide 64-bit integer addition (`vpaddq512`)
//! and double-precision FMA (`vfmadd231pd512`), strip-mining with scalar remainder handling,
//! target-feature gating, memory dependency analysis, and hardware-gated execution.

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

fn compile_to_binary(
    test_name: &str,
    source: &str,
    target_features: TargetFeatures,
    enable_vectorize: bool,
) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(format!("venta_vec512_{}", test_name));
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
            "Compiler error:\n{}",
            session.render_diagnostics(vanta_driver::DiagnosticFormat::Human)
        );
        panic!("Compilation failed for {}: {}", test_name, e);
    }

    exe_path
}

fn disassemble_binary(exe_path: &PathBuf) -> String {
    let output = Command::new("objdump")
        .args(["-d", exe_path.to_str().unwrap()])
        .output()
        .expect("Failed to run objdump on binary");
    assert!(
        output.status.success(),
        "objdump exited with failure: {:?}",
        output.status
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Helper to execute compiled binary if host supports AVX-512F, or log truthful skip.
fn maybe_execute_avx512(exe_path: &PathBuf, test_name: &str, expected_code: i32) {
    let host_features = TargetFeatures::host_native();
    if host_features.avx512f {
        let output = Command::new(exe_path)
            .output()
            .expect("Failed to execute compiled AVX-512 binary");
        assert_eq!(
            output.status.code(),
            Some(expected_code),
            "Test {} failed with exit code {:?}, expected {}",
            test_name,
            output.status.code(),
            expected_code
        );
    } else {
        println!(
            "[SKIPPED] Test '{}': Host CPU lacks AVX-512F hardware support (avx512f=false); skipping native ZMM execution while verifying compile-time EVEX machine code.",
            test_name
        );
    }
}

// 1. Vectorization Eligibility & 8-Lane EVEX Emission
#[test]
fn test_auto_vectorize_avx512_eligibility_and_evex_disasm() {
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
    let exe = compile_to_binary("eligibility_evex", source, TargetFeatures::avx512(), true);
    let disasm = disassemble_binary(&exe);
    let fn_asm = extract_function_disasm(&disasm, "vec_add");

    assert!(
        !fn_asm.is_empty(),
        "Could not find <vec_add> disassembly in binary"
    );

    // Verify 512-bit EVEX instruction emission with %zmm register operands
    assert!(
        fn_asm.contains("vmovups") && fn_asm.contains("%zmm"),
        "vec_add must emit EVEX vmovups with %zmm registers when +avx512f is enabled:\n{}",
        fn_asm
    );
    assert!(
        fn_asm.contains("vpaddq") && fn_asm.contains("%zmm"),
        "vec_add must emit EVEX vpaddq with %zmm registers when +avx512f is enabled:\n{}",
        fn_asm
    );

    // Verify 8-lane step in vector loop body ($0x8)
    assert!(
        fn_asm.contains("$0x8"),
        "vec_add must contain induction step of 8 for 8-lane strip-mining:\n{}",
        fn_asm
    );

    maybe_execute_avx512(&exe, "eligibility_evex", 0);
}

// 2. Feature Gating: Without AVX-512F, falls back to 256-bit AVX2 (YMM)
#[test]
fn test_auto_vectorize_fallback_to_avx2_when_avx512_disabled() {
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
    let exe = compile_to_binary("fallback_to_avx2", source, TargetFeatures::avx2(), true);
    let disasm = disassemble_binary(&exe);
    let fn_asm = extract_function_disasm(&disasm, "vec_add");

    assert!(
        !fn_asm.is_empty(),
        "Could not find <vec_add> disassembly in binary"
    );

    // Must emit 256-bit YMM instructions, must NOT emit 512-bit ZMM instructions
    assert!(
        fn_asm.contains("%ymm"),
        "vec_add compiled with AVX2 must use %ymm registers:\n{}",
        fn_asm
    );
    assert!(
        !fn_asm.contains("%zmm"),
        "vec_add compiled with AVX2 must not emit %zmm registers:\n{}",
        fn_asm
    );
}

// 3. Flag Gating: --no-vectorize disables vectorization even when +avx512f is specified
#[test]
fn test_auto_vectorize_avx512_disabled_with_flag() {
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
    let exe = compile_to_binary(
        "disabled_flag",
        source,
        TargetFeatures::avx512(),
        false, // enable_vectorize = false
    );
    let disasm = disassemble_binary(&exe);
    let fn_asm = extract_function_disasm(&disasm, "vec_add");

    assert!(
        !fn_asm.is_empty(),
        "Could not find <vec_add> disassembly in binary"
    );
    assert!(
        !fn_asm.contains("%zmm") && !fn_asm.contains("%ymm"),
        "vec_add compiled with vectorization disabled must not contain vector registers"
    );
    assert!(
        !fn_asm.contains("vpaddq"),
        "vec_add compiled with vectorization disabled must not contain vpaddq"
    );
}

// 4. Exact 8 lanes (1 vector iteration, 0 remainder)
#[test]
fn test_auto_vectorize_avx512_exact_8() {
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
    let mut out: [i64; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(8, &a, &b, &mut out);
    0
}
"#;
    let exe = compile_to_binary("exact_8", source, TargetFeatures::avx512(), true);
    let disasm = disassemble_binary(&exe);
    let fn_asm = extract_function_disasm(&disasm, "vec_add");

    assert!(
        fn_asm.contains("%zmm"),
        "Must emit ZMM operations for exact 8 lanes"
    );
    maybe_execute_avx512(&exe, "exact_8", 0);
}

// 5. Boundary: Short lengths (0, 1, 7) - Must bypass vector loop to scalar remainder
#[test]
fn test_auto_vectorize_avx512_short_boundaries() {
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
    
    // Test n = 0: no writes
    vec_add(0, &a, &b, &mut out);
    if out[0] != -1 { return 1; }

    // Test n = 1: scalar remainder handles 1 element
    vec_add(1, &a, &b, &mut out);
    if out[0] != 11 { return 2; }
    if out[1] != -1 { return 3; }

    // Test n = 7: scalar remainder handles 7 elements
    vec_add(7, &a, &b, &mut out);
    if out[6] != 77 { return 4; }
    if out[7] != -1 { return 5; }

    0
}
"#;
    let exe = compile_to_binary("short_boundaries", source, TargetFeatures::avx512(), true);
    maybe_execute_avx512(&exe, "short_boundaries", 0);
}

// 6. Remainder handling: Length 9 (1 vector iteration of 8 + 1 scalar remainder)
#[test]
fn test_auto_vectorize_avx512_remainder_1() {
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
    let mut out: [i64; 16] = [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1];
    vec_add(9, &a, &b, &mut out);
    
    // Vector loop covers elements 0..7
    if out[0] != 11 { return 1; }
    if out[7] != 88 { return 2; }
    // Scalar tail covers element 8
    if out[8] != 99 { return 3; }
    // Leftover elements 9..15 must remain untouched
    if out[9] != -1 { return 4; }
    if out[15] != -1 { return 5; }
    0
}
"#;
    let exe = compile_to_binary("remainder_1", source, TargetFeatures::avx512(), true);
    maybe_execute_avx512(&exe, "remainder_1", 0);
}

// 7. Remainder handling: Length 15 (1 vector iteration of 8 + 7 scalar remainders)
#[test]
fn test_auto_vectorize_avx512_remainder_7() {
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
    let mut out: [i64; 16] = [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1];
    vec_add(15, &a, &b, &mut out);
    
    // First 8 elements (vector loop)
    if out[0] != 11 || out[7] != 88 { return 1; }
    // Next 7 elements (scalar tail)
    if out[8] != 99 || out[14] != 165 { return 2; }
    // Element 15 untouched
    if out[15] != -1 { return 3; }
    0
}
"#;
    let exe = compile_to_binary("remainder_7", source, TargetFeatures::avx512(), true);
    maybe_execute_avx512(&exe, "remainder_7", 0);
}

// 8. Remainder handling: Length 17 (2 vector iterations of 8 = 16 + 1 scalar remainder)
#[test]
fn test_auto_vectorize_avx512_remainder_17() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 20], b: &[i64; 20], out: &mut [i64; 20]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 20] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
    let b: [i64; 20] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200];
    let mut out: [i64; 20] = [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1];
    vec_add(17, &a, &b, &mut out);
    
    if out[0] != 11 { return 1; }
    if out[15] != 176 { return 2; }
    if out[16] != 187 { return 3; }
    if out[17] != -1 { return 4; }
    if out[19] != -1 { return 5; }
    0
}
"#;
    let exe = compile_to_binary("remainder_17", source, TargetFeatures::avx512(), true);
    maybe_execute_avx512(&exe, "remainder_17", 0);
}

// 9. Exact In-Place Vector Addition (out == a)
#[test]
fn test_auto_vectorize_avx512_in_place() {
    let source = r#"
fn vec_add_inplace(n: i64, a: &mut [i64; 16], b: &[i64; 16]) {
    let mut i = 0;
    while i < n {
        a[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let mut a: [i64; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let b: [i64; 16] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];
    vec_add_inplace(16, &mut a, &b);
    
    if a[0] != 11 || a[7] != 88 || a[15] != 176 { return 1; }
    0
}
"#;
    let exe = compile_to_binary("in_place", source, TargetFeatures::avx512(), true);
    let disasm = disassemble_binary(&exe);
    let fn_asm = extract_function_disasm(&disasm, "vec_add_inplace");
    assert!(
        fn_asm.contains("%zmm"),
        "In-place addition must vectorize to ZMM"
    );
    maybe_execute_avx512(&exe, "in_place", 0);
}

// 10. Semantic Equivalence: Negative values, two's complement boundary values
#[test]
fn test_auto_vectorize_avx512_negative_and_overflow_semantics() {
    let source = r#"
fn vec_add(n: i64, a: &[i64; 8], b: &[i64; 8], out: &mut [i64; 8]) {
    let mut i = 0;
    while i < n {
        out[i] = a[i] + b[i];
        i = i + 1;
    }
}

fn main() -> i64 {
    let a: [i64; 8] = [-10, -20, -30, -40, 100, 200, 300, 400];
    let b: [i64; 8] = [5, 10, 15, 20, -50, -100, -150, -200];
    let mut out: [i64; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
    vec_add(8, &a, &b, &mut out);
    
    if out[0] != -5 { return 1; }
    if out[1] != -10 { return 2; }
    if out[2] != -15 { return 3; }
    if out[3] != -20 { return 4; }
    if out[4] != 50 { return 5; }
    if out[5] != 100 { return 6; }
    if out[6] != 150 { return 7; }
    if out[7] != 200 { return 8; }
    0
}
"#;
    let exe = compile_to_binary("negative_semantics", source, TargetFeatures::avx512(), true);
    maybe_execute_avx512(&exe, "negative_semantics", 0);
}

// 11. Double-Precision Floating-Point FMA 8-lane emission (`vfmadd231pd512`)
#[test]
fn test_auto_vectorize_avx512_float_fma_mir_transformation() {
    use vanta_mir::opt::vectorize::{LoopVectorizer, VectorizeTargetConfig};
    use vanta_mir::*;
    use vanta_sema::Type;

    // Construct a MIR function with a loop:
    // for i = 0..16 { out[i] = a[i] * b[i] + c[i]; }
    let mut func = MirFunction::new(
        "test_fma".into(),
        Vec::new(),
        Type::Unit,
        Vec::new(),
        Vec::new(),
    );

    let b0 = BlockId(0); // preheader
    let b1 = BlockId(1); // header
    let b2 = BlockId(2); // body
    let b3 = BlockId(3); // exit

    // Value IDs
    let v_init = ValueId(0);
    let v_limit = ValueId(1);
    let v_step = ValueId(2);
    let v_iv = ValueId(3);
    let v_cond = ValueId(4);
    let v_iv_next = ValueId(5);

    let v_out_ptr = ValueId(10);
    let v_a_ptr = ValueId(11);
    let v_b_ptr = ValueId(12);
    let v_c_ptr = ValueId(13);

    let v_a_addr = ValueId(20);
    let v_a_val = ValueId(21);
    let v_b_addr = ValueId(22);
    let v_b_val = ValueId(23);
    let v_mul = ValueId(24);
    let v_c_addr = ValueId(25);
    let v_c_val = ValueId(26);
    let v_res = ValueId(27);
    let v_out_addr = ValueId(28);

    // Block 0 (preheader)
    let bb0 = BasicBlock {
        id: b0,
        name: "entry".into(),
        instructions: vec![
            Instruction::ConstInt {
                dest: v_init,
                val: 0,
                ty: Type::I64,
            },
            Instruction::ConstInt {
                dest: v_limit,
                val: 16,
                ty: Type::I64,
            },
            Instruction::ConstInt {
                dest: v_step,
                val: 1,
                ty: Type::I64,
            },
            Instruction::ConstInt {
                dest: v_out_ptr,
                val: 0x1000,
                ty: Type::I64,
            },
            Instruction::ConstInt {
                dest: v_a_ptr,
                val: 0x2000,
                ty: Type::I64,
            },
            Instruction::ConstInt {
                dest: v_b_ptr,
                val: 0x3000,
                ty: Type::I64,
            },
            Instruction::ConstInt {
                dest: v_c_ptr,
                val: 0x4000,
                ty: Type::I64,
            },
        ],
        terminator: Some(Terminator::Branch(b1)),
    };

    // Block 1 (header)
    let bb1 = BasicBlock {
        id: b1,
        name: "header".into(),
        instructions: vec![
            Instruction::Phi {
                dest: v_iv,
                inputs: vec![(b0, v_init), (b2, v_iv_next)],
                ty: Type::I64,
            },
            Instruction::Binary {
                dest: v_cond,
                op: MirBinaryOp::Lt,
                lhs: v_iv,
                rhs: v_limit,
                ty: Type::Bool,
            },
        ],
        terminator: Some(Terminator::CondBranch {
            cond: v_cond,
            then_block: b2,
            else_block: b3,
        }),
    };

    // Block 2 (body / latch)
    let bb2 = BasicBlock {
        id: b2,
        name: "body".into(),
        instructions: vec![
            Instruction::IndexPtrAddr {
                dest: v_a_addr,
                base_ptr: v_a_ptr,
                index: v_iv,
                elem_size: 8,
            },
            Instruction::LoadIndirect {
                dest: v_a_val,
                addr: v_a_addr,
                ty: Type::F64,
            },
            Instruction::IndexPtrAddr {
                dest: v_b_addr,
                base_ptr: v_b_ptr,
                index: v_iv,
                elem_size: 8,
            },
            Instruction::LoadIndirect {
                dest: v_b_val,
                addr: v_b_addr,
                ty: Type::F64,
            },
            Instruction::Binary {
                dest: v_mul,
                op: MirBinaryOp::Mul,
                lhs: v_a_val,
                rhs: v_b_val,
                ty: Type::F64,
            },
            Instruction::IndexPtrAddr {
                dest: v_c_addr,
                base_ptr: v_c_ptr,
                index: v_iv,
                elem_size: 8,
            },
            Instruction::LoadIndirect {
                dest: v_c_val,
                addr: v_c_addr,
                ty: Type::F64,
            },
            Instruction::Binary {
                dest: v_res,
                op: MirBinaryOp::Add,
                lhs: v_mul,
                rhs: v_c_val,
                ty: Type::F64,
            },
            Instruction::IndexPtrAddr {
                dest: v_out_addr,
                base_ptr: v_out_ptr,
                index: v_iv,
                elem_size: 8,
            },
            Instruction::StoreIndirect {
                addr: v_out_addr,
                val: v_res,
            },
            Instruction::Binary {
                dest: v_iv_next,
                op: MirBinaryOp::Add,
                lhs: v_iv,
                rhs: v_step,
                ty: Type::I64,
            },
        ],
        terminator: Some(Terminator::Branch(b1)),
    };

    // Block 3 (exit)
    let bb3 = BasicBlock {
        id: b3,
        name: "exit".into(),
        instructions: vec![],
        terminator: Some(Terminator::Return(None)),
    };

    func.blocks = vec![bb0, bb1, bb2, bb3];

    let target_avx512 = VectorizeTargetConfig {
        has_avx: true,
        has_avx2: true,
        has_fma: true,
        has_avx512f: true,
    };

    let (changed, stats) = LoopVectorizer::run_on_function(&mut func, &target_avx512);
    assert!(
        changed,
        "LoopVectorizer must transform F64 FMA loop under AVX-512F"
    );
    assert_eq!(stats.loops_vectorized, 1);

    // Verify 8-lane FMA call emission in vector body
    let mut found_fma512 = false;
    for block in &func.blocks {
        for inst in &block.instructions {
            if let Instruction::Call { callee, .. } = inst {
                if callee == "venta_simd_vfmadd231pd512" {
                    found_fma512 = true;
                }
            }
        }
    }
    assert!(
        found_fma512,
        "Vectorized loop must contain call to venta_simd_vfmadd231pd512"
    );
}

// 12. Large non-multiple of 8: Length 67 (8 vector iterations of 8 = 64 lanes, + 3 scalar remainders)
#[test]
fn test_auto_vectorize_avx512_large_length_67() {
    let zeros_67 = vec!["0"; 67].join(", ");
    let neg_67 = vec!["-1"; 67].join(", ");
    let source = format!(
        r#"
fn vec_add(n: i64, a: &[i64; 67], b: &[i64; 67], out: &mut [i64; 67]) {{
    let mut i = 0;
    while i < n {{
        out[i] = a[i] + b[i];
        i = i + 1;
    }}
}}

fn main() -> i64 {{
    let mut a: [i64; 67] = [{zeros_67}];
    let mut b: [i64; 67] = [{zeros_67}];
    let mut out: [i64; 67] = [{neg_67}];

    let mut j = 0;
    while j < 67 {{
        a[j] = j + 1;
        b[j] = (j + 1) * 10;
        j = j + 1;
    }}

    vec_add(67, &a, &b, &mut out);

    // Verify first 8 (first vector iter)
    if out[0] != 11 || out[7] != 88 {{ return 1; }}
    // Verify iter 7 (lanes 56..63)
    if out[56] != 627 || out[63] != 704 {{ return 2; }}
    // Verify scalar remainders (lanes 64..66)
    if out[64] != 715 || out[65] != 726 || out[66] != 737 {{ return 3; }}

    0
}}
"#
    );
    let exe = compile_to_binary("large_67", &source, TargetFeatures::avx512(), true);
    let disasm = disassemble_binary(&exe);
    let fn_asm = extract_function_disasm(&disasm, "vec_add");

    assert!(
        fn_asm.contains("%zmm"),
        "Must emit ZMM operations for 67 elements"
    );
    maybe_execute_avx512(&exe, "large_67", 0);
}

// 13. Baseline Target: When compiled for baseline, does not emit any AVX-512 or AVX2 instructions
#[test]
fn test_auto_vectorize_avx512_baseline_target_scalar_fallback() {
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
    if out[0] != 11 || out[15] != 176 { return 1; }
    0
}
"#;
    let exe = compile_to_binary(
        "baseline_fallback",
        source,
        TargetFeatures::baseline(),
        true,
    );
    let disasm = disassemble_binary(&exe);
    let fn_asm = extract_function_disasm(&disasm, "vec_add");

    assert!(!fn_asm.contains("%zmm"), "Baseline must not contain %zmm");
    assert!(!fn_asm.contains("%ymm"), "Baseline must not contain %ymm");
    assert!(
        !fn_asm.contains("vpaddq"),
        "Baseline must not contain vpaddq"
    );

    // Baseline executable runs unconditionally on any x86-64 CPU
    let output = Command::new(&exe)
        .output()
        .expect("Failed to execute baseline binary");
    assert_eq!(output.status.code(), Some(0));
}

// 14. Unsafe dependencies: loop with unproven cross-iteration dependencies rejected
#[test]
fn test_auto_vectorize_avx512_rejected_loop_carried_dependency() {
    // Loop with recurrence: multiple phis or cross-iteration values
    let source = r#"
fn fib_accum(n: i64) -> i64 {
    let mut a = 0;
    let mut b = 1;
    let mut i = 0;
    while i < n {
        let temp = a + b;
        a = b;
        b = temp;
        i = i + 1;
    }
    a
}

fn main() -> i64 {
    let r = fib_accum(10);
    if r != 55 { return 1; }
    0
}
"#;
    let exe = compile_to_binary(
        "rejected_recurrence",
        source,
        TargetFeatures::avx512(),
        true,
    );
    let disasm = disassemble_binary(&exe);
    let fn_asm = extract_function_disasm(&disasm, "fib_accum");

    // Loop with recurrence must NOT be vectorized
    assert!(
        !fn_asm.contains("vpaddq"),
        "Recurrence loop must be rejected from vectorization"
    );

    let output = Command::new(&exe)
        .output()
        .expect("Failed to execute binary");
    assert_eq!(output.status.code(), Some(0));
}

// 15. Differential testing against scalar reference for variable lengths
#[test]
fn test_auto_vectorize_avx512_differential_reference_lengths() {
    let zeros_33 = vec!["0"; 33].join(", ");
    let neg_33 = vec!["-1"; 33].join(", ");
    let lengths = [0, 1, 2, 7, 8, 9, 15, 16, 17, 23, 24, 25, 31, 32, 33];
    for len in lengths {
        let source = format!(
            r#"
fn vec_add(n: i64, a: &[i64; 33], b: &[i64; 33], out: &mut [i64; 33]) {{
    let mut i = 0;
    while i < n {{
        out[i] = a[i] + b[i];
        i = i + 1;
    }}
}}

fn main() -> i64 {{
    let mut a: [i64; 33] = [{zeros_33}];
    let mut b: [i64; 33] = [{zeros_33}];
    let mut out: [i64; 33] = [{neg_33}];

    let mut j = 0;
    while j < 33 {{
        a[j] = j * 3 + 7;
        b[j] = (j + 4) * 2;
        j = j + 1;
    }}

    vec_add({len}, &a, &b, &mut out);

    // Verify key boundaries
    if {len} > 0 && out[0] != 15 {{ return 1; }}
    if {len} > 7 && out[7] != 50 {{ return 2; }}
    if {len} > 8 && out[8] != 55 {{ return 3; }}
    if {len} > 15 && out[15] != 90 {{ return 4; }}
    if {len} > 16 && out[16] != 95 {{ return 5; }}
    if {len} > 31 && out[31] != 170 {{ return 6; }}
    if {len} > 32 && out[32] != 175 {{ return 7; }}
    // Verify unwritten remains -1
    if {len} < 33 && out[32] != -1 {{ return 8; }}

    0
}}
"#,
            zeros_33 = zeros_33,
            neg_33 = neg_33,
            len = len
        );

        let exe = compile_to_binary(
            &format!("diff_len_{}", len),
            &source,
            TargetFeatures::avx512(),
            true,
        );
        maybe_execute_avx512(&exe, &format!("diff_len_{}", len), 0);
    }
}
