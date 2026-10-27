//! Comprehensive integration and differential tests for Milestone 10:
//! Traits, Interfaces, Generic Constraints, and Static Dispatch.

use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

/// Helper to compile and run a single-file Vanta program, checking stdout and exit code under -O0 and -O1.
fn run_differential_stdout_test(
    name: &str,
    source: &str,
    expected_exit_code: i32,
    expected_stdout: &[u8],
) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join(format!("{}.va", name));
    fs::write(&src_path, source).unwrap();

    // 1. Unoptimized build (-O0)
    let out_o0 = temp_dir.path().join(format!("{}_o0", name));
    let opts_o0 = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_o0.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };
    let mut session_o0 = CompilerSession::new();
    let res_o0 = session_o0.compile(&opts_o0);
    if res_o0.is_err() {
        eprintln!("{}", session_o0.render_diagnostics(DiagnosticFormat::Human));
    }
    assert!(res_o0.is_ok(), "O0 Compilation failed: {:?}", res_o0.err());
    let output_o0 = Command::new(&out_o0).output().expect("Execute O0 binary");
    assert_eq!(
        output_o0.status.code(),
        Some(expected_exit_code),
        "O0 exit code mismatch for {}",
        name
    );
    assert_eq!(
        output_o0.stdout.as_slice(),
        expected_stdout,
        "O0 stdout mismatch for {}: got {:?}, expected {:?}",
        name,
        String::from_utf8_lossy(&output_o0.stdout),
        String::from_utf8_lossy(expected_stdout)
    );

    // 2. Optimized build (-O1)
    let out_o1 = temp_dir.path().join(format!("{}_o1", name));
    let opts_o1 = CompilerOptions {
        input_file: src_path,
        output_file: out_o1.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };
    let mut session_o1 = CompilerSession::new();
    let res_o1 = session_o1.compile(&opts_o1);
    if res_o1.is_err() {
        eprintln!("{}", session_o1.render_diagnostics(DiagnosticFormat::Human));
    }
    assert!(res_o1.is_ok(), "O1 Compilation failed: {:?}", res_o1.err());
    let output_o1 = Command::new(&out_o1).output().expect("Execute O1 binary");
    assert_eq!(
        output_o1.status.code(),
        Some(expected_exit_code),
        "O1 exit code mismatch for {}",
        name
    );
    assert_eq!(
        output_o1.stdout.as_slice(),
        expected_stdout,
        "O1 stdout mismatch for {}: got {:?}, expected {:?}",
        name,
        String::from_utf8_lossy(&output_o1.stdout),
        String::from_utf8_lossy(expected_stdout)
    );

    // 3. Differential check
    assert_eq!(
        output_o0.stdout, output_o1.stdout,
        "Differential stdout mismatch between O0 and O1 for {}",
        name
    );
}

/// Helper to compile and run a multi-file Vanta project, checking stdout and exit code under -O0 and -O1.
fn run_multi_file_stdout_test(
    name: &str,
    files: &[(&str, &str)],
    entry_file: &str,
    expected_exit_code: i32,
    expected_stdout: &[u8],
) {
    let temp_dir = tempfile::tempdir().unwrap();
    for (rel_path, content) in files {
        let full_path = temp_dir.path().join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full_path, content).unwrap();
    }

    let src_path = temp_dir.path().join(entry_file);

    // 1. Unoptimized build (-O0)
    let out_o0 = temp_dir.path().join(format!("{}_o0", name));
    let opts_o0 = CompilerOptions {
        input_file: src_path.clone(),
        output_file: out_o0.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };
    let mut session_o0 = CompilerSession::new();
    let res_o0 = session_o0.compile(&opts_o0);
    if res_o0.is_err() {
        eprintln!("{}", session_o0.render_diagnostics(DiagnosticFormat::Human));
    }
    assert!(res_o0.is_ok(), "O0 Compilation failed: {:?}", res_o0.err());
    let output_o0 = Command::new(&out_o0).output().expect("Execute O0 binary");
    assert_eq!(
        output_o0.status.code(),
        Some(expected_exit_code),
        "O0 exit code mismatch for {}",
        name
    );
    assert_eq!(
        output_o0.stdout.as_slice(),
        expected_stdout,
        "O0 stdout mismatch for {}: got {:?}, expected {:?}",
        name,
        String::from_utf8_lossy(&output_o0.stdout),
        String::from_utf8_lossy(expected_stdout)
    );

    // 2. Optimized build (-O1)
    let out_o1 = temp_dir.path().join(format!("{}_o1", name));
    let opts_o1 = CompilerOptions {
        input_file: src_path,
        output_file: out_o1.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        ..Default::default()
    };
    let mut session_o1 = CompilerSession::new();
    let res_o1 = session_o1.compile(&opts_o1);
    if res_o1.is_err() {
        eprintln!("{}", session_o1.render_diagnostics(DiagnosticFormat::Human));
    }
    assert!(res_o1.is_ok(), "O1 Compilation failed: {:?}", res_o1.err());
    let output_o1 = Command::new(&out_o1).output().expect("Execute O1 binary");
    assert_eq!(
        output_o1.status.code(),
        Some(expected_exit_code),
        "O1 exit code mismatch for {}",
        name
    );
    assert_eq!(
        output_o1.stdout.as_slice(),
        expected_stdout,
        "O1 stdout mismatch for {}: got {:?}, expected {:?}",
        name,
        String::from_utf8_lossy(&output_o1.stdout),
        String::from_utf8_lossy(expected_stdout)
    );

    // 3. Differential check
    assert_eq!(
        output_o0.stdout, output_o1.stdout,
        "Differential stdout mismatch between O0 and O1 for {}",
        name
    );
}

/// Helper to verify that compiling/checking fails with an expected diagnostic code.
fn check_error(source: &str, expected_code: &str) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("main.va");
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("unused.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Json,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let result = session.check(&options);
    assert!(
        result.is_err(),
        "Expected compilation to fail with {}",
        expected_code
    );

    let json_output = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(
        json_output.contains(expected_code),
        "Diagnostic output missing code {}: {}",
        expected_code,
        json_output
    );
}

#[test]
fn test_basic_trait_impl_and_static_call() {
    let source = r#"
trait Describe {
    fn describe(self: &Self) -> i64;
}

struct User {
    id: i64,
}

impl Describe for User {
    fn describe(self: &User) -> i64 {
        self.id
    }
}

fn main() {
    let u = User { id: 42 };
    let d = u.describe();
    print_i64(d);
}
"#;
    run_differential_stdout_test("basic_trait_impl", source, 0, b"42");
}

#[test]
fn test_two_structs_implementing_same_trait() {
    let source = r#"
trait Area {
    fn area(self: &Self) -> i64;
}

struct Rect {
    w: i64,
    h: i64,
}

struct Square {
    side: i64,
}

impl Area for Rect {
    fn area(self: &Rect) -> i64 {
        self.w * self.h
    }
}

impl Area for Square {
    fn area(self: &Square) -> i64 {
        self.side * self.side
    }
}

fn main() {
    let r = Rect { w: 6, h: 7 };
    let s = Square { side: 9 };
    print_i64(r.area());
    print_i64(s.area());
}
"#;
    run_differential_stdout_test("two_structs_trait", source, 0, b"4281");
}

#[test]
fn test_generic_function_with_trait_bound_static_dispatch() {
    let source = r#"
trait Value {
    fn get_val(self: &Self) -> i64;
}

struct ItemA {
    x: i64,
}

struct ItemB {
    y: i64,
}

impl Value for ItemA {
    fn get_val(self: &ItemA) -> i64 {
        self.x
    }
}

impl Value for ItemB {
    fn get_val(self: &ItemB) -> i64 {
        self.y * 2
    }
}

fn print_doubled<T: Value>(item: &T) {
    let v = item.get_val();
    print_i64(v);
}

fn main() {
    let a = ItemA { x: 100 };
    let b = ItemB { y: 25 };
    print_doubled(&a);
    print_doubled(&b);
}
"#;
    run_differential_stdout_test("generic_trait_bound", source, 0, b"10050");
}

#[test]
fn test_mutable_self_receiver() {
    let source = r#"
trait Counter {
    fn increment(self: &mut Self);
}

struct Tracker {
    count: i64,
}

impl Counter for Tracker {
    fn increment(self: &mut Tracker) {
        self.count = self.count + 5;
    }
}

fn main() {
    let mut t = Tracker { count: 10 };
    t.increment();
    t.increment();
    print_i64(t.count);
}
"#;
    run_differential_stdout_test("mut_self_receiver", source, 0, b"20");
}

#[test]
fn test_enum_implementing_trait() {
    let source = r#"
trait Score {
    fn points(self: &Self) -> i64;
}

enum Outcome {
    Loss,
    Draw,
    Win(i64),
}

impl Score for Outcome {
    fn points(self: &Outcome) -> i64 {
        match *self {
            Outcome::Loss => 0,
            Outcome::Draw => 1,
            Outcome::Win(bonus) => 3 + bonus,
        }
    }
}

fn main() {
    let o1 = Outcome::Loss;
    let o2 = Outcome::Draw;
    let o3 = Outcome::Win(10);
    print_i64(o1.points());
    print_i64(o2.points());
    print_i64(o3.points());
}
"#;
    run_differential_stdout_test("enum_trait_impl", source, 0, b"0113");
}

#[test]
fn test_inherent_method_precedence_over_trait() {
    let source = r#"
trait Display {
    fn show(self: &Self) -> i64;
}

struct Widget {
    id: i64,
}

impl Widget {
    fn show(self: &Widget) -> i64 {
        999
    }
}

impl Display for Widget {
    fn show(self: &Widget) -> i64 {
        self.id
    }
}

fn main() {
    let w = Widget { id: 7 };
    // Inherent method takes precedence over trait method
    print_i64(w.show());
}
"#;
    run_differential_stdout_test("inherent_precedence", source, 0, b"999");
}

#[test]
fn test_cross_module_traits_and_implementations() {
    let files = &[
        (
            "main.va",
            r#"
mod geometry;
import geometry::{Shape, Circle};

fn render<T: Shape>(s: &T) {
    print_i64(s.perimeter());
}

fn main() {
    let c = Circle { radius: 10 };
    render(&c);
}
"#,
        ),
        (
            "geometry.va",
            r#"
pub trait Shape {
    fn perimeter(self: &Self) -> i64;
}

pub struct Circle {
    pub radius: i64,
}

impl Shape for Circle {
    fn perimeter(self: &Circle) -> i64 {
        self.radius * 6
    }
}
"#,
        ),
    ];

    run_multi_file_stdout_test("cross_module_traits", files, "main.va", 0, b"60");
}

// -------------------------------------------------------------
// Compile-fail tests for all E0701 - E0709 trait error codes
// -------------------------------------------------------------

#[test]
fn test_compile_fail_e0701_unknown_trait_impl() {
    let source = r#"
struct User { id: i64 }
impl NonExistentTrait for User {
    fn test(self: &User) {}
}
fn main() {}
"#;
    check_error(source, "E0701");
}

#[test]
fn test_compile_fail_e0701_unknown_trait_bound() {
    let source = r#"
fn run<T: MissingTrait>(item: &T) {}
fn main() {}
"#;
    check_error(source, "E0701");
}

#[test]
fn test_compile_fail_e0702_duplicate_implementation() {
    let source = r#"
trait Foo {
    fn bar(self: &Self);
}
struct Data { x: i64 }
impl Foo for Data {
    fn bar(self: &Data) {}
}
impl Foo for Data {
    fn bar(self: &Data) {}
}
fn main() {}
"#;
    check_error(source, "E0702");
}

#[test]
fn test_compile_fail_e0703_missing_trait_method() {
    let source = r#"
trait Math {
    fn add(self: &Self) -> i64;
    fn sub(self: &Self) -> i64;
}
struct Pair { a: i64, b: i64 }
impl Math for Pair {
    fn add(self: &Pair) -> i64 { self.a + self.b }
}
fn main() {}
"#;
    check_error(source, "E0703");
}

#[test]
fn test_compile_fail_e0704_unknown_trait_method() {
    let source = r#"
trait Math {
    fn add(self: &Self) -> i64;
}
struct Pair { a: i64, b: i64 }
impl Math for Pair {
    fn add(self: &Pair) -> i64 { self.a + self.b }
    fn extra_method(self: &Pair) -> i64 { self.a }
}
fn main() {}
"#;
    check_error(source, "E0704");
}

#[test]
fn test_compile_fail_e0705_signature_mismatch_return_type() {
    let source = r#"
trait Check {
    fn is_valid(self: &Self) -> bool;
}
struct Config { val: i64 }
impl Check for Config {
    fn is_valid(self: &Config) -> i64 { self.val }
}
fn main() {}
"#;
    check_error(source, "E0705");
}

#[test]
fn test_compile_fail_e0705_signature_mismatch_mutability() {
    let source = r#"
trait Modifiable {
    fn update(self: &mut Self);
}
struct Config { val: i64 }
impl Modifiable for Config {
    fn update(self: &Config) {}
}
fn main() {}
"#;
    check_error(source, "E0705");
}

#[test]
fn test_compile_fail_e0706_ambiguous_trait_method() {
    let source = r#"
trait TraitA {
    fn action(self: &Self) -> i64;
}
trait TraitB {
    fn action(self: &Self) -> i64;
}
struct Bot { id: i64 }
impl TraitA for Bot {
    fn action(self: &Bot) -> i64 { 1 }
}
impl TraitB for Bot {
    fn action(self: &Bot) -> i64 { 2 }
}
fn main() {
    let b = Bot { id: 10 };
    b.action();
}
"#;
    check_error(source, "E0706");
}

#[test]
fn test_compile_fail_e0707_trait_bound_not_satisfied() {
    let source = r#"
trait Walkable {
    fn walk(self: &Self);
}
struct Fish { speed: i64 }
fn go<T: Walkable>(item: &T) {
    item.walk();
}
fn main() {
    let f = Fish { speed: 5 };
    go(&f);
}
"#;
    check_error(source, "E0707");
}

#[test]
fn test_compile_fail_e0708_duplicate_trait() {
    let source = r#"
trait Greeter {
    fn greet(self: &Self);
}
trait Greeter {
    fn greet(self: &Self);
}
fn main() {}
"#;
    check_error(source, "E0708");
}

#[test]
fn test_compile_fail_e0709_invalid_trait_receiver_no_self() {
    let source = r#"
trait Logger {
    fn log(msg: i64);
}
fn main() {}
"#;
    check_error(source, "E0709");
}

#[test]
fn test_compile_fail_e0709_invalid_trait_receiver_not_ref() {
    let source = r#"
trait Logger {
    fn log(self: i64);
}
fn main() {}
"#;
    check_error(source, "E0709");
}
