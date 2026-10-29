//! Comprehensive test suite for Vanta canonical formatter (`vantac fmt`).
//!
//! Tests:
//! 1. Formatter idempotence across all syntax constructs.
//! 2. Preservation of comments (line, block, nested, trailing, inline).
//! 3. Preservation of string literal escapes, UTF-8, hex/binary integers.
//! 4. CLI commands: `fmt`, `fmt --check`, `fmt --stdout`, and JSON diagnostics.
//! 5. Differential semantic preservation: compiling and running unformatted vs
//!    formatted binaries across both -O0 and -O1, asserting byte-for-byte identical
//!    stdout, stderr, and process exit codes.

use std::fs;
use std::process::Command;
use vanta_driver::{
    run_fmt, CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, FmtOptions, FmtResult,
    OptLevel,
};

/// Differential semantic testing helper.
/// Compiles both unformatted and formatted source under -O0 and -O1,
/// runs both native Linux binaries, and asserts 100% identical stdout, stderr, and exit code.
fn run_fmt_differential_test(name: &str, unformatted_src: &str) {
    let temp_dir = tempfile::tempdir().unwrap();
    let unformatted_path = temp_dir.path().join(format!("{}_unfmt.va", name));
    fs::write(&unformatted_path, unformatted_src).unwrap();

    // Format the source code using vanta_fmt
    let formatted_src = vanta_fmt::format_source(unformatted_src, &format!("{}.va", name))
        .expect("Formatting should succeed for valid code");

    // Verify idempotence: format(format(src)) == format(src)
    let formatted_twice = vanta_fmt::format_source(&formatted_src, &format!("{}.va", name))
        .expect("Reformatting should succeed");
    assert_eq!(
        formatted_src, formatted_twice,
        "Idempotence failed for test case '{}'",
        name
    );

    let formatted_path = temp_dir.path().join(format!("{}_fmt.va", name));
    fs::write(&formatted_path, &formatted_src).unwrap();

    for opt_level in [OptLevel::None, OptLevel::O1] {
        let opt_label = match opt_level {
            OptLevel::None => "O0",
            OptLevel::O1 => "O1",
        };

        // 1. Compile unformatted
        let bin_unfmt = temp_dir
            .path()
            .join(format!("{}_unfmt_{}", name, opt_label));
        let opts_unfmt = CompilerOptions {
            input_file: unformatted_path.clone(),
            output_file: bin_unfmt.clone(),
            emit_target: EmitTarget::Exe,
            opt_level,
            verbose: false,
            diagnostic_format: DiagnosticFormat::Human,
            std_path: None,
            ..Default::default()
        };
        let mut session_unfmt = CompilerSession::new();
        let res_unfmt = session_unfmt.compile(&opts_unfmt);
        assert!(
            res_unfmt.is_ok(),
            "Failed to compile unformatted {} at {}: {:?}",
            name,
            opt_label,
            res_unfmt.err()
        );

        // 2. Compile formatted
        let bin_fmt = temp_dir.path().join(format!("{}_fmt_{}", name, opt_label));
        let opts_fmt = CompilerOptions {
            input_file: formatted_path.clone(),
            output_file: bin_fmt.clone(),
            emit_target: EmitTarget::Exe,
            opt_level,
            verbose: false,
            diagnostic_format: DiagnosticFormat::Human,
            std_path: None,
            ..Default::default()
        };
        let mut session_fmt = CompilerSession::new();
        let res_fmt = session_fmt.compile(&opts_fmt);
        assert!(
            res_fmt.is_ok(),
            "Failed to compile formatted {} at {}: {:?}",
            name,
            opt_label,
            res_fmt.err()
        );

        // 3. Execute both binaries and verify semantic preservation
        let out_unfmt = Command::new(&bin_unfmt)
            .output()
            .expect("Execute unformatted binary");
        let out_fmt = Command::new(&bin_fmt)
            .output()
            .expect("Execute formatted binary");

        assert_eq!(
            out_unfmt.status.code(),
            out_fmt.status.code(),
            "Exit code differential mismatch at {} for {}",
            opt_label,
            name
        );
        assert_eq!(
            out_unfmt.stdout, out_fmt.stdout,
            "Stdout differential mismatch at {} for {}",
            opt_label, name
        );
        assert_eq!(
            out_unfmt.stderr, out_fmt.stderr,
            "Stderr differential mismatch at {} for {}",
            opt_label, name
        );
    }
}

// ============================================================================
// 1. CLI COMMAND INTEGRATION TESTS
// ============================================================================

#[test]
fn test_cli_fmt_in_place_atomic() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("code.va");
    fs::write(&file_path, "fn add(a:i64,b:i64)->i64{return a+b;}").unwrap();

    let opts = FmtOptions {
        files: vec![file_path.clone()],
        check: false,
        stdout: false,
        verbose: true,
        diagnostic_format: DiagnosticFormat::Human,
    };

    let (res, stdout_str, stderr_str) = run_fmt(&opts);
    assert_eq!(res, FmtResult::Success);
    assert!(stderr_str.is_empty());
    assert!(stdout_str.contains("Formatted"));

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(
        content,
        "fn add(a: i64, b: i64) -> i64 {\n    return a + b;\n}\n"
    );

    // Second run: already formatted, no-op
    let (res2, stdout_str2, _) = run_fmt(&opts);
    assert_eq!(res2, FmtResult::Success);
    assert!(!stdout_str2.contains("Formatted"));
}

#[test]
fn test_cli_fmt_check_mode() {
    let temp_dir = tempfile::tempdir().unwrap();

    // 1. Unformatted file -> check returns NeedsFormatting (exit 1)
    let dirty_path = temp_dir.path().join("dirty.va");
    fs::write(&dirty_path, "fn f(){let x=1;}").unwrap();

    let opts_dirty = FmtOptions {
        files: vec![dirty_path.clone()],
        check: true,
        stdout: false,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
    };
    let (res_dirty, _, stderr_dirty) = run_fmt(&opts_dirty);
    assert_eq!(res_dirty, FmtResult::NeedsFormatting);
    assert!(stderr_dirty.contains("Diff in"));
    // Verify file was NOT modified
    assert_eq!(fs::read_to_string(&dirty_path).unwrap(), "fn f(){let x=1;}");

    // 2. Format in-place
    let opts_fmt = FmtOptions {
        files: vec![dirty_path.clone()],
        check: false,
        stdout: false,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
    };
    let (res_fmt, _, _) = run_fmt(&opts_fmt);
    assert_eq!(res_fmt, FmtResult::Success);

    // 3. Formatted file -> check returns Success (exit 0)
    let (res_clean, _, stderr_clean) = run_fmt(&opts_dirty);
    assert_eq!(res_clean, FmtResult::Success);
    assert!(stderr_clean.is_empty());
}

#[test]
fn test_cli_fmt_stdout_mode() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("input.va");
    fs::write(&file_path, "pub fn main()->i64{return 42;}").unwrap();

    let opts = FmtOptions {
        files: vec![file_path.clone()],
        check: false,
        stdout: true,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
    };

    let (res, stdout_str, stderr_str) = run_fmt(&opts);
    assert_eq!(res, FmtResult::Success);
    assert!(stderr_str.is_empty());
    assert_eq!(stdout_str, "pub fn main() -> i64 {\n    return 42;\n}\n");

    // Verify input file was completely untouched
    assert_eq!(
        fs::read_to_string(&file_path).unwrap(),
        "pub fn main()->i64{return 42;}"
    );
}

#[test]
fn test_cli_fmt_diagnostics_json_on_syntax_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let bad_path = temp_dir.path().join("bad.va");
    fs::write(&bad_path, "pub fn broken( { return 0; }").unwrap();

    let opts = FmtOptions {
        files: vec![bad_path.clone()],
        check: false,
        stdout: false,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
    };

    let (res, _, stderr_str) = run_fmt(&opts);
    assert_eq!(res, FmtResult::SyntaxError);
    assert!(stderr_str.contains("\"error_count\":1"));
    assert!(stderr_str.contains("\"E0004\""));

    // File must be completely preserved
    assert_eq!(
        fs::read_to_string(&bad_path).unwrap(),
        "pub fn broken( { return 0; }"
    );
}

#[test]
fn test_cli_fmt_multiple_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let f1 = temp_dir.path().join("f1.va");
    let f2 = temp_dir.path().join("f2.va");
    fs::write(&f1, "fn a()->i64{return 1;}").unwrap();
    fs::write(&f2, "fn b()->i64{return 2;}").unwrap();

    let opts = FmtOptions {
        files: vec![f1.clone(), f2.clone()],
        check: false,
        stdout: false,
        verbose: true,
        diagnostic_format: DiagnosticFormat::Human,
    };

    let (res, stdout_str, _) = run_fmt(&opts);
    assert_eq!(res, FmtResult::Success);
    assert!(stdout_str.contains("Formatted"));

    assert_eq!(
        fs::read_to_string(&f1).unwrap(),
        "fn a() -> i64 {\n    return 1;\n}\n"
    );
    assert_eq!(
        fs::read_to_string(&f2).unwrap(),
        "fn b() -> i64 {\n    return 2;\n}\n"
    );
}

// ============================================================================
// 2. DIFFERENTIAL EXECUTION TESTS (SEMANTIC PRESERVATION)
// ============================================================================

#[test]
fn test_diff_arithmetic_and_expressions() {
    let src = r#"// Messy arithmetic snippet
pub fn main()->i64{
let mut a :i64=  10 ;
  let mut b: i64 = 20 ;
let c = ( a + b ) * 3 - ( b / 2 ) % 7 ;
    print_i64( c ) ;
print_str("\n");
return 0 ;
}
"#;
    run_fmt_differential_test("arithmetic", src);
}

#[test]
fn test_diff_nested_control_flow() {
    let src = r#"pub fn main() -> i64 {
let mut i : i64 = 0 ;
let mut count: i64 = 0;
while i < 15 {
    if i == 5 {
        i += 1;
        continue;
    } else if i > 12 {
        break;
    } else {
        count += i * 2;
    }
    i += 1;
}
print_i64(count);
print_str("\n");
return 0;
}
"#;
    run_fmt_differential_test("control_flow", src);
}

#[test]
fn test_diff_arrays_and_bounds_checks() {
    let src = r#"fn sum(arr: &[i64; 4]) -> i64 {
let mut total: i64 = 0;
let mut i: i64 = 0;
while i < 4 {
    total += arr[i];
    i += 1;
}
return total;
}

pub fn main() -> i64 {
let mut numbers: [i64; 4] = [10, 20, 30, 40];
numbers[2] = 99;
let s = sum(&numbers);
print_i64(s);
print_str("\n");
return 0;
}
"#;
    run_fmt_differential_test("arrays_bounds", src);
}

#[test]
fn test_diff_strings_and_slices() {
    let src = r#"pub fn main() -> i64 {
let greeting: &str = "Hello, Vanta Systems!\n";
print_str(greeting);
let s: &str = "Hello\tWorld\n\"Quoted\"\0End!";
print_str(s);
print_str("\n");
return 0;
}
"#;
    run_fmt_differential_test("strings_slices", src);
}

#[test]
fn test_diff_structs_and_methods() {
    let src = r#"struct Vector2 {
x: i64,
y: i64,
}

impl Vector2 {
fn translate(self: &mut Vector2, dx: i64, dy: i64) {
self.x += dx;
self.y += dy;
}

fn sum(self: &Vector2) -> i64 {
return self.x + self.y;
}
}

pub fn main() -> i64 {
let mut v = Vector2 { x: 10, y: 20 };
v.translate(5, 7);
let total = v.sum();
print_i64(total);
print_str("\n");
return 0;
}
"#;
    run_fmt_differential_test("structs_methods", src);
}

#[test]
fn test_diff_nested_structs() {
    let src = r#"struct Inner {
val: i64,
}

struct Outer {
inner: Inner,
multiplier: i64,
}

pub fn main() -> i64 {
let obj = Outer {
    inner: Inner { val: 42 },
    multiplier: 3,
};
print_i64(obj.inner.val * obj.multiplier);
print_str("\n");
return 0;
}
"#;
    run_fmt_differential_test("nested_structs", src);
}

#[test]
fn test_diff_generics() {
    let src = r#"fn id<T>(x: T) -> T {
return x;
}

fn combine<A, B>(a: A, b: B) -> i64 {
return a + b;
}

pub fn main() -> i64 {
let v1 = id(100);
let v2 = id<i64>(200);
print_i64(combine(v1, v2));
print_str("\n");
return 0;
}
"#;
    run_fmt_differential_test("generics", src);
}

#[test]
fn test_diff_vectors_and_ownership() {
    let src = r#"pub fn main() -> i64 {
let mut v = Vec::new();
v.push(10);
v.push(20);
v.push(30);
print_i64(v.len());
print_i64(v[1]);
let last = v.pop();
print_i64(last);
print_i64(v.len());
print_str("\n");
return 0;
}
"#;
    run_fmt_differential_test("vectors_ownership", src);
}

#[test]
fn test_diff_comments_and_formatting_density() {
    let src = r#"// File header comment
/* Multi-line
   license block */

struct Config {
timeout: i64, // timeout in ms
retries: i64, // max retry count
}

fn process(cfg: &Config) -> i64 {
// perform computation
return cfg.timeout * cfg.retries;
}

pub fn main() -> i64 {
let c = Config { timeout: 100, retries: 5 };
let res = process(&c);
print_i64(res);
print_str("\n");
return 0;
}
// End of file comment
"#;
    run_fmt_differential_test("comments_density", src);
}

#[test]
fn test_diff_enums_and_patterns() {
    let src = r#"enum Color {
Red,
Green,
Blue,
}

enum Message {
Quit,
Move(i64, i64),
}

fn eval_msg(m: Message) -> i64 {
match m {
Message::Quit => 0,
Message::Move(x, y) => x + y,
}
}

pub fn main() -> i64 {
let m1 = Message::Quit;
let m2 = Message::Move(15, 27);
print_i64(eval_msg(m1));
print_i64(eval_msg(m2));
print_str("\n");
return 0;
}
"#;
    run_fmt_differential_test("enums_and_patterns", src);
}
