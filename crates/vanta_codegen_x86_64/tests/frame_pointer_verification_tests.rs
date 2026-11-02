//! Milestone 26: Frame Pointer Omission Verification Suite.
//!
//! Verifies:
//! 1. Leaf functions with zero stack traffic omit the frame pointer.
//! 2. Generated prologue has no `push rbp` or `mov rbp, rsp`.
//! 3. Generated epilogue has no `pop rbp` and is a single `ret`.
//! 4. Non-leaf functions, functions with stack slots, and functions spilling
//!    callee-saved registers preserve the standard RBP frame pointer.

use std::collections::HashMap;
use vanta_codegen_x86_64::frame::FrameLayout;
use vanta_codegen_x86_64::machine_ir::{MachineInst, Operand};
use vanta_codegen_x86_64::reg_alloc::AllocationResult;
use vanta_codegen_x86_64::register::Reg;
use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
use vanta_mir::opt::{OptLevel, OptimizationPipeline};
use vanta_mir::{MirLowerer, MirProgram, MirVerifier};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_to_mir(src: &str) -> MirProgram {
    let mut sm = SourceMap::new();
    let id = sm.add_file("frame_test.va".into(), src.into());
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
fn test_frame_pointer_omitted_for_leaf_functions() {
    let src = r#"
    pub fn get_answer() -> i64 {
        return 42;
    }

    pub fn bitwise_leaf() -> i64 {
        let x: i64 = 100;
        return (x << 3) ^ 42;
    }
    "#;

    let mir = compile_to_mir(src);
    let structs = HashMap::new();
    let enums = HashMap::new();

    for func in &mir.functions {
        let alloc_res = vanta_codegen_x86_64::reg_alloc::RegisterAllocator::allocate(func);
        let frame = FrameLayout::compute(func, &structs, &enums, Some(&alloc_res));
        assert!(
            frame.can_omit_frame_pointer,
            "Function {} is a pure zero-stack leaf and MUST omit frame pointer!",
            func.name
        );
        assert_eq!(frame.total_frame_size, 0);

        let mut prologue = Vec::new();
        frame.emit_prologue(&mut prologue, func, &structs, &enums);
        assert!(
            prologue.is_empty(),
            "Prologue for leaf {} must be completely empty (no push rbp, no mov rbp, rsp)!",
            func.name
        );

        let mut epilogue = Vec::new();
        frame.emit_epilogue(&mut epilogue);
        assert_eq!(
            epilogue,
            vec![MachineInst::Ret],
            "Epilogue for leaf {} must be a single ret instruction!",
            func.name
        );
    }
}

#[test]
fn test_frame_pointer_preserved_for_non_leaf_functions() {
    let src = r#"
    fn callee(x: i64) -> i64 {
        return x + 1;
    }

    pub fn caller_non_leaf(x: i64) -> i64 {
        return callee(x) * 2;
    }
    "#;

    let mir = compile_to_mir(src);
    let structs = HashMap::new();
    let enums = HashMap::new();

    let caller = mir
        .functions
        .iter()
        .find(|f| f.name == "caller_non_leaf")
        .expect("caller_non_leaf found");

    let frame = FrameLayout::compute(caller, &structs, &enums, None);
    assert!(
        !frame.can_omit_frame_pointer,
        "caller_non_leaf makes a function call and MUST NOT omit frame pointer!"
    );

    let mut prologue = Vec::new();
    frame.emit_prologue(&mut prologue, caller, &structs, &enums);
    let has_push_rbp = prologue
        .iter()
        .any(|i| matches!(i, MachineInst::Push(Reg::Rbp)));
    let has_mov_rbp_rsp = prologue.iter().any(|i| {
        matches!(
            i,
            MachineInst::Mov {
                dest: Operand::Reg(Reg::Rbp),
                src: Operand::Reg(Reg::Rsp)
            }
        )
    });
    assert!(has_push_rbp, "Non-leaf prologue must push rbp!");
    assert!(has_mov_rbp_rsp, "Non-leaf prologue must mov rbp, rsp!");

    let mut epilogue = Vec::new();
    frame.emit_epilogue(&mut epilogue);
    let has_pop_rbp = epilogue
        .iter()
        .any(|i| matches!(i, MachineInst::Pop(Reg::Rbp)));
    let has_ret = epilogue.iter().any(|i| matches!(i, MachineInst::Ret));
    assert!(has_pop_rbp, "Non-leaf epilogue must pop rbp!");
    assert!(has_ret, "Non-leaf epilogue must return!");
}

#[test]
fn test_frame_pointer_preserved_for_stack_slots() {
    let src = r#"
    pub fn with_slots() -> i64 {
        let arr: [i64; 4] = [10, 20, 30, 40];
        return arr[2];
    }
    "#;

    let mir = compile_to_mir(src);
    let structs = HashMap::new();
    let enums = HashMap::new();

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "with_slots")
        .expect("with_slots found");

    let frame = FrameLayout::compute(func, &structs, &enums, None);

    assert!(
        !frame.can_omit_frame_pointer,
        "Function with stack slots must preserve frame pointer!"
    );

    let mut prologue = Vec::new();
    frame.emit_prologue(&mut prologue, func, &structs, &enums);
    assert!(
        prologue
            .iter()
            .any(|i| matches!(i, MachineInst::Push(Reg::Rbp))),
        "Prologue must push rbp for stack slots!"
    );
}

#[test]
fn test_frame_pointer_preserved_for_callee_saved_spills() {
    let src = r#"
    pub fn dummy_leaf(x: i64) -> i64 {
        return x + 1;
    }
    "#;

    let mir = compile_to_mir(src);
    let structs = HashMap::new();
    let enums = HashMap::new();

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "dummy_leaf")
        .expect("dummy_leaf found");

    let alloc_res = AllocationResult {
        assignments: HashMap::new(),
        used_callee_saved: vec![Reg::Rbx, Reg::R12],
        num_spills: 0,
    };

    let frame = FrameLayout::compute(func, &structs, &enums, Some(&alloc_res));

    assert!(
        !frame.can_omit_frame_pointer,
        "Function spilling callee-saved registers must preserve frame pointer!"
    );
    assert_eq!(frame.used_callee_saved, vec![Reg::Rbx, Reg::R12]);

    let mut epilogue = Vec::new();
    frame.emit_epilogue(&mut epilogue);
    assert!(
        epilogue
            .iter()
            .any(|i| matches!(i, MachineInst::Pop(Reg::Rbx))),
        "Epilogue must restore callee-saved registers!"
    );
}
