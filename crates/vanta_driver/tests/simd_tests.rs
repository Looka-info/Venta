//! SIMD Instruction Emission & Execution Tests (Milestone 26).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use vanta_codegen_x86_64::target::TargetFeatures;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, OptLevel};

fn run_simd_test(
    test_name: &str,
    source: &str,
    target_features: TargetFeatures,
    expected_stdout: &[u8],
) {
    let temp_dir = std::env::temp_dir().join(format!(
        "venta_simd_test_{}_{:?}",
        test_name, target_features
    ));
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("main.va");
    let exe_path = temp_dir.join("simd_bin");
    fs::write(&src_path, source).expect("Failed to write test source");

    let mut session = CompilerSession::new();
    let mut options = CompilerOptions::new(src_path.clone(), exe_path.clone());
    options.opt_level = OptLevel::O1;
    options.emit_target = EmitTarget::Exe;
    options.std_path = Some(PathBuf::from("library"));
    options.target_features = target_features;

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
        .expect("Failed to execute compiled SIMD binary");

    assert_eq!(
        output.status.code(),
        Some(0),
        "Test {} exited with nonzero status: {:?}",
        test_name,
        output.status
    );

    assert_eq!(
        output.stdout,
        expected_stdout,
        "Test {} stdout mismatch!\nGot: {}\nExpected: {}",
        test_name,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(expected_stdout)
    );

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_simd_vmovups_execution() {
    let host = TargetFeatures::host_native();
    if !host.avx {
        println!("Skipping vmovups test: host CPU does not support AVX");
        return;
    }

    let source = r#"
struct Data256 {
    v0: i64,
    v1: i64,
    v2: i64,
    v3: i64,
}

fn main() {
    let src = Data256 {
        v0: 100,
        v1: 200,
        v2: 300,
        v3: 400,
    };
    let mut dest = Data256 {
        v0: 0,
        v1: 0,
        v2: 0,
        v3: 0,
    };

    venta_simd_vmovups_load256(&mut dest, &src);

    if dest.v0 == 100 && dest.v1 == 200 && dest.v2 == 300 && dest.v3 == 400 {
        print_str("VMOVUPS_OK\n");
    } else {
        print_str("VMOVUPS_FAIL\n");
    }
}
"#;

    run_simd_test(
        "vmovups_256",
        source,
        TargetFeatures::avx(),
        b"VMOVUPS_OK\n",
    );
}

#[test]
fn test_simd_vpaddq_execution() {
    let host = TargetFeatures::host_native();
    if !host.avx2 {
        println!("Skipping vpaddq test: host CPU does not support AVX2");
        return;
    }

    let source = r#"
struct Vec4I64 {
    v0: i64,
    v1: i64,
    v2: i64,
    v3: i64,
}

fn main() {
    let a = Vec4I64 {
        v0: 1000,
        v1: 2000,
        v2: 3000,
        v3: 4000,
    };
    let b = Vec4I64 {
        v0: 11,
        v1: 22,
        v2: 33,
        v3: 44,
    };
    let mut dest = Vec4I64 {
        v0: 0,
        v1: 0,
        v2: 0,
        v3: 0,
    };

    venta_simd_vpaddq256(&mut dest, &a, &b);

    if dest.v0 == 1011 && dest.v1 == 2022 && dest.v2 == 3033 && dest.v3 == 4044 {
        print_str("VPADDQ_OK\n");
    } else {
        print_str("VPADDQ_FAIL\n");
    }
}
"#;

    run_simd_test("vpaddq_256", source, TargetFeatures::avx2(), b"VPADDQ_OK\n");
}

#[test]
fn test_simd_vfmadd231pd_execution() {
    let host = TargetFeatures::host_native();
    if !host.avx || !host.fma {
        println!("Skipping vfmadd231pd test: host CPU does not support AVX+FMA");
        return;
    }

    let source = r#"
struct Vec4I64 {
    v0: i64,
    v1: i64,
    v2: i64,
    v3: i64,
}

fn main() {
    // 64-bit IEEE-754 representation:
    // 2.0 = 0x4000000000000000
    // 3.0 = 0x4008000000000000
    // 4.0 = 0x4010000000000000
    // 5.0 = 0x4014000000000000
    let a = Vec4I64 {
        v0: 4611686018427387904,
        v1: 4613937818241073152,
        v2: 4616189618054758400,
        v3: 4617315517961601024,
    };

    // 10.0 = 0x4024000000000000
    // 20.0 = 0x4034000000000000
    // 30.0 = 0x403E000000000000
    // 40.0 = 0x4044000000000000
    let b = Vec4I64 {
        v0: 4621818617589137408,
        v1: 4626322217216507904,
        v2: 4628574017030193152,
        v3: 4629699916937035776,
    };

    // 1.0 = 0x3FF0000000000000
    // 2.0 = 0x4000000000000000
    // 3.0 = 0x4008000000000000
    // 4.0 = 0x4010000000000000
    let c = Vec4I64 {
        v0: 4607182418800017408,
        v1: 4611686018427387904,
        v2: 4613937818241073152,
        v3: 4616189618054758400,
    };

    let mut dest = Vec4I64 {
        v0: 0,
        v1: 0,
        v2: 0,
        v3: 0,
    };

    // dest = c + (a * b)
    // Lane 0: 0.5 + (2.0 * 10.0) = 20.5 = 4626603692193218560
    // Lane 1: 2.0 + (3.0 * 20.0) = 62.0 = 4633921791587778560
    // Lane 2: 3.0 + (4.0 * 30.0) = 123.0 = 4637792322517467136
    // Lane 3: 4.0 + (5.0 * 40.0) = 204.0 = 4639973941086912512
    venta_simd_vfmadd231pd256(&mut dest, &a, &b, &c);

    let expected0: i64 = 4626603692193218560;
    let expected1: i64 = 4633921791587778560;
    let expected2: i64 = 4637792322517467136;
    let expected3: i64 = 4639973941086912512;

    if dest.v0 == expected0 && dest.v1 == expected1 && dest.v2 == expected2 && dest.v3 == expected3 {
        print_str("VFMADD231PD_OK\n");
    } else {
        print_str("VFMADD231PD_FAIL\n");
    }
}
"#;

    run_simd_test(
        "vfmadd231pd_256",
        source,
        TargetFeatures::avx2_fma(),
        b"VFMADD231PD_OK\n",
    );
}

#[test]
fn test_simd_avx512_execution_gated() {
    let host = TargetFeatures::host_native();
    if !host.avx512f {
        println!("Skipping AVX-512 execution test: host CPU does not support AVX-512F");
        return;
    }

    let source = r#"
struct Vec8I64 {
    v0: i64,
    v1: i64,
    v2: i64,
    v3: i64,
    v4: i64,
    v5: i64,
    v6: i64,
    v7: i64,
}

fn main() {
    let a = Vec8I64 {
        v0: 1, v1: 2, v2: 3, v3: 4,
        v4: 5, v5: 6, v6: 7, v7: 8,
    };
    let b = Vec8I64 {
        v0: 10, v1: 20, v2: 30, v3: 40,
        v4: 50, v5: 60, v6: 70, v7: 80,
    };
    let mut dest = Vec8I64 {
        v0: 0, v1: 0, v2: 0, v3: 0,
        v4: 0, v5: 0, v6: 0, v7: 0,
    };

    venta_simd_vpaddq512(&mut dest, &a, &b);

    if dest.v0 == 11 && dest.v7 == 88 {
        print_str("AVX512_OK\n");
    } else {
        print_str("AVX512_FAIL\n");
    }
}
"#;

    run_simd_test(
        "avx512_vpaddq",
        source,
        TargetFeatures::avx512(),
        b"AVX512_OK\n",
    );
}

#[test]
fn test_negative_target_feature_rejection() {
    let source_vpaddq = r#"
struct Vec4I64 {
    v0: i64, v1: i64, v2: i64, v3: i64,
}

fn main() {
    let a = Vec4I64 { v0: 1, v1: 2, v2: 3, v3: 4 };
    let b = Vec4I64 { v0: 5, v1: 6, v2: 7, v3: 8 };
    let mut dest = Vec4I64 { v0: 0, v1: 0, v2: 0, v3: 0 };
    venta_simd_vpaddq256(&mut dest, &a, &b);
}
"#;

    let temp_dir = std::env::temp_dir().join("venta_simd_reject_test");
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("main.va");
    let exe_path = temp_dir.join("simd_bin");
    fs::write(&src_path, source_vpaddq).expect("Failed to write test source");

    // Case 1: Baseline target (no AVX) rejects the initial vmovups load
    {
        let mut session = CompilerSession::new();
        let mut options = CompilerOptions::new(src_path.clone(), exe_path.clone());
        options.opt_level = OptLevel::O1;
        options.emit_target = EmitTarget::Exe;
        options.target_features = TargetFeatures::baseline();

        let compile_res = session.compile(&options);
        assert!(
            compile_res.is_err(),
            "Compiling SIMD for baseline target must fail!"
        );
        let err_msg = compile_res.unwrap_err();
        assert!(
            err_msg.contains("requires target feature '+avx'"),
            "Error message must specify missing target feature '+avx', got: {}",
            err_msg
        );
    }

    // Case 2: AVX target (has AVX, lacks AVX2) allows vmovups but rejects vpaddq
    {
        let mut session = CompilerSession::new();
        let mut options = CompilerOptions::new(src_path.clone(), exe_path.clone());
        options.opt_level = OptLevel::O1;
        options.emit_target = EmitTarget::Exe;
        options.target_features = TargetFeatures::avx();

        let compile_res = session.compile(&options);
        assert!(
            compile_res.is_err(),
            "Compiling VPADDQ for AVX (non-AVX2) target must fail!"
        );
        let err_msg = compile_res.unwrap_err();
        assert!(
            err_msg.contains("requires target feature '+avx2'"),
            "Error message must specify missing target feature '+avx2', got: {}",
            err_msg
        );
    }

    // Case 3: AVX2 target (lacks FMA) rejects vfmadd231pd
    let source_fma = r#"
struct Vec4I64 {
    v0: i64, v1: i64, v2: i64, v3: i64,
}

fn main() {
    let a = Vec4I64 { v0: 1, v1: 2, v2: 3, v3: 4 };
    let b = Vec4I64 { v0: 5, v1: 6, v2: 7, v3: 8 };
    let c = Vec4I64 { v0: 9, v1: 10, v2: 11, v3: 12 };
    let mut dest = Vec4I64 { v0: 0, v1: 0, v2: 0, v3: 0 };
    venta_simd_vfmadd231pd256(&mut dest, &a, &b, &c);
}
"#;
    fs::write(&src_path, source_fma).expect("Failed to write FMA test source");
    {
        let mut session = CompilerSession::new();
        let mut options = CompilerOptions::new(src_path.clone(), exe_path.clone());
        options.opt_level = OptLevel::O1;
        options.emit_target = EmitTarget::Exe;
        options.target_features = TargetFeatures::avx2(); // Has AVX2, but fma = false

        let compile_res = session.compile(&options);
        assert!(
            compile_res.is_err(),
            "Compiling VFMADD231PD for AVX2 (without FMA) target must fail!"
        );
        let err_msg = compile_res.unwrap_err();
        assert!(
            err_msg.contains("requires target feature '+fma'"),
            "Error message must specify missing target feature '+fma', got: {}",
            err_msg
        );
    }

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_simd_virtual_register_allocation() {
    let host = TargetFeatures::host_native();
    if !host.avx2 {
        println!("Skipping virtual register allocation test: host CPU lacks AVX2");
        return;
    }

    let source = r#"
struct Vec4I64 {
    v0: i64, v1: i64, v2: i64, v3: i64,
}

fn main() {
    let a = Vec4I64 { v0: 10, v1: 20, v2: 30, v3: 40 };
    let b = Vec4I64 { v0: 1, v1: 2, v2: 3, v3: 4 };
    let c = Vec4I64 { v0: 100, v1: 200, v2: 300, v3: 400 };
    let mut out = Vec4I64 { v0: 0, v1: 0, v2: 0, v3: 0 };

    let va = venta_simd_reg_load256(&a);
    let vb = venta_simd_reg_load256(&b);
    let vc = venta_simd_reg_load256(&c);

    let v_sum = venta_simd_reg_add256(va, vb);
    let v_res = venta_simd_reg_add256(v_sum, vc);

    venta_simd_reg_store256(&mut out, v_res);

    if out.v0 == 111 && out.v1 == 222 && out.v2 == 333 && out.v3 == 444 {
        print_str("VIRTUAL_REG_ALLOC_OK\n");
    } else {
        print_str("VIRTUAL_REG_ALLOC_FAIL\n");
    }
}
"#;

    run_simd_test(
        "simd_virtual_reg_alloc",
        source,
        TargetFeatures::avx2(),
        b"VIRTUAL_REG_ALLOC_OK\n",
    );
}

#[test]
fn test_simd_call_preservation_and_spilling() {
    let host = TargetFeatures::host_native();
    if !host.avx2 {
        println!("Skipping call preservation test: host CPU lacks AVX2");
        return;
    }

    let source = r#"
struct Vec4I64 {
    v0: i64, v1: i64, v2: i64, v3: i64,
}

fn clobber_registers() {
    // A regular function call whose execution may touch scratch vector registers
}

fn main() {
    let a = Vec4I64 { v0: 50, v1: 60, v2: 70, v3: 80 };
    let b = Vec4I64 { v0: 5, v1: 6, v2: 7, v3: 8 };
    let mut out = Vec4I64 { v0: 0, v1: 0, v2: 0, v3: 0 };

    let va = venta_simd_reg_load256(&a);
    let vb = venta_simd_reg_load256(&b);

    // Call across which va and vb are live
    clobber_registers();

    let v_res = venta_simd_reg_add256(va, vb);
    venta_simd_reg_store256(&mut out, v_res);

    if out.v0 == 55 && out.v1 == 66 && out.v2 == 77 && out.v3 == 88 {
        print_str("SIMD_CALL_PRESERVATION_OK\n");
    } else {
        print_str("SIMD_CALL_PRESERVATION_FAIL\n");
    }
}
"#;

    run_simd_test(
        "simd_call_preservation",
        source,
        TargetFeatures::avx2(),
        b"SIMD_CALL_PRESERVATION_OK\n",
    );
}

#[test]
fn test_simd_high_register_pressure_spilling() {
    let host = TargetFeatures::host_native();
    if !host.avx2 {
        println!("Skipping high register pressure test: host CPU lacks AVX2");
        return;
    }

    let source = r#"
struct Vec4I64 {
    v0: i64, v1: i64, v2: i64, v3: i64,
}

fn main() {
    let d0 = Vec4I64 { v0: 1, v1: 1, v2: 1, v3: 1 };
    let d1 = Vec4I64 { v0: 2, v1: 2, v2: 2, v3: 2 };
    let d2 = Vec4I64 { v0: 3, v1: 3, v2: 3, v3: 3 };
    let d3 = Vec4I64 { v0: 4, v1: 4, v2: 4, v3: 4 };
    let d4 = Vec4I64 { v0: 5, v1: 5, v2: 5, v3: 5 };
    let d5 = Vec4I64 { v0: 6, v1: 6, v2: 6, v3: 6 };
    let d6 = Vec4I64 { v0: 7, v1: 7, v2: 7, v3: 7 };
    let d7 = Vec4I64 { v0: 8, v1: 8, v2: 8, v3: 8 };
    let d8 = Vec4I64 { v0: 9, v1: 9, v2: 9, v3: 9 };
    let d9 = Vec4I64 { v0: 10, v1: 10, v2: 10, v3: 10 };
    let d10 = Vec4I64 { v0: 11, v1: 11, v2: 11, v3: 11 };
    let d11 = Vec4I64 { v0: 12, v1: 12, v2: 12, v3: 12 };
    let d12 = Vec4I64 { v0: 13, v1: 13, v2: 13, v3: 13 };
    let d13 = Vec4I64 { v0: 14, v1: 14, v2: 14, v3: 14 };
    let d14 = Vec4I64 { v0: 15, v1: 15, v2: 15, v3: 15 };
    let d15 = Vec4I64 { v0: 16, v1: 16, v2: 16, v3: 16 };
    let d16 = Vec4I64 { v0: 17, v1: 17, v2: 17, v3: 17 };
    let d17 = Vec4I64 { v0: 18, v1: 18, v2: 18, v3: 18 };

    // 18 simultaneous live virtual SIMD values exceeding the 14-register allocatable pool:
    let v0 = venta_simd_reg_load256(&d0);
    let v1 = venta_simd_reg_load256(&d1);
    let v2 = venta_simd_reg_load256(&d2);
    let v3 = venta_simd_reg_load256(&d3);
    let v4 = venta_simd_reg_load256(&d4);
    let v5 = venta_simd_reg_load256(&d5);
    let v6 = venta_simd_reg_load256(&d6);
    let v7 = venta_simd_reg_load256(&d7);
    let v8 = venta_simd_reg_load256(&d8);
    let v9 = venta_simd_reg_load256(&d9);
    let v10 = venta_simd_reg_load256(&d10);
    let v11 = venta_simd_reg_load256(&d11);
    let v12 = venta_simd_reg_load256(&d12);
    let v13 = venta_simd_reg_load256(&d13);
    let v14 = venta_simd_reg_load256(&d14);
    let v15 = venta_simd_reg_load256(&d15);
    let v16 = venta_simd_reg_load256(&d16);
    let v17 = venta_simd_reg_load256(&d17);

    // Sum all 18 values together:
    // sum(1..18) = 18 * 19 / 2 = 171 per lane
    let s1 = venta_simd_reg_add256(v0, v1);
    let s2 = venta_simd_reg_add256(s1, v2);
    let s3 = venta_simd_reg_add256(s2, v3);
    let s4 = venta_simd_reg_add256(s3, v4);
    let s5 = venta_simd_reg_add256(s4, v5);
    let s6 = venta_simd_reg_add256(s5, v6);
    let s7 = venta_simd_reg_add256(s6, v7);
    let s8 = venta_simd_reg_add256(s7, v8);
    let s9 = venta_simd_reg_add256(s8, v9);
    let s10 = venta_simd_reg_add256(s9, v10);
    let s11 = venta_simd_reg_add256(s10, v11);
    let s12 = venta_simd_reg_add256(s11, v12);
    let s13 = venta_simd_reg_add256(s12, v13);
    let s14 = venta_simd_reg_add256(s13, v14);
    let s15 = venta_simd_reg_add256(s14, v15);
    let s16 = venta_simd_reg_add256(s15, v16);
    let s17 = venta_simd_reg_add256(s16, v17);

    let mut out = Vec4I64 { v0: 0, v1: 0, v2: 0, v3: 0 };
    venta_simd_reg_store256(&mut out, s17);

    if out.v0 == 171 && out.v1 == 171 && out.v2 == 171 && out.v3 == 171 {
        print_str("HIGH_REGISTER_PRESSURE_OK\n");
    } else {
        print_str("HIGH_REGISTER_PRESSURE_FAIL\n");
    }
}
"#;

    run_simd_test(
        "simd_high_pressure",
        source,
        TargetFeatures::avx2(),
        b"HIGH_REGISTER_PRESSURE_OK\n",
    );
}

#[test]
fn test_simd_vector_abi_direct_passing_and_returning() {
    let host = TargetFeatures::host_native();
    if !host.avx2 {
        println!("Skipping vector ABI test: host CPU does not support AVX2");
        return;
    }

    let source = r#"
struct v256 {
    pub q0: i64,
    pub q1: i64,
    pub q2: i64,
    pub q3: i64,
}

fn add_vectors(a: v256, b: v256) -> v256 {
    venta_simd_vec_add256(a, b)
}

fn main() {
    let a = v256 { q0: 100, q1: 200, q2: 300, q3: 400 };
    let b = v256 { q0: 10, q1: 20, q2: 30, q3: 40 };
    let res = add_vectors(a, b);

    if res.q0 == 110 && res.q1 == 220 && res.q2 == 330 && res.q3 == 440 {
        print_str("DIRECT_VECTOR_ABI_OK\n");
    } else {
        print_str("DIRECT_VECTOR_ABI_FAIL\n");
    }
}
"#;

    run_simd_test(
        "simd_vec_abi_direct",
        source,
        TargetFeatures::avx2(),
        b"DIRECT_VECTOR_ABI_OK\n",
    );
}

#[test]
fn test_simd_vector_abi_multiple_args_fma() {
    let host = TargetFeatures::host_native();
    if !host.avx2 || !host.fma {
        println!("Skipping vector ABI FMA test: host CPU does not support AVX2 + FMA");
        return;
    }

    let source = r#"
struct v256 {
    pub q0: i64,
    pub q1: i64,
    pub q2: i64,
    pub q3: i64,
}

fn fma_vectors(acc: v256, a: v256, b: v256) -> v256 {
    venta_simd_vec_fma256(acc, a, b)
}

fn main() {
    // 1.0 + 2.0 * 3.0 = 7.0 in IEEE-754 double precision:
    // 1.0 = 4607182418800017408
    // 2.0 = 4611686018427387904
    // 3.0 = 4613937818241073152
    // 7.0 = 4619567317775286272
    let acc = v256 {
        q0: 4607182418800017408,
        q1: 4607182418800017408,
        q2: 4607182418800017408,
        q3: 4607182418800017408,
    };
    let a = v256 {
        q0: 4611686018427387904,
        q1: 4611686018427387904,
        q2: 4611686018427387904,
        q3: 4611686018427387904,
    };
    let b = v256 {
        q0: 4613937818241073152,
        q1: 4613937818241073152,
        q2: 4613937818241073152,
        q3: 4613937818241073152,
    };

    let res = fma_vectors(acc, a, b);

    if res.q0 == 4619567317775286272 && res.q1 == 4619567317775286272 && res.q2 == 4619567317775286272 && res.q3 == 4619567317775286272 {
        print_str("MULTIPLE_VECTOR_FMA_OK\n");
    } else {
        print_str("MULTIPLE_VECTOR_FMA_FAIL\n");
    }
}
"#;

    run_simd_test(
        "simd_vec_abi_fma",
        source,
        TargetFeatures::avx2_fma(),
        b"MULTIPLE_VECTOR_FMA_OK\n",
    );
}

#[test]
fn test_simd_vector_abi_mixed_int_and_vector() {
    let host = TargetFeatures::host_native();
    if !host.avx2 {
        println!("Skipping vector ABI mixed test: host CPU does not support AVX2");
        return;
    }

    let source = r#"
struct v256 {
    pub q0: i64,
    pub q1: i64,
    pub q2: i64,
    pub q3: i64,
}

fn mixed_calc(scale: i64, a: v256, offset: i64, b: v256) -> v256 {
    let sum = venta_simd_vec_add256(a, b);
    let mut res = sum;
    res.q0 = (res.q0 * scale) + offset;
    res.q1 = (res.q1 * scale) + offset;
    res.q2 = (res.q2 * scale) + offset;
    res.q3 = (res.q3 * scale) + offset;
    res
}

fn main() {
    let a = v256 { q0: 10, q1: 20, q2: 30, q3: 40 };
    let b = v256 { q0: 1, q1: 2, q2: 3, q3: 4 };
    let res = mixed_calc(2, a, 5, b);

    // sum = [11, 22, 33, 44]
    // res = sum * 2 + 5 = [27, 49, 71, 93]
    if res.q0 == 27 && res.q1 == 49 && res.q2 == 71 && res.q3 == 93 {
        print_str("MIXED_INT_VECTOR_ABI_OK\n");
    } else {
        print_str("MIXED_INT_VECTOR_ABI_FAIL\n");
    }
}
"#;

    run_simd_test(
        "simd_vec_abi_mixed",
        source,
        TargetFeatures::avx2(),
        b"MIXED_INT_VECTOR_ABI_OK\n",
    );
}

#[test]
fn test_simd_vector_abi_register_exhaustion_stack_fallback() {
    let host = TargetFeatures::host_native();
    if !host.avx2 {
        println!("Skipping vector ABI exhaustion test: host CPU does not support AVX2");
        return;
    }

    let source = r#"
struct v256 {
    pub q0: i64,
    pub q1: i64,
    pub q2: i64,
    pub q3: i64,
}

fn sum_9_vectors(
    v0: v256,
    v1: v256,
    v2: v256,
    v3: v256,
    v4: v256,
    v5: v256,
    v6: v256,
    v7: v256,
    v8: v256
) -> v256 {
    let s01 = venta_simd_vec_add256(v0, v1);
    let s23 = venta_simd_vec_add256(v2, v3);
    let s45 = venta_simd_vec_add256(v4, v5);
    let s67 = venta_simd_vec_add256(v6, v7);
    let s03 = venta_simd_vec_add256(s01, s23);
    let s47 = venta_simd_vec_add256(s45, s67);
    let s07 = venta_simd_vec_add256(s03, s47);
    venta_simd_vec_add256(s07, v8)
}

fn main() {
    let v0 = v256 { q0: 1, q1: 1, q2: 1, q3: 1 };
    let v1 = v256 { q0: 2, q1: 2, q2: 2, q3: 2 };
    let v2 = v256 { q0: 3, q1: 3, q2: 3, q3: 3 };
    let v3 = v256 { q0: 4, q1: 4, q2: 4, q3: 4 };
    let v4 = v256 { q0: 5, q1: 5, q2: 5, q3: 5 };
    let v5 = v256 { q0: 6, q1: 6, q2: 6, q3: 6 };
    let v6 = v256 { q0: 7, q1: 7, q2: 7, q3: 7 };
    let v7 = v256 { q0: 8, q1: 8, q2: 8, q3: 8 };
    let v8 = v256 { q0: 9, q1: 9, q2: 9, q3: 9 };

    let res = sum_9_vectors(v0, v1, v2, v3, v4, v5, v6, v7, v8);

    // Sum of 1..9 = 45
    if res.q0 == 45 && res.q1 == 45 && res.q2 == 45 && res.q3 == 45 {
        print_str("VECTOR_EXHAUSTION_STACK_OK\n");
    } else {
        print_str("VECTOR_EXHAUSTION_STACK_FAIL\n");
    }
}
"#;

    run_simd_test(
        "simd_vec_abi_exhaustion",
        source,
        TargetFeatures::avx2(),
        b"VECTOR_EXHAUSTION_STACK_OK\n",
    );
}

#[test]
fn test_simd_vector_abi_c_oracle_bidirectional() {
    let host = TargetFeatures::host_native();
    if !host.avx2 {
        println!("Skipping C ABI oracle test: host CPU does not support AVX2");
        return;
    }

    // Direction 1: Venta caller invokes GCC-compiled vanta_c_vector_add
    let venta_caller_src = r#"
struct v256 {
    pub q0: i64,
    pub q1: i64,
    pub q2: i64,
    pub q3: i64,
}

fn main() {
    let a = v256 { q0: 100, q1: 200, q2: 300, q3: 400 };
    let b = v256 { q0: 10, q1: 20, q2: 30, q3: 40 };
    let res = vanta_c_vector_add(a, b);

    if res.q0 == 110 && res.q1 == 220 && res.q2 == 330 && res.q3 == 440 {
        print_str("VENTA_CALLS_C_OK\n");
    } else {
        print_str("VENTA_CALLS_C_FAIL\n");
    }
}
"#;

    run_simd_test(
        "simd_c_oracle_v_calls_c",
        venta_caller_src,
        TargetFeatures::avx2(),
        b"VENTA_CALLS_C_OK\n",
    );

    // Direction 2: GCC caller invokes Venta-compiled vector function
    let temp_dir = std::env::temp_dir().join("venta_c_oracle_c_calls_v");
    let _ = fs::create_dir_all(&temp_dir);
    let v_src = temp_dir.join("lib.va");
    let v_obj = temp_dir.join("lib.o");
    let c_src = temp_dir.join("main.c");
    let bin_path = temp_dir.join("c_oracle_bin");

    let venta_callee_src = r#"
pub struct v256 {
    pub q0: i64,
    pub q1: i64,
    pub q2: i64,
    pub q3: i64,
}

pub fn venta_exported_vector_add(a: v256, b: v256) -> v256 {
    venta_simd_vec_add256(a, b)
}
"#;
    fs::write(&v_src, venta_callee_src).unwrap();

    let mut session = CompilerSession::new();
    let mut options = CompilerOptions::new(v_src.clone(), v_obj.clone());
    options.opt_level = OptLevel::O1;
    options.emit_target = EmitTarget::Obj;
    options.std_path = Some(PathBuf::from("library"));
    options.target_features = TargetFeatures::avx2();

    session
        .compile(&options)
        .expect("Compile Venta callee to object file");

    let c_caller_code = r#"
#include <immintrin.h>
#include <stdint.h>
#include <stdio.h>

extern __m256i venta_exported_vector_add(__m256i a, __m256i b);

int main() {
    __m256i a = _mm256_set_epi64x(400, 300, 200, 100);
    __m256i b = _mm256_set_epi64x(40, 30, 20, 10);
    __m256i res = venta_exported_vector_add(a, b);

    int64_t out[4];
    _mm256_storeu_si256((__m256i*)out, res);

    if (out[0] == 110 && out[1] == 220 && out[2] == 330 && out[3] == 440) {
        printf("C_CALLS_VENTA_OK\n");
        return 0;
    } else {
        printf("C_CALLS_VENTA_FAIL: got [%ld, %ld, %ld, %ld]\n", out[0], out[1], out[2], out[3]);
        return 1;
    }
}
"#;
    fs::write(&c_src, c_caller_code).unwrap();

    let rt_archive = CompilerSession::find_runtime_archive(&options).expect("Locate libvanta_rt.a");

    let gcc_status = Command::new("gcc")
        .arg("-O2")
        .arg("-mavx2")
        .arg(&c_src)
        .arg(&v_obj)
        .arg(&rt_archive)
        .arg("-o")
        .arg(&bin_path)
        .status()
        .expect("Invoke gcc linker for C oracle");

    assert!(gcc_status.success(), "GCC link failed for C oracle");

    let run_out = Command::new(&bin_path)
        .output()
        .expect("Run C oracle binary");

    assert_eq!(
        run_out.stdout,
        b"C_CALLS_VENTA_OK\n",
        "C oracle stdout mismatch: {}",
        String::from_utf8_lossy(&run_out.stdout)
    );

    let _ = fs::remove_dir_all(&temp_dir);
}
