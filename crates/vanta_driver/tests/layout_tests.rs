//! Milestone 24: Branch-Aware Machine-Code Layout & Alignment Tests.

use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

fn run_layout_differential_test(
    name: &str,
    src: &str,
    expected_exit_code: i32,
) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", name));
    fs::write(&src_path, src).unwrap();

    // 1. With branch-aware layout enabled (default)
    let out_layout = temp_dir.path().join(format!("{}_layout", name));
    let opts_layout = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_layout.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: true,
        enable_gvn: true,
        enable_inline: true,
        enable_ipa_constprop: true,
        enable_licm: true,
        enable_indvar: true,
        enable_strength_reduction: true,
        enable_sroa: true,
        enable_loop_peel: true,
        enable_loop_unroll: true,
        enable_instruction_schedule: true,
        enable_global_dce: true,
        enable_tail_calls: true,
        enable_block_layout: true,
        enable_code_align: true,
    };
    let mut session1 = CompilerSession::new();
    let res1 = session1.compile(&opts_layout);
    assert!(res1.is_ok(), "Layout compilation failed: {:?}", res1.err());
    let status1 = Command::new(&out_layout).status().expect("Execute layout binary");
    assert_eq!(
        status1.code(),
        Some(expected_exit_code),
        "Layout exit code mismatch for {}",
        name
    );

    // 2. With branch-aware layout disabled (--no-block-layout)
    let out_no_layout = temp_dir.path().join(format!("{}_no_layout", name));
    let opts_no_layout = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_no_layout.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: true,
        enable_gvn: true,
        enable_inline: true,
        enable_ipa_constprop: true,
        enable_licm: true,
        enable_indvar: true,
        enable_strength_reduction: true,
        enable_sroa: true,
        enable_loop_peel: true,
        enable_loop_unroll: true,
        enable_instruction_schedule: true,
        enable_global_dce: true,
        enable_tail_calls: true,
        enable_block_layout: false,
        enable_code_align: true,
    };
    let mut session2 = CompilerSession::new();
    let res2 = session2.compile(&opts_no_layout);
    assert!(res2.is_ok(), "No-layout compilation failed: {:?}", res2.err());
    let status2 = Command::new(&out_no_layout).status().expect("Execute no-layout binary");
    assert_eq!(
        status2.code(),
        Some(expected_exit_code),
        "No-layout exit code mismatch for {}",
        name
    );

    // 3. With code alignment disabled (--no-code-align)
    let out_no_align = temp_dir.path().join(format!("{}_no_align", name));
    let opts_no_align = CompilerOptions {
        input_file: src_path,
        output_file: out_no_align.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: true,
        enable_gvn: true,
        enable_inline: true,
        enable_ipa_constprop: true,
        enable_licm: true,
        enable_indvar: true,
        enable_strength_reduction: true,
        enable_sroa: true,
        enable_loop_peel: true,
        enable_loop_unroll: true,
        enable_instruction_schedule: true,
        enable_global_dce: true,
        enable_tail_calls: true,
        enable_block_layout: true,
        enable_code_align: false,
    };
    let mut session3 = CompilerSession::new();
    let res3 = session3.compile(&opts_no_align);
    assert!(res3.is_ok(), "No-align compilation failed: {:?}", res3.err());
    let status3 = Command::new(&out_no_align).status().expect("Execute no-align binary");
    assert_eq!(
        status3.code(),
        Some(expected_exit_code),
        "No-align exit code mismatch for {}",
        name
    );

    // Differential equivalence check across all modes
    assert_eq!(status1.code(), status2.code());
    assert_eq!(status1.code(), status3.code());
}

#[test]
fn test_layout_conditional_branches_and_fall_through() {
    run_layout_differential_test(
        "cond_branch_fallthrough",
        r#"
        pub fn main() -> i64 {
            let mut x: i64 = 10;
            let mut y: i64 = 20;
            if x < y {
                x += 5;
            } else {
                y += 5;
            }
            if x == 15 {
                return 42;
            }
            return 0;
        }
        "#,
        42,
    );
}

#[test]
fn test_layout_multi_branch_ladder() {
    run_layout_differential_test(
        "branch_ladder",
        r#"
        fn classify(val: i64) -> i64 {
            if val < 0 {
                return 1;
            } else if val == 0 {
                return 2;
            } else if val < 10 {
                return 3;
            } else if val < 100 {
                return 4;
            } else {
                return 5;
            }
        }

        pub fn main() -> i64 {
            let r1: i64 = classify(-5);   // 1
            let r2: i64 = classify(0);    // 2
            let r3: i64 = classify(7);    // 3
            let r4: i64 = classify(50);   // 4
            let r5: i64 = classify(500);  // 5
            return r1 + r2 + r3 + r4 + r5; // 15
        }
        "#,
        15,
    );
}

#[test]
fn test_layout_nested_loops_with_early_break() {
    run_layout_differential_test(
        "nested_loops",
        r#"
        pub fn main() -> i64 {
            let mut total: i64 = 0;
            let mut i: i64 = 0;
            while i < 10 {
                let mut j: i64 = 0;
                while j < 10 {
                    if i + j >= 12 {
                        break;
                    }
                    total += 1;
                    j += 1;
                }
                i += 1;
            }
            return total;
        }
        "#,
        72,
    );
}
