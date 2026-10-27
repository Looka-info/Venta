//! Dedicated verification test suite for Loop Unrolling in Vanta.

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::ir::*;
use vanta_mir::opt::{DominatorTree, NaturalLoop, OptimizationPipeline};
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
fn test_arrays_bounds_sum() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        fn sum(arr: &[i64; 4]) -> i64 {
            let mut total: i64 = 0;
            let mut i: i64 = 0;
            while i < 4 {
                total += arr[i];
                i += 1;
            }
            return total;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        OptLevel::O1,
        true, // mem2reg
        true, // gvn
        true, // inlining
        true, // ipa
        true, // licm
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true,  // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    for func in &mir.functions {
        println!("=== Function {} ===", func.name);
        for b in &func.blocks {
            println!("Block {:?} ({}) -> term {:?}", b.id, b.name, b.terminator);
            for inst in &b.instructions {
                println!("   {:?}", inst);
            }
        }
    }
    assert!(!diags.has_errors(), "Diags: {:?}", diags);
}

#[test]
fn test_loop_unroll_full_eliminates_small_loop() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn test_small_loop() -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < 4 {
                sum = sum + i;
                i = i + 1;
            }
            return sum;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        OptLevel::O1,
        true, // mem2reg
        true, // gvn
        false, // inlining
        false, // ipa
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        true,  // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "test_small_loop").unwrap();
    let dom_tree = DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);

    // After full unrolling, the natural loop is COMPLETELY ELIMINATED!
    assert_eq!(loops.len(), 0, "Small constant loop (4 iters) must be completely eliminated by full unrolling!");
}

#[test]
fn test_loop_unroll_partial_factor_2() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn test_large_loop() -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < 100 {
                sum = sum + i;
                i = i + 1;
            }
            return sum;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        OptLevel::O1,
        true, // mem2reg
        true, // gvn
        false, // inlining
        false, // ipa
        false, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        true,  // loop_unroll (partial unrolling)
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "test_large_loop").unwrap();
    // In partial unrolling, duplicated blocks exist
    let has_dup_blocks = func.blocks.iter().any(|b| b.name.contains("dup"));
    assert!(has_dup_blocks, "Large exact-multiple loop (100 iters) must have duplicated body blocks from factor-2 partial unrolling!");
}

#[test]
fn test_loop_unroll_preserves_zero_trip_loop() {
    let temp_dir = std::env::temp_dir().join(format!("vanta_unroll_zero_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    let src_path = temp_dir.join("test_zero.va");
    let exe_path = temp_dir.join("test_zero");

    let src = r#"
    fn compute() -> i64 {
        let mut sum: i64 = 42;
        let mut i: i64 = 0;
        while i < 0 {
            sum = sum + 100;
            i = i + 1;
        }
        return sum;
    }

    fn main() -> i64 {
        let res = compute();
        print_i64(res);
        print_str("\n");
        return 0;
    }
    "#;
    fs::write(&src_path, src).unwrap();

    let opts = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_path.clone(),
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
        enable_loop_peel: false,
        enable_loop_unroll: true, // UNROLL ENABLED
        enable_instruction_schedule: true,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    if let Err(e) = session.compile(&opts) {
        panic!("Compilation failed: {:?}", e);
    }

    let out = Command::new(&exe_path).output().expect("Execution failed");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "42", "Zero-trip loop must preserve initial value 42!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_loop_unroll_differential_equivalence() {
    let temp_dir = std::env::temp_dir().join(format!("vanta_unroll_diff_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    let src_path = temp_dir.join("test_diff.va");
    let exe_no_unroll = temp_dir.join("test_no_unroll");
    let exe_unroll = temp_dir.join("test_unroll");

    let src = r#"
    fn compute(iters: i64) -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < iters {
            sum = sum + (i * 2 + 3);
            i = i + 1;
        }
        return sum;
    }

    fn main() -> i64 {
        let res = compute(100);
        print_i64(res);
        print_str("\n");
        return 0;
    }
    "#;
    fs::write(&src_path, src).unwrap();

    // 1. Compile WITHOUT unrolling
    let opts_no_unroll = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_unroll.clone(),
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
        enable_loop_peel: false,
        enable_loop_unroll: false, // DISABLED
        enable_instruction_schedule: true,
        ..Default::default()
    };
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts_no_unroll).is_ok());

    // 2. Compile WITH unrolling
    let opts_unroll = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_unroll.clone(),
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
        enable_loop_peel: false,
        enable_loop_unroll: true, // ENABLED
        enable_instruction_schedule: true,
        ..Default::default()
    };
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts_unroll).is_ok());

    let out1 = Command::new(&exe_no_unroll).output().unwrap();
    let out2 = Command::new(&exe_unroll).output().unwrap();
    assert_eq!(out1.stdout, out2.stdout, "Unrolled and non-unrolled executions must produce identical results!");

    let _ = fs::remove_dir_all(&temp_dir);
}
