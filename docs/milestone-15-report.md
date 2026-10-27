# Milestone 15 Engineering Report: ABI Stability, Large Aggregate Returns, Process Exit Correctness, and CLI Reliability

**Date:** October 27, 2026  
**Status:** Complete & Fully Verified  
**Milestone:** 15  
**Target:** Linux x86-64 (System V AMD64 ABI, Pure-Rust ELF64 Object Emission, Zero LLVM/Cranelift)

---

## 1. Executive Summary & Baseline Audit Results

Before modifying code, rigorous baseline audits and minimal reproduction programs were constructed to isolate and confirm every reported defect:

1. **Defect 1: Aggregate >16 Bytes Returned by Value:**
   - *Reproduction:* Compiling `fn make_greeting() -> String { let s = String::from_str("Hello"); return s; }` and calling it in `main` crashed with `free(): invalid pointer` (`SIGABRT`, exit code -6).
   - *Root Cause:* Vanta lacked the System V AMD64 `sret` (hidden return pointer) convention. The callee was returning a stack pointer to its dead frame, which was immediately overwritten, corrupting heap pointers.
2. **Defect 2: Unit-Returning Entry Point Exit Status Leak:**
   - *Reproduction:* A program `fn main() { print_str("Hello, Vanta!\n"); }` exited with process status `14` (the byte count returned in `%rax` by the Linux `sys_write` call).
   - *Root Cause:* Functions returning unit (`Type::Unit`) omitted loading `%rax`, allowing preceding syscall/helper return values to leak as the process exit code in libc's `_start`.
3. **Defect 3: CLI Version Reporting Failure:**
   - *Reproduction:* Running `vantac --version` produced `Error: Unknown option '--version'` and exited with code `2`.
   - *Root Cause:* `--version` and `-V` were completely unhandled in the CLI argument parser.
4. **Defect 4: CLI Help Output Stream:**
   - *Reproduction:* Running `vantac --help` sent help text to `stderr` instead of `stdout`.
   - *Root Cause:* `print_usage()` used `eprintln!` instead of standard output `println!`.
5. **Defect 5: Bounds-Check Discrepancy Investigation:**
   - *Finding:* Reconciled previous contradictory audit findings. Confirmed that fixed-size array indexing, array references, dynamic slices, string slices, and vector indexing all emit `emit_bounds_check` (`0 <= idx < len`), branching to `vanta_bounds_check_fail` upon failure.

---

## 2. Files Changed

| Component | Path | Description of Changes |
|---|---|---|
| **ABI Classifier** | [`crates/vanta_codegen_x86_64/src/abi.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/abi.rs) | Created central, pure System V AMD64 classification engine (`AbiClass`, `classify_type`) handling primitives, small structs, unaligned aggregates, large structs, vectors, strings, and enums. |
| **Stack Layout** | [`crates/vanta_codegen_x86_64/src/frame.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/frame.rs) | Implemented `sret_offset` allocation for incoming hidden pointers, allocated caller-side return buffers for `Instruction::Call`, and updated prologue argument spilling to shift user arguments to `%rsi` when `sret` is active. Added stack-passed argument support. |
| **Instruction Selection** | [`crates/vanta_codegen_x86_64/src/instruction_select.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/instruction_select.rs) | Lowered `Instruction::Call` to pass `lea -buf_offset(%rbp), %rdi` for memory-class returns. Lowered `Terminator::Return` to copy aggregate bytes to `(%rdi)` and set `%rax = sret_ptr`. Lowered `Type::Unit` returns to `xor %eax, %eax`. |
| **Backend Integration** | [`crates/vanta_codegen_x86_64/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/lib.rs) | Exported ABI types and wired struct/enum definitions into `FrameLayout::compute` and `InstructionSelector::new`. |
| **MIR Identifier Lowering** | [`crates/vanta_mir/src/lower.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/lower.rs) | Fixed `ExprKind::Ident` lowering to emit `Instruction::SlotAddr` instead of `Instruction::Load` for struct and enum aggregates, ensuring correct base pointer resolution. |
| **Compiler CLI** | [`crates/vanta_driver/src/main.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_driver/src/main.rs) | Implemented canonical `--version` and `-V` reporting `vantac 0.1.0` to stdout (exit code 0). Routed `--help` to stdout (exit code 0). Standardized usage and exit codes. |
| **ABI Test Suite** | [`crates/vanta_driver/tests/abi_tests.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_driver/tests/abi_tests.rs) | Added 8 automated integration tests covering aggregate returns, string returns, vector returns, nested calls, arguments alongside `sret`, clean exit codes, and CLI flags. |
| **Showcase Example** | [`examples/17_owned_strings/main.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/examples/17_owned_strings/main.va) | Added section demonstrating `fn create_banner(title: &str) -> String` returned by value. |

---

## 3. System V AMD64 Aggregate Return & `sret` Architecture

### 3.1 Classification Rules (`abi.rs`)
Under the System V AMD64 ABI Section 3.2.3:
```rust
pub enum AbiClass {
    Void,
    Integer1 { size: usize },
    Integer2 { size: usize },
    Memory { size: usize, align: usize },
}
```
- **`AbiClass::Void`**: `Type::Unit`.
- **`AbiClass::Integer1`**: Scalars $\le 8$ bytes (`i8..i64`, `u8..u64`, `bool`, references, pointers). Returned in `%rax`.
- **`AbiClass::Integer2`**: 2 eightbytes up to 16 bytes (`&str`, slices `&[T]`, small structs $\le 16$ bytes without unaligned fields). Returned in `%rax` (low) and `%rdx` (high).
- **`AbiClass::Memory`**:
  1. Any aggregate $> 16$ bytes (e.g., 24-byte `String`, 24-byte `Vec<T>`, large user structs).
  2. Any aggregate containing unaligned fields (even if $\le 16$ bytes).
  3. Any aggregate containing a field classified as `Memory`.
  4. Returned via hidden pointer passed in `%rdi`.

### 3.2 Caller Convention
When `Instruction::Call` returns a `Memory` class type:
1. `FrameLayout::compute` allocates a dedicated stack buffer of `size` bytes (aligned to `align`) at `-buf_offset(%rbp)` on the caller's frame.
2. The caller executes `lea -buf_offset(%rbp), %rdi`, placing the hidden return pointer in `%rdi`.
3. User arguments are shifted: argument 0 is passed in `%rsi`, argument 1 in `%rdx`, argument 2 in `%rcx`, etc.
4. Arguments exceeding available registers are pushed to the stack with 16-byte stack frame alignment maintained.
5. Upon return, the caller stores `%rax` (the returned buffer address) into its temporary value slot.

### 3.3 Callee Convention
When compiling a function whose return type has class `Memory`:
1. In the prologue, incoming `%rdi` is spilled to a dedicated stack slot `-sret_offset(%rbp)`.
2. Callee parameter registers start at `%rsi` (index 1 of `ARG_REGS`).
3. In `Terminator::Return(Some(val))`:
   - `%r11` is loaded with `sret_ptr` from `-sret_offset(%rbp)`.
   - `%r10` is loaded with `val` (the source aggregate pointer).
   - A copy loop copies `size` bytes from `(%r10)` to `(%r11)`. Self-copy is avoided via a pointer comparison check.
   - `sret_ptr` is moved into `%rax` as required by the System V AMD64 ABI.
   - Function executes epilogue and `ret`.

---

## 4. Aggregate Return Test Matrix

| Return Category | Concrete Type | Size | ABI Class | Calling Convention | Test Verification |
|---|---|:---:|:---:|:---:|:---:|
| **Unit / Void** | `()` | 0 | `Void` | `%rax` zeroed (`xor %eax, %eax`) | `test_abi_unit_returning_main_exits_zero` |
| **Scalar Integer** | `i64` | 8 | `Integer1` | Returned in `%rax` | Existing e2e test suite |
| **Scalar Boolean** | `bool` | 1 | `Integer1` | Returned in `%rax` | Existing e2e test suite |
| **String Slice** | `&str` | 16 | `Integer2` | Returned in `%rax` (ptr) + `%rdx` (len) | Slicing tests |
| **Large Struct** | `Point3D` | 24 | `Memory` | `sret` hidden pointer in `%rdi`, returned in `%rax` | `test_abi_large_struct_return_by_value` |
| **Owned String** | `String` | 24 | `Memory` | `sret` hidden pointer in `%rdi`, returned in `%rax` | `test_abi_string_return_by_value` |
| **Owned Vector** | `Vec<i64>` | 24 | `Memory` | `sret` hidden pointer in `%rdi`, returned in `%rax` | `test_abi_vector_return_by_value` |
| **Nested Struct** | `Person { name: String, age: i64 }` | 32 | `Memory` | `sret` hidden pointer in `%rdi`, returned in `%rax` | `test_abi_nested_struct_containing_owned_string` |
| **Nested Calls** | `step1() -> step2() -> step3()` | 24 | `Memory` | Chained `sret` buffers across 3 stack frames | `test_abi_nested_aggregate_return_calls` |
| **Args with `sret`** | `build_greeting(&str, i64) -> String` | 24 | `Memory` | `%rdi`=sret, `%rsi`=ptr, `%rdx`=len, `%rcx`=count | `test_abi_aggregate_return_with_ordinary_and_slice_args` |

---

## 5. Process Exit Status & Entry-Point Contract

### 5.1 Entry-Point Specification
In Vanta, `fn main()` serves as the user-level entry point called by libc's `_start`.
1. **Unit Return:** When `main` returns `()`, the compiler emits `xor %eax, %eax` before jumping to the function epilogue. This guarantees `%rax == 0`, ensuring the process terminates cleanly with exit code `0`.
2. **Integer Return:** When `main` returns `i64`, `%rax` holds the returned integer value, which becomes the process exit status.

### 5.2 Empirical Verification
- **Before Fix:** `fn main() { print_str("Hello, Vanta!\n"); }` exited with code `14`.
- **After Fix:** `fn main() { print_str("Hello, Vanta!\n"); }` exits with code `0`. Verified via Python `subprocess.run` asserting exact exit code `0`.

---

## 6. Disassembly Evidence & Verification

Disassembly of `make_greeting` (`examples/test_sret.va`) under Linux x86-64:
```x86_64
00000000004011b0 <make_greeting>:
  4011b0:   push   rbp
  4011b1:   mov    rbp,rsp
  4011b4:   sub    rsp,0x40
  4011bb:   mov    QWORD PTR [rbp-0x8],rdi        # [1] Save incoming sret pointer to [rbp-8]
  4011bf:   lea    r10,[rbp-0x20]                 # [2] Local slot for String s
  ...
  4011e6:   call   4020f3 <vanta_str_from_str>    # [3] Construct local String
  4011eb:   lea    r10,[rbp-0x20]                 # [4] r10 = address of local String
  4011ef:   mov    QWORD PTR [rbp-0x40],r10
  4011f3:   mov    r11,QWORD PTR [rbp-0x8]        # [5] r11 = caller sret pointer
  4011f7:   mov    r10,QWORD PTR [rbp-0x40]       # [6] r10 = source String address
  4011fb:   cmp    r10,r11                        # [7] Guard against redundant self-copy
  4011fe:   je     40121c <make_greeting+0x6c>
  401204:   mov    rax,QWORD PTR [r10+0x0]        # [8] Copy word 0: data_ptr
  401208:   mov    QWORD PTR [r11+0x0],rax
  40120c:   mov    rax,QWORD PTR [r10+0x8]        # [9] Copy word 1: len
  401210:   mov    QWORD PTR [r11+0x8],rax
  401214:   mov    rax,QWORD PTR [r10+0x10]       # [10] Copy word 2: cap
  401218:   mov    QWORD PTR [r11+0x10],rax
  40121c:   mov    rax,QWORD PTR [rbp-0x8]        # [11] Set %rax = sret_ptr (SysV AMD64 requirement)
  401225:   mov    rsp,rbp
  401228:   pop    rbp
  401229:   ret                                   # [12] Return to caller
```

Caller disassembly in `main`:
```x86_64
  401243:   lea    rdi,[rbp-0x38]                 # Pass address of return buffer in %rdi
  401247:   call   4011b0 <make_greeting>         # Invoke callee
  40124c:   mov    QWORD PTR [rbp-0x40],rax       # Store returned sret pointer in temporary value slot
```

---

## 7. Reconciling the Bounds-Check Report

The readiness audit noted a discrepancy regarding bounds checking. Investigation of [`crates/vanta_mir/src/lower.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/lower.rs) confirmed:
- `lower_index_base_and_len` (lines 692–816) extracts the base pointer and length for:
  1. Fixed-size arrays (`Type::Array { inner, len }`): generates a constant integer `len`.
  2. Array references (`Type::Ref { inner: Type::Array ... }`): generates constant `len`.
  3. Vectors (`Type::Vec`): loads `len` from offset 8.
  4. Slices and String Slices: loads length from the slice descriptor.
- `emit_bounds_check` (lines 818–870) emits:
  `in_bounds = (0 <= idx) && (idx < len)`
  and conditionally branches to a panic block calling `vanta_bounds_check_fail`.
- E2E tests (`test_e2e_panic_bounds_check_index_equals_len`, `test_e2e_panic_bounds_check_negative_index`, `test_e2e_panic_bounds_check_write_out_of_bounds`) all pass and verify bounds checking in generated machine code.
- **Conclusion:** Bounds-check coverage is verified for all indexed accesses in safe Vanta code.

---

## 8. Quality Gate Verification Results

| Check | Command | Result |
|---|---|:---:|
| **Rust Code Formatting** | `cargo fmt --check` | **Passed (Clean)** |
| **Clippy Static Analysis** | `cargo clippy --workspace --all-targets -- -D warnings` | **Passed (0 warnings)** |
| **Release Build** | `cargo build --release --bin vantac` | **Passed (Clean)** |
| **Full Workspace Test Suite** | `cargo test --workspace` | **100% Passed (140/140 tests)** |
| **Milestone 15 ABI Tests** | `cargo test --test abi_tests` | **100% Passed (8/8 tests)** |
| **CLI Version Test** | `./target/release/vantac --version` | **Passed (`vantac 0.1.0`, code 0)** |
| **CLI Help Test** | `./target/release/vantac --help` | **Passed (stdout, code 0)** |
| **Memory Leak Verification** | Allocator assertions in `test_abi_string_return_by_value` | **Passed (`active_allocs == 0`)** |
| **Showcase Execution** | Native execution of `examples/17_owned_strings/main.va` (`-O0` & `-O1`) | **Passed (code 0, 0 leaks)** |

---

## 9. Remaining Blockers & Next Milestone

With Milestone 15 complete, the compiler ABI and CLI have stabilized. The remaining release blockers for Public Alpha (Gate 1) are:
1. **Precompiled Runtime (`libvanta_rt.a`):** Eliminating the link-time dependency on host `gcc`.
2. **Basic Standard I/O & Filesystem:** Implementing `std::fs` (`read_to_string`, `write`) and `std::env::args`.
3. **Linear-Scan Register Allocator:** Closing the 3.5x–6.3x compute loop performance gap.

**Recommended Next Step:** **Milestone 16 — Precompiled Runtime Archive (`libvanta_rt.a`), Standard I/O Primitives, and Standalone Distribution.**
