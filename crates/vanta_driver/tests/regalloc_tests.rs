use std::fs;
use std::process::Command;
use tempfile::tempdir;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

fn run_differential_test(name: &str, source: &str, expected_exit: i32, expected_stdout: &str) {
    let temp_dir = tempdir().expect("Create temp dir");
    let src_path = temp_dir.path().join(format!("{}.va", name));
    fs::write(&src_path, source).expect("Write source file");

    // 1. Compile WITHOUT register allocation (legacy stack spill mode)
    let out_no_reg = temp_dir.path().join(format!("{}_noreg", name));
    let opts_no_reg = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_no_reg.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: false,
        ..Default::default()
    };
    let mut session_no_reg = CompilerSession::new();
    let res_no_reg = session_no_reg.compile(&opts_no_reg);
    assert!(
        res_no_reg.is_ok(),
        "No-regalloc compilation failed: {:?}",
        res_no_reg.err()
    );
    let output_no_reg = Command::new(&out_no_reg)
        .output()
        .expect("Execute no-regalloc binary");
    assert_eq!(
        output_no_reg.status.code(),
        Some(expected_exit),
        "No-regalloc exit code mismatch for {}",
        name
    );
    let stdout_no_reg = String::from_utf8_lossy(&output_no_reg.stdout).to_string();
    assert_eq!(
        stdout_no_reg, expected_stdout,
        "No-regalloc stdout mismatch for {}",
        name
    );

    // 2. Compile WITH custom physical register allocation
    let out_reg = temp_dir.path().join(format!("{}_reg", name));
    let opts_reg = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_reg.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        runtime_path: None,
        enable_reg_alloc: true,
        ..Default::default()
    };
    let mut session_reg = CompilerSession::new();
    let res_reg = session_reg.compile(&opts_reg);
    assert!(
        res_reg.is_ok(),
        "Regalloc compilation failed: {:?}",
        res_reg.err()
    );
    let output_reg = Command::new(&out_reg)
        .output()
        .expect("Execute regalloc binary");
    assert_eq!(
        output_reg.status.code(),
        Some(expected_exit),
        "Regalloc exit code mismatch for {}",
        name
    );
    let stdout_reg = String::from_utf8_lossy(&output_reg.stdout).to_string();
    assert_eq!(
        stdout_reg, expected_stdout,
        "Regalloc stdout mismatch for {}",
        name
    );

    // 3. Strict differential equivalence
    assert_eq!(
        output_no_reg.status.code(),
        output_reg.status.code(),
        "Exit codes must match between no-regalloc and regalloc for {}",
        name
    );
    assert_eq!(
        stdout_no_reg, stdout_reg,
        "Outputs must be 100% byte-for-byte identical between no-regalloc and regalloc for {}",
        name
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
    while i < 100000 {
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
    run_differential_test("diff_loop_accum", source, 0, "-8610759238673993206\n");
}

#[test]
fn test_differential_arith_tree() {
    let source = r#"
fn eval_tree(x: i64, y: i64) -> i64 {
    let a = (x + y) * (x - y) + (x * 3) - (y * 5);
    let b = (a + x) * 2 - (a - y) * 3 + (x + y);
    let c = (a + b) + (a * 2) - (b * 2) + 7;
    return c;
}

fn main() -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < 10000 {
        sum = sum + eval_tree(i % 100, (i + 3) % 100);
        i = i + 1;
    }
    print_i64(sum);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_arith_tree", source, 0, "-7355000\n");
}

#[test]
fn test_differential_high_register_pressure_spilling() {
    let source = r#"
fn compute_pressure(x: i64) -> i64 {
    let v1 = x + 1;
    let v2 = x + 2;
    let v3 = x + 3;
    let v4 = x + 4;
    let v5 = x + 5;
    let v6 = x + 6;
    let v7 = x + 7;
    let v8 = x + 8;
    let v9 = x + 9;
    let v10 = x + 10;
    let res = (v1 * v2) - (v3 * v4) + (v5 * v6) - (v7 * v8) + (v9 * v10)
        + (v1 + v2 + v3 + v4 + v5 + v6 + v7 + v8 + v9 + v10);
    return res;
}

fn main() -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < 10000 {
        sum = sum + compute_pressure(i % 50);
        i = i + 1;
    }
    print_i64(sum);
    print_str("\n");
    return 0;
}
"#;
    run_differential_test("diff_reg_pressure", source, 0, "14320000\n");
}

#[test]
fn test_differential_callee_saved_preservation_across_calls() {
    let source = r#"
fn clobber_work(x: i64) -> i64 {
    let a = x * 3 + 1;
    let b = a * 5 + 2;
    let c = b * 7 + 3;
    return c % 97;
}

fn caller(x: i64) -> i64 {
    let v1 = x + 10;
    let v2 = x + 20;
    let v3 = x + 30;
    let v4 = x + 40;
    // Call helper which executes and returns
    let c1 = clobber_work(v1);
    let c2 = clobber_work(v2);
    // All of v1..v4 must still be perfectly intact!
    let total = v1 + v2 + v3 + v4 + c1 + c2;
    return total;
}

fn main() -> i64 {
    let res = caller(5);
    print_i64(res);
    print_str("\n");
    return 0;
}
"#;
    // x = 5
    // v1 = 15, v2 = 25, v3 = 35, v4 = 45 -> sum = 120
    // clobber_work(15): a = 46, b = 232, c = 1627 -> 1627 % 97 = 75
    // clobber_work(25): a = 76, b = 382, c = 2677 -> 2677 % 97 = 58
    // total = 120 + 75 + 58 = 253
    run_differential_test("diff_callee_saved", source, 0, "253\n");
}
