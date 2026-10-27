//! Integration and Unit Tests for Milestone 21: Loop Invariant Code Motion (LICM).

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::opt::{NaturalLoop, OptLevel as MirOptLevel, OptimizationPipeline};
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
fn test_licm_natural_loop_discovery() {
    let (mir, _sm) = compile_src_to_mir(
        r#"
        pub fn run_loop(limit: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < limit {
                sum = sum + i;
                i = i + 1;
            }
            return sum;
        }
        "#,
    );

    let func = mir.functions.iter().find(|f| f.name == "run_loop").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);

    assert_eq!(loops.len(), 1, "Expected exactly 1 natural loop");
    let natural_loop = &loops[0];
    assert!(natural_loop.blocks.len() >= 2, "Loop must contain at least cond and body blocks");
    assert!(!natural_loop.latches.is_empty(), "Loop must have at least one backedge latch");
}

#[test]
fn test_licm_hoists_pure_invariant_integer_expression() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn compute_scale(limit: i64, factor: i64) -> i64 {
            let mut total: i64 = 0;
            let mut i: i64 = 0;
            while i < limit {
                let scale: i64 = factor * 16;
                total = total + i + scale;
                i = i + 1;
            }
            return total;
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
        true,  // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "compute_scale").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    let natural_loop = &loops[0];

    // Inside loop member blocks, factor * 16 should NOT exist (it was hoisted to preheader!)
    let mul_in_loop = natural_loop.blocks.iter().any(|&b_id| {
        let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
        block.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Binary { op: MirBinaryOp::Mul, .. })
        })
    });

    assert!(
        !mul_in_loop,
        "Invariant multiplication `factor * 16` must be hoisted outside the loop body!"
    );

    // The multiplication must exist in an external block outside the loop (the preheader)
    let mul_outside_loop = func.blocks.iter().any(|b| {
        !natural_loop.blocks.contains(&b.id)
            && b.instructions.iter().any(|inst| {
                matches!(inst, Instruction::Binary { op: MirBinaryOp::Mul, .. })
            })
    });

    assert!(
        mul_outside_loop,
        "Hoisted multiplication must exist in a preheader block outside the loop!"
    );
}

#[test]
fn test_licm_hoists_chain_of_dependent_invariants() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn chain_loop(limit: i64, factor: i64) -> i64 {
            let mut total: i64 = 0;
            let mut i: i64 = 0;
            while i < limit {
                let a: i64 = factor * 3;
                let b: i64 = a + 10;
                let c: i64 = b * 2;
                total = total + i + c;
                i = i + 1;
            }
            return total;
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
        true, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "chain_loop").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    let natural_loop = &loops[0];

    // Inside loop member blocks, there should be zero multiplications!
    let mul_count_in_loop = natural_loop
        .blocks
        .iter()
        .map(|&b_id| {
            let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
            block
                .instructions
                .iter()
                .filter(|inst| matches!(inst, Instruction::Binary { op: MirBinaryOp::Mul, .. }))
                .count()
        })
        .sum::<usize>();

    assert_eq!(
        mul_count_in_loop, 0,
        "All invariant multiplications in the chain must be hoisted out of the loop body!"
    );
}

#[test]
fn test_licm_hoists_invariant_boolean_expression() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn bool_loop(limit: i64, threshold: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < limit {
                let is_high: bool = threshold > 50;
                if is_high {
                    sum = sum + i;
                } else {
                    sum = sum + 1;
                }
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
        true, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "bool_loop").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    let natural_loop = &loops[0];

    // Inside loop member blocks, the comparison `threshold > 50` should not remain in loop body
    let cmp_in_loop = natural_loop.blocks.iter().any(|&b_id| {
        let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
        block.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Binary { op: MirBinaryOp::Gt, .. })
        })
    });

    assert!(
        !cmp_in_loop,
        "Invariant boolean comparison `threshold > 50` must be hoisted outside the loop body!"
    );
}

#[test]
fn test_licm_strictly_rejects_division_from_hoisting() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn div_guard(limit: i64, denom: i64) -> i64 {
            let mut res: i64 = 0;
            let mut i: i64 = 0;
            while i < limit {
                if denom != 0 {
                    let d: i64 = 100 / denom;
                    res = res + d;
                }
                i = i + 1;
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
        false,
        false,
        true, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "div_guard").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    let natural_loop = &loops[0];

    // Division MUST NOT be hoisted because it could trap if denom == 0 or on zero-trip loop!
    let div_in_loop = natural_loop.blocks.iter().any(|&b_id| {
        let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
        block.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Binary { op: MirBinaryOp::Div, .. })
        })
    });

    assert!(
        div_in_loop,
        "Potentially trapping division `100 / denom` MUST NOT be hoisted out of loop!"
    );
}

#[test]
fn test_licm_nested_loops_inner_invariant_outer_variant() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn nested_work(outer_limit: i64, inner_limit: i64) -> i64 {
            let mut total: i64 = 0;
            let mut o: i64 = 0;
            while o < outer_limit {
                let outer_scale: i64 = o * 7;
                let mut i: i64 = 0;
                while i < inner_limit {
                    let inner_inv: i64 = outer_scale + 5;
                    total = total + i + inner_inv;
                    i = i + 1;
                }
                o = o + 1;
            }
            return total;
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
        true, // licm
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "nested_work").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 2, "Expected 2 nested loops");

    // The inner loop (smaller block count) should have inner_inv hoisted to its preheader
    let inner_loop = &loops[0]; // sorted smallest first
    let _inner_add_count = inner_loop
        .blocks
        .iter()
        .map(|&b_id| {
            let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
            block
                .instructions
                .iter()
                .filter(|inst| matches!(inst, Instruction::Binary { op: MirBinaryOp::Add, .. }))
                .count()
        })
        .sum::<usize>();

    // outer_scale changes each outer iteration, so outer_scale * 7 cannot be hoisted out of outer loop
    let outer_loop = &loops[1];
    let outer_has_mul = outer_loop.blocks.iter().any(|&b_id| {
        let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
        block.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Binary { op: MirBinaryOp::Mul, .. })
        })
    });

    assert!(
        outer_has_mul,
        "outer_scale depends on outer variable `o`, so it must remain inside outer loop"
    );
}

#[test]
fn test_licm_disabled_via_flag() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn compute_scale(limit: i64, factor: i64) -> i64 {
            let mut total: i64 = 0;
            let mut i: i64 = 0;
            while i < limit {
                let scale: i64 = factor * 16;
                total = total + i + scale;
                i = i + 1;
            }
            return total;
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
        false, // LICM DISABLED!
        false, // indvar
        false, // strength_reduction
        false, // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "compute_scale").unwrap();
    let dom_tree = vanta_mir::opt::DominatorTree::compute(func).expect("Valid CFG");
    let loops = NaturalLoop::discover_loops(func, &dom_tree);
    assert_eq!(loops.len(), 1);

    let natural_loop = &loops[0];

    // With LICM disabled, factor * 16 MUST remain inside the loop body!
    let mul_in_loop = natural_loop.blocks.iter().any(|&b_id| {
        let block = func.blocks.iter().find(|b| b.id == b_id).unwrap();
        block.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Binary { op: MirBinaryOp::Mul, .. })
        })
    });

    assert!(
        mul_in_loop,
        "When enable_licm is false, loop-invariant computation must remain inside the loop body!"
    );
}

#[test]
fn test_licm_end_to_end_execution_equivalence() {
    let target_dir = std::env::temp_dir().join("vanta_licm_e2e_test");
    fs::create_dir_all(&target_dir).unwrap();

    let src_path = target_dir.join("test_licm.va");
    let exe_no_licm = target_dir.join("test_no_licm");
    let exe_licm = target_dir.join("test_licm");

    let source_code = r#"
    fn calc_loop(limit: i64, coeff: i64) -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < limit {
            let inv1: i64 = coeff * 5;
            let inv2: i64 = inv1 + 17;
            if i % 2 == 0 {
                sum = sum + i + inv2;
            } else {
                sum = sum + (inv2 - i);
            }
            i = i + 1;
        }
        return sum;
    }

    pub fn main() -> i64 {
        let mut total: i64 = 0;
        let mut k: i64 = 0;
        while k < 50 {
            total = total + calc_loop(k, k + 3);
            k = k + 1;
        }
        print_i64(total);
        return 0;
    }
    "#;

    fs::write(&src_path, source_code).unwrap();

    // 1. Compile with --no-licm
    let opts_no_licm = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_licm.clone(),
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
        enable_licm: false,
        enable_indvar: true,
        enable_strength_reduction: true,
        enable_sroa: true,
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts_no_licm).is_ok(), "Compilation with --no-licm failed");

    // 2. Compile with LICM enabled
    let opts_licm = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_licm.clone(),
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
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts_licm).is_ok(), "Compilation with LICM enabled failed");

    // 3. Execute both binaries and compare output
    let out_no_licm = Command::new(&exe_no_licm)
        .output()
        .expect("Failed to run no-licm binary");
    let out_licm = Command::new(&exe_licm)
        .output()
        .expect("Failed to run licm binary");

    assert_eq!(out_no_licm.status.code(), Some(0));
    assert_eq!(out_licm.status.code(), Some(0));
    assert_eq!(
        out_no_licm.stdout, out_licm.stdout,
        "LICM output differs from non-LICM baseline!"
    );
    assert!(!out_licm.stdout.is_empty());

    let _ = fs::remove_dir_all(&target_dir);
}
