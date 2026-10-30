//! Comprehensive integration and native execution tests for Milestone 29:
//! Generics, Trait Bounds, Monomorphization & Dynamic Dispatch.

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
    if output_o0.status.code() != Some(expected_exit_code) {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(unix)]
        eprintln!("Signal: {:?}", output_o0.status.signal());
        eprintln!("Stderr: {}", String::from_utf8_lossy(&output_o0.stderr));
    }
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

/// Helper to verify semantic diagnostic errors.
fn check_semantic_error(source: &str, expected_code: &str) {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test.va");
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: temp_dir.path().join("test_out"),
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
fn test_generic_function_identity_native() {
    let source = r#"
fn identity<T>(x: T) -> T {
    x
}

fn main() {
    let a = identity(42);
    let b = identity(true);
    print_i64(a);
    if b {
        print_i64(1);
    } else {
        print_i64(0);
    }
}
"#;
    run_differential_stdout_test("generic_identity", source, 0, b"421");
}

#[test]
fn test_generic_struct_pair_native() {
    let source = r#"
struct Pair<A, B> {
    first: A,
    second: B,
}

fn main() {
    let p = Pair { first: 123, second: 456 };
    print_i64(p.first);
    print_i64(p.second);
}
"#;
    run_differential_stdout_test("generic_pair", source, 0, b"123456");
}

#[test]
fn test_monomorphization_trait_bounds_native() {
    let source = r#"
trait Value {
    fn get_val(self: &Self) -> i64;
}

struct ItemA {
    v: i64,
}

struct ItemB {
    v: i64,
}

impl Value for ItemA {
    fn get_val(self: &ItemA) -> i64 {
        self.v
    }
}

impl Value for ItemB {
    fn get_val(self: &ItemB) -> i64 {
        self.v * 3
    }
}

fn print_val<T: Value>(item: &T) {
    print_i64(item.get_val());
}

fn main() {
    let a = ItemA { v: 15 };
    let b = ItemB { v: 20 };
    print_val(&a);
    print_val(&b);
}
"#;
    run_differential_stdout_test("trait_bounds_mono", source, 0, b"1560");
}

#[test]
fn test_trait_bound_unsatisfied_rejection() {
    let source = r#"
trait Printable {
    fn print_it(self: &Self);
}

struct Unprintable {
    x: i64,
}

fn show<T: Printable>(item: &T) {
    item.print_it();
}

fn main() {
    let u = Unprintable { x: 10 };
    show(&u);
}
"#;
    check_semantic_error(source, "E0707");
}

#[test]
fn test_trait_object_dynamic_dispatch_native() {
    let source = r#"
trait Speaker {
    fn speak(self: &Self) -> i64;
}

struct Dog {
    id: i64,
}

struct Cat {
    id: i64,
}

impl Speaker for Dog {
    fn speak(self: &Dog) -> i64 {
        self.id
    }
}

impl Speaker for Cat {
    fn speak(self: &Cat) -> i64 {
        self.id + 100
    }
}

fn main() {
    let d = Dog { id: 7 };
    let c = Cat { id: 8 };
    let s1: &dyn Speaker = &d;
    let s2: &dyn Speaker = &c;
    print_i64(s1.speak());
    print_i64(s2.speak());
}
"#;
    run_differential_stdout_test("trait_object_dispatch", source, 0, b"7108");
}

#[test]
fn test_trait_object_passed_across_functions_native() {
    let source = r#"
trait Greeter {
    fn greet(self: &Self) -> i64;
}

struct Person {
    code: i64,
}

impl Greeter for Person {
    fn greet(self: &Person) -> i64 {
        self.code
    }
}

fn call_greet(g: &dyn Greeter) {
    print_i64(g.greet());
}

fn main() {
    let p1 = Person { code: 99 };
    let p2 = Person { code: 101 };
    call_greet(&p1);
    call_greet(&p2);
}
"#;
    run_differential_stdout_test("trait_object_across_fn", source, 0, b"99101");
}

#[test]
fn test_trait_object_multi_method_vtable_native() {
    let source = r#"
trait Shape {
    fn area(self: &Self) -> i64;
    fn perimeter(self: &Self) -> i64;
}

struct Rect {
    w: i64,
    h: i64,
}

impl Shape for Rect {
    fn area(self: &Rect) -> i64 {
        self.w * self.h
    }
    fn perimeter(self: &Rect) -> i64 {
        (self.w + self.h) * 2
    }
}

fn describe_shape(s: &dyn Shape) {
    print_i64(s.area());
    print_i64(s.perimeter());
}

fn main() {
    let r = Rect { w: 10, h: 5 };
    describe_shape(&r);
}
"#;
    run_differential_stdout_test("trait_object_multi_method", source, 0, b"5030");
}

#[test]
fn test_trait_object_mutable_dispatch_native() {
    let source = r#"
trait Accumulator {
    fn add(self: &mut Self, x: i64) -> i64;
}

struct Counter {
    count: i64,
}

impl Accumulator for Counter {
    fn add(self: &mut Counter, x: i64) -> i64 {
        self.count = self.count + x;
        self.count
    }
}

fn main() {
    let mut c = Counter { count: 10 };
    let acc: &mut dyn Accumulator = &mut c;
    print_i64(acc.add(5));
    print_i64(acc.add(7));
}
"#;
    run_differential_stdout_test("trait_object_mut_dispatch", source, 0, b"1522");
}

#[test]
fn test_object_safety_violation_rejection() {
    let source = r#"
trait Cloneable {
    fn clone(self: &Self) -> Self;
}

fn main() {
    let x: &dyn Cloneable = 0;
}
"#;
    check_semantic_error(source, "E0710");
}
