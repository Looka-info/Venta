//! Milestone 26: Native Arithmetic Correctness & Hardware Trap Verification Suite.
//!
//! Verifies:
//! 1. Native execution of signed division and remainder across boundary inputs
//!    (0, 1, -1, 7, -7, 100, -100, i64::MAX, i64::MAX - 1, i64::MIN, i64::MIN + 1)
//!    and divisors (1, 2, 4, 8, 16, 32, 64, 1024, 3, 5, -1, -2, -4).
//! 2. Preservation of hardware arithmetic traps (SIGFPE) on:
//!    - Division by zero (`x / 0`, `x % 0`)
//!    - Overflow trap on `i64::MIN / -1` and `i64::MIN % -1`
//! 3. Differential equivalence between unoptimized (-O0) and optimized (-O1) builds.

use std::fs;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use vanta_codegen_x86_64::machine_ir::MachineInst;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::{MirLowerer, MirVerifier};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn compile_and_run(src: &str, opt: OptLevel, name: &str) -> std::process::Output {
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
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&opts);
    assert!(
        res.is_ok(),
        "Compilation failed for {} (opt: {:?}): {:?}",
        name,
        opt,
        res.err()
    );

    Command::new(&exe_path)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run binary {}: {}", name, e))
}

#[test]
fn test_native_division_and_remainder_constant_powers_of_two() {
    let src = r#"
    pub fn test_div_rem_constants() -> i64 {
        // Boundary inputs:
        // 0, 1, -1, 7, -7, 100, -100, i64::MAX, i64::MAX - 1, i64::MIN, i64::MIN + 1
        let max: i64 = 9223372036854775807;
        let max_minus_1: i64 = 9223372036854775806;
        let min: i64 = -9223372036854775807 - 1;
        let min_plus_1: i64 = -9223372036854775807;

        // Verify div by 2
        if 0 / 2 != 0 { return 1; }
        if 1 / 2 != 0 { return 2; }
        if -1 / 2 != 0 { return 3; }
        if 7 / 2 != 3 { return 4; }
        if -7 / 2 != -3 { return 5; }
        if 100 / 2 != 50 { return 6; }
        if -100 / 2 != -50 { return 7; }
        if max / 2 != 4611686018427387903 { return 8; }
        if max_minus_1 / 2 != 4611686018427387903 { return 9; }
        if min / 2 != -4611686018427387904 { return 10; }
        if min_plus_1 / 2 != -4611686018427387903 { return 11; }

        // Verify rem by 2
        if 0 % 2 != 0 { return 12; }
        if 1 % 2 != 1 { return 13; }
        if -1 % 2 != -1 { return 14; }
        if 7 % 2 != 1 { return 15; }
        if -7 % 2 != -1 { return 16; }
        if 100 % 2 != 0 { return 17; }
        if -100 % 2 != 0 { return 18; }
        if max % 2 != 1 { return 19; }
        if max_minus_1 % 2 != 0 { return 20; }
        if min % 2 != 0 { return 21; }
        if min_plus_1 % 2 != -1 { return 22; }

        // Verify div & rem by 16
        if 100 / 16 != 6 { return 23; }
        if -100 / 16 != -6 { return 24; }
        if 100 % 16 != 4 { return 25; }
        if -100 % 16 != -4 { return 26; }
        if max / 16 != 576460752303423487 { return 27; }
        if max % 16 != 15 { return 28; }
        if min / 16 != -576460752303423488 { return 29; }
        if min % 16 != 0 { return 30; }

        // Verify div & rem by 1024
        if 2048 / 1024 != 2 { return 31; }
        if -2048 / 1024 != -2 { return 32; }
        if 2050 % 1024 != 2 { return 33; }
        if -2050 % 1024 != -2 { return 34; }

        // Verify div & rem by 1
        if 100 / 1 != 100 { return 35; }
        if -100 / 1 != -100 { return 36; }
        if 100 % 1 != 0 { return 37; }
        if -100 % 1 != 0 { return 38; }

        return 0;
    }

    pub fn main() -> i64 {
        return test_div_rem_constants();
    }
    "#;

    // Test under -O0
    let out_o0 = compile_and_run(src, OptLevel::None, "div_rem_const_o0");
    assert!(
        out_o0.status.success(),
        "O0 failed with exit code: {:?}",
        out_o0.status.code()
    );
    assert_eq!(out_o0.status.code(), Some(0));

    // Test under -O1 (with strength reduction active)
    let out_o1 = compile_and_run(src, OptLevel::O1, "div_rem_const_o1");
    assert!(
        out_o1.status.success(),
        "O1 failed with exit code: {:?}",
        out_o1.status.code()
    );
    assert_eq!(out_o1.status.code(), Some(0));
}

#[test]
fn test_native_division_and_remainder_dynamic_divisors() {
    let src = r#"
    fn dynamic_div(x: i64, d: i64) -> i64 {
        return x / d;
    }

    fn dynamic_rem(x: i64, d: i64) -> i64 {
        return x % d;
    }

    pub fn main() -> i64 {
        // Non-power-of-two divisors: 3, 5
        if dynamic_div(100, 3) != 33 { return 1; }
        if dynamic_rem(100, 3) != 1 { return 2; }
        if dynamic_div(-100, 3) != -33 { return 3; }
        if dynamic_rem(-100, 3) != -1 { return 4; }

        if dynamic_div(100, 5) != 20 { return 5; }
        if dynamic_rem(100, 5) != 0 { return 6; }
        if dynamic_div(-100, 5) != -20 { return 7; }
        if dynamic_rem(-100, 5) != 0 { return 8; }

        // Negative divisors: -2, -4
        if dynamic_div(100, -2) != -50 { return 9; }
        if dynamic_rem(100, -2) != 0 { return 10; }
        if dynamic_div(-100, -2) != 50 { return 11; }
        if dynamic_rem(-100, -2) != 0 { return 12; }
        if dynamic_div(7, -4) != -1 { return 13; }
        if dynamic_rem(7, -4) != 3 { return 14; }
        if dynamic_div(-7, -4) != 1 { return 15; }
        if dynamic_rem(-7, -4) != -3 { return 16; }

        return 0;
    }
    "#;

    let out_o0 = compile_and_run(src, OptLevel::None, "div_rem_dyn_o0");
    assert!(
        out_o0.status.success(),
        "O0 failed with exit code: {:?}",
        out_o0.status.code()
    );

    let out_o1 = compile_and_run(src, OptLevel::O1, "div_rem_dyn_o1");
    assert!(
        out_o1.status.success(),
        "O1 failed with exit code: {:?}",
        out_o1.status.code()
    );
}

#[test]
fn test_hardware_trap_division_by_zero() {
    let src = r#"
    pub fn main() -> i64 {
        let zero: i64 = 0;
        let x: i64 = 42;
        return x / zero;
    }
    "#;

    let out = compile_and_run(src, OptLevel::O1, "trap_div_zero");
    // Hardware arithmetic trap must terminate abnormally
    assert!(
        !out.status.success(),
        "Division by zero must terminate abnormally!"
    );
    #[cfg(unix)]
    {
        // On Linux, x86-64 hardware division by zero raises SIGFPE (signal 8)
        assert_eq!(
            out.status.signal(),
            Some(8),
            "Expected SIGFPE (signal 8) for division by zero"
        );
    }
}

#[test]
fn test_hardware_trap_modulo_by_zero() {
    let src = r#"
    pub fn main() -> i64 {
        let zero: i64 = 0;
        let x: i64 = 42;
        return x % zero;
    }
    "#;

    let out = compile_and_run(src, OptLevel::O1, "trap_rem_zero");
    assert!(
        !out.status.success(),
        "Modulo by zero must terminate abnormally!"
    );
    #[cfg(unix)]
    {
        assert_eq!(
            out.status.signal(),
            Some(8),
            "Expected SIGFPE (signal 8) for modulo by zero"
        );
    }
}

#[test]
fn test_hardware_trap_i64_min_div_neg_one() {
    let src = r#"
    pub fn main() -> i64 {
        let min: i64 = -9223372036854775807 - 1;
        let neg1: i64 = -1;
        return min / neg1;
    }
    "#;

    let out = compile_and_run(src, OptLevel::O1, "trap_min_div_neg1");
    // i64::MIN / -1 overflows signed 64-bit representation, raising #DE / SIGFPE on x86-64
    assert!(
        !out.status.success(),
        "i64::MIN / -1 must terminate abnormally!"
    );
    #[cfg(unix)]
    {
        assert_eq!(
            out.status.signal(),
            Some(8),
            "Expected SIGFPE (signal 8) for i64::MIN / -1"
        );
    }
}

#[test]
fn test_hardware_trap_i64_min_rem_neg_one() {
    let src = r#"
    pub fn main() -> i64 {
        let min: i64 = -9223372036854775807 - 1;
        let neg1: i64 = -1;
        return min % neg1;
    }
    "#;

    let out = compile_and_run(src, OptLevel::O1, "trap_min_rem_neg1");
    assert!(
        !out.status.success(),
        "i64::MIN % -1 must terminate abnormally!"
    );
    #[cfg(unix)]
    {
        assert_eq!(
            out.status.signal(),
            Some(8),
            "Expected SIGFPE (signal 8) for i64::MIN % -1"
        );
    }
}

#[test]
fn test_strength_reduction_mir_and_machine_code_inspection() {
    let src = r#"
    pub fn div_pow2(x: i64) -> i64 {
        return x / 8;
    }

    pub fn rem_pow2(x: i64) -> i64 {
        return x % 8;
    }

    pub fn div_neg1(x: i64) -> i64 {
        return x / -1;
    }

    pub fn rem_neg1(x: i64) -> i64 {
        return x % -1;
    }

    pub fn div_non_pow(x: i64) -> i64 {
        return x / 7;
    }
    "#;

    let mut sm = SourceMap::new();
    let id = sm.add_file("arith_inspect.va".into(), src.into());
    let file = sm.get_file(id).unwrap();
    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&file);
    let tokens = lexer.tokenize(&mut diags);
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program(&mut diags);
    let sema = SemanticAnalyzer::new();
    let ctx = sema.analyze(&program, &mut diags);
    assert!(!diags.has_errors());

    let mut lowerer = MirLowerer::new(&ctx);
    let mir = lowerer.lower_program(&program);
    MirVerifier::verify_program(&mir, &mut diags);
    assert!(!diags.has_errors());

    for func in &mir.functions {
        let structs = std::collections::HashMap::new();
        let enums = std::collections::HashMap::new();
        let frame = vanta_codegen_x86_64::frame::FrameLayout::compute(func, &structs, &enums, None);
        let selector = vanta_codegen_x86_64::instruction_select::InstructionSelector::new(
            func, &frame, &structs, &enums, None,
        );
        let insts = selector.select_instructions();

        let has_idiv = insts.iter().any(|i| matches!(i, MachineInst::Idiv(_)));
        let has_sar = insts.iter().any(|i| matches!(i, MachineInst::Sar { .. }));

        if func.name == "div_pow2" || func.name == "rem_pow2" {
            assert!(
                !has_idiv,
                "Function {} must NOT use IDIV (must use strength reduction)!",
                func.name
            );
            assert!(
                has_sar,
                "Function {} must use SAR instruction for strength reduction!",
                func.name
            );
        } else if func.name == "div_neg1" || func.name == "rem_neg1" {
            assert!(
                has_idiv,
                "Function {} must retain IDIV to preserve i64::MIN trap semantics!",
                func.name
            );
        } else if func.name == "div_non_pow" {
            assert!(
                has_idiv,
                "Function {} with non-power divisor must use IDIV!",
                func.name
            );
        }
    }
}
