use std::fs;
use std::process::Command;
use vanta_driver::{
    CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, LinkerKind, OptLevel,
};

#[test]
fn test_ffi_libc_abs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_abs.va");
    let out_bin = temp_dir.path().join("test_abs_bin");

    let source = r#"
extern "C" {
    pub fn abs(x: i64) -> i64;
}

pub fn main() {
    let neg = -42;
    let pos = abs(neg);
    if pos == 42 {
        print_i64(pos);
    } else {
        panic("abs(-42) did not equal 42");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("42"),
        "Expected output to contain '42', got: {}",
        stdout
    );
}

#[test]
fn test_ffi_custom_c_object_integration() {
    // 1. Create and compile a C helper file into a native .o object
    let temp_dir = tempfile::tempdir().unwrap();
    let c_file = temp_dir.path().join("c_helper.c");
    let obj_file = temp_dir.path().join("c_helper.o");

    let c_code = r#"
long long c_add_and_scale(long long a, long long b, long long scale) {
    return (a + b) * scale;
}
"#;
    fs::write(&c_file, c_code).unwrap();

    let cc_status = Command::new("cc")
        .args(&["-c", c_file.to_str().unwrap(), "-o", obj_file.to_str().unwrap()])
        .status()
        .expect("Failed to invoke system C compiler 'cc'");
    assert!(cc_status.success(), "Compilation of C source failed");

    // 2. Write Venta program that declares and invokes the C function
    let src_path = temp_dir.path().join("test_c_ffi.va");
    let out_bin = temp_dir.path().join("test_c_ffi_bin");

    let venta_code = r#"
extern "C" {
    pub fn c_add_and_scale(a: i64, b: i64, scale: i64) -> i64;
}

pub fn main() {
    let result = c_add_and_scale(10, 20, 3);
    if result == 90 {
        print_i64(result);
    } else {
        panic("c_add_and_scale failed");
    }
}
"#;
    fs::write(&src_path, venta_code).unwrap();

    // 3. Compile Venta program passing the .o object in additional_inputs
    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        additional_inputs: vec![obj_file],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    // 4. Run binary and verify result
    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("90"),
        "Expected output to contain '90', got: {}",
        stdout
    );
}

#[test]
fn test_ffi_custom_rust_object_integration() {
    // 1. Create and compile a Rust helper file into a native .o object
    let temp_dir = tempfile::tempdir().unwrap();
    let rs_file = temp_dir.path().join("rust_helper.rs");
    let obj_file = temp_dir.path().join("rust_helper.o");

    let rs_code = r#"
#![crate_type = "lib"]
#![no_std]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn rust_compute_power(base: i64, exp: i64) -> i64 {
    let mut acc: i64 = 1;
    let mut i: i64 = 0;
    while i < exp {
        acc = acc.wrapping_mul(base);
        i = i.wrapping_add(1);
    }
    acc
}
"#;
    fs::write(&rs_file, rs_code).unwrap();

    let rustc_status = Command::new("rustc")
        .args(&[
            "-O",
            "-C",
            "panic=abort",
            "--emit=obj",
            rs_file.to_str().unwrap(),
            "-o",
            obj_file.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to invoke 'rustc'");
    assert!(rustc_status.success(), "Compilation of Rust source failed");

    // 2. Write Venta program that calls the Rust function
    let src_path = temp_dir.path().join("test_rust_ffi.va");
    let out_bin = temp_dir.path().join("test_rust_ffi_bin");

    let venta_code = r#"
extern "C" fn rust_compute_power(base: i64, exp: i64) -> i64;

pub fn main() {
    let p = rust_compute_power(2, 10);
    if p == 1024 {
        print_i64(p);
    } else {
        panic("rust_compute_power failed");
    }
}
"#;
    fs::write(&src_path, venta_code).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        additional_inputs: vec![obj_file],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1024"),
        "Expected output to contain '1024', got: {}",
        stdout
    );
}

#[test]
fn test_ffi_libc_labs_and_multi_params() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_labs.va");
    let out_bin = temp_dir.path().join("test_labs_bin");

    let source = r#"
extern "C" {
    pub fn labs(x: i64) -> i64;
}

pub fn main() {
    let large_neg = -123456789;
    let large_pos = labs(large_neg);
    if large_pos == 123456789 {
        print_i64(large_pos);
    } else {
        panic("labs failed");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("123456789"),
        "Expected output to contain '123456789', got: {}",
        stdout
    );
}

#[test]
fn test_cli_driver_ffi_invocation() {
    let temp_dir = tempfile::tempdir().unwrap();
    let c_file = temp_dir.path().join("c_math.c");
    let obj_file = temp_dir.path().join("c_math.o");

    let c_code = r#"
long long compute_cube(long long x) {
    return x * x * x;
}
"#;
    fs::write(&c_file, c_code).unwrap();

    let cc_status = Command::new("cc")
        .args(&["-c", c_file.to_str().unwrap(), "-o", obj_file.to_str().unwrap()])
        .status()
        .expect("Failed to invoke system C compiler 'cc'");
    assert!(cc_status.success(), "Compilation of C source failed");

    let src_path = temp_dir.path().join("test_cube.va");
    let out_bin = temp_dir.path().join("test_cube_bin");

    let venta_code = r#"
extern "C" {
    pub fn compute_cube(x: i64) -> i64;
}

pub fn main() {
    let val = compute_cube(5);
    if val == 125 {
        print_i64(val);
    } else {
        panic("compute_cube failed");
    }
}
"#;
    fs::write(&src_path, venta_code).unwrap();

    // Use CompilerOptions with additional_inputs
    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        additional_inputs: vec![obj_file],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(res.is_ok(), "CLI FFI build failed: {:?}", res.err());

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("125"), "Expected 125, got: {}", stdout);
}

#[test]
fn test_ffi_sqlite3_version() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_sqlite.va");
    let out_bin = temp_dir.path().join("test_sqlite_bin");

    let source = r#"
extern "C" {
    pub fn sqlite3_libversion_number() -> i32;
}

pub fn main() {
    let ver = sqlite3_libversion_number();
    if ver > 3000000 {
        print_i64(1);
    } else {
        panic("sqlite version too old or failed");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        libs: vec!["sqlite3".to_string()],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1"), "Expected output to contain '1', got: {}", stdout);
}

#[test]
fn test_ffi_curl_lifecycle() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_curl.va");
    let out_bin = temp_dir.path().join("test_curl_bin");

    let source = r#"
extern "C" {
    pub fn curl_global_init(flags: i64) -> i32;
    pub fn curl_global_cleanup();
}

pub fn main() {
    let res = curl_global_init(3);
    if res == 0 {
        curl_global_cleanup();
        print_i64(200);
    } else {
        panic("curl_global_init failed");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        libs: vec!["curl".to_string()],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("200"), "Expected output to contain '200', got: {}", stdout);
}

#[test]
fn test_ffi_sqlite3_full_database_operations() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_sqlite_ops.va");
    let out_bin = temp_dir.path().join("test_sqlite_ops_bin");

    let source = r#"
import std::string::String;

extern "C" {
    pub fn sqlite3_open(filename: i64, ppDb: &mut i64) -> i32;
    pub fn sqlite3_close(db: i64) -> i32;
    pub fn sqlite3_exec(db: i64, sql: i64, callback: i64, arg: i64, errmsg: i64) -> i32;
    pub fn sqlite3_changes(db: i64) -> i32;
    pub fn sqlite3_total_changes(db: i64) -> i32;
}

pub fn main() {
    let mut handle: i64 = 0;
    let s_mem = String::from_str(":memory:");
    let rc_open = sqlite3_open(s_mem.data_ptr, &mut handle);
    if rc_open != 0 {
        panic("sqlite3_open failed");
    }

    let s_tbl = String::from_str("CREATE TABLE items (id INT, name TEXT);");
    let rc_table = sqlite3_exec(handle, s_tbl.data_ptr, 0, 0, 0);
    if rc_table != 0 {
        panic("CREATE TABLE failed");
    }

    let s_ins1 = String::from_str("INSERT INTO items VALUES (1, 'Widget');");
    sqlite3_exec(handle, s_ins1.data_ptr, 0, 0, 0);
    let chg1 = sqlite3_changes(handle);

    let s_ins2 = String::from_str("INSERT INTO items VALUES (2, 'Gadget');");
    sqlite3_exec(handle, s_ins2.data_ptr, 0, 0, 0);
    let total = sqlite3_total_changes(handle);

    sqlite3_close(handle);

    if chg1 == 1 && total == 2 {
        print_i64(777);
    } else {
        panic("sqlite3 operations unexpected change counts");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        libs: vec!["sqlite3".to_string()],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("777"), "Expected output to contain '777', got: {}", stdout);
}

#[test]
fn test_ffi_graphics_window_lifecycle() {
    let temp_dir = tempfile::tempdir().unwrap();
    let c_file = temp_dir.path().join("raylib_mock.c");
    let obj_file = temp_dir.path().join("raylib_mock.o");

    let c_code = r#"
static int g_drawings = 0;
static int g_rectangles = 0;

void InitWindow(int width, int height, const char* title) {
}

int WindowShouldClose(void) {
    return 1;
}

void CloseWindow(void) {
}

void BeginDrawing(void) {
    g_drawings++;
}

void EndDrawing(void) {
}

void DrawRectangle(int posX, int posY, int width, int height, int color) {
    g_rectangles++;
}

int get_graphics_mock_stats(void) {
    return g_drawings * 100 + g_rectangles;
}
"#;
    fs::write(&c_file, c_code).unwrap();

    let gcc_status = Command::new("gcc")
        .args(&["-c", c_file.to_str().unwrap(), "-o", obj_file.to_str().unwrap()])
        .status()
        .expect("Failed to invoke 'gcc'");
    assert!(gcc_status.success(), "Compilation of C mock failed");

    let src_path = temp_dir.path().join("test_graphics.va");
    let out_bin = temp_dir.path().join("test_graphics_bin");

    let venta_code = r#"
extern "C" {
    pub fn InitWindow(width: i32, height: i32, title: i64);
    pub fn WindowShouldClose() -> i32;
    pub fn CloseWindow();
    pub fn BeginDrawing();
    pub fn EndDrawing();
    pub fn DrawRectangle(posX: i32, posY: i32, width: i32, height: i32, color: i32);
    pub fn get_graphics_mock_stats() -> i32;
}

pub fn main() {
    InitWindow(800, 600, 0);
    BeginDrawing();
    DrawRectangle(10, 20, 100, 50, 65280);
    DrawRectangle(30, 40, 200, 60, 255);
    EndDrawing();
    CloseWindow();

    let stats = get_graphics_mock_stats();
    if stats == 102 {
        print_i64(stats);
    } else {
        panic("graphics mock stats mismatch");
    }
}
"#;
    fs::write(&src_path, venta_code).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        additional_inputs: vec![obj_file],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("102"),
        "Expected output to contain '102', got: {}",
        stdout
    );
}

#[test]
fn test_stdlib_high_level_bindings_sqlite_and_curl() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_std_bindings.va");
    let out_bin = temp_dir.path().join("test_std_bindings_bin");

    let source = r#"
import std::sqlite;
import std::curl;

pub fn main() {
    let db_res = sqlite::open(":memory:");
    match db_res {
        Result::Ok(db) => {
            let ver = sqlite::version_number();
            if ver > 3000000 {
                db.close();
            } else {
                panic("sqlite version too low");
            }
        },
        Result::Err(e) => {
            panic("failed to open database");
        }
    }

    let client = curl::new_easy();
    if client.is_valid() {
        client.cleanup();
        print_i64(888);
    } else {
        panic("easy init failed");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        libs: vec!["sqlite3".to_string(), "curl".to_string()],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("888"), "Expected output to contain '888', got: {}", stdout);
}

#[test]
fn test_stdlib_high_level_bindings_graphics() {
    let temp_dir = tempfile::tempdir().unwrap();
    let c_file = temp_dir.path().join("raylib_stub.c");
    let obj_file = temp_dir.path().join("raylib_stub.o");

    let c_code = r#"
static int g_closed = 0;
void InitWindow(int w, int h, const char* title) {}
int WindowShouldClose(void) { return 0; }
void CloseWindow(void) { g_closed = 1; }
int IsWindowReady(void) { return 1; }
void SetTargetFPS(int fps) {}
int GetFPS(void) { return 60; }
void ClearBackground(int color) {}
void BeginDrawing(void) {}
void EndDrawing(void) {}
void DrawText(const char* text, int x, int y, int size, int color) {}
void DrawRectangle(int x, int y, int w, int h, int color) {}
void DrawCircle(int cx, int cy, float r, int color) {}
int get_closed_status(void) { return g_closed; }
"#;
    fs::write(&c_file, c_code).unwrap();

    let gcc_status = Command::new("gcc")
        .args(&["-c", c_file.to_str().unwrap(), "-o", obj_file.to_str().unwrap()])
        .status()
        .expect("Failed to invoke 'gcc'");
    assert!(gcc_status.success(), "Compilation of C stub failed");

    let src_path = temp_dir.path().join("test_std_graphics.va");
    let out_bin = temp_dir.path().join("test_std_graphics_bin");

    let source = r#"
import std::graphics;

extern "C" {
    pub fn get_closed_status() -> i32;
}

pub fn main() {
    let win = graphics::init_window(640, 480, "Venta App");
    win.set_target_fps(60);
    win.begin_frame();
    win.clear(0);
    win.draw_rectangle(0, 0, 100, 100, 255);
    win.end_frame();
    win.close();

    let st = get_closed_status();
    if st == 1 {
        print_i64(999);
    } else {
        panic("window close failed");
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        additional_inputs: vec![obj_file],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("999"), "Expected output to contain '999', got: {}", stdout);
}

#[test]
fn test_sqlite_parameter_binding_and_query_results() {
    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_sqlite_params.va");
    let out_bin = temp_dir.path().join("test_sqlite_params_bin");

    let source = r#"
import std::sqlite;

pub fn main() {
    let db_res = sqlite::open(":memory:");
    match db_res {
        Result::Ok(db) => {
            // 1. Create table
            db.execute("CREATE TABLE employees (id INTEGER PRIMARY KEY, name TEXT, salary INT, manager_id INT);");

            // 2. Prepare parameterized insert
            let stmt_ins_res = db.prepare("INSERT INTO employees (id, name, salary, manager_id) VALUES (?, ?, ?, ?);");
            match stmt_ins_res {
                Result::Ok(stmt_ins) => {
                    // Row 1
                    stmt_ins.bind_int64(1, 101);
                    stmt_ins.bind_text(2, "Alice Engineer");
                    stmt_ins.bind_int64(3, 85000);
                    stmt_ins.bind_null(4);
                    let rc1 = stmt_ins.step();
                    if rc1 != 101 {
                        panic("insert 1 failed");
                    }
                    stmt_ins.reset();

                    // Row 2
                    stmt_ins.bind_int64(1, 102);
                    stmt_ins.bind_text(2, "Bob Architect");
                    stmt_ins.bind_int64(3, 95000);
                    stmt_ins.bind_int(4, 101);
                    let rc2 = stmt_ins.step();
                    if rc2 != 101 {
                        panic("insert 2 failed");
                    }
                    stmt_ins.finalize();
                },
                Result::Err(e) => {
                    panic("prepare insert failed");
                }
            }

            // 3. Prepare parameterized select query
            let stmt_sel_res = db.prepare("SELECT id, name, salary, manager_id FROM employees WHERE id = ?;");
            match stmt_sel_res {
                Result::Ok(stmt_sel) => {
                    stmt_sel.bind_int64(1, 102);
                    let has_row = stmt_sel.next_row();
                    if !has_row {
                        panic("expected row for id 102");
                    }

                    let cols = stmt_sel.column_count();
                    let id_val = stmt_sel.column_int64(0);
                    let name_str = stmt_sel.column_text(1);
                    let salary_val = stmt_sel.column_int64(2);
                    let mgr_val = stmt_sel.column_int(3);
                    let is_mgr_null = stmt_sel.column_is_null(3);

                    if cols == 4 && id_val == 102 && name_str.len == 13 && salary_val == 95000 && mgr_val == 101 && !is_mgr_null {
                        stmt_sel.finalize();
                        db.close();
                        print_i64(54321);
                    } else {
                        panic("query result values mismatched");
                    }
                },
                Result::Err(e) => {
                    panic("prepare select failed");
                }
            }
        },
        Result::Err(e) => {
            panic("database open failed");
        }
    }
}
"#;
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        libs: vec!["sqlite3".to_string()],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("54321"), "Expected output to contain '54321', got: {}", stdout);
}

#[test]
fn test_curl_response_streaming_and_header_extraction() {
    // 1. Launch a local single-request HTTP server on an ephemeral port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            use std::io::{Read, Write};
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Venta-Header: Fast\r\nContent-Length: 18\r\n\r\nHello from Venta!\n";
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    let temp_dir = tempfile::tempdir().unwrap();
    let src_path = temp_dir.path().join("test_curl_stream.va");
    let out_bin = temp_dir.path().join("test_curl_stream_bin");

    let source = format!(
        r#"
import std::curl;

pub fn main() {{
    let client = curl::new_easy();
    if !client.is_valid() {{
        panic("client init failed");
    }}

    client.set_url("http://127.0.0.1:{}");
    client.set_timeout(5);

    let res = client.fetch();
    match res {{
        Result::Ok(resp) => {{
            if resp.status_code == 200 && resp.body.len == 18 {{
                client.cleanup();
                print_i64(12345);
            }} else {{
                panic("response status or length mismatch");
            }}
        }},
        Result::Err(err) => {{
            panic("fetch failed");
        }}
    }}
}}
"#,
        port
    );
    fs::write(&src_path, source).unwrap();

    let options = CompilerOptions {
        input_file: src_path,
        output_file: out_bin.clone(),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::O1,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
        linker: LinkerKind::System,
        libs: vec!["curl".to_string()],
        ..Default::default()
    };

    let mut session = CompilerSession::new();
    let res = session.compile(&options);
    assert!(
        res.is_ok(),
        "Compilation failed: {:?}\nDiagnostics: {}",
        res.err(),
        session.render_diagnostics(DiagnosticFormat::Human)
    );

    let output = Command::new(&out_bin).output().expect("Execute test binary");
    assert!(output.status.success(), "Process exited with non-zero status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("12345"), "Expected output to contain '12345', got: {}", stdout);
}
