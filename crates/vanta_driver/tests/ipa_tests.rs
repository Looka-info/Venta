//! Integration and Unit Tests for Milestone 20: Interprocedural Constant Propagation (IPA).

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
fn test_ipa_folds_pure_call_with_constant_arguments() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn multiply_add(a: i64, b: i64) -> i64 {
            return a * 3 + b;
        }

        pub fn calculate() -> i64 {
            let res: i64 = multiply_add(10, 5);
            return res;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    // Inlining is DISABLED; IPA is ENABLED
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        false, // inline = false
        true,  // ipa_constprop = true
        false, // licm = false
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "calculate").unwrap();
    // The call should be completely folded into a ConstInt(35) without inlining the body!
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 0,
        "Expected call to multiply_add(10, 5) to be folded to constant without inlining, found {}",
        call_count
    );

    let has_const_35 = func.blocks.iter().flat_map(|b| &b.instructions).any(|inst| {
        matches!(inst, Instruction::ConstInt { val, .. } if *val == 35)
    });

    assert!(
        has_const_35,
        "Expected folded constant 35 (10 * 3 + 5) in calculate()"
    );
}

#[test]
fn test_ipa_folds_boolean_pure_call() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn is_valid(code: i64) -> bool {
            return code > 100;
        }

        pub fn check() -> bool {
            let ok: bool = is_valid(150);
            return ok;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        false, // inline = false
        true,  // ipa_constprop = true
        false, // licm = false
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "check").unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(call_count, 0, "Expected call to be folded to ConstBool(true)");

    let has_const_true = func.blocks.iter().flat_map(|b| &b.instructions).any(|inst| {
        matches!(inst, Instruction::ConstBool { val: true, .. })
    });

    assert!(has_const_true, "Expected folded constant boolean 'true'");
}

#[test]
fn test_ipa_skips_non_constant_args() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn multiply_add(a: i64, b: i64) -> i64 {
            return a * 3 + b;
        }

        pub fn calculate_dynamic(x: i64) -> i64 {
            let res: i64 = multiply_add(x, 5);
            return res;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        false, // inline = false
        true,  // ipa_constprop = true
        false, // licm = false
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
        .find(|f| f.name == "calculate_dynamic")
        .unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 1,
        "Expected call with dynamic argument to remain unfolded"
    );
}

#[test]
fn test_ipa_disabled_via_flag() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn multiply_add(a: i64, b: i64) -> i64 {
            return a * 3 + b;
        }

        pub fn calculate() -> i64 {
            let res: i64 = multiply_add(10, 5);
            return res;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        false, // inline = false
        false, // ipa_constprop = false (DISABLED)
        false, // licm = false
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "calculate").unwrap();
    let call_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Call { .. }))
        .count();

    assert_eq!(
        call_count, 1,
        "Expected call to be preserved when ipa_constprop is false, found {}",
        call_count
    );
}

#[test]
fn test_ipa_end_to_end_execution_equivalence() {
    let target_dir = std::env::temp_dir().join("vanta_ipa_e2e_test");
    fs::create_dir_all(&target_dir).unwrap();

    let src_path = target_dir.join("test_ipa.va");
    let exe_no_ipa = target_dir.join("test_no_ipa");
    let exe_ipa = target_dir.join("test_ipa");

    let source_code = r#"
    fn power3(x: i64) -> i64 {
        return x * x * x;
    }

    fn offset_cube(a: i64, b: i64) -> i64 {
        let p: i64 = power3(a);
        return p + b;
    }

    pub fn main() -> i64 {
        let v1: i64 = offset_cube(3, 10);
        let v2: i64 = offset_cube(4, 20);
        print_i64(v1 + v2);
        return 0;
    }
    "#;

    fs::write(&src_path, source_code).unwrap();

    // 1. Compile with --no-ipa-constprop
    let opts_no_ipa = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_ipa.clone(),
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
    assert!(session1.compile(&opts_no_ipa).is_ok(), "Compilation with --no-ipa-constprop failed");

    // 2. Compile with IPA enabled
    let opts_ipa = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_ipa.clone(),
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
        enable_ipa_constprop: true,
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
    assert!(session2.compile(&opts_ipa).is_ok(), "Compilation with IPA enabled failed");

    // 3. Execute both binaries and compare output
    let out_no_ipa = Command::new(&exe_no_ipa)
        .output()
        .expect("Failed to run no-ipa binary");
    let out_ipa = Command::new(&exe_ipa)
        .output()
        .expect("Failed to run ipa binary");

    assert_eq!(out_no_ipa.status.code(), Some(0));
    assert_eq!(out_ipa.status.code(), Some(0));
    assert_eq!(
        out_no_ipa.stdout, out_ipa.stdout,
        "IPA output differs from non-IPA baseline!"
    );
    assert!(!out_ipa.stdout.is_empty());

    let _ = fs::remove_dir_all(&target_dir);
}
