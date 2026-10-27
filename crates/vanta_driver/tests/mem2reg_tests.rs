use std::fs;
use std::process::Command;
use tempfile::tempdir;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};
use vanta_lexer::Lexer;
use vanta_mir::opt::mem2reg::Mem2Reg;
use vanta_mir::MirLowerer;
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

fn run_differential_test(name: &str, source: &str, expected_exit: i32, expected_stdout: &str) {
    let temp_dir = tempdir().expect("Create temp dir");
    let src_path = temp_dir.path().join(format!("{}.va", name));
    fs::write(&src_path, source).expect("Write source file");

    // 1. Compile WITHOUT Mem2Reg (stack slot loads/stores preserved)
    let out_no_mem = temp_dir.path().join(format!("{}_nomem2reg", name));
    let opts_no_mem = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_no_mem.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: false,
        ..Default::default()
    };
    let mut session_no_mem = CompilerSession::new();
    let res_no_mem = session_no_mem.compile(&opts_no_mem);
    assert!(
        res_no_mem.is_ok(),
        "No-mem2reg compilation failed: {:?}",
        res_no_mem.err()
    );
    let output_no_mem = Command::new(&out_no_mem)
        .output()
        .expect("Execute no-mem2reg binary");
    assert_eq!(
        output_no_mem.status.code(),
        Some(expected_exit),
        "No-mem2reg exit code mismatch for {}",
        name
    );
    let stdout_no_mem = String::from_utf8_lossy(&output_no_mem.stdout).to_string();
    assert_eq!(
        stdout_no_mem, expected_stdout,
        "No-mem2reg stdout mismatch for {}",
        name
    );

    // 2. Compile WITH Mem2Reg (SSA construction + phi placement + physical register allocation)
    let out_mem = temp_dir.path().join(format!("{}_mem2reg", name));
    let opts_mem = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_mem.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: true,
        ..Default::default()
    };
    let mut session_mem = CompilerSession::new();
    let res_mem = session_mem.compile(&opts_mem);
    assert!(
        res_mem.is_ok(),
        "Mem2Reg compilation failed: {:?}",
        res_mem.err()
    );
    let output_mem = Command::new(&out_mem)
        .output()
        .expect("Execute mem2reg binary");
    assert_eq!(
        output_mem.status.code(),
        Some(expected_exit),
        "Mem2Reg exit code mismatch for {}",
        name
    );
    let stdout_mem = String::from_utf8_lossy(&output_mem.stdout).to_string();
    assert_eq!(
        stdout_mem, expected_stdout,
        "Mem2Reg stdout mismatch for {}",
        name
    );

    // Exact parity verification
    assert_eq!(stdout_no_mem, stdout_mem, "Outputs must be byte-for-byte identical");
}

#[test]
fn test_differential_loop_accumulation() {
    let source = r#"
fn main() -> i64 {
    let mut a: i64 = 1;
    let mut b: i64 = 2;
    let mut c: i64 = 3;
    let mut d: i64 = 4;
    let mut i: i64 = 0;
    while i < 10000 {
        a = (a + b) ^ (c + d);
        b = (b + 3) ^ a;
        c = (c + 5) ^ b;
        d = (d + 7) ^ c;
        i = i + 1;
    }
    let total = a + b + c + d;
    print_i64(total);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_loop_accum", source, 0, "-3359397228231239542\n");
}

#[test]
fn test_differential_if_else_phi_merges() {
    let source = r#"
fn choose_and_accum(cond1: bool, cond2: bool, count: i64) -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < count {
        let mut delta: i64 = 0;
        if cond1 {
            delta = i * 2;
        } else {
            delta = i * 3;
        }

        if cond2 {
            sum = sum + delta;
        } else {
            sum = sum - delta;
        }
        i = i + 1;
    }
    return sum;
}

fn main() -> i64 {
    let r1 = choose_and_accum(true, true, 10);
    let r2 = choose_and_accum(false, true, 10);
    let r3 = choose_and_accum(true, false, 10);
    let r4 = choose_and_accum(false, false, 10);
    print_i64(r1);
    print_str(" ");
    print_i64(r2);
    print_str(" ");
    print_i64(r3);
    print_str(" ");
    print_i64(r4);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_if_else", source, 0, "90 135 -90 -135\n");
}

#[test]
fn test_differential_if_without_else_critical_edge_split() {
    let source = r#"
fn compute_partial(cond: bool, initial: i64) -> i64 {
    let mut x: i64 = initial;
    if cond {
        x = x * 7 + 1;
    }
    return x + 10;
}

fn main() -> i64 {
    let val_true = compute_partial(true, 5);
    let val_false = compute_partial(false, 5);
    print_i64(val_true);
    print_str(" ");
    print_i64(val_false);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_if_no_else", source, 0, "46 15\n");
}

#[test]
fn test_differential_nested_loops() {
    let source = r#"
fn main() -> i64 {
    let mut total: i64 = 0;
    let mut i: i64 = 0;
    while i < 15 {
        let mut inner_total: i64 = 0;
        let mut j: i64 = 0;
        while j < 10 {
            inner_total = inner_total + (i + 1) * (j + 1);
            j = j + 1;
        }
        total = total + inner_total;
        i = i + 1;
    }
    print_i64(total);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_nested_loops", source, 0, "6600\n");
}

#[test]
fn test_differential_parallel_copy_hazard_swap() {
    let source = r#"
fn main() -> i64 {
    let mut x: i64 = 42;
    let mut y: i64 = 99;
    let mut i: i64 = 0;
    while i < 7 {
        let temp: i64 = x;
        x = y;
        y = temp;
        i = i + 1;
    }
    print_i64(x);
    print_str(" ");
    print_i64(y);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_swap", source, 0, "99 42\n");
}

#[test]
fn test_differential_conservative_fallback_reference_mutation() {
    let source = r#"
fn helper(val: &mut i64) {
    *val = *val + 50;
}

fn main() -> i64 {
    let mut ordinary: i64 = 10;
    let mut referenced: i64 = 20;

    ordinary = ordinary + 5;
    helper(&mut referenced);
    let total = ordinary + referenced;

    print_i64(total);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_ref_mutation", source, 0, "85\n");
}

#[test]
fn test_mem2reg_promotes_all_eligible_slots_in_mir() {
    let source = r#"
fn compute() -> i64 {
    let mut a: i64 = 1;
    let mut b: i64 = 2;
    let mut i: i64 = 0;
    while i < 50 {
        a = a + b;
        b = b + 1;
        i = i + 1;
    }
    return a;
}
"#;
    let mut sm = SourceMap::new();
    let id = sm.add_file("test.va".into(), source.into());
    let file = sm.get_file(id).unwrap();

    let mut diags = DiagnosticBag::new();
    let mut lexer = Lexer::new(&file);
    let tokens = lexer.tokenize(&mut diags);
    assert!(!diags.has_errors());

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program(&mut diags);
    assert!(!diags.has_errors());

    let sema = SemanticAnalyzer::new();
    let sema_ctx = sema.analyze(&program, &mut diags);
    assert!(!diags.has_errors());

    let mut lowerer = MirLowerer::new(&sema_ctx);
    let mut mir = lowerer.lower_program(&program);

    let func = mir.functions.iter_mut().find(|f| f.name == "compute").expect("Find compute fn");
    let initial_slots = func.slots.len();
    assert!(initial_slots >= 3, "Expected at least 3 local slots (a, b, i)");

    let (changed, stats) = Mem2Reg::run_on_function(func);
    assert!(changed, "Mem2Reg must make changes");
    assert!(stats.promoted_slots >= 3, "Expected at least 3 slots promoted");
    assert!(stats.inserted_phis >= 3, "Expected at least 3 phis in loop header");
    assert!(stats.eliminated_loads > 0, "Expected loads eliminated");
    assert!(stats.eliminated_stores > 0, "Expected stores eliminated");
    assert_eq!(func.slots.len(), 0, "All eligible slots must be promoted out of func.slots");
}
