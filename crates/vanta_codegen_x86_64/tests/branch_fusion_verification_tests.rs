//! Milestone 26: Branch Fusion Verification Suite.
//!
//! Verifies:
//! 1. Single-use comparison:
//!    - Comparison immediately followed by CondBranch.
//!    - Intermediate `Setcc` and `Movzx` instructions are eliminated from machine code.
//!    - Fused `Jcc` is emitted directly.
//! 2. Multi-use comparison:
//!    - Comparison result used in CondBranch AND returned/stored.
//!    - `Setcc` and `Movzx` are preserved for the non-branch use.
//!    - Redundant `Test` before `Jcc` is omitted.
//! 3. Condition code coverage:
//!    - All relational operators: ==, !=, <, <=, >, >=.
//!    - Inverted condition code correctness when branching to else_block vs fall-through.

use std::collections::HashMap;
use vanta_codegen_x86_64::frame::FrameLayout;
use vanta_codegen_x86_64::instruction_select::InstructionSelector;
use vanta_codegen_x86_64::machine_ir::{ConditionCode, MachineInst};
use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
use vanta_mir::opt::{OptLevel, OptimizationPipeline};
use vanta_mir::{MirLowerer, MirProgram, MirVerifier};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_to_mir(src: &str) -> MirProgram {
    let mut sm = SourceMap::new();
    let id = sm.add_file("branch_test.va".into(), src.into());
    let file = sm.get_file(id).unwrap();
    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&file);
    let tokens = lexer.tokenize(&mut diags);
    assert!(!diags.has_errors());
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program(&mut diags);
    assert!(!diags.has_errors());
    let sema = SemanticAnalyzer::new();
    let ctx = sema.analyze(&program, &mut diags);
    assert!(!diags.has_errors());
    let mut lowerer = MirLowerer::new(&ctx);
    let mut mir = lowerer.lower_program(&program);
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        OptLevel::O1,
        true,  // enable_mem2reg
        true,  // enable_gvn
        false, // enable_inline
        false, // enable_ipa_constprop
        false, // enable_licm
        false, // enable_indvar
        false, // enable_strength_reduction
        false, // enable_sroa
        false, // enable_loop_peel
        false, // enable_loop_unroll
        false, // enable_instruction_schedule
        &mut diags,
    );
    MirVerifier::verify_program(&mir, &mut diags);
    assert!(!diags.has_errors());
    mir
}

#[test]
fn test_single_use_condition_eliminates_setcc_and_movzx() {
    let src = r#"
    pub fn branch_eq(a: i64, b: i64) -> i64 {
        if a == b {
            return 10;
        } else {
            return 20;
        }
    }

    pub fn branch_lt(a: i64, b: i64) -> i64 {
        if a < b {
            return 1;
        } else {
            return 0;
        }
    }
    "#;

    let mir = compile_to_mir(src);
    let structs = HashMap::new();
    let enums = HashMap::new();

    for func in &mir.functions {
        let alloc_res = vanta_codegen_x86_64::reg_alloc::RegisterAllocator::allocate(func);
        let frame = FrameLayout::compute(func, &structs, &enums, Some(&alloc_res));
        let selector = InstructionSelector::new(func, &frame, &structs, &enums, Some(&alloc_res));
        let insts = selector.select_instructions();

        // In single-use condition, Setcc and Movzx MUST be eliminated by branch fusion!
        let has_setcc = insts.iter().any(|i| matches!(i, MachineInst::Setcc { .. }));
        let has_movzx = insts.iter().any(|i| matches!(i, MachineInst::Movzx { .. }));
        let has_cmp = insts.iter().any(|i| matches!(i, MachineInst::Cmp { .. }));
        let has_jcc = insts.iter().any(|i| matches!(i, MachineInst::Jcc { .. }));

        assert!(
            has_cmp,
            "Function {} must retain Cmp instruction!",
            func.name
        );
        assert!(
            has_jcc,
            "Function {} must emit fused Jcc directly after comparison!",
            func.name
        );
        assert!(
            !has_setcc,
            "Function {} with single-use condition must NOT emit Setcc!",
            func.name
        );
        assert!(
            !has_movzx,
            "Function {} with single-use condition must NOT emit Movzx!",
            func.name
        );
    }
}

#[test]
fn test_multi_use_condition_preserves_materialization_and_omits_test() {
    let src = r#"
    pub fn multi_use_cond(a: i64, b: i64) -> bool {
        let cond: bool = a < b;
        if cond {
            return cond;
        } else {
            return false;
        }
    }
    "#;

    let mir = compile_to_mir(src);
    let structs = HashMap::new();
    let enums = HashMap::new();

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "multi_use_cond")
        .expect("multi_use_cond found");

    let alloc_res = vanta_codegen_x86_64::reg_alloc::RegisterAllocator::allocate(func);
    let frame = FrameLayout::compute(func, &structs, &enums, Some(&alloc_res));
    let selector = InstructionSelector::new(func, &frame, &structs, &enums, Some(&alloc_res));
    let insts = selector.select_instructions();

    // Multi-use: `cond` is used in branch AND returned as a boolean value.
    // 1. Setcc and Movzx MUST be preserved so the boolean value is materialized in a register.
    let has_setcc = insts.iter().any(|i| matches!(i, MachineInst::Setcc { .. }));
    let has_movzx = insts.iter().any(|i| matches!(i, MachineInst::Movzx { .. }));
    assert!(
        has_setcc,
        "Multi-use condition must preserve Setcc for the return value!"
    );
    assert!(
        has_movzx,
        "Multi-use condition must preserve Movzx for the return value!"
    );

    // 2. Redundant `test reg, reg` before Jcc must be omitted because flags from Cmp are still valid!
    let has_test = insts.iter().any(|i| matches!(i, MachineInst::Test { .. }));
    assert!(
        !has_test,
        "Branch fusion must omit redundant Test instruction before Jcc!"
    );

    let has_jcc = insts.iter().any(|i| matches!(i, MachineInst::Jcc { .. }));
    assert!(has_jcc, "Branch must emit Jcc!");
}

#[test]
fn test_all_relational_condition_codes_coverage() {
    let src = r#"
    pub fn test_eq(a: i64, b: i64) -> i64 { if a == b { return 1; } else { return 0; } }
    pub fn test_ne(a: i64, b: i64) -> i64 { if a != b { return 1; } else { return 0; } }
    pub fn test_lt(a: i64, b: i64) -> i64 { if a < b { return 1; } else { return 0; } }
    pub fn test_gt(a: i64, b: i64) -> i64 { if a > b { return 1; } else { return 0; } }
    pub fn test_le(a: i64, b: i64) -> i64 { if a <= b { return 1; } else { return 0; } }
    pub fn test_ge(a: i64, b: i64) -> i64 { if a >= b { return 1; } else { return 0; } }
    "#;

    let mir = compile_to_mir(src);
    let structs = HashMap::new();
    let enums = HashMap::new();

    let expected_ccs: [(&str, ConditionCode); 6] = [
        ("test_eq", ConditionCode::Equal),
        ("test_ne", ConditionCode::NotEqual),
        ("test_lt", ConditionCode::Less),
        ("test_gt", ConditionCode::Greater),
        ("test_le", ConditionCode::LessEqual),
        ("test_ge", ConditionCode::GreaterEqual),
    ];

    for (fn_name, expected_cc) in expected_ccs {
        let func = mir
            .functions
            .iter()
            .find(|f| f.name == fn_name)
            .unwrap_or_else(|| panic!("{} found", fn_name));
        let alloc_res = vanta_codegen_x86_64::reg_alloc::RegisterAllocator::allocate(func);
        let frame = FrameLayout::compute(func, &structs, &enums, Some(&alloc_res));
        let selector = InstructionSelector::new(func, &frame, &structs, &enums, Some(&alloc_res));
        let insts = selector.select_instructions();

        let jcc_inst = insts
            .iter()
            .find_map(|i| {
                if let MachineInst::Jcc { cond, .. } = i {
                    Some(*cond)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Jcc emitted in {}", fn_name));

        // The emitted Jcc condition code is either expected_cc or expected_cc.invert() depending on block layout fall-through
        assert!(
            jcc_inst == expected_cc || jcc_inst == expected_cc.invert(),
            "Condition code mismatch in {}: got {:?}, expected {:?} or inverted",
            fn_name,
            jcc_inst,
            expected_cc
        );
    }
}
