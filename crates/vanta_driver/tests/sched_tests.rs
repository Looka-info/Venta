//! Dedicated verification test suite for Dependency-Aware Instruction Scheduling.

use std::fs;
use std::process::Command;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::ir::*;
use vanta_mir::opt::instruction_schedule::InstructionScheduler;
use vanta_mir::verify::MirVerifier;
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
fn test_sched_reorders_independent_arithmetic_to_hide_latency() {
    // In this block, a high-latency Div instruction can be scheduled early
    // before independent additions/bitwise operations.
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn test_latency(a: i64, b: i64, c: i64, d: i64) -> i64 {
            let x = a + 1;
            let y = b / 2; // high latency division!
            let z = c + 3;
            let w = d + 4;
            return x + y + z + w;
        }
        "#,
    );

    let func = mir
        .functions
        .iter_mut()
        .find(|f| f.name == "test_latency")
        .unwrap();
    let (changed, stats) = InstructionScheduler::run_on_function(func);
    assert!(
        changed,
        "Independent operations must be reordered according to critical path height!"
    );
    assert!(stats.instructions_reordered > 0);

    // Verify SSA invariants remain valid
    let mut diags = vanta_diagnostics::DiagnosticBag::new();
    MirVerifier::verify_program(&mir, &mut diags);
    assert!(!diags.has_errors());
}

#[test]
fn test_sched_preserves_raw_data_dependencies() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn test_raw(a: i64) -> i64 {
            let b = a + 10;
            let c = b * 2;
            let d = c + 5;
            let e = d ^ 7;
            return e;
        }
        "#,
    );

    let func = mir
        .functions
        .iter_mut()
        .find(|f| f.name == "test_raw")
        .unwrap();
    let (_changed, _) = InstructionScheduler::run_on_function(func);

    // Verify SSA definition-before-use across all blocks
    let mut diags = DiagnosticBag::new();
    MirVerifier::verify_program(&mir, &mut diags);
    assert!(
        !diags.has_errors(),
        "MIR Verifier must pass without errors: {:?}",
        diags
    );
}

#[test]
fn test_sched_preserves_memory_effects_and_call_barriers() {
    let (mut mir, _sm) = compile_src_to_mir(
        r#"
        pub fn side_effect() -> i64 {
            return 42;
        }

        pub fn test_barrier(mut x: i64) -> i64 {
            x = x + 1;
            let y = side_effect(); // Call barrier!
            x = x + y;
            return x;
        }
        "#,
    );

    let func = mir
        .functions
        .iter_mut()
        .find(|f| f.name == "test_barrier")
        .unwrap();
    let (_changed, _) = InstructionScheduler::run_on_function(func);

    // In the entry block, the Store before Call must remain before Call,
    // and Call must remain before subsequent Load/Store.
    let mut call_seen = false;
    let mut store_after_call = false;

    for block in &func.blocks {
        for inst in &block.instructions {
            match inst {
                Instruction::Call { callee, .. } if callee == "side_effect" => {
                    call_seen = true;
                }
                Instruction::Store { .. } => {
                    if call_seen {
                        store_after_call = true;
                    }
                }
                _ => {}
            }
        }
    }

    assert!(call_seen);
    assert!(store_after_call);
}

#[test]
fn test_sched_end_to_end_execution() {
    let temp_dir = std::env::temp_dir().join(format!("vanta_sched_e2e_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    let src_path = temp_dir.join("test_sched.va");
    let exe_path = temp_dir.join("test_sched");

    let src = r#"
    fn compute(a: i64, b: i64, c: i64) -> i64 {
        let x = a * 7;
        let y = b + 13;
        let z = c * 3;
        let d = (x ^ y) + z;
        return d;
    }

    fn main() -> i64 {
        let res = compute(10, 20, 30);
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
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: true, // SCHEDULE ENABLED
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    if let Err(e) = session.compile(&opts) {
        panic!(
            "Compilation with scheduler enabled failed: {}\nDiagnostics:\n{}",
            e,
            session.render_diagnostics(DiagnosticFormat::Human)
        );
    }

    let out = Command::new(&exe_path).output().expect("Execution failed");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "193"); // (10*7 ^ 20+13) + 30*3 = (70 ^ 33) + 90 = 103 + 90 = 193

    let _ = fs::remove_dir_all(&temp_dir);
}
