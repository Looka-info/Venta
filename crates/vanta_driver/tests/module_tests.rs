use std::fs;
use std::process::Command;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

/// Helper to compile and run a multi-file Vanta project under both -O0 and -O1.
fn run_multi_file_differential_test(
    name: &str,
    files: &[(&str, &str)], // (relative_path, content)
    entry_file: &str,
    expected_exit_code: i32,
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
    let status_o0 = Command::new(&out_o0).status().expect("Execute O0 binary");
    assert_eq!(
        status_o0.code(),
        Some(expected_exit_code),
        "O0 exit code mismatch for {}",
        name
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
    let status_o1 = Command::new(&out_o1).status().expect("Execute O1 binary");
    assert_eq!(
        status_o1.code(),
        Some(expected_exit_code),
        "O1 exit code mismatch for {}",
        name
    );

    // 3. Differential check
    assert_eq!(
        status_o0.code(),
        status_o1.code(),
        "Differential mismatch between O0 and O1 for {}",
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
}

/// Helper to verify that compiling/checking a multi-file project fails with an expected diagnostic code.
fn check_multi_file_error(files: &[(&str, &str)], entry_file: &str, expected_code: &str) {
    let temp_dir = tempfile::tempdir().unwrap();
    for (rel_path, content) in files {
        let full_path = temp_dir.path().join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full_path, content).unwrap();
    }

    let src_path = temp_dir.path().join(entry_file);
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
    let json_report = session.render_diagnostics(DiagnosticFormat::Json);
    assert!(
        json_report.contains(&format!("\"code\":\"{}\"", expected_code)),
        "Expected error code {} in diagnostics report:\n{}",
        expected_code,
        json_report
    );
}

// =========================================================================
// SUCCESS CASES: Cross-Module Execution & Differential Testing (-O0 vs -O1)
// =========================================================================

#[test]
fn test_basic_module_import_and_call() {
    let files = [
        (
            "main.va",
            r#"
            mod math;
            import math::add;

            pub fn main() -> i64 {
                let x = add(10, 32);
                x
            }
            "#,
        ),
        (
            "math.va",
            r#"
            pub fn add(a: i64, b: i64) -> i64 {
                a + b
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("basic_mod", &files, "main.va", 42);
}

#[test]
fn test_multi_item_import_and_qualified_calls() {
    let files = [
        (
            "main.va",
            r#"
            mod math;
            import math::{add, multiply};

            pub fn main() -> i64 {
                let sum = add(3, 4);
                let prod = multiply(sum, 5);
                let res = math::sub(prod, 10);
                res
            }
            "#,
        ),
        (
            "math.va",
            r#"
            pub fn add(a: i64, b: i64) -> i64 {
                a + b
            }

            pub fn multiply(a: i64, b: i64) -> i64 {
                a * b
            }

            pub fn sub(a: i64, b: i64) -> i64 {
                a - b
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("multi_import_call", &files, "main.va", 25);
}

#[test]
fn test_directory_based_module_mod_va() {
    let files = [
        (
            "main.va",
            r#"
            mod models;
            import models::magic_number;

            pub fn main() -> i64 {
                magic_number()
            }
            "#,
        ),
        (
            "models/mod.va",
            r#"
            pub fn magic_number() -> i64 {
                99
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("dir_mod_va", &files, "main.va", 99);
}

#[test]
fn test_nested_modules_and_hierarchical_calls() {
    let files = [
        (
            "main.va",
            r#"
            mod math;
            import math::geometry;

            pub fn main() -> i64 {
                let dist = math::geometry::dist_sq(3, 4);
                dist
            }
            "#,
        ),
        (
            "math.va",
            r#"
            pub mod geometry;
            "#,
        ),
        (
            "math/geometry.va",
            r#"
            pub fn dist_sq(x: i64, y: i64) -> i64 {
                x * x + y * y
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("nested_mod", &files, "main.va", 25);
}

#[test]
fn test_re_export_via_pub_import() {
    let files = [
        (
            "main.va",
            r#"
            mod facade;
            import facade::secret_value;

            pub fn main() -> i64 {
                secret_value()
            }
            "#,
        ),
        (
            "facade.va",
            r#"
            mod internal;
            pub import internal::secret_value;
            "#,
        ),
        (
            "facade/internal.va",
            r#"
            pub fn secret_value() -> i64 {
                123
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("pub_import_reexport", &files, "main.va", 123);
}

#[test]
fn test_cross_module_struct_and_methods() {
    let files = [
        (
            "main.va",
            r#"
            mod geometry;
            import geometry::Point;

            pub fn main() -> i64 {
                let mut p = Point { x: 10, y: 20 };
                p.translate(2, 5);
                p.sum()
            }
            "#,
        ),
        (
            "geometry.va",
            r#"
            pub struct Point {
                pub x: i64,
                pub y: i64,
            }

            impl Point {
                pub fn translate(self: &mut Self, dx: i64, dy: i64) {
                    self.x = self.x + dx;
                    self.y = self.y + dy;
                }

                pub fn sum(self: &Self) -> i64 {
                    self.x + self.y
                }
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("cross_mod_struct_methods", &files, "main.va", 37);
}

#[test]
fn test_cross_module_enums_and_pattern_matching() {
    let files = [
        (
            "main.va",
            r#"
            mod types;
            import types::{Color, Action};

            pub fn main() -> i64 {
                let c = Color::Green;
                let c_code = match c {
                    Color::Red => 1,
                    Color::Green => 2,
                    Color::Blue => 3,
                };

                let a = Action::Move(10, 20);
                let a_code = match a {
                    Action::Quit => 0,
                    Action::Move(dx, dy) => dx + dy,
                };

                c_code * 100 + a_code
            }
            "#,
        ),
        (
            "types.va",
            r#"
            pub enum Color {
                Red,
                Green,
                Blue,
            }

            pub enum Action {
                Quit,
                Move(i64, i64),
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("cross_mod_enums_matching", &files, "main.va", 230);
}

#[test]
fn test_cross_module_generics() {
    let files = [
        (
            "main.va",
            r#"
            mod util;
            import util::{identity, Pair};

            pub fn main() -> i64 {
                let x = identity<i64>(42);
                let p = Pair<i64, i64> { first: x, second: 8 };
                p.first + p.second
            }
            "#,
        ),
        (
            "util.va",
            r#"
            pub fn identity<T>(val: T) -> T {
                val
            }

            pub struct Pair<A, B> {
                pub first: A,
                pub second: B,
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("cross_mod_generics", &files, "main.va", 50);
}

#[test]
fn test_cross_module_vectors_and_heap_cleanup() {
    let files = [
        (
            "main.va",
            r#"
            mod data;
            import data::populate_numbers;

            pub fn main() -> i64 {
                {
                    let mut numbers = Vec_new();
                    populate_numbers(&mut numbers);
                    numbers.push(30);

                    let total = numbers.len();
                    print_i64(total);

                    let allocs = vanta_active_allocs();
                    // 1 allocation active while vector is in scope
                    print_i64(allocs);
                }

                // Scope exited, numbers dropped, zero active heap allocations
                let allocs_after = vanta_active_allocs();
                print_i64(allocs_after);

                0
            }
            "#,
        ),
        (
            "data.va",
            r#"
            pub fn populate_numbers(v: &mut Vec<i64>) {
                v.push(10);
                v.push(20);
            }
            "#,
        ),
    ];

    run_multi_file_stdout_test("cross_mod_vec_cleanup", &files, "main.va", 0, b"310");
}

#[test]
fn test_collision_free_same_named_symbols() {
    let files = [
        (
            "main.va",
            r#"
            mod mod_a;
            mod mod_b;

            pub fn main() -> i64 {
                let val_a = mod_a::get_value();
                let val_b = mod_b::get_value();

                let item_a = mod_a::Item { val: 10 };
                let item_b = mod_b::Item { val: 20 };

                val_a + val_b + item_a.val + item_b.val
            }
            "#,
        ),
        (
            "mod_a.va",
            r#"
            pub fn get_value() -> i64 { 1 }

            pub struct Item {
                pub val: i64,
            }
            "#,
        ),
        (
            "mod_b.va",
            r#"
            pub fn get_value() -> i64 { 2 }

            pub struct Item {
                pub val: i64,
            }
            "#,
        ),
    ];

    run_multi_file_differential_test("symbol_collision_avoidance", &files, "main.va", 33);
}

// =========================================================================
// COMPILE-FAIL CASES: Diagnosing Visibility & Resolution Errors
// =========================================================================

#[test]
fn test_error_e0601_module_not_found() {
    let files = [(
        "main.va",
        r#"
            mod nonexistent;

            pub fn main() -> i64 {
                0
            }
            "#,
    )];

    check_multi_file_error(&files, "main.va", "E0601");
}

#[test]
fn test_error_e0602_module_cycle() {
    let files = [(
        "main.va",
        r#"
            mod main;

            pub fn main() -> i64 { 0 }
            "#,
    )];

    check_multi_file_error(&files, "main.va", "E0602");
}

#[test]
fn test_error_e0603_unknown_import() {
    let files = [
        (
            "main.va",
            r#"
            mod math;
            import math::does_not_exist;

            pub fn main() -> i64 { 0 }
            "#,
        ),
        (
            "math.va",
            r#"
            pub fn add(a: i64, b: i64) -> i64 { a + b }
            "#,
        ),
    ];

    check_multi_file_error(&files, "main.va", "E0603");
}

#[test]
fn test_error_e0604_private_function_call() {
    let files = [
        (
            "main.va",
            r#"
            mod internal;
            import internal::secret;

            pub fn main() -> i64 {
                secret()
            }
            "#,
        ),
        (
            "internal.va",
            r#"
            fn secret() -> i64 {
                42
            }
            "#,
        ),
    ];

    check_multi_file_error(&files, "main.va", "E0604");
}

#[test]
fn test_error_e0604_private_method_call() {
    let files = [
        (
            "main.va",
            r#"
            mod models;
            import models::Widget;

            pub fn main() -> i64 {
                let w = Widget { id: 1 };
                w.secret_id()
            }
            "#,
        ),
        (
            "models.va",
            r#"
            pub struct Widget {
                pub id: i64,
            }

            impl Widget {
                fn secret_id(self: &Self) -> i64 {
                    self.id
                }
            }
            "#,
        ),
    ];

    check_multi_file_error(&files, "main.va", "E0604");
}

#[test]
fn test_error_e0604_private_type_in_public_signature() {
    let files = [
        (
            "main.va",
            r#"
            mod api;

            pub fn main() -> i64 { 0 }
            "#,
        ),
        (
            "api.va",
            r#"
            struct InternalData {
                x: i64,
            }

            pub fn leak_internal(d: InternalData) -> i64 {
                d.x
            }
            "#,
        ),
    ];

    check_multi_file_error(&files, "main.va", "E0604");
}

#[test]
fn test_error_e0605_duplicate_module_decl() {
    let files = [
        (
            "main.va",
            r#"
            mod math;
            mod math;

            pub fn main() -> i64 { 0 }
            "#,
        ),
        (
            "math.va",
            r#"
            pub fn add(a: i64, b: i64) -> i64 { a + b }
            "#,
        ),
    ];

    check_multi_file_error(&files, "main.va", "E0605");
}

#[test]
fn test_error_e0606_private_field_access() {
    let files = [
        (
            "main.va",
            r#"
            mod models;
            import models::Account;

            pub fn main() -> i64 {
                let a = models::new_account();
                a.balance
            }
            "#,
        ),
        (
            "models.va",
            r#"
            pub struct Account {
                pub id: i64,
                balance: i64,
            }

            pub fn new_account() -> Account {
                Account { id: 1, balance: 100 }
            }
            "#,
        ),
    ];

    check_multi_file_error(&files, "main.va", "E0606");
}

#[test]
fn test_error_e0606_private_field_literal() {
    let files = [
        (
            "main.va",
            r#"
            mod models;
            import models::Account;

            pub fn main() -> i64 {
                let a = Account { id: 1, balance: 100 };
                a.id
            }
            "#,
        ),
        (
            "models.va",
            r#"
            pub struct Account {
                pub id: i64,
                balance: i64,
            }
            "#,
        ),
    ];

    check_multi_file_error(&files, "main.va", "E0606");
}

#[test]
fn test_error_e0607_ambiguous_module_file() {
    let files = [
        (
            "main.va",
            r#"
            mod foo;

            pub fn main() -> i64 { 0 }
            "#,
        ),
        (
            "foo.va",
            r#"
            pub fn bar() -> i64 { 1 }
            "#,
        ),
        (
            "foo/mod.va",
            r#"
            pub fn bar() -> i64 { 2 }
            "#,
        ),
    ];

    check_multi_file_error(&files, "main.va", "E0607");
}
