//! Dedicated Integration and Correctness Test Suite for Induction Variable Optimization (IVO).

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::ir::MirProgram;
use vanta_mir::opt::{InductionVarOpt, NaturalLoop, OptLevel as MirOptLevel, OptimizationPipeline};
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
fn test_ivo_basic_induction_variable_discovery() {
    let (mir, _sm) = compile_src_to_mir(
        r#"
        pub fn count_loop(n: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < n {
                sum = sum + i;
                i = i + 1;
            }
            return sum;
        }
        "#,
    );

    let mut func = mir
        .functions
        .into_iter()
        .find(|f| f.name == "count_loop")
        .unwrap();
    vanta_mir::opt::Mem2Reg::run_on_function(&mut func);

    let dom_tree = vanta_mir::opt::DominatorTree::compute(&func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(&func, &dom_tree);
    assert_eq!(loops.len(), 1, "Should find exactly 1 natural loop");

    let ivs = InductionVarOpt::find_basic_induction_vars(&func, &loops[0]);
    assert!(
        !ivs.is_empty(),
        "Should discover basic induction variable i"
    );
    let iv = ivs
        .iter()
        .find(|iv| iv.step == 1)
        .expect("Should find IV with step +1");
    assert_eq!(iv.step, 1);
}

#[test]
fn test_ivo_eliminates_redundant_induction_variable() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn redundant_loop(n: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            let mut j: i64 = 0;
            while i < n {
                sum = sum + i + j;
                i = i + 1;
                j = j + 1;
            }
            return sum;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,  // mem2reg
        true,  // gvn
        false, // inline
        false, // ipa
        false, // licm
        true,  // indvar
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
        .find(|f| f.name == "redundant_loop")
        .unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    // In the optimized loop header, there should now be only 2 Phis (sum and i),
    // because j was recognized as completely redundant and merged with i!
    let header = func
        .blocks
        .iter()
        .find(|b| b.id == loops[0].header)
        .unwrap();
    let phi_count = header
        .instructions
        .iter()
        .filter(|inst| matches!(inst, vanta_mir::ir::Instruction::Phi { .. }))
        .count();

    assert!(
        phi_count <= 2,
        "Redundant IV j must be eliminated, leaving at most 2 phis (sum and i), found: {}",
        phi_count
    );
}

#[test]
fn test_ivo_preserves_distinct_initial_values() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn distinct_loop(n: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            let mut j: i64 = 100; // different initial value!
            while i < n {
                sum = sum + i + j;
                i = i + 1;
                j = j + 1;
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
        false,
        true, // indvar
        false,
        false,
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "distinct_loop")
        .unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    // i (init 0) and j (init 100) must NOT be conflated; both must retain distinct phis
    let header = func
        .blocks
        .iter()
        .find(|b| b.id == loops[0].header)
        .unwrap();
    let phi_count = header
        .instructions
        .iter()
        .filter(|inst| matches!(inst, vanta_mir::ir::Instruction::Phi { .. }))
        .count();

    assert_eq!(
        phi_count, 3,
        "IVs with different initial values must not be merged (sum, i, j)"
    );
}

#[test]
fn test_ivo_eliminates_dead_induction_variable() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn dead_iv_loop(n: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            let mut unused_counter: i64 = 0;
            while i < n {
                sum = sum + i;
                i = i + 1;
                unused_counter = unused_counter + 1;
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
        false,
        true, // indvar
        false,
        false,
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "dead_iv_loop")
        .unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    // unused_counter should be completely removed!
    let header = func
        .blocks
        .iter()
        .find(|b| b.id == loops[0].header)
        .unwrap();
    let phi_count = header
        .instructions
        .iter()
        .filter(|inst| matches!(inst, vanta_mir::ir::Instruction::Phi { .. }))
        .count();

    assert_eq!(
        phi_count, 2,
        "Dead unused_counter must be eliminated, leaving exactly 2 phis (sum and i)"
    );
}

#[test]
fn test_ivo_disabled_via_flag() {
    let src = r#"
    fn main() -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        let mut j: i64 = 0;
        while i < 100 {
            sum = sum + i + j;
            i = i + 1;
            j = j + 1;
        }
        print_i64(sum);
        print_str("\n");
        return 0;
    }
    "#;

    let temp_dir = std::env::temp_dir().join(format!("vanta_ivo_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("test_ivo.va");
    let exe_no_ivo = temp_dir.join("bin_no_ivo");
    let exe_ivo = temp_dir.join("bin_ivo");

    fs::write(&src_path, src).unwrap();

    // 1. Compile with --no-indvar
    let opts_no_ivo = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_ivo.clone(),
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
        enable_indvar: false, // DISABLED
        enable_strength_reduction: false,
        enable_sroa: false,
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts_no_ivo).is_ok());

    // 2. Compile with IVO enabled
    let opts_ivo = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_ivo.clone(),
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
        enable_indvar: true, // ENABLED
        enable_strength_reduction: false,
        enable_sroa: false,
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts_ivo).is_ok());

    // 3. Both binaries must execute and produce identical output
    let out_no_ivo = Command::new(&exe_no_ivo).output().unwrap();
    let out_ivo = Command::new(&exe_ivo).output().unwrap();

    assert_eq!(out_no_ivo.status.code(), Some(0));
    assert_eq!(out_ivo.status.code(), Some(0));
    assert_eq!(out_no_ivo.stdout, out_ivo.stdout);

    let _ = fs::remove_dir_all(&temp_dir);
}
