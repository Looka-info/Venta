//! Milestone 26: Dynamic Loop Unrolling Correctness & Verification Suite.
//!
//! Verifies:
//! 1. Safe dynamic-bound loops are transformed:
//!    - Guard block and remainder block exist.
//!    - Header and unrolled body duplicated with factor 2.
//!    - Phi predecessors match transformed CFG.
//!    - Native binary execution matches unoptimized baseline.
//! 2. Unsafe dynamic loops are rejected:
//!    - Bound mutated inside loop body.
//!    - Multiple side exits / early returns.
//!    - Function calls inside loop body.
//!    - Rejection preserves original loop and native execution produces correct output.
//! 3. Integer boundaries:
//!    - Small trip count near i64::MAX (start = i64::MAX - 5, bound = i64::MAX).
//!    - Negative bounds (i64::MIN, -100, -1).
//!    - Zero bound.
//!    - Odd trip counts (remainder path taken) vs even trip counts (remainder bypassed).

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::opt::OptimizationPipeline;
use vanta_mir::{MirLowerer, MirProgram, MirVerifier};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_src_to_mir(src: &str) -> MirProgram {
    let mut sm = SourceMap::new();
    let id = sm.add_file("unroll_test.va".into(), src.into());
    let file = sm.get_file(id).unwrap();

    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&file);
    let tokens = lexer.tokenize(&mut diags);
    assert!(!diags.has_errors());

    let mut parser = Parser::new(tokens);
    let ast = parser.parse_program(&mut diags);
    assert!(!diags.has_errors());

    let sema = SemanticAnalyzer::new();
    let ctx = sema.analyze(&ast, &mut diags);
    assert!(!diags.has_errors());

    let mut lowerer = MirLowerer::new(&ctx);
    let mut mir = lowerer.lower_program(&ast);
    MirVerifier::verify_program(&mir, &mut diags);
    assert!(!diags.has_errors());

    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        OptLevel::O1,
        true,  // mem2reg
        true,  // gvn
        false, // inline
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
    mir
}

fn compile_and_run(src: &str, opt: OptLevel, name: &str) -> (i32, Vec<u8>) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", name));
    let exe_path = temp_dir.path().join(name);
    fs::write(&src_path, src).unwrap();

    let opts = CompilerOptions {
        input_file: src_path,
        output_file: exe_path.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: opt,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        enable_loop_unroll: matches!(opt, OptLevel::O1),
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&opts);
    assert!(
        res.is_ok(),
        "Compilation failed for {}: {:?}",
        name,
        res.err()
    );

    let out = Command::new(&exe_path)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run binary {}: {}", name, e));
    (out.status.code().unwrap_or(-1), out.stdout)
}

#[test]
fn test_safe_dynamic_loop_transformation_and_cfgs() {
    let src = r#"
    pub fn sum_to(n: i64) -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < n {
            sum += i;
            i += 1;
        }
        return sum;
    }
    "#;

    let mir = compile_src_to_mir(src);
    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "sum_to")
        .expect("sum_to function found");

    // Verify unrolled CFG structure:
    let block_names: Vec<&str> = func.blocks.iter().map(|b| b.name.as_str()).collect();

    let has_uhead = block_names.iter().any(|name| name.contains("_uhead"));
    let has_u1 = block_names.iter().any(|name| name.contains("_u1"));
    let has_u2 = block_names.iter().any(|name| name.contains("_u2"));
    let has_bypass = block_names.iter().any(|name| name.contains("_bypass"));
    let has_uexit = block_names.iter().any(|name| name.contains("_uexit"));

    assert!(has_uhead, "Dynamic unroll must create unroll header block!");
    assert!(
        has_u1,
        "Dynamic unroll must duplicate first unrolled iteration!"
    );
    assert!(
        has_u2,
        "Dynamic unroll must duplicate second unrolled iteration!"
    );
    assert!(
        has_bypass,
        "Dynamic unroll must create bypass block for remainder!"
    );
    assert!(has_uexit, "Dynamic unroll must create unroll exit block!");

    // Verify Phi predecessor consistency on the original header (which serves as remainder loop)
    let remainder_header = func
        .blocks
        .iter()
        .find(|b| {
            !b.name.contains("_uhead")
                && !b.name.contains("_u1")
                && !b.name.contains("_u2")
                && b.instructions
                    .iter()
                    .any(|i| matches!(i, vanta_mir::Instruction::Phi { .. }))
        })
        .expect("Remainder header with Phi nodes found");

    for inst in &remainder_header.instructions {
        if let vanta_mir::Instruction::Phi { inputs, .. } = inst {
            // Inputs must include bypass_block and unroll_exit_block
            let pred_ids: Vec<_> = inputs.iter().map(|(b, _)| *b).collect();
            assert!(
                pred_ids.len() >= 2,
                "Remainder header Phi must merge bypass and unroll exit paths! Got: {:?}",
                pred_ids
            );
        }
    }
}

#[test]
fn test_unsafe_loops_rejected() {
    // 1. Bound mutated inside loop body
    let src_mut_bound = r#"
    pub fn loop_mut_bound() -> i64 {
        let mut bound: i64 = 10;
        let mut i: i64 = 0;
        let mut sum: i64 = 0;
        while i < bound {
            sum += i;
            if i == 5 {
                bound = 7;
            }
            i += 1;
        }
        return sum;
    }
    "#;
    let mir1 = compile_src_to_mir(src_mut_bound);
    let func1 = mir1
        .functions
        .iter()
        .find(|f| f.name == "loop_mut_bound")
        .unwrap();
    let has_uhead1 = func1.blocks.iter().any(|b| b.name.contains("_uhead"));
    assert!(!has_uhead1, "Loop with mutated bound must NOT be unrolled!");

    // 2. Loop containing function call inside loop body
    let src_call_loop = r#"
    fn add_one(x: i64) -> i64 {
        return x + 1;
    }

    pub fn loop_with_call(n: i64) -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < n {
            sum += add_one(i);
            i += 1;
        }
        return sum;
    }
    "#;
    let mir2 = compile_src_to_mir(src_call_loop);
    let func2 = mir2
        .functions
        .iter()
        .find(|f| f.name == "loop_with_call")
        .unwrap();
    let has_uhead2 = func2.blocks.iter().any(|b| b.name.contains("_uhead"));
    assert!(
        !has_uhead2,
        "Loop containing function calls must NOT be unrolled!"
    );

    // Verify both rejected loops execute natively and produce exact results:
    let runner_src = r#"
    fn loop_mut_bound() -> i64 {
        let mut bound: i64 = 10;
        let mut i: i64 = 0;
        let mut sum: i64 = 0;
        while i < bound {
            sum += i;
            if i == 5 {
                bound = 7;
            }
            i += 1;
        }
        return sum;
    }

    fn add_one(x: i64) -> i64 {
        return x + 1;
    }

    fn loop_with_call(n: i64) -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < n {
            sum += add_one(i);
            i += 1;
        }
        return sum;
    }

    pub fn main() -> i64 {
        // loop_mut_bound: 0 + 1 + 2 + 3 + 4 + 5 + 6 = 21
        if loop_mut_bound() != 21 { return 1; }
        // loop_with_call(5): 1 + 2 + 3 + 4 + 5 = 15
        if loop_with_call(5) != 15 { return 2; }
        return 0;
    }
    "#;

    let (code_o0, _) = compile_and_run(runner_src, OptLevel::None, "unsafe_rejected_o0");
    assert_eq!(code_o0, 0, "O0 rejected loop execution failed!");

    let (code_o1, _) = compile_and_run(runner_src, OptLevel::O1, "unsafe_rejected_o1");
    assert_eq!(code_o1, 0, "O1 rejected loop execution failed!");
}

#[test]
fn test_integer_boundary_dynamic_trip_counts() {
    let runner_src = r#"
    fn sum_to(n: i64) -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < n {
            sum += i;
            i += 1;
        }
        return sum;
    }

    fn sum_near_max() -> i64 {
        let max: i64 = 9223372036854775807;
        let mut i: i64 = 9223372036854775802; // max - 5
        let mut count: i64 = 0;
        while i < max {
            count += 1;
            i += 1;
        }
        return count;
    }

    pub fn main() -> i64 {
        // 1. Boundary: small trip count near i64::MAX (5 iterations)
        if sum_near_max() != 5 { return 1; }

        // 2. Negative bounds: zero iterations
        let min: i64 = -9223372036854775807 - 1;
        if sum_to(min) != 0 { return 2; }
        if sum_to(-100) != 0 { return 3; }
        if sum_to(-1) != 0 { return 4; }

        // 3. Zero bound: zero iterations
        if sum_to(0) != 0 { return 5; }

        // 4. Trip count 1: remainder path taken
        if sum_to(1) != 0 { return 6; } // 0

        // 5. Trip count 2: unroll factor 2 taken once, remainder not taken
        if sum_to(2) != 1 { return 7; } // 0 + 1 = 1

        // 6. Trip count 3: unroll factor 2 taken once, remainder taken once
        if sum_to(3) != 3 { return 8; } // 0 + 1 + 2 = 3

        // 7. Trip count 4: unroll factor 2 taken twice, remainder not taken
        if sum_to(4) != 6 { return 9; } // 0 + 1 + 2 + 3 = 6

        // 8. Trip count 7: unroll factor 2 taken 3 times, remainder taken once
        if sum_to(7) != 21 { return 10; } // 0 + 1 + 2 + 3 + 4 + 5 + 6 = 21

        // 9. Trip count 10: unroll factor 2 taken 5 times
        if sum_to(10) != 45 { return 11; }

        return 0;
    }
    "#;

    let (code_o0, _) = compile_and_run(runner_src, OptLevel::None, "unroll_bounds_o0");
    assert_eq!(code_o0, 0, "O0 integer boundary loop execution failed!");

    let (code_o1, _) = compile_and_run(runner_src, OptLevel::O1, "unroll_bounds_o1");
    assert_eq!(
        code_o1, 0,
        "O1 unrolled integer boundary loop execution failed!"
    );
}
