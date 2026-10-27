//! Milestone 24: Global Dead Code Elimination (Global DCE) Tests.

use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_mir::opt::global_dce::GlobalDce;
use vanta_mir::opt::OptimizationPipeline;
use vanta_mir::{MirBinaryOp, Instruction};
use vanta_lexer::Lexer;
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_to_mir(src: &str) -> (vanta_mir::MirProgram, SourceMap) {
    let mut sm = SourceMap::new();
    let file_id = sm.add_file("test.va".to_string(), src.to_string());
    let file = sm.get_file(file_id).unwrap();

    let mut diags = vanta_diagnostics::DiagnosticBag::new();
    let mut lexer = Lexer::new(&file);
    let tokens = lexer.tokenize(&mut diags);

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program(&mut diags);

    let sema = SemanticAnalyzer::new();
    let sema_ctx = sema.analyze(&program, &mut diags);

    let mut lowerer = vanta_mir::MirLowerer::new(&sema_ctx);
    let mir = lowerer.lower_program(&program);
    (mir, sm)
}

fn run_executable_test(name: &str, src: &str, expected_exit_code: i32, enable_global_dce: bool) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", name));
    let out_exe = temp_dir.path().join(name);
    fs::write(&src_path, src).unwrap();

    let opts = CompilerOptions {
        input_file: src_path,
        output_file: out_exe.clone(),
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
        enable_loop_peel: true,
        enable_loop_unroll: true,
        enable_instruction_schedule: true,
        enable_global_dce,
        enable_tail_calls: true,
        enable_block_layout: true,
        enable_code_align: true,
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&opts);
    assert!(res.is_ok(), "Compilation failed: {:?}", res.err());

    let status = Command::new(&out_exe).status().expect("Execute binary");
    assert_eq!(
        status.code(),
        Some(expected_exit_code),
        "Exit code mismatch for {}",
        name
    );
}

#[test]
fn test_global_dce_eliminates_unreachable_function() {
    let (mut mir, _sm) = compile_to_mir(
        r#"
        fn dead_helper() -> i64 {
            return 999;
        }

        fn alive_helper() -> i64 {
            return 42;
        }

        pub fn main() -> i64 {
            return alive_helper();
        }
        "#,
    );

    let initial_count = mir.functions.len();
    assert!(mir.functions.iter().any(|f| f.name.contains("dead_helper")));

    let (changed, stats) = GlobalDce::run_on_program(&mut mir, true);
    assert!(changed);
    assert_eq!(stats.functions_eliminated, 1);
    assert_eq!(mir.functions.len(), initial_count - 1);
    assert!(!mir.functions.iter().any(|f| f.name.contains("dead_helper")));
    assert!(mir.functions.iter().any(|f| f.name.contains("alive_helper")));
    assert!(mir.functions.iter().any(|f| f.name == "main"));
}

#[test]
fn test_global_dce_preserves_reachable_call_chain() {
    let (mut mir, _sm) = compile_to_mir(
        r#"
        fn level3() -> i64 {
            return 10;
        }

        fn level2() -> i64 {
            return level3() + 20;
        }

        fn level1() -> i64 {
            return level2() + 12;
        }

        pub fn main() -> i64 {
            return level1();
        }
        "#,
    );

    let (_changed, stats) = GlobalDce::run_on_program(&mut mir, true);
    assert_eq!(stats.functions_eliminated, 0);
    assert!(mir.functions.iter().any(|f| f.name.contains("level1")));
    assert!(mir.functions.iter().any(|f| f.name.contains("level2")));
    assert!(mir.functions.iter().any(|f| f.name.contains("level3")));
    assert!(mir.functions.iter().any(|f| f.name == "main"));
}

#[test]
fn test_global_dce_disabled_via_flag_preserves_dead_functions() {
    let (mut mir, _sm) = compile_to_mir(
        r#"
        fn unused_function() -> i64 {
            return 123;
        }

        pub fn main() -> i64 {
            return 42;
        }
        "#,
    );

    let mut diags = vanta_diagnostics::DiagnosticBag::new();
    OptimizationPipeline::optimize_program_full(
        &mut mir,
        OptLevel::O1,
        true, true, false, false, false, false, false, false, false, false, false,
        false, // enable_global_dce: false
        true,  // enable_tail_calls
        true,  // is_executable
        &mut diags,
    );

    assert!(mir.functions.iter().any(|f| f.name.contains("unused_function")),
        "Dead function must NOT be removed when global DCE is disabled!");
}

#[test]
fn test_global_dce_eliminates_unreachable_basic_blocks() {
    let (mut mir, _sm) = compile_to_mir(
        r#"
        pub fn main() -> i64 {
            let mut x: i64 = 10;
            if true {
                x = 42;
            } else {
                let mut dead: i64 = 999;
                dead += 1;
                x = dead;
            }
            return x;
        }
        "#,
    );

    let mut diags = vanta_diagnostics::DiagnosticBag::new();
    OptimizationPipeline::optimize_program_full(
        &mut mir,
        OptLevel::O1,
        true, true, true, true, true, true, true, true, true, true, true,
        true, true, true,
        &mut diags,
    );

    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_dead_inst = main_fn.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| matches!(inst, Instruction::ConstInt { val: 999, .. }))
    });
    assert!(!has_dead_inst, "Unreachable else block and dead 999 computation must be pruned!");
    assert_eq!(main_fn.blocks.len(), 3, "Only entry, then, and merge blocks should remain");
}

#[test]
fn test_global_dce_preserves_potential_traps_div_rem() {
    let (mut mir, _sm) = compile_to_mir(
        r#"
        pub fn main() -> i64 {
            let divisor: i64 = 0;
            // Unused division, but may trap!
            let _unused: i64 = 100 / divisor;
            return 0;
        }
        "#,
    );

    let mut diags = vanta_diagnostics::DiagnosticBag::new();
    // Run optimization without constant folding zero divisor
    OptimizationPipeline::optimize_program_full(
        &mut mir,
        OptLevel::O1,
        false, false, false, false, false, false, false, false, false, false, false,
        true, true, true,
        &mut diags,
    );

    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_div = main_fn.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Binary { op: MirBinaryOp::Div, .. })
        })
    });
    assert!(has_div, "Potentially trapping division must NOT be eliminated by DCE even if result is unused!");
}

#[test]
fn test_global_dce_e2e_executable() {
    run_executable_test(
        "dce_exec_test",
        r#"
        fn uncalled_worker() -> i64 {
            let mut sum: i64 = 0;
            let mut i: i64 = 0;
            while i < 1000 {
                sum += i;
                i += 1;
            }
            return sum;
        }

        fn called_worker(x: i64) -> i64 {
            return x * 2;
        }

        pub fn main() -> i64 {
            let a: i64 = 21;
            return called_worker(a);
        }
        "#,
        42,
        true,
    );
}
