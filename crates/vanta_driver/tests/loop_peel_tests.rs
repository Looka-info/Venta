//! Dedicated verification test suite for Loop Peeling in Vanta.

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::ir::*;
use vanta_mir::opt::OptimizationPipeline;
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
fn test_loop_peel_with_inlining_and_strength_reduction() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn compute(iters: i64) -> i64 {
            let mut acc: i64 = 0;
            let mut i: i64 = 0;
            while i < iters {
                acc = acc + (i * 3 + 1);
                i = i + 1;
            }
            return acc;
        }

        pub fn main() -> i64 {
            let res = compute(20);
            return res;
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
        false, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors(), "Diagnostics had errors: {:?}", diags);

    let func = mir.functions.iter().find(|f| f.name == "compute").unwrap();
    let has_peeled = func.blocks.iter().any(|b| b.name.contains("peeled"));
    assert!(has_peeled, "Loop in compute must be peeled!");
}

#[test]
fn test_loop_peel_peels_first_iteration() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn test_peel(n: i64) -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < n {
                sum = sum + i + 1;
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
        true, // inlining
        true, // ipa
        true, // licm
        true, // indvar
        true, // strength_reduction
        true, // sroa
        true,  // loop_peel
        false, // loop_unroll
        true, // instruction_schedule
        &mut diags,
    );
    assert!(!diags.has_errors(), "Diagnostics had errors: {:?}", diags);

    let func = mir.functions.iter().find(|f| f.name == "test_peel").unwrap();
    for b in &func.blocks {
        println!("Block {:?} ({}) -> term {:?}", b.id, b.name, b.terminator);
        for inst in &b.instructions {
            println!("   {:?}", inst);
        }
    }
    // After peeling, there should be peeled blocks outside the loop header
    let has_peeled_blocks = func.blocks.iter().any(|b| b.name.contains("peeled"));
    assert!(has_peeled_blocks, "Loop body must have a peeled first iteration outside the main loop!");
}

#[test]
fn test_loop_peel_preserves_zero_trip_condition() {
    let temp_dir = std::env::temp_dir().join(format!("vanta_peel_zero_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    let src_path = temp_dir.join("test_zero.va");
    let exe_path = temp_dir.join("test_zero");

    let src = r#"
    fn compute(n: i64) -> i64 {
        let mut sum: i64 = 100;
        let mut i: i64 = 0;
        while i < n {
            sum = sum + 50;
            i = i + 1;
        }
        return sum;
    }

    fn main() -> i64 {
        let res = compute(0); // ZERO TRIPS!
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
        enable_loop_peel: true, // PEEL ENABLED
        enable_loop_unroll: true,
        enable_instruction_schedule: true,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    assert!(session.compile(&opts).is_ok());

    let out = Command::new(&exe_path).output().expect("Execution failed");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "100", "Zero-trip loop must not execute peeled body!");

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_loop_peel_differential_equivalence() {
    let temp_dir = std::env::temp_dir().join(format!("vanta_peel_diff_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    let src_path = temp_dir.join("test_diff.va");
    let exe_no_peel = temp_dir.join("test_no_peel");
    let exe_peel = temp_dir.join("test_peel");

    let src = r#"
    fn compute(iters: i64) -> i64 {
        let mut acc: i64 = 0;
        let mut i: i64 = 0;
        while i < iters {
            acc = acc + (i * 3 + 1);
            i = i + 1;
        }
        return acc;
    }

    fn main() -> i64 {
        let res = compute(20);
        print_i64(res);
        print_str("\n");
        return 0;
    }
    "#;
    fs::write(&src_path, src).unwrap();

    // 1. Compile WITHOUT peeling
    let opts_no_peel = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_no_peel.clone(),
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
        enable_loop_peel: false, // DISABLED
        enable_loop_unroll: false,
        enable_instruction_schedule: true,
        ..Default::default()
    };
    let mut session1 = CompilerSession::new();
    assert!(session1.compile(&opts_no_peel).is_ok());

    // 2. Compile WITH peeling
    let opts_peel = CompilerOptions {
        input_file: src_path.clone(),
        output_file: exe_peel.clone(),
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
        enable_loop_peel: true, // ENABLED
        enable_loop_unroll: false,
        enable_instruction_schedule: true,
        ..Default::default()
    };
    let mut session2 = CompilerSession::new();
    assert!(session2.compile(&opts_peel).is_ok());

    // Print MIR of compute from session2 if possible or print tokens
    println!("Dumping diff test...");

    let out1 = Command::new(&exe_no_peel).output().unwrap();
    let out2 = Command::new(&exe_peel).output().unwrap();
    println!("out1 stdout: {:?}", String::from_utf8_lossy(&out1.stdout));
    println!("out2 stdout: {:?}", String::from_utf8_lossy(&out2.stdout));
    assert_eq!(out1.stdout, out2.stdout, "Peeled and non-peeled executions must produce identical results!");

    let _ = fs::remove_dir_all(&temp_dir);
}
