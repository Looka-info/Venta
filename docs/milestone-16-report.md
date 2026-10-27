# Vanta Milestone 16 Engineering Report
## Standalone Runtime Packaging, Standard I/O, Filesystem, CLI Arguments, and Linux Installer

---

### Executive Summary

Milestone 16 transitions Vanta from an internal compiler-development repository into an independently installable, standalone systems programming toolchain on Linux x86-64.

Prior to Milestone 16:
1. Every executable build invoked `gcc` to compile embedded C runtime source (`vanta_rt.c`) on the fly, introducing a hard compilation dependency and latency during user program builds.
2. The standard library lacked basic stream I/O (`std::io`), filesystem operations (`std::fs`), and command-line argument access (`std::env`).
3. Standard library resolution relied on repository-specific relative paths.
4. There was no unified developer CLI, no standalone release archive, and no reversible installation/uninstallation mechanism.

In Milestone 16, all of these limitations were systematically resolved:
- **Precompiled Native Runtime Archive (`libvanta_rt.a`)**: The native runtime is now precompiled into a static archive. User program linking invokes the linker directly against `libvanta_rt.a`, eliminating C compilation on every user build.
- **Hierarchical Toolchain Discovery**: Both the compiler driver and standard library loader search `CLI flags` (`--std-path`, `--runtime-path`), environment variables (`VANTA_HOME`, `VANTA_STD_PATH`, `VANTA_RUNTIME_PATH`), executable-relative paths (`../lib/vanta/...`), and local development fallbacks.
- **Standard I/O (`std::io`)**: Implemented `write_stdout(&str)`, `write_stderr(&str)`, and `read_to_string() -> Result<String, i64>` reading from standard input until EOF.
- **Filesystem API (`std::fs`)**: Implemented `read_to_string(&str) -> Result<String, i64>` and `write(&str, &str) -> Result<i64, i64>` with RFC 3629 UTF-8 validation and POSIX error propagation.
- **Command-Line Arguments (`std::env`)**: Implemented `args() -> Result<Vec<String>, i64>` backed by an ELF constructor in `vanta_rt.c` (`__attribute__((constructor))`) that captures `argc` and `argv` before user `main` is called without altering Vanta function signatures.
- **Unified `vanta` CLI**: Created a top-level developer CLI binary supporting `vanta run <file.va> [-- <args>...]`, `vanta compile <file.va> [-o <out>]`, `vanta check <file.va>`, `vanta fmt [files...] [--check]`, `vanta --version`, and `vanta --help`.
- **Reproducible Release Packaging & Installer**: Created `scripts/package_release.sh` (producing `vanta-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` and SHA-256 checksum), `scripts/install.sh` (with customizable `--prefix`, manifest generation, and PATH checking), and `scripts/uninstall.sh` (safe, manifest-driven removal).
- **Clean-Environment Verification**: Verified complete installation, standalone program compilation, execution, and uninstallation outside the development repository without Rust or Cargo in `$PATH`.

---

### Technical Architecture & Implementation

#### 1. Precompiled Runtime Archive (`libvanta_rt.a`) & Linker Integration
- **Static Archive Creation**: `runtime/vanta_rt.c` is compiled via `gcc -c -O2 -fPIC` and packaged via `ar rcs libvanta_rt.a`.
- **Driver Integration** ([`crates/vanta_driver/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_driver/src/lib.rs)):
  - Replaced temporary `.rt.c` file generation with direct static archive linking:
    ```bash
    gcc -no-pie <obj_path> <libvanta_rt.a> -o <out_path>
    ```
  - Implemented `CompilerSession::find_runtime_archive(&options)` with lookup hierarchy:
    1. `options.runtime_path` (CLI argument `--runtime-path`)
    2. Environment variable `VANTA_RUNTIME_PATH`
    3. Environment variable `VANTA_HOME` (`$VANTA_HOME/lib/vanta/libvanta_rt.a`)
    4. Executable-relative path (`<exe_dir>/../lib/vanta/libvanta_rt.a`)
    5. Local working directory / Cargo manifest fallback
    6. Cached automatic build fallback if running in a fresh development checkout.

#### 2. Runtime C Extensions & RFC 3629 UTF-8 Validation ([`runtime/vanta_rt.c`](file:///c:/Users/mumer/OneDrive/Documents/Venta/runtime/vanta_rt.c))
- **Command-Line Initialization**:
  ```c
  static int g_argc = 0;
  static char **g_argv = NULL;

  __attribute__((constructor))
  void vanta_rt_ctor(int argc, char **argv) {
      g_argc = argc;
      g_argv = argv;
  }
  ```
  The ELF constructor attribute executes prior to Vanta `main`, storing `g_argc` and `g_argv`. This allows Vanta user code to inspect command-line arguments while keeping `fn main() -> i64` and `fn main() -> ()` cleanly compatible with standard C `_start` startup conventions.
- **Strict UTF-8 Validator**: Implemented `vanta_is_valid_utf8(const char *buf, size_t len)` enforcing RFC 3629:
  - Rejects truncated multi-byte sequences.
  - Rejects overlong encodings (2-byte, 3-byte, and 4-byte).
  - Rejects UTF-16 surrogates ($U+D800 \dots U+DFFF$).
  - Rejects codepoints above $U+10FFFF$.
- **Runtime File and Stream Functions**:
  - `vanta_io_write_stdout(ptr, len)` and `vanta_io_write_stderr(ptr, len)`.
  - `vanta_io_read_stdin(VantaString *dest)`: Reads standard input until EOF into dynamic buffer, validates UTF-8, and populates `dest`.
  - `vanta_fs_read_to_string(path, path_len, VantaString *dest)`: Safely NUL-terminates `path`, opens file, queries `fstat`, reads into buffer, validates UTF-8, and populates `dest`. Returns negative error code on failure.
  - `vanta_fs_write(path, path_len, data, data_len)`: Safely NUL-terminates `path`, opens with `O_WRONLY | O_CREAT | O_TRUNC, 0666`, writes all bytes with `EINTR` loop, closes file, and returns bytes written.
  - `vanta_env_args_count()`: Returns argument count.
  - `vanta_env_arg_get(idx, VantaString *dest)`: Validates bounds and UTF-8, cloning argument into `dest`.

#### 3. Standard Library Modules ([`library/std/`](file:///c:/Users/mumer/OneDrive/Documents/Venta/library/std/))
- **`std::io`** ([`library/std/io.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/library/std/io.va)):
  ```vanta
  pub fn write_stdout(s: &str);
  pub fn write_stderr(s: &str);
  pub fn read_to_string() -> Result<String, i64>;
  ```
- **`std::fs`** ([`library/std/fs.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/library/std/fs.va)):
  ```vanta
  pub fn read_to_string(path: &str) -> Result<String, i64>;
  pub fn write(path: &str, contents: &str) -> Result<i64, i64>;
  ```
- **`std::env`** ([`library/std/env.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/library/std/env.va)):
  ```vanta
  pub fn args() -> Result<Vec<String>, i64>;
  ```
- **`std::mod`** ([`library/std/mod.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/library/std/mod.va)):
  Re-exports `option`, `result`, `iter`, `string`, `io`, `fs`, and `env`.

#### 4. Unified `vanta` CLI Wrapper ([`crates/vanta_driver/src/vanta_cli.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_driver/src/vanta_cli.rs))
Implemented commands:
- `vanta run <file.va> [-- <args>...]`: Compiles source into temporary binary, executes with user arguments, forwards process exit status, and cleans up temporary binary.
- `vanta compile <file.va> [-o <out>]`: Compiles to a permanent native binary.
- `vanta check <file.va>`: Quick type, borrow, and MIR verification without native compilation or linking.
- `vanta fmt [files...] [--check]`: Canonical source code formatting.
- `vanta -V` / `vanta --version`: Version output (`vanta 0.1.0`).
- `vanta -h` / `vanta --help`: Comprehensive usage text.

#### 5. Release Packaging & Linux Installer Scripts ([`scripts/`](file:///c:/Users/mumer/OneDrive/Documents/Venta/scripts/))
- **`scripts/package_release.sh`**:
  - Compiles release binaries (`vantac`, `vanta`).
  - Precompiles `libvanta_rt.a`.
  - Assembles standalone layout under `dist/vanta-v0.1.0-x86_64-unknown-linux-gnu/`:
    ```
    ├── bin/
    │   ├── vanta
    │   └── vantac
    ├── lib/
    │   └── vanta/
    │       ├── libvanta_rt.a
    │       └── std/
    │           ├── env.va
    │           ├── fs.va
    │           ├── io.va
    │           ├── iter.va
    │           ├── mod.va
    │           ├── option.va
    │           ├── result.va
    │           └── string.va
    ├── install.sh
    ├── uninstall.sh
    ├── README.md
    └── LICENSE
    ```
  - Compresses to `.tar.gz` and calculates SHA-256 checksum.
- **`scripts/install.sh`**:
  - Accepts `--prefix <dir>` (defaults to `~/.local`).
  - Copies binaries to `<prefix>/bin` (mode 0755).
  - Copies static archive and standard library to `<prefix>/lib/vanta/` (mode 0644).
  - Writes `.install_manifest.txt` listing all copied files.
  - Inspects `$PATH` and gives clear shell export instructions if missing.
- **`scripts/uninstall.sh`**:
  - Reads `.install_manifest.txt` and safely removes only files installed by Vanta.
  - Cleans up empty parent directories.

---

### Verification and Test Results

#### 1. Integration Test Suites
All integration test suites pass in WSL2 Linux x86-64:
- **`tests/fs_tests.rs`**: 3/3 tests pass:
  - `test_fs_write_and_read_roundtrip`: Verifies `write` and `read_to_string` round-trip.
  - `test_fs_read_nonexistent_file`: Verifies `read_to_string` error handling (`-ENOENT`).
  - `test_fs_read_invalid_utf8_rejected`: Verifies strict rejection (`-1001`) of invalid UTF-8 byte sequences.
- **`tests/io_tests.rs`**: 2/2 tests pass:
  - `test_io_stdout_and_stderr`: Verifies independent writing to fd 1 and fd 2.
  - `test_io_read_to_string_stdin`: Verifies piped standard input reading until EOF.
- **`tests/env_tests.rs`**: 3/3 tests pass:
  - `test_env_args_basic`: Verifies `env::args()` parsing argument counts and argument strings.
  - `test_env_args_memory_safety_and_drop`: Verifies vector and string memory reclamation via `vanta_active_allocs() == 0`.
  - `test_unified_vanta_cli_run_command`: Verifies `vanta run` forwarding command-line arguments and exit status.
- **`tests/packaging_tests.rs`**: 1/1 test passes:
  - Extracts `dist/vanta-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`.
  - Installs via `./install.sh --prefix=<temp_prefix>`.
  - Compiles and runs a standalone program importing `std::io`, `std::fs`, `std::env`, and `std::result::Result` outside the repository.
  - Verifies executable-relative toolchain discovery for `libvanta_rt.a` and `library/std/`.
  - Executes `./uninstall.sh` and verifies that all files and directories are cleanly removed.
- **`tests/abi_tests.rs`**: 8/8 tests pass.
- **`tests/e2e_tests.rs`**: 62/62 tests pass.
- **`tests/string_tests.rs`**: 23/23 tests pass.
- **`tests/std_tests.rs`**: 11/11 tests pass.
- **`tests/trait_tests.rs`**: 12/12 tests pass.
- **`tests/opt_tests.rs`**: 12/12 tests pass.
- **`tests/fmt_tests.rs`**: 14/14 tests pass.
- **Total Test Count**: **149 tests across workspace, 100% passing (0 failures)**.

#### 2. Code Quality Checks
- `cargo fmt --check`: 100% clean across all workspace crates and tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: 100% clean (0 warnings).
- `cargo build --release --bin vantac --bin vanta`: Builds cleanly.

---

### Deliverables Checklist

| Deliverable | Location | Status |
|---|---|---|
| Precompiled native runtime archive | `runtime/libvanta_rt.a` | **Verified** |
| Toolchain & stdlib discovery | `vanta_driver/src/lib.rs`, `vanta_sema/src/module.rs` | **Verified** |
| Standard I/O module (`std::io`) | `library/std/io.va` | **Verified** |
| Filesystem module (`std::fs`) | `library/std/fs.va` | **Verified** |
| Command-line arguments (`std::env`) | `library/std/env.va`, `runtime/vanta_rt.c` | **Verified** |
| Unified `vanta` CLI | `crates/vanta_driver/src/vanta_cli.rs` | **Verified** |
| Linux release packaging script | `scripts/package_release.sh` | **Verified** |
| Linux installer script | `scripts/install.sh` | **Verified** |
| Linux uninstaller script | `scripts/uninstall.sh` | **Verified** |
| Filesystem integration tests | `crates/vanta_driver/tests/fs_tests.rs` | **Verified** |
| Standard I/O integration tests | `crates/vanta_driver/tests/io_tests.rs` | **Verified** |
| Command-line args integration tests | `crates/vanta_driver/tests/env_tests.rs` | **Verified** |
| Packaging & installation integration tests | `crates/vanta_driver/tests/packaging_tests.rs` | **Verified** |
| Milestone 16 report | `docs/milestone-16-report.md` | **Verified** |
