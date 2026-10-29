//! Integration and Unit Tests for Milestone 20: Conservative Function Inlining.

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::opt::{OptLevel as MirOptLevel, OptimizationPipeline};
use vanta_mir::{Instruction, MirLowerer};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_src_to_mir(src: &str) -> (vanta_mir::MirProgram, SourceMap) {
    let mut sm = SourceMap::new();
    let id = sm.add_file("test.va".into(), src.into());
    let file = sm.get_file(id).unwrap();

    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&file);
    let tokens = lexer.tokenize(&mut diags);
    assert!(
        !diags.has_errors(),
        "Lex error: {}",
        diags.render_all(&sm, false)
    );

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program(&mut diags);
    assert!(
        !diags.has_errors(),
        "Parse error: {}",
        diags.render_all(&sm, false)
    );

    let sema = SemanticAnalyzer::new();
    let sema_ctx = sema.analyze(&program, &mut diags);
    assert!(
        !diags.has_errors(),
        "Sema error: {}",
        diags.render_all(&sm, false)
    );

    let mut lowerer = MirLowerer::new(&sema_ctx);
    let mir = lowerer.lower_program(&program);
    (mir, sm)
}

#[test]
fn test_inline_leaf_arithmetic_helper() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn add_helper(a: i64, b: i64) -> i64 {
            return a + b;
        }

        pub fn compute(x: i64) -> i64 {
            let res: i64 = add_helper(x, 42);
            return res;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,  // mem2reg
        true,  // gvn
        true,  // inline
        false, // ipa_constprop
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "compute").unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 0,
        "Expected add_helper call to be inlined and eliminated from compute(), found {}",
        call_count
    );
}

#[test]
fn test_inline_multi_param_helper() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn lerp(a: i64, b: i64, t: i64) -> i64 {
            return a + (b - a) * t;
        }

        pub fn calculate(val: i64) -> i64 {
            let r: i64 = lerp(val, 100, 2);
            return r;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        true,
        false,
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "calculate")
        .unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 0,
        "Expected lerp call to be inlined and eliminated from calculate(), found {}",
        call_count
    );
}

#[test]
fn test_inline_unit_return_helper() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn dummy_void(x: i64) {
            let y: i64 = x * 2;
        }

        pub fn run_void(a: i64) -> i64 {
            dummy_void(a);
            return a + 1;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        true,
        false,
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "run_void").unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 0,
        "Expected dummy_void call to be inlined into run_void(), found {}",
        call_count
    );
}

#[test]
fn test_inline_multiple_return_paths_phi() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn abs_val(x: i64) -> i64 {
            if x < 0 {
                return 0 - x;
            } else {
                return x;
            }
        }

        pub fn check_abs(v: i64) -> i64 {
            let res: i64 = abs_val(v);
            return res * 2;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        true,
        false,
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "check_abs")
        .unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 0,
        "Expected abs_val call with branching return to be inlined into check_abs(), found {}",
        call_count
    );
    // Caller should now have multiple blocks due to branching
    assert!(
        func.blocks.len() > 1,
        "Expected multiple blocks in check_abs() after inlining branch"
    );
}

#[test]
fn test_inline_callee_with_loop() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn sum_to(n: i64) -> i64 {
            let mut acc: i64 = 0;
            let mut i: i64 = 0;
            while i < n {
                acc = acc + i;
                i = i + 1;
            }
            return acc;
        }

        pub fn compute_sum(limit: i64) -> i64 {
            return sum_to(limit);
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        true,
        false,
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "compute_sum")
        .unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 0,
        "Expected sum_to loop helper to be inlined into compute_sum(), found {}",
        call_count
    );
}

#[test]
fn test_inline_called_multiple_times() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn step(x: i64) -> i64 {
            return x + 10;
        }

        pub fn multi_step(val: i64) -> i64 {
            let s1: i64 = step(val);
            let s2: i64 = step(s1);
            return s2;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        true,
        false,
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "multi_step")
        .unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 0,
        "Expected both step calls to be inlined into multi_step(), found {}",
        call_count
    );
}

#[test]
fn test_inline_direct_recursion_rejected() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn fact(n: i64) -> i64 {
            if n <= 1 {
                return 1;
            }
            return n * fact(n - 1);
        }

        pub fn compute_fact(x: i64) -> i64 {
            return fact(x);
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        true,
        false,
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let fact_func = mir.functions.iter().find(|f| f.name == "fact").unwrap();
    let recursive_call_count = fact_func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { callee, .. } if callee == "fact"))
        .count();

    assert_eq!(
        recursive_call_count, 1,
        "Expected recursive call to fact inside fact to be preserved and not inlined"
    );

    let compute_func = mir
        .functions
        .iter()
        .find(|f| f.name == "compute_fact")
        .unwrap();
    let call_to_fact = compute_func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { callee, .. } if callee == "fact"))
        .count();

    assert_eq!(
        call_to_fact, 1,
        "Expected call to recursive function fact to NOT be inlined into compute_fact"
    );
}

#[test]
fn test_inline_mutual_recursion_rejected() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn is_even(n: i64) -> bool {
            if n == 0 {
                return true;
            }
            return is_odd(n - 1);
        }

        fn is_odd(n: i64) -> bool {
            if n == 0 {
                return false;
            }
            return is_even(n - 1);
        }

        pub fn check_parity(x: i64) -> bool {
            return is_even(x);
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        true,
        false,
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let parity_func = mir
        .functions
        .iter()
        .find(|f| f.name == "check_parity")
        .unwrap();
    let call_count = parity_func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 1,
        "Expected mutually recursive call to be preserved and NOT inlined"
    );
}

#[test]
fn test_inline_disabled_via_flag() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn add_helper(a: i64, b: i64) -> i64 {
            return a + b;
        }

        pub fn compute(x: i64) -> i64 {
            return add_helper(x, 10);
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        false, // Inlining disabled!
        false,
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "compute").unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 1,
        "Expected add_helper call to remain when enable_inline is false, found {}",
        call_count
    );
}

#[test]
fn test_inline_end_to_end_execution_equivalence() {
    let target_dir = std::env::temp_dir().join("vanta_inlining_e2e_test");
    fs::create_dir_all(&target_dir).unwrap();

    let src_path = target_dir.join("test_inline.va");
    let exe_no_inline = target_dir.join("test_no_inline");
    let exe_inline = target_dir.join("test_inline");

    let source_code = r#"
    fn square(x: i64) -> i64 {
        return x * x;
    }

    fn clamp_sub(a: i64, b: i64) -> i64 {
        if a < b {
            return 0;
        } else {
            return a - b;
        }
    }

    fn work(v: i64) -> i64 {
        let sq: i64 = square(v);
        let cl: i64 = clamp_sub(sq, 50);
        return cl;
    }

    pub fn main() -> i64 {
        let mut total: i64 = 0;
        let mut i: i64 = 0;
        while i < 15 {
            total = total + work(i);
            i = i + 1;
        }
        print_i64(total);
        return 0;
    }
    "#;

    fs::write(&src_path, source_code).unwrap();

    // 1. Compile with --no-inline
    let opts_no_inline = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_inline.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: true,
        enable_gvn: true,
        enable_inline: false,
        enable_ipa_constprop: false,
        enable_licm: false,
        enable_indvar: false,
        enable_strength_reduction: false,
        enable_sroa: false,
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session1 = CompilerSession::new();
    assert!(
        session1.compile(&opts_no_inline).is_ok(),
        "Compilation with --no-inline failed"
    );

    // 2. Compile with inlining enabled
    let opts_inline = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_inline.clone(),
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
        enable_ipa_constprop: false,
        enable_licm: false,
        enable_indvar: false,
        enable_strength_reduction: false,
        enable_sroa: false,
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session2 = CompilerSession::new();
    assert!(
        session2.compile(&opts_inline).is_ok(),
        "Compilation with inlining enabled failed"
    );

    // 3. Execute both binaries and compare output
    let out_no_inline = Command::new(&exe_no_inline)
        .output()
        .expect("Failed to run no-inline binary");
    let out_inline = Command::new(&exe_inline)
        .output()
        .expect("Failed to run inlined binary");

    assert_eq!(out_no_inline.status.code(), Some(0));
    assert_eq!(out_inline.status.code(), Some(0));
    assert_eq!(
        out_no_inline.stdout, out_inline.stdout,
        "Inlined output differs from non-inlined baseline!"
    );
    assert!(!out_inline.stdout.is_empty());

    let _ = fs::remove_dir_all(&target_dir);
}
