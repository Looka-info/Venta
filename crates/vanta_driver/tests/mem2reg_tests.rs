use std::fs;
use std::process::Command;
use tempfile::tempdir;
use vanta_diagnostics::DiagnosticBag;
use vanta_driver::{
    CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, LinkerKind, OptLevel,
    TargetTriple,
};
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

    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };

    // 1. Compile WITHOUT Mem2Reg (stack slot loads/stores preserved)
    let out_no_mem = temp_dir
        .path()
        .join(format!("{}_nomem2reg{}", name, exe_suffix));
    let opts_no_mem = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_no_mem.clone(),
        emit_target: EmitTarget::Exe,
        target: TargetTriple::host(),
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: false,
        linker: match std::env::var("VANTA_LINKER").as_deref() {
            Ok("system") => LinkerKind::System,
            _ => LinkerKind::Internal,
        },
        ..Default::default()
    };
    let mut session_no_mem = CompilerSession::new();
    let res_no_mem = session_no_mem.compile(&opts_no_mem);
    if res_no_mem.is_err() {
        eprintln!(
            "{}",
            session_no_mem.render_diagnostics(DiagnosticFormat::Human)
        );
    }
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
    let stdout_no_mem = String::from_utf8_lossy(&output_no_mem.stdout).replace("\r\n", "\n");
    assert_eq!(
        stdout_no_mem, expected_stdout,
        "No-mem2reg stdout mismatch for {}",
        name
    );

    // 2. Compile WITH Mem2Reg (SSA construction + phi placement + physical register allocation)
    let out_mem = temp_dir
        .path()
        .join(format!("{}_mem2reg{}", name, exe_suffix));
    let opts_mem = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_mem.clone(),
        emit_target: EmitTarget::Exe,
        target: TargetTriple::host(),
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        enable_mem2reg: true,
        linker: match std::env::var("VANTA_LINKER").as_deref() {
            Ok("system") => LinkerKind::System,
            _ => LinkerKind::Internal,
        },
        ..Default::default()
    };
    let mut session_mem = CompilerSession::new();
    let res_mem = session_mem.compile(&opts_mem);
    if res_mem.is_err() {
        eprintln!(
            "{}",
            session_mem.render_diagnostics(DiagnosticFormat::Human)
        );
    }
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
    let stdout_mem = String::from_utf8_lossy(&output_mem.stdout).replace("\r\n", "\n");
    assert_eq!(
        stdout_mem, expected_stdout,
        "Mem2Reg stdout mismatch for {}",
        name
    );

    // Exact parity verification
    assert_eq!(
        stdout_no_mem, stdout_mem,
        "Outputs must be byte-for-byte identical"
    );
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

    let func = mir
        .functions
        .iter_mut()
        .find(|f| f.name == "compute")
        .expect("Find compute fn");
    let initial_slots = func.slots.len();
    assert!(
        initial_slots >= 3,
        "Expected at least 3 local slots (a, b, i)"
    );

    let (changed, stats) = Mem2Reg::run_on_function(func);
    assert!(changed, "Mem2Reg must make changes");
    assert!(
        stats.promoted_slots >= 3,
        "Expected at least 3 slots promoted"
    );
    assert!(
        stats.inserted_phis >= 3,
        "Expected at least 3 phis in loop header"
    );
    assert!(stats.eliminated_loads > 0, "Expected loads eliminated");
    assert!(stats.eliminated_stores > 0, "Expected stores eliminated");
    assert_eq!(
        func.slots.len(),
        0,
        "All eligible slots must be promoted out of func.slots"
    );
}

#[test]
fn test_mem2reg_parameter_promotion_reassignment_branches_loops() {
    let source = r#"
fn param_branch_loop(mut x: i64, mut y: i64, cond: bool) -> i64 {
    if cond {
        x = x + 10;
    } else {
        x = x + 20;
    }

    let mut i: i64 = 0;
    while i < 5 {
        y = y + x;
        x = x + 1;
        i = i + 1;
    }

    return x + y;
}

fn main() -> i64 {
    let r1 = param_branch_loop(5, 10, true);
    let r2 = param_branch_loop(5, 10, false);
    print_i64(r1);
    print_str(" ");
    print_i64(r2);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_param_promotion", source, 0, "115 175\n");
}

#[test]
fn test_regression_1_loop_early_return() {
    let source = r#"
fn find_first_even(limit: i64) -> i64 {
    let mut i: i64 = 1;
    let mut result: i64 = -1;
    while i <= limit {
        if i % 2 == 0 {
            result = i;
            return result;
        }
        i = i + 1;
    }
    return result;
}

fn main() -> i64 {
    let res = find_first_even(10);
    print_i64(res);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("reg_loop_early_return", source, 0, "2\n");
}

#[test]
fn test_regression_2_multiple_return_blocks_different_definitions() {
    let source = r#"
fn branch_return(mode: i64) -> i64 {
    let mut val: i64 = 0;
    if mode == 1 {
        val = 100;
        return val + 1;
    } else if mode == 2 {
        val = 200;
        return val + 2;
    } else {
        val = 300;
        return val + 3;
    }
}

fn main() -> i64 {
    let a = branch_return(1);
    let b = branch_return(2);
    let c = branch_return(3);
    print_i64(a);
    print_str(" ");
    print_i64(b);
    print_str(" ");
    print_i64(c);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("reg_multiple_returns", source, 0, "101 202 303\n");
}

#[test]
fn test_regression_3_conditional_branch_definition_used_after() {
    let source = r#"
fn branch_used_after(x: i64) -> i64 {
    let mut val: i64 = 10;
    if x > 5 {
        val = val + 20;
    } else {
        val = val + 5;
    }
    let res: i64 = val * 2;
    return res;
}

fn main() -> i64 {
    let a = branch_used_after(10);
    let b = branch_used_after(2);
    print_i64(a);
    print_str(" ");
    print_i64(b);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("reg_branch_used_after", source, 0, "60 30\n");
}

#[test]
fn test_regression_4_loop_carried_variable() {
    let source = r#"
fn sum_accumulate(n: i64) -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 1;
    while i <= n {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}

fn main() -> i64 {
    let s = sum_accumulate(10);
    print_i64(s);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("reg_loop_carried", source, 0, "55\n");
}

#[test]
fn test_regression_5_definition_not_dominating_return() {
    let source = r#"
fn not_dominating_return(flag: bool) -> i64 {
    let mut x: i64 = 1;
    if flag {
        x = 99;
    }
    return x;
}

fn main() -> i64 {
    let a = not_dominating_return(true);
    let b = not_dominating_return(false);
    print_i64(a);
    print_str(" ");
    print_i64(b);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("reg_not_dominating_return", source, 0, "99 1\n");
}

#[test]
fn test_regression_6_ascii_eq_ignore_case_ventanext() {
    let source = r#"
fn ascii_eq_ignore_case(a: &str, b: &str) -> bool {
    let len_a = a.len();
    let len_b = b.len();
    if len_a != len_b {
        return false;
    }
    let mut i = 0;
    while i < len_a {
        let mut ch_a: i64 = a[i];
        let mut ch_b: i64 = b[i];
        if ch_a >= 65 && ch_a <= 90 {
            ch_a = ch_a + 32;
        }
        if ch_b >= 65 && ch_b <= 90 {
            ch_b = ch_b + 32;
        }
        if ch_a != ch_b {
            return false;
        }
        i = i + 1;
    }
    return true;
}

fn main() -> i64 {
    let eq1 = ascii_eq_ignore_case("Content-Type", "content-type");
    let eq2 = ascii_eq_ignore_case("GET", "get");
    let neq = ascii_eq_ignore_case("hello", "world");
    if eq1 {
        print_str("eq1:true ");
    } else {
        print_str("eq1:false ");
    }
    if eq2 {
        print_str("eq2:true ");
    } else {
        print_str("eq2:false ");
    }
    if neq {
        print_str("neq:true\n");
    } else {
        print_str("neq:false\n");
    }
    return 0;
}
"#;
    run_differential_test(
        "reg_ascii_eq_ignore_case",
        source,
        0,
        "eq1:true eq2:true neq:false\n",
    );
}

#[test]
fn test_regression_7_normalize_request_path_reduced() {
    let source = r#"
fn normalize_path_reduced(path: &str) -> i64 {
    let len = path.len();
    if len == 0 {
        return 0;
    }
    let mut i = 0;
    let mut dots = 0;
    while i < len {
        let c: i64 = path[i];
        if c == 92 {
            return -1;
        }
        if c == 46 {
            dots = dots + 1;
            if dots > 2 {
                return -2;
            }
        } else {
            dots = 0;
        }
        i = i + 1;
    }
    return dots;
}

fn main() -> i64 {
    let r1 = normalize_path_reduced("/api/v1/user");
    let r2 = normalize_path_reduced("/foo/../bar");
    let r3 = normalize_path_reduced("/foo\\bar");
    print_i64(r1);
    print_str(" ");
    print_i64(r2);
    print_str(" ");
    print_i64(r3);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("reg_normalize_path", source, 0, "0 0 -1\n");
}
