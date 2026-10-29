//! Dedicated Integration and Correctness Test Suite for Scalar Replacement of Aggregates (SROA).

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::ir::MirProgram;
use vanta_mir::opt::{OptLevel as MirOptLevel, OptimizationPipeline};
use vanta_parser::Parser;
use vanta_sema::{SemanticAnalyzer, Type};
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
fn test_sroa_promotes_local_scalar_struct() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        struct Point {
            x: i64,
            y: i64,
        }

        pub fn test_point() -> i64 {
            let mut p = Point { x: 10, y: 20 };
            p.x = p.x + 5;
            p.y = p.y + 15;
            return p.x + p.y;
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
        false, // indvar
        false, // strength_reduction
        true,  // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "test_point")
        .unwrap();

    // The original Type::Struct("Point") slot must NO LONGER exist in func.slots!
    let has_struct_slot = func.slots.iter().any(|s| matches!(s.ty, Type::Struct(_)));
    assert!(
        !has_struct_slot,
        "Struct slot must be decomposed and replaced by scalar field slots!"
    );

    // After Mem2Reg ran on the field slots, all slots are promoted to SSA registers!
    // Thus func.slots should now be completely empty or contain only non-promotable slots.
    assert_eq!(
        func.slots.len(),
        0,
        "All scalarized struct fields should be promoted into SSA registers by Mem2Reg!"
    );
}

#[test]
fn test_sroa_promotes_3_field_struct() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        struct Vector3D {
            x: i64,
            y: i64,
            z: i64,
        }

        pub fn test_vec() -> i64 {
            let mut v = Vector3D { x: 1, y: 2, z: 3 };
            v.x = v.x + 10;
            v.y = v.y + 20;
            v.z = v.z + 30;
            return v.x + v.y + v.z;
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
        false,
        false,
        true,  // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir.functions.iter().find(|f| f.name == "test_vec").unwrap();
    let has_struct_slot = func.slots.iter().any(|s| matches!(s.ty, Type::Struct(_)));
    assert!(!has_struct_slot, "Vector3D struct slot must be scalarized!");
}

#[test]
fn test_sroa_strictly_rejects_escaping_struct() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        struct Point {
            x: i64,
            y: i64,
        }

        fn external_escape(p: &Point) -> i64 {
            return p.x + p.y;
        }

        pub fn run_escape() -> i64 {
            let p = Point { x: 10, y: 20 };
            return external_escape(&p);
        }
        "#,
    );

    let mut diags = DiagnosticBag::new();
    // Run WITHOUT inlining so that &p genuinely escapes to the Call instruction!
    OptimizationPipeline::optimize_program_with_options(
        &mut mir,
        MirOptLevel::O1,
        true,
        true,
        false, // inlining disabled!
        false,
        false,
        false,
        false,
        true,  // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "run_escape")
        .unwrap();
    let has_struct_slot = func.slots.iter().any(|s| matches!(s.ty, Type::Struct(_)));
    assert!(
        has_struct_slot,
        "Escaping struct &p passed to function call MUST NOT be scalarized!"
    );
}

#[test]
fn test_sroa_strictly_rejects_struct_with_drop_fields() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        struct NamedItem {
            id: i64,
            name: String,
        }

        pub fn test_named(s: String) -> i64 {
            let item = NamedItem { id: 42, name: s };
            return item.id;
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
        true,
        true,
        true,  // sroa
        false, // loop_peel
        false, // loop_unroll
        false, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors());

    let func = mir
        .functions
        .iter()
        .find(|f| f.name == "test_named")
        .unwrap();
    let has_struct_slot = func.slots.iter().any(|s| matches!(s.ty, Type::Struct(_)));
    assert!(
        has_struct_slot,
        "Struct containing String field (requires destructor/drop) MUST NOT be scalarized by SROA!"
    );
}

#[test]
fn test_sroa_disabled_via_flag() {
    let src = r#"
    struct Point {
        x: i64,
        y: i64,
    }

    fn main() -> i64 {
        let mut p = Point { x: 123, y: 456 };
        p.x = p.x + 10;
        p.y = p.y + 20;
        let res = p.x + p.y;
        print_i64(res);
        print_str("\n");
        return 0;
    }
    "#;

    let temp_dir = std::env::temp_dir().join(format!("vanta_sroa_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);
    let src_path = temp_dir.join("test_sroa.va");
    let exe_no_sroa = temp_dir.join("bin_no_sroa");
    let exe_sroa = temp_dir.join("bin_sroa");

    fs::write(&src_path, src).unwrap();

    // 1. Compile with --no-sroa
    let opts_no_sroa = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_sroa.clone(),
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
        enable_sroa: false, // DISABLED
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts_no_sroa).is_ok());

    // 2. Compile with SROA enabled
    let opts_sroa = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_sroa.clone(),
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
        enable_sroa: true, // ENABLED
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        ..Default::default()
    };
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts_sroa).is_ok());

    // 3. Both binaries must execute and produce identical output (609)
    let out_no_sroa = Command::new(&exe_no_sroa).output().unwrap();
    let out_sroa = Command::new(&exe_sroa).output().unwrap();

    assert_eq!(out_no_sroa.status.code(), Some(0));
    assert_eq!(out_sroa.status.code(), Some(0));
    assert_eq!(out_no_sroa.stdout, out_sroa.stdout);
    assert_eq!(String::from_utf8_lossy(&out_sroa.stdout).trim(), "609");

    let _ = fs::remove_dir_all(&temp_dir);
}
