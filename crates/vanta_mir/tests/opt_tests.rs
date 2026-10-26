use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
use vanta_mir::opt::{OptLevel, OptimizationPipeline};
use vanta_mir::{Instruction, MirBinaryOp, MirLowerer, MirVerifier, Terminator};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_to_mir(src: &str) -> (vanta_mir::MirProgram, SourceMap) {
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
fn test_1_folding_constant_arithmetic_expression() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn main() -> i64 {
            let x: i64 = 20 + 22;
            return x;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(
        !diags.has_errors(),
        "Verification error: {}",
        diags.render_all(&sm, false)
    );

    let func = &mir.functions[0];
    // Verify that x was folded to 42
    let has_42 = func.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| match inst {
            Instruction::ConstInt { val: 42, .. } => true,
            _ => false,
        })
    });
    assert!(has_42, "Expected folded constant 42 in MIR: {:#?}", func);
}

#[test]
fn test_2_propagating_constant_through_multiple_assignments() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn main() -> i64 {
            let a: i64 = 10;
            let b: i64 = a;
            let c: i64 = b + 5;
            return c;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let func = &mir.functions[0];
    let has_15 = func.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| match inst {
            Instruction::ConstInt { val: 15, .. } => true,
            _ => false,
        })
    });
    assert!(
        has_15,
        "Expected propagated constant 15 in MIR: {:#?}",
        func
    );
}

#[test]
fn test_3_folding_constant_comparison_and_conditional_branch() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn main() -> i64 {
            let mut res: i64 = 0;
            if 5 > 3 {
                res = 100;
            } else {
                res = 200;
            }
            return res;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let func = &mir.functions[0];
    // The conditional branch should be folded and the else branch eliminated
    let has_200 = func.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| match inst {
            Instruction::ConstInt { val: 200, .. } => true,
            _ => false,
        })
    });
    assert!(
        !has_200,
        "Else branch with constant 200 should be eliminated!"
    );
    // CondBranch should no longer exist
    let has_cond_branch = func
        .blocks
        .iter()
        .any(|b| matches!(b.terminator, Some(Terminator::CondBranch { .. })));
    assert!(
        !has_cond_branch,
        "CondBranch should have been folded into unconditional branch!"
    );
}

#[test]
fn test_4_removing_unused_pure_computation() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn main() -> i64 {
            let unused: i64 = 100 * 2;
            return 42;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let func = &mir.functions[0];
    // Multiplications should be removed
    let has_mul = func.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| {
            matches!(
                inst,
                Instruction::Binary {
                    op: MirBinaryOp::Mul,
                    ..
                }
            )
        })
    });
    assert!(!has_mul, "Unused multiplication should be eliminated!");
}

#[test]
fn test_5_removing_chain_of_computations_that_becomes_dead() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn main() -> i64 {
            let a: i64 = 1;
            let b: i64 = a + 2;
            let c: i64 = b * 3;
            return 99;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let func = &mir.functions[0];
    let has_mul = func.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| {
            matches!(
                inst,
                Instruction::Binary {
                    op: MirBinaryOp::Mul,
                    ..
                }
            )
        })
    });
    assert!(!has_mul, "Dead computation chain must be eliminated!");
}

#[test]
fn test_6_preserving_computations_whose_results_are_used() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn compute(a: i64, b: i64) -> i64 {
            let sum: i64 = a + b;
            return sum;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let func = &mir.functions[0];
    let has_add = func.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| {
            matches!(
                inst,
                Instruction::Binary {
                    op: MirBinaryOp::Add,
                    ..
                }
            )
        })
    });
    assert!(has_add, "Used addition must be preserved!");
}

#[test]
fn test_7_preserving_function_calls_with_possible_side_effects() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        fn effect() {}

        pub fn main() -> i64 {
            effect();
            return 42;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_call = main_fn.blocks.iter().any(|b| {
        b.instructions
            .iter()
            .any(|inst| matches!(inst, Instruction::Call { callee, .. } if callee == "effect"))
    });
    assert!(
        has_call,
        "Call with side effects must be preserved even if result is unused!"
    );
}

#[test]
fn test_8_preserving_stores_and_pointer_memory_effects() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        fn mutate_ptr(p: &mut i64) {
            *p = 50;
        }

        pub fn main() -> i64 {
            let mut val: i64 = 10;
            mutate_ptr(&mut val);
            return val;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let mutate_fn = mir
        .functions
        .iter()
        .find(|f| f.name == "mutate_ptr")
        .unwrap();
    let has_store_indirect = mutate_fn.blocks.iter().any(|b| {
        b.instructions
            .iter()
            .any(|inst| matches!(inst, Instruction::StoreIndirect { .. }))
    });
    assert!(
        has_store_indirect,
        "Indirect pointer write must be preserved!"
    );
}

#[test]
fn test_9_correctly_handling_differing_values_at_merge_points() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn main(cond: bool) -> i64 {
            let mut val: i64 = 0;
            if cond {
                val = 10;
            } else {
                val = 20;
            }
            return val;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let func = &mir.functions[0];
    // Return value cannot be a constant folded 10 or 20 at compile time
    let return_is_constant = func.blocks.iter().any(|b| match b.terminator {
        Some(Terminator::Return(Some(val_id))) => b.instructions.iter().any(|inst| match inst {
            Instruction::ConstInt { dest, .. } if *dest == val_id => true,
            _ => false,
        }),
        _ => false,
    });
    assert!(
        !return_is_constant,
        "Merge point with differing values must not fold to constant!"
    );
}

#[test]
fn test_10_correctly_handling_loops_and_loop_carried_values() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn main() -> i64 {
            let mut i: i64 = 0;
            while i < 10 {
                i += 1;
            }
            return i;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let func = &mir.functions[0];
    // Loop condition must still exist (not folded to dead loop or infinite loop)
    let has_cond_branch = func
        .blocks
        .iter()
        .any(|b| matches!(b.terminator, Some(Terminator::CondBranch { .. })));
    assert!(has_cond_branch, "Loop header condition must be preserved!");
}

#[test]
fn test_11_preserving_division_by_zero_and_overflow() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        pub fn main(z: i64) -> i64 {
            let bad: i64 = 10 / 0;
            return bad;
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    // Verifier should still succeed (MIR is valid)
    assert!(!diags.has_errors(), "{}", diags.render_all(&sm, false));

    let func = &mir.functions[0];
    // Division by zero MUST NOT be folded away to an arbitrary number
    let has_div = func.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| {
            matches!(
                inst,
                Instruction::Binary {
                    op: MirBinaryOp::Div,
                    ..
                }
            )
        })
    });
    assert!(
        has_div,
        "Division by zero must be preserved to maintain runtime trap semantics!"
    );
}

#[test]
fn test_12_verifying_that_optimized_mir_satisfies_all_invariants() {
    let (mut mir, sm) = compile_to_mir(
        r#"
        fn helper(x: i64) -> i64 {
            let a: i64 = x + 10;
            let b: i64 = 5 * 2;
            let mut res: i64 = a + b;
            let mut i: i64 = 0;
            while i < 3 {
                res += i;
                i += 1;
            }
            return res;
        }

        pub fn main() -> i64 {
            return helper(20);
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    OptimizationPipeline::optimize_program(&mut mir, OptLevel::O1, &mut diags);
    assert!(
        !diags.has_errors(),
        "Optimization pipeline verification: {}",
        diags.render_all(&sm, false)
    );

    // Explicit post-verification check
    MirVerifier::verify_program(&mir, &mut diags);
    assert!(
        !diags.has_errors(),
        "Explicit post-verification failed: {}",
        diags.render_all(&sm, false)
    );
}
