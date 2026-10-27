//! Integration and Unit Tests for Milestone 19: Global Value Numbering (GVN) and CSE.

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::opt::{OptLevel as MirOptLevel, OptimizationPipeline};
use vanta_mir::{Instruction, MirBinaryOp, MirLowerer};
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
fn test_gvn_eliminates_straightline_repeated_expressions() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn compute(x: i64, y: i64) -> i64 {
            let a: i64 = x + y;
            let b: i64 = x + y;
            let c: i64 = a * b;
            return c;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    // Run with Mem2Reg and GVN
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        true,
        true,
        true,
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true, // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "compute").unwrap();
    // In compute(x, y), x + y should only appear ONCE as an Add
    let add_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Binary { op: MirBinaryOp::Add, .. }))
        .count();

    assert_eq!(
        add_count, 1,
        "Expected exactly 1 Add instruction after GVN, but found {}",
        add_count
    );
}

#[test]
fn test_gvn_commutative_normalization() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn compute_comm(x: i64, y: i64) -> i64 {
            let a: i64 = x + y;
            let b: i64 = y + x;
            return a + b;
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
        true,
        true,
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true, // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "compute_comm").unwrap();
    // a = x + y, b = y + x => b should reuse a, so there should be 1 Add for (x+y) and 1 Add for (a+b)
    let add_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Binary { op: MirBinaryOp::Add, .. }))
        .count();

    assert_eq!(
        add_count, 2,
        "Expected exactly 2 Add instructions (1 for subexpression, 1 for return sum), found {}",
        add_count
    );
}

#[test]
fn test_gvn_comparison_dual_normalization() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn compare_dual(x: i64, y: i64) -> bool {
            let a: bool = x > y;
            let b: bool = y < x;
            return a == b;
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
        true,
        true,
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true, // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "compare_dual").unwrap();
    // x > y and y < x should be normalized to the same expression!
    let cmp_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| {
            matches!(
                inst,
                Instruction::Binary {
                    op: MirBinaryOp::Lt | MirBinaryOp::Gt,
                    ..
                }
            )
        })
        .count();

    assert_eq!(
        cmp_count, 1,
        "Expected exactly 1 comparison instruction after dual normalization, found {}",
        cmp_count
    );
}

#[test]
fn test_gvn_cross_block_dominating_subexpression() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn cross_block(x: i64, y: i64, flag: bool) -> i64 {
            let base: i64 = x * y;
            let mut result: i64 = 0;
            if flag {
                let duplicate: i64 = x * y;
                result = base + duplicate;
            } else {
                result = base;
            }
            return result;
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
        true,
        true,
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true, // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "cross_block").unwrap();
    // x * y in then_block is dominated by x * y in entry block, so only 1 Mul should exist
    let mul_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Binary { op: MirBinaryOp::Mul, .. }))
        .count();

    assert_eq!(
        mul_count, 1,
        "Expected exactly 1 Mul instruction across dominating blocks, found {}",
        mul_count
    );
}

#[test]
fn test_gvn_non_dominating_branches_not_improperly_shared() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn non_dom(flag: bool, x: i64, y: i64) -> i64 {
            let mut res: i64 = 0;
            if flag {
                let a: i64 = x + y;
                res = a;
            } else {
                let b: i64 = x + y;
                res = b * 2;
            }
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
        true,
        true,
        true,
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true, // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "non_dom").unwrap();
    // Neither branch dominates the other; without hoisting, both branches compute x + y
    let add_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Binary { op: MirBinaryOp::Add, .. }))
        .count();

    assert_eq!(
        add_count, 2,
        "Expected 2 Add instructions since branches do not dominate each other, found {}",
        add_count
    );
}

#[test]
fn test_gvn_phi_simplification_identical_inputs() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn phi_simple(flag: bool, val: i64) -> i64 {
            let k: i64 = val;
            let mut x: i64 = 0;
            if flag {
                x = k;
            } else {
                x = k;
            }
            return x;
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
        true,
        true,
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true, // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "phi_simple").unwrap();
    // The phi with identical inputs from both branches should be simplified away
    let phi_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Phi { .. }))
        .count();

    assert_eq!(
        phi_count, 0,
        "Expected 0 Phi nodes after GVN phi simplification, found {}",
        phi_count
    );
}

#[test]
fn test_e2e_differential_execution_gvn_vs_no_gvn() {
    let temp_dir = std::env::temp_dir().join(format!("vanta_gvn_test_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).unwrap();

    let src_path = temp_dir.join("test_gvn.va");
    let exe_no_gvn = temp_dir.join("test_no_gvn");
    let exe_gvn = temp_dir.join("test_gvn");

    let source_code = r#"
    pub fn complex_calc(x: i64, y: i64) -> i64 {
        let a: i64 = x * 3 + y * 7;
        let b: i64 = y * 7 + x * 3;
        let c: i64 = a ^ b;
        let d: i64 = (x * 3 + y * 7) * 2;
        return a + b + c + d;
    }

    pub fn main() -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < 100 {
            sum = sum + complex_calc(i, i + 5);
            i = i + 1;
        }
        print_i64(sum);
        return 0;
    }
    "#;

    fs::write(&src_path, source_code).unwrap();

    // 1. Compile with --no-gvn
    let opts_no_gvn = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_gvn.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: true,
        enable_gvn: false,
        ..Default::default()
    };
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts_no_gvn).is_ok());

    // 2. Compile with GVN enabled
    let opts_gvn = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_gvn.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: true,
        enable_gvn: true,
        ..Default::default()
    };
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts_gvn).is_ok());

    // 3. Execute both binaries and compare output
    let out_no_gvn = Command::new(&exe_no_gvn).output().expect("Failed to execute no_gvn binary");
    let out_gvn = Command::new(&exe_gvn).output().expect("Failed to execute gvn binary");

    assert_eq!(out_no_gvn.status.code(), Some(0));
    assert_eq!(out_gvn.status.code(), Some(0));
    assert_eq!(out_no_gvn.stdout, out_gvn.stdout);
    assert!(!out_gvn.stdout.is_empty(), "Expected non-empty output");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_gvn_unary_expression_elimination() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn unary_calc(x: i64) -> i64 {
            let a: i64 = -x;
            let b: i64 = -x;
            return a + b;
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
        true,
        true,
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true, // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "unary_calc").unwrap();
    let neg_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::Unary { .. }))
        .count();

    assert_eq!(
        neg_count, 1,
        "Expected exactly 1 Unary negation instruction after GVN, found {}",
        neg_count
    );
}

#[test]
fn test_gvn_field_addr_elimination() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub struct Point {
            pub x: i64,
            pub y: i64,
        }

        pub fn get_x_twice(pt: Point) -> i64 {
            let x1: i64 = pt.x;
            let x2: i64 = pt.x;
            return x1 + x2;
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
        true,
        true,
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true, // loop_peel
        true, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "get_x_twice").unwrap();
    let field_addr_count = func
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter(|inst| matches!(inst, Instruction::FieldAddr { .. }))
        .count();

    assert_eq!(
        field_addr_count, 1,
        "Expected exactly 1 FieldAddr instruction after GVN, found {}",
        field_addr_count
    );
}

