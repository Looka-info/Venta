//! Milestone 26: Natural-Loop Alignment Verification Suite.
//!
//! Verifies:
//! 1. Loop header blocks in natural loops receive `MachineInst::Align(16)`.
//! 2. Straight-line functions and non-loop blocks do NOT receive alignment directives.
//! 3. ELF code emitter pads misaligned boundaries up to 16 bytes using optimal multi-byte NOPs:
//!    - 1 byte: 90
//!    - 2 bytes: 66 90
//!    - 3 bytes: 0F 1F 00
//!    - 4 bytes: 0F 1F 40 00
//!    - 5 bytes: 0F 1F 44 00 00
//!    - 6 bytes: 66 0F 1F 44 00 00
//!    - 7 bytes: 0F 1F 80 00 00 00 00
//!    - 8 bytes: 0F 1F 84 00 00 00 00 00

use std::collections::HashMap;
use vanta_codegen_x86_64::encoding::CodeEmitter;
use vanta_codegen_x86_64::frame::FrameLayout;
use vanta_codegen_x86_64::instruction_select::InstructionSelector;
use vanta_codegen_x86_64::machine_ir::MachineInst;
use vanta_codegen_x86_64::target::TargetFeatures;
use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
use vanta_mir::{MirLowerer, MirProgram, MirVerifier};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_to_mir(src: &str) -> MirProgram {
    let mut sm = SourceMap::new();
    let id = sm.add_file("loop_align_test.va".into(), src.into());
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
    let mir = lowerer.lower_program(&program);
    MirVerifier::verify_program(&mir, &mut diags);
    assert!(!diags.has_errors());
    mir
}

#[test]
fn test_loop_header_receives_align_16() {
    let src = r#"
    pub fn loop_func(n: i64) -> i64 {
        let mut sum: i64 = 0;
        let mut i: i64 = 0;
        while i < n {
            sum += i;
            i += 1;
        }
        return sum;
    }

    pub fn no_loop_func(a: i64, b: i64) -> i64 {
        if a > b {
            return a - b;
        } else {
            return b - a;
        }
    }
    "#;

    let mir = compile_to_mir(src);
    let structs = HashMap::new();
    let enums = HashMap::new();

    // 1. `loop_func` contains a natural loop -> header MUST have Align(16)
    let loop_func = mir
        .functions
        .iter()
        .find(|f| f.name == "loop_func")
        .expect("loop_func found");
    let frame_loop = FrameLayout::compute(loop_func, &structs, &enums, None);
    let selector_loop = InstructionSelector::new(loop_func, &frame_loop, &structs, &enums, None);
    let insts_loop = selector_loop.select_instructions();

    let align_count = insts_loop
        .iter()
        .filter(|i| matches!(i, MachineInst::Align(16)))
        .count();
    assert_eq!(
        align_count, 1,
        "Loop function must have exactly one 16-byte alignment directive for its loop header!"
    );

    // 2. `no_loop_func` has branching but no loops -> MUST NOT have Align(16)
    let no_loop_func = mir
        .functions
        .iter()
        .find(|f| f.name == "no_loop_func")
        .expect("no_loop_func found");
    let frame_no_loop = FrameLayout::compute(no_loop_func, &structs, &enums, None);
    let selector_no_loop =
        InstructionSelector::new(no_loop_func, &frame_no_loop, &structs, &enums, None);
    let insts_no_loop = selector_no_loop.select_instructions();

    let no_align_count = insts_no_loop
        .iter()
        .filter(|i| matches!(i, MachineInst::Align(_)))
        .count();
    assert_eq!(
        no_align_count, 0,
        "Non-loop function must NOT receive any loop alignment directives!"
    );
}

#[test]
fn test_multi_byte_nop_emission() {
    let mut bytes = Vec::new();

    // Test exact multi-byte sequences:
    // 1-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 1);
    assert_eq!(bytes, vec![0x90]);

    // 2-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 2);
    assert_eq!(bytes, vec![0x66, 0x90]);

    // 3-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 3);
    assert_eq!(bytes, vec![0x0F, 0x1F, 0x00]);

    // 4-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 4);
    assert_eq!(bytes, vec![0x0F, 0x1F, 0x40, 0x00]);

    // 5-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 5);
    assert_eq!(bytes, vec![0x0F, 0x1F, 0x44, 0x00, 0x00]);

    // 6-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 6);
    assert_eq!(bytes, vec![0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00]);

    // 7-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 7);
    assert_eq!(bytes, vec![0x0F, 0x1F, 0x80, 0x00, 0x00, 0x00, 0x00]);

    // 8-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 8);
    assert_eq!(bytes, vec![0x0F, 0x1F, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00]);

    // Compound padding: 13 bytes = 9-byte NOP + 4-byte NOP
    bytes.clear();
    CodeEmitter::emit_nops(&mut bytes, 13);
    assert_eq!(bytes.len(), 13);
    assert_eq!(
        &bytes[0..9],
        &[0x66, 0x0F, 0x1F, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
    assert_eq!(&bytes[9..13], &[0x0F, 0x1F, 0x40, 0x00]);
}

#[test]
fn test_code_emitter_align_directive() {
    let target = TargetFeatures::default();
    let mut emitter = CodeEmitter::with_target(target);

    // Write 5 dummy bytes
    emitter
        .bytes
        .extend_from_slice(&[0x48, 0x89, 0xC3, 0x90, 0x90]);
    assert_eq!(emitter.bytes.len(), 5);

    // Align to 16
    emitter
        .encode_instruction_checked(&MachineInst::Align(16), &target)
        .expect("Align must encode");

    // Total length must now be 16 (padding = 11 bytes: 9-byte NOP + 2-byte NOP)
    assert_eq!(emitter.bytes.len(), 16);
    assert_eq!(emitter.bytes.len() % 16, 0);

    assert_eq!(
        &emitter.bytes[5..14],
        &[0x66, 0x0F, 0x1F, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
    assert_eq!(&emitter.bytes[14..16], &[0x66, 0x90]);
}
