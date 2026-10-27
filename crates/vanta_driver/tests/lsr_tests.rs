//! Dedicated Integration and Correctness Test Suite for Loop Strength Reduction (LSR).

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::opt::{
    NaturalLoop, OptLevel as MirOptLevel, OptimizationPipeline,
};
use vanta_mir::ir::{Instruction, MirBinaryOp, MirProgram};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_src_to_mir(src: &str) -> (MirProgram, SourceMap) {
    let mut sm = SourceMap::new();
    let id = sm.add_file("test.va".into(), src.into());
    let file = sm.get_file(id).unwrap();

    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&file);
    let tokens = lexer.tokenize(&mut diags);
    assert!(!diags.has_errors(), "Lex error: {:?}", diags);

    let mut parser = Parser::new(tokens);
    let ast = parser.parse_program(&mut diags);
    assert!(!diags.has_errors(), "Parse error: {:?}", diags);

    let sema = SemanticAnalyzer::new();
    let sema_ctx = sema.analyze(&ast, &mut diags);
    assert!(!diags.has_errors(), "Sema error: {:?}", diags);

    let mut lowerer = vanta_mir::lower::MirLowerer::new(&sema_ctx);
    let mir = lowerer.lower_program(&ast);
    (mir, sm)
}

#[test]
fn test_lsr_reduces_multiplication_to_addition() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn stride_loop(n: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < n {
                let scaled: i64 = i * 8;
                sum = sum + scaled;
                i = i + 1;
            }
            return sum;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true, // mem2reg
        true, // gvn
        false, // inline
        false, // ipa
        false, // licm
        true,  // indvar
        true,  // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "stride_loop").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    // In the loop body, there must be ZERO multiplications:
    // the i * 8 was converted into a loop-carried recurrence addition!
    let mul_count = loops[0].blocks.iter().map(|&b_id| {
        let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
        block.instructions.iter().filter(|inst| {
            matches!(inst, Instruction::Binary { op: MirBinaryOp::Mul, .. })
        }).count()
    }).sum::<usize>();

    assert_eq!(
        mul_count, 0,
        "Loop body must contain zero multiplications after LSR; i * 8 must be reduced to addition!"
    );
}

#[test]
fn test_lsr_handles_parameter_invariant_stride() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn param_stride_loop(n: i64, stride: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < n {
                let scaled: i64 = i * stride;
                sum = sum + scaled;
                i = i + 1;
            }
            return sum;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        false,
        false,
        true,  // licm hoists parameter load
        true,  // indvar
        true,  // strength_reduction
        false,
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "param_stride_loop").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    let mul_in_loop = loops[0].blocks.iter().any(|&b_id| {
        let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
        block.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Binary { op: MirBinaryOp::Mul, .. })
        })
    });

    assert!(
        !mul_in_loop,
        "Multiplication with invariant parameter stride must be reduced out of loop body!"
    );
}

#[test]
fn test_lsr_disabled_via_flag() {
    let src = r#"
    fn main() -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < 1000 {
            let scaled: i64 = i * 7;
            sum = sum + scaled;
            i = i + 1;
        }
        print_i64(sum);
        print_str("\n");
        return 0;
    }
    "#;

    let temp_dir = std::env::temp_dir().join(format!("vanta_lsr_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("test_lsr.va");
    let exe_no_lsr = temp_dir.join("bin_no_lsr");
    let exe_lsr = temp_dir.join("bin_lsr");

    fs::write(&src_path, src).unwrap();

    // 1. Compile with --no-strength-reduction
    let opts_no_lsr = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_lsr.clone(),
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
        enable_strength_reduction: false, // DISABLED
        enable_sroa: false,
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts_no_lsr).is_ok());

    // 2. Compile with LSR enabled
    let opts_lsr = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_lsr.clone(),
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
        enable_strength_reduction: true, // ENABLED
        enable_sroa: false,
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts_lsr).is_ok());

    // 3. Both binaries must execute and produce identical output (3496500)
    let out_no_lsr = Command::new(&exe_no_lsr).output().unwrap();
    let out_lsr = Command::new(&exe_lsr).output().unwrap();

    assert_eq!(out_no_lsr.status.code(), Some(0));
    assert_eq!(out_lsr.status.code(), Some(0));
    assert_eq!(out_no_lsr.stdout, out_lsr.stdout);
    assert_eq!(String::from_utf8_lossy(&out_lsr.stdout).trim(), "3496500");

    let _ = fs::remove_dir_all(&temp_dir);
}
