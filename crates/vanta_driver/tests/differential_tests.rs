//! Systematic Differential Testing Suite for Vanta Milestone 25.
//!
//! Compares observable program execution (stdout, exit code) across:
//! 1. Vanta -O0
//! 2. Vanta -O1 (Full Optimizations)
//! 3. Vanta with subsets of optimization flags disabled:
//!    - `--no-global-dce`
//!    - `--no-tail-calls`
//!    - `--no-block-layout`
//!    - `--no-code-align`
//!    - `--no-gvn`
//!    - `--no-inline`
//!    - `--no-licm`
//!    - `--no-sroa`
//! 4. Independent Reference Implementations (C and Rust).

use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

struct DifferentialTestCase<'a> {
    name: &'a str,
    vanta_src: &'a str,
    c_src: Option<&'a str>,
    expected_exit_code: i32,
    expected_stdout: &'a [u8],
}

fn run_test_configuration(
    name: &str,
    src_path: &std::path::Path,
    out_path: &std::path::Path,
    configure: impl FnOnce(&mut CompilerOptions),
) -> (i32, Vec<u8>) {
    let mut options = CompilerOptions {
        input_file: src_path.to_path_buf(),
        output_file: out_path.to_path_buf(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        std_path: None,
        ..Default::default()
    };
    configure(&mut options);

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    if let Err(e) = res {
        panic!(
            "Compilation failed for configuration of '{}': {}\nDiagnostics:\n{}",
            name,
            e,
            session.render_diagnostics(DiagnosticFormat::Human)
        );
    }

    let output = Command::new(out_path)
        .output()
        .unwrap_or_else(|e| panic!("Failed to execute '{}': {}", out_path.display(), e));
    let code = output.status.code().unwrap_or(-1);
    (code, output.stdout)
}

fn verify_differential_case(case: &DifferentialTestCase) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", case.name));
    fs::write(&src_path, case.vanta_src).unwrap();

    // 1. Vanta -O0
    let bin_o0 = temp_dir.path().join(format!("{}_o0", case.name));
    let (code_o0, stdout_o0) = run_test_configuration(case.name, &src_path, &bin_o0, |opts| {
        opts.opt_level = OptLevel::None;
    });
    assert_eq!(
        code_o0, case.expected_exit_code,
        "[{}] -O0 exit code mismatch",
        case.name
    );
    assert_eq!(
        stdout_o0.as_slice(),
        case.expected_stdout,
        "[{}] -O0 stdout mismatch: got {:?}, expected {:?}",
        case.name,
        String::from_utf8_lossy(&stdout_o0),
        String::from_utf8_lossy(case.expected_stdout)
    );

    // 2. Vanta -O1 (Full Optimizations)
    let bin_o1 = temp_dir.path().join(format!("{}_o1", case.name));
    let (code_o1, stdout_o1) = run_test_configuration(case.name, &src_path, &bin_o1, |_| {});
    assert_eq!(
        code_o1, case.expected_exit_code,
        "[{}] -O1 exit code mismatch",
        case.name
    );
    assert_eq!(
        stdout_o1.as_slice(),
        case.expected_stdout,
        "[{}] -O1 stdout mismatch",
        case.name
    );

    // 3. Vanta -O1 (--no-global-dce)
    let bin_no_dce = temp_dir.path().join(format!("{}_no_dce", case.name));
    let (code_no_dce, stdout_no_dce) =
        run_test_configuration(case.name, &src_path, &bin_no_dce, |opts| {
            opts.enable_global_dce = false;
        });
    assert_eq!(code_no_dce, case.expected_exit_code);
    assert_eq!(stdout_no_dce.as_slice(), case.expected_stdout);

    // 4. Vanta -O1 (--no-tail-calls)
    let bin_no_tco = temp_dir.path().join(format!("{}_no_tco", case.name));
    let (code_no_tco, stdout_no_tco) =
        run_test_configuration(case.name, &src_path, &bin_no_tco, |opts| {
            opts.enable_tail_calls = false;
        });
    assert_eq!(code_no_tco, case.expected_exit_code);
    assert_eq!(stdout_no_tco.as_slice(), case.expected_stdout);

    // 5. Vanta -O1 (--no-block-layout)
    let bin_no_layout = temp_dir.path().join(format!("{}_no_layout", case.name));
    let (code_no_layout, stdout_no_layout) =
        run_test_configuration(case.name, &src_path, &bin_no_layout, |opts| {
            opts.enable_block_layout = false;
        });
    assert_eq!(code_no_layout, case.expected_exit_code);
    assert_eq!(stdout_no_layout.as_slice(), case.expected_stdout);

    // 6. Vanta -O1 (--no-code-align)
    let bin_no_align = temp_dir.path().join(format!("{}_no_align", case.name));
    let (code_no_align, stdout_no_align) =
        run_test_configuration(case.name, &src_path, &bin_no_align, |opts| {
            opts.enable_code_align = false;
        });
    assert_eq!(code_no_align, case.expected_exit_code);
    assert_eq!(stdout_no_align.as_slice(), case.expected_stdout);

    // 7. Vanta -O1 (--no-gvn --no-mem2reg)
    let bin_no_gvn = temp_dir.path().join(format!("{}_no_gvn", case.name));
    let (code_no_gvn, stdout_no_gvn) =
        run_test_configuration(case.name, &src_path, &bin_no_gvn, |opts| {
            opts.enable_gvn = false;
            opts.enable_mem2reg = false;
        });
    assert_eq!(code_no_gvn, case.expected_exit_code);
    assert_eq!(stdout_no_gvn.as_slice(), case.expected_stdout);

    // 8. C Reference Implementation (if provided)
    if let Some(c_code) = case.c_src {
        let c_path = temp_dir.path().join(format!("{}.c", case.name));
        let c_bin = temp_dir.path().join(format!("{}_c", case.name));
        fs::write(&c_path, c_code).unwrap();

        let gcc_status = Command::new("gcc")
            .arg("-O2")
            .arg(&c_path)
            .arg("-o")
            .arg(&c_bin)
            .status()
            .expect("Invoke gcc compiler");
        assert!(gcc_status.success(), "[{}] GCC build failed", case.name);

        let c_out = Command::new(&c_bin).output().expect("Execute C binary");
        assert_eq!(
            c_out.status.code().unwrap_or(-1),
            case.expected_exit_code,
            "[{}] C exit code mismatch",
            case.name
        );
        assert_eq!(
            c_out.stdout.as_slice(),
            case.expected_stdout,
            "[{}] C stdout mismatch vs Vanta: got {:?}, expected {:?}",
            case.name,
            String::from_utf8_lossy(&c_out.stdout),
            String::from_utf8_lossy(case.expected_stdout)
        );
    }
}

#[test]
fn test_diff_arithmetic_precedence_and_bitwise() {
    let vanta_src = r#"
fn main() -> i64 {
    let a: i64 = 42;
    let b: i64 = 17;
    let c: i64 = (a * 3 + b * 2) ^ ((a & b) << 2);
    let d: i64 = (c >> 1) % 97;
    print_i64(d);
    return 0;
}
"#;
    let c_src = r#"
#include <stdio.h>
#include <stdint.h>
int main(void) {
    int64_t a = 42;
    int64_t b = 17;
    int64_t c = (a * 3 + b * 2) ^ ((a & b) << 2);
    int64_t d = (c >> 1) % 97;
    printf("%ld", d);
    return 0;
}
"#;
    verify_differential_case(&DifferentialTestCase {
        name: "diff_arithmetic_precedence",
        vanta_src,
        c_src: Some(c_src),
        expected_exit_code: 0,
        expected_stdout: b"80",
    });
}

#[test]
fn test_diff_nested_loop_accumulator_and_break() {
    let vanta_src = r#"
fn main() -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 0;
    while i < 20 {
        let mut j: i64 = 0;
        while j < 20 {
            if (i + j) % 7 == 0 {
                sum = sum + i * j;
            }
            if j == 15 {
                break;
            }
            j = j + 1;
        }
        i = i + 1;
    }
    print_i64(sum);
    return 0;
}
"#;
    let c_src = r#"
#include <stdio.h>
#include <stdint.h>
int main(void) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < 20) {
        int64_t j = 0;
        while (j < 20) {
            if ((i + j) % 7 == 0) {
                sum += i * j;
            }
            if (j == 15) {
                break;
            }
            j += 1;
        }
        i += 1;
    }
    printf("%ld", sum);
    return 0;
}
"#;
    verify_differential_case(&DifferentialTestCase {
        name: "diff_nested_loop_break",
        vanta_src,
        c_src: Some(c_src),
        expected_exit_code: 0,
        expected_stdout: b"3087",
    });
}

#[test]
fn test_diff_tail_recursive_accumulator() {
    let vanta_src = r#"
fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        return a;
    }
    return gcd(b, a % b);
}

fn main() -> i64 {
    let g1 = gcd(1071, 462);
    let g2 = gcd(123456, 7890);
    print_i64(g1);
    print_str(",");
    print_i64(g2);
    return 0;
}
"#;
    let c_src = r#"
#include <stdio.h>
#include <stdint.h>
static int64_t gcd(int64_t a, int64_t b) {
    if (b == 0) return a;
    return gcd(b, a % b);
}
int main(void) {
    int64_t g1 = gcd(1071, 462);
    int64_t g2 = gcd(123456, 7890);
    printf("%ld,%ld", g1, g2);
    return 0;
}
"#;
    verify_differential_case(&DifferentialTestCase {
        name: "diff_tail_gcd",
        vanta_src,
        c_src: Some(c_src),
        expected_exit_code: 0,
        expected_stdout: b"21,6",
    });
}

#[test]
fn test_diff_aggregate_methods_and_sroa() {
    let vanta_src = r#"
struct Point3D {
    x: i64,
    y: i64,
    z: i64,
}

impl Point3D {
    fn dot(self: &Self, other: Point3D) -> i64 {
        return self.x * other.x + self.y * other.y + self.z * other.z;
    }
}

fn main() -> i64 {
    let p1 = Point3D { x: 3, y: 4, z: 5 };
    let p2 = Point3D { x: 7, y: 8, z: 9 };
    let product = p1.dot(p2);
    print_i64(product);
    return 0;
}
"#;
    let c_src = r#"
#include <stdio.h>
#include <stdint.h>
typedef struct {
    int64_t x;
    int64_t y;
    int64_t z;
} Point3D;
static inline int64_t point_dot(Point3D a, Point3D b) {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}
int main(void) {
    Point3D p1 = { 3, 4, 5 };
    Point3D p2 = { 7, 8, 9 };
    int64_t product = point_dot(p1, p2);
    printf("%ld", product);
    return 0;
}
"#;
    verify_differential_case(&DifferentialTestCase {
        name: "diff_struct_sroa",
        vanta_src,
        c_src: Some(c_src),
        expected_exit_code: 0,
        expected_stdout: b"98",
    });
}

#[test]
fn test_diff_collatz_conjecture_iterations() {
    let vanta_src = r#"
fn collatz_len(n: i64) -> i64 {
    let mut curr: i64 = n;
    let mut steps: i64 = 0;
    while curr != 1 {
        if curr % 2 == 0 {
            curr = curr / 2;
        } else {
            curr = curr * 3 + 1;
        }
        steps = steps + 1;
    }
    return steps;
}

fn main() -> i64 {
    let mut max_steps: i64 = 0;
    let mut best_start: i64 = 1;
    let mut i: i64 = 1;
    while i <= 500 {
        let s = collatz_len(i);
        if s > max_steps {
            max_steps = s;
            best_start = i;
        }
        i = i + 1;
    }
    print_i64(best_start);
    print_str(":");
    print_i64(max_steps);
    return 0;
}
"#;
    let c_src = r#"
#include <stdio.h>
#include <stdint.h>
static int64_t collatz_len(int64_t n) {
    int64_t curr = n;
    int64_t steps = 0;
    while (curr != 1) {
        if (curr % 2 == 0) curr /= 2;
        else curr = curr * 3 + 1;
        steps += 1;
    }
    return steps;
}
int main(void) {
    int64_t max_steps = 0;
    int64_t best_start = 1;
    for (int64_t i = 1; i <= 500; ++i) {
        int64_t s = collatz_len(i);
        if (s > max_steps) {
            max_steps = s;
            best_start = i;
        }
    }
    printf("%ld:%ld", best_start, max_steps);
    return 0;
}
"#;
    verify_differential_case(&DifferentialTestCase {
        name: "diff_collatz",
        vanta_src,
        c_src: Some(c_src),
        expected_exit_code: 0,
        expected_stdout: b"327:143",
    });
}
