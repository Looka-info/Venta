//! Milestone 24: Safe Tail-Call Optimization (TCO) Tests.

use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_mir::opt::tail_call::TailCall;
use vanta_lexer::Lexer;
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_to_mir(src: &str) -> (vanta_mir::MirProgram, SourceMap) {
    let mut sm = SourceMap::new();
    let file_id = sm.add_file("tco_test.va".to_string(), src.to_string());
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

fn run_executable_test(
    name: &str,
    src: &str,
    expected_exit_code: i32,
    enable_tail_calls: bool,
) {
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
        enable_inline: false, // inlining disabled to test pure TCO
        enable_ipa_constprop: false,
        enable_licm: true,
        enable_indvar: true,
        enable_strength_reduction: true,
        enable_sroa: true,
        enable_loop_peel: true,
        enable_loop_unroll: true,
        enable_instruction_schedule: true,
        enable_global_dce: true,
        enable_tail_calls,
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
fn test_tco_mir_recognition_and_transformation() {
    let (mut mir, _sm) = compile_to_mir(
        r#"
        fn sum_tail(n: i64, acc: i64) -> i64 {
            if n <= 0 {
                return acc;
            }
            return sum_tail(n - 1, acc + n);
        }

        pub fn main() -> i64 {
            return sum_tail(10, 0);
        }
        "#,
    );

    let (changed, stats) = TailCall::run_on_program(&mut mir);
    assert!(changed, "Tail-call pass should recognize self-tail call");
    assert_eq!(stats.self_tail_calls_eliminated, 1);

    let sum_fn = mir.functions.iter().find(|f| f.name.contains("sum_tail")).unwrap();
    // After transformation, sum_fn has loop header and branches back instead of recursive call
    let has_recursive_call = sum_fn.blocks.iter().any(|b| {
        b.instructions.iter().any(|inst| match inst {
            vanta_mir::Instruction::Call { callee, .. } => callee.contains("sum_tail"),
            _ => false,
        })
    });
    assert!(!has_recursive_call, "Self-recursive call must be converted to loop branch");
}

#[test]
fn test_tco_deep_recursion_does_not_overflow_stack() {
    // 500,000 recursive frames would overflow a standard 8MB stack (which holds ~100k frames).
    // With O(1) stack TCO, this executes safely and returns the accumulator.
    run_executable_test(
        "tco_deep_stack",
        r#"
        fn count_down(n: i64, acc: i64) -> i64 {
            if n <= 0 {
                return acc;
            }
            return count_down(n - 1, acc + 1);
        }

        pub fn main() -> i64 {
            let res: i64 = count_down(500000, 0);
            if res == 500000 {
                return 42;
            }
            return 1;
        }
        "#,
        42,
        true,
    );
}

#[test]
fn test_tco_tail_fibonacci_multiple_arguments() {
    run_executable_test(
        "tco_tail_fib",
        r#"
        fn fib_tail(n: i64, a: i64, b: i64) -> i64 {
            if n == 0 {
                return a;
            }
            if n == 1 {
                return b;
            }
            return fib_tail(n - 1, b, a + b);
        }

        pub fn main() -> i64 {
            // fib(10) = 55
            return fib_tail(10, 0, 1);
        }
        "#,
        55,
        true,
    );
}

#[test]
fn test_tco_sibling_tail_call() {
    run_executable_test(
        "tco_sibling",
        r#"
        fn target(x: i64, y: i64) -> i64 {
            return x + y;
        }

        fn intermediate(a: i64, b: i64) -> i64 {
            return target(a * 2, b + 2);
        }

        pub fn main() -> i64 {
            // 20*2 + 1+2 = 40 + 3 = 43
            return intermediate(20, 1);
        }
        "#,
        43,
        true,
    );
}

#[test]
fn test_tco_rejects_non_tail_call() {
    let (mut mir, _sm) = compile_to_mir(
        r#"
        fn non_tail(n: i64) -> i64 {
            if n <= 1 {
                return 1;
            }
            // Addition happens AFTER the call; not in tail position!
            return non_tail(n - 1) + n;
        }

        pub fn main() -> i64 {
            return non_tail(5);
        }
        "#,
    );

    let (changed, stats) = TailCall::run_on_program(&mut mir);
    assert!(!changed, "Non-tail call must NOT be transformed");
    assert_eq!(stats.self_tail_calls_eliminated, 0);
}

#[test]
fn test_tco_disabled_flag_differential() {
    let src = r#"
        fn add_tail(n: i64, acc: i64) -> i64 {
            if n <= 0 {
                return acc;
            }
            return add_tail(n - 1, acc + 2);
        }

        pub fn main() -> i64 {
            return add_tail(10, 0);
        }
    "#;

    // Both with TCO enabled and disabled must produce identical result (20)
    run_executable_test("tco_enabled", src, 20, true);
    run_executable_test("tco_disabled", src, 20, false);
}
