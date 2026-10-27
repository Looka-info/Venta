# Vanta Milestone 11 Engineering Report
## Standard Library Foundation, `Option<T>`, `Result<T, E>`, and Error Propagation

---

### Executive Summary

Milestone 11 establishes a robust, self-contained standard-library foundation for the Vanta programming language, introducing type-safe optionality (`Option<T>`), explicit recoverable failure handling (`Result<T, E>`), and the postfix `?` error propagation operator.

Key capabilities implemented, verified, and integrated:
1. **Documented Standard Library Tree**:
   - `library/std/mod.va`, `library/std/option.va`, `library/std/result.va`.
   - Automatic and configurable discovery via CLI `--std-path`, `VANTA_STD_PATH` environment variable, relative path hierarchy, and compile-time embedded fallback via `include_str!`.
   - Explicit `import std::option::Option;` and `import std::result::Result;` without compiler magic or hidden runtime libraries.
2. **Generic `Option<T>` and `Result<T, E>`**:
   - Implemented in clean Vanta source code.
   - Tagged union layouts with 32-bit discriminant tag and memory-aligned variant payloads.
   - Full support for nested generics (e.g. `Option<Vec<i64> >`, `Result<Vec<i64>, i64>`).
   - Dynamic drop glue ensuring active payloads are cleanly destroyed on scope exit, while inactive variants run zero destructor code.
3. **Inherent Helper Methods**:
   - `opt.is_some()`, `opt.is_none()`, `opt.unwrap_or(default)`, `opt.unwrap()`.
   - `res.is_ok()`, `res.is_err()`, `res.unwrap_or(default)`, `res.unwrap()`.
   - Inlined directly into MIR basic blocks without runtime function call overhead.
   - Panics on `unwrap()` invoke `vanta_panic` runtime intrinsic with descriptive error messages.
4. **Postfix `?` Error Propagation Operator**:
   - Single-evaluation guarantee for try operands.
   - Success path unwraps payload seamlessly.
   - Error path constructs return variant (`None` or `Err`) and executes an early return.
   - LIFO drop scope unwinding: all still-owned local variables allocated in active scopes prior to the `?` operator are destroyed before returning, guaranteeing zero memory leaks.
   - Transferred error payload is safely delivered to the caller without premature destruction.
5. **Context-Sensitive Expected Type Inference**:
   - Bidirectional expected-type propagation for enum variant constructors, let bindings, return statements, trailing expressions, and conditional branches.
   - Resolves variants like `Option::None` and `Result::Ok(val)` without verbose type arguments when the surrounding context provides the expected type.
6. **Custom Backend & Zero Virtual Dispatch**:
   - Preserved custom pure-Rust x86-64 machine code backend and ELF64 object writer. No LLVM or Cranelift dependencies.
   - Zero vtables or virtual dispatch. Identical execution under `-O0` and `-O1`.

---

### 1. Standard Library Foundation & Discovery Architecture

The standard library source tree is organized under `library/std/`:
```
library/
└── std/
    ├── mod.va
    ├── option.va
    └── result.va
```

#### Module Loader Discovery Pipeline
In `crates/vanta_sema/src/module.rs`, `ModuleLoader::load_module_tree_with_std` resolves the `std` module according to a strict priority:
1. **Explicit Option**: Passed via CLI flag `--std-path <PATH>`.
2. **Environment Variable**: `VANTA_STD_PATH`.
3. **Ancestor Search**: Searching parent directories of the root file for `library/std/mod.va`.
4. **Current Working Directory**: `./library/std/mod.va`.
5. **Cargo Manifest Directory**: Compile-time cargo workspace location.
6. **Embedded Source Fallback**: Using Rust's `include_str!` macro, the exact contents of `mod.va`, `option.va`, and `result.va` are embedded into the `vantac` compiler binary. If no files exist on disk (such as in containerized or single-binary environments), the compiler seamlessly falls back to the embedded standard library.

---

### 2. Postfix `?` Error Propagation Operator

The postfix `?` operator is parsed as a postfix expression (`ExprKind::Try(Box<Expr>)`) attached directly to the operand.

#### MIR Lowering Flow
```text
               [ Operand Place Evaluation ]
                            │
               [ Load 32-bit Tag from Offset 0 ]
                            │
               [ Compare with Success Tag (Some/Ok) ]
                            │
             ┌──────────────┴──────────────┐
             ▼                             ▼
       (Tag == Success)              (Tag == Error)
      [ try.success ]               [ try.error ]
             │                             │
    Unwrap Success Payload        Construct Return Error Enum
             │                             │
    Continue In-Place             Unwind Active Drop Scopes (LIFO)
                                           │
                                  Return Error Enum to Caller
```

#### Memory Safety & Drop Scope Unwinding
When `?` triggers an early return:
- The lowerer inspects `self.drop_scopes` in reverse order.
- For every active candidate variable where `!candidate.is_moved`, `emit_drop_for_place` emits the appropriate destructor call (`vanta_free` for heap vectors, recursive struct drops, etc.).
- The return error payload is transferred directly into the function's return slot, preventing double-free or use-after-move.
- Verified by differential heap tracking tests: `vanta_active_allocs()` returns to zero after early-returning functions.

---

### 3. Diagnostics Catalog (Milestone 11)

All try propagation errors emit stable, structured JSON diagnostics with source spans, notes, and machine-actionable suggestions:

| Code | Severity | Description | Triggers When |
| :--- | :--- | :--- | :--- |
| `E0801` | Error | Invalid try operand | `?` applied to non-Option and non-Result type (e.g. `let y = 42?;`) |
| `E0802` | Error | Incompatible return type | `?` used inside a function that does not return `Option` or `Result` |
| `E0803` | Error | Mismatched Result error types | Function returns `Result<T, E1>` but operand has error type `E2` |
| `E0804` | Error | Try outside function | `?` operator placed outside any function body |
| `E0805` | Error | Mixed propagation types | Propagating `Option` using `?` inside a function returning `Result` (or vice-versa) |
| `E0806` | Error | Unsafe payload move | Attempting an invalid move of an owned payload during try propagation |

---

### 4. Verification and Quality Gates

The implementation was subjected to comprehensive test suites covering unit tests, integration tests, differential execution, formatting idempotence, and linter conformance.

#### Test Results Summary
- **Workspace Test Suite**: 220 tests passed, 0 failed.
  - `e2e_tests.rs`: 62 tests passed.
  - `std_tests.rs`: 16 comprehensive integration tests passed.
  - `trait_tests.rs`: 19 tests passed.
  - `module_tests.rs`: 36 tests passed.
  - `fmt_tests.rs`: 14 tests passed.
  - `opt_tests.rs`: 12 tests passed.
  - Crate unit tests (`vanta_codegen_x86_64`, `vanta_diagnostics`, `vanta_fmt`, `vanta_lexer`, `vanta_mir`, `vanta_parser`, `vanta_sema`, `vanta_source`): 61 tests passed.
- **Differential Execution**: Every test in `std_tests.rs` executed under both `-O0` and `-O1` with identical stdout and exit codes.
- **Example Program**: `examples/14_standard_library/main.va` compiles and executes cleanly under both `-O0` and `-O1`.
- **Formatter Idempotency**: `vantac fmt --check` passes across all standard library files and example code with 0 diffs.
- **Code Linter**: `cargo clippy --workspace --all-targets` clean.
- **Release Compilation**: `cargo build --release --bin vantac` builds successfully.
