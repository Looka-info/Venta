# Vanta Milestone 4 Engineering Report
## Array Bounds Safety, Slice Bounds Safety, and Runtime Panic Handling

**Date**: October 2026  
**Compiler Version**: `vantac 0.1.0`  
**Target Architecture**: Linux x86-64, System V AMD64 ABI, ELF64  
**Optimization Profiles**: `-O0` (No optimization), `-O1` (Constant propagation & Dead code elimination)

---

## 1. Executive Summary

Milestone 4 closes the spatial memory safety gap in Vanta by implementing comprehensive, deterministic bounds safety and runtime panic handling for all fixed-size arrays (`[T; N]`), array references (`&[T; N]`), and string slices (`&str`).

Previously, indexing expressions calculated raw memory addresses directly without verifying that the index was inside the allocated range `[0, len)`. Milestone 4 introduces:
1. **Strict Spatial Bounds Verification**: Enforces that an index `i` is valid if and only if `0 <= i < length`.
2. **Preservation of Signed Semantics**: Evaluates indices using signed comparisons (`i >= 0 && i < len`) without reinterpreting signed negatives as unsigned integers, ensuring negative indices are caught cleanly and reported with their exact signed value.
3. **Control Flow Dominance**: In the Control Flow Graph (CFG), bounds checking strictly dominates memory access. No out-of-bounds pointer calculation or load/store instruction can execute.
4. **Deterministic Runtime Panic Handling**: Implements `vanta_panic` and `vanta_bounds_check_fail` in `runtime/vanta_rt.c`, outputting formatted diagnostics to `stderr` (fd 2), exiting with status code `101`, with zero heap allocation and `EINTR`-safe signal handling.
5. **Built-in `panic(msg: &str)` Intrinsic**: Available globally to Vanta programs, protected from redefinition.
6. **Optimizer Invariants**: Constant propagation and DCE eliminate bounds checks only when statically proven safe. Statically proven out-of-bounds accesses preserve deterministic panic paths and eliminate unreachable memory accesses.
7. **Empirical Verification**: 68 automated workspace tests, including 47 native execution end-to-end tests running under both `-O0` and `-O1`, asserting exact exit codes, stdout, and stderr matching.

---

## 2. Threat Model and Safety Analysis

### 2.1 The Vulnerability (Pre-Milestone 4)
Prior to Milestone 4, indexing operations `arr[i]` were lowered directly to:
```text
addr = slot_addr + i * elem_size
val = load(addr)
```
This allowed arbitrary out-of-bounds memory reads and writes:
- Positive out-of-bounds (`i >= len`) read adjacent stack slots or return addresses.
- Negative indices (`i < 0`) read stack frames higher in memory.
- In unoptimized builds, indexing past buffer limits risked corrupted stack pointers.

### 2.2 Invariant Verification
Milestone 4 establishes the following formal invariants:
- **Spatial Memory Safety**: For every memory access instruction `LoadIndirect` or `StoreIndirect` originating from an indexing operation `base[i]`, there exists a preceding conditional branch dominated by the condition `(i >= 0) & (i < len)`.
- **Termination Guarantee**: If `(i < 0) | (i >= len)`, control transitions unconditionally to `vanta_bounds_check_fail`, which invokes POSIX `_exit(101)` without executing the memory operation.
- **Mutability Enforcement**: Assigning to indexed elements (`arr[i] = v`) requires that `arr` be a mutable binding (`let mut`) or a mutable reference. Immutable bindings (`let arr = [...]`) and string slices (`&str`) reject indexed writes at compile time with diagnostic `E0303_MUTATE_IMMUTABLE`.

---

## 3. Runtime Panic Architecture (`runtime/vanta_rt.c`)

The runtime panic handler is designed for robust systems programming and zero-dependency execution:
- **No Dynamic Heap Allocation**: Uses fixed 32-byte stack buffers for base-10 integer formatting.
- **Signal & Interruption Safety**: Uses a non-blocking `write` loop handling `EINTR` and partial writes.
- **File Descriptor Routing**: Diagnostic text is written strictly to file descriptor `2` (`stderr`), leaving file descriptor `1` (`stdout`) uncontaminated.
- **Standardized Process Termination**: Invokes `_exit(101)` directly via POSIX syscall, avoiding recursive library destructors or unmanaged unwinding.

### Runtime Symbols
1. `void vanta_panic(const char *ptr, uint64_t len)`:
   - Emits: `Vanta panic: <msg>\n` to `stderr`.
   - Exits with status `101`.
2. `void vanta_bounds_check_fail(int64_t index, int64_t length)`:
   - Emits: `Vanta panic: index out of bounds (index %ld, length %ld)\n` to `stderr`.
   - Exits with status `101`.

---

## 4. Compiler Architecture and MIR Lowering

### 4.1 New MIR Instructions
1. `IndexPtrAddr { dest: ValueId, base_ptr: ValueId, index: ValueId, elem_size: usize }`:
   Computes `dest = base_ptr + index * elem_size` for dynamic pointers, array references, and slice buffers.
2. `MovzxMem { dest: Reg, base: Reg, offset: i32 }`:
   Machine instruction in x86-64 backend emitting `REX.W + 0F B6 /r`, performing zero-extended 1-byte loads from memory into 64-bit destination registers.

### 4.2 Bounds Check Lowering Pattern
In `vanta_mir::lower`, every indexed read and write emits:
```text
bb_current:
    %zero = ConstInt 0
    %ge_zero = Binary GtEq, %idx, %zero
    %lt_len = Binary Lt, %idx, %len
    %in_bounds = Binary And, %ge_zero, %lt_len
    CondBranch %in_bounds, bb_ok, bb_fail

bb_fail:
    Call vanta_bounds_check_fail(%idx, %len)
    Unreachable

bb_ok:
    %addr = IndexPtrAddr %base, %idx, elem_size
    %val = LoadIndirect %addr, elem_ty  // Or StoreIndirect %addr, %final_val
```

### 4.3 Optimization Invariants under `-O1`
- When `idx` and `len` are constant:
  - `const_prop` folds `%ge_zero`, `%lt_len`, and `%in_bounds` to a constant boolean.
  - If in-bounds: `CondBranch` folds to `Branch bb_ok`. DCE identifies `bb_fail` as unreachable from the entry block and removes it completely. Machine code generation emits zero bounds check instructions or branches.
  - If out-of-bounds: `CondBranch` folds to `Branch bb_fail`. DCE identifies `bb_ok` as unreachable and eliminates the downstream memory access. Machine code deterministically calls the panic handler.
- When `idx` is dynamic:
  - Both `-O0` and `-O1` evaluate the checks at runtime and branch identically.

---

## 5. Verification Test Suite and Empirical Results

The test suite in [`crates/vanta_driver/tests/e2e_tests.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_driver/tests/e2e_tests.rs) was expanded with native execution tests covering all required cases:

| Test Case | Description | Optimization | Result |
| :--- | :--- | :--- | :--- |
| `test_e2e_array_first_and_last_valid_element` | Indices `0` and `len - 1` | `-O0` and `-O1` | PASS (Exit 40) |
| `test_e2e_array_single_element` | `[42][0]` single element | `-O0` and `-O1` | PASS (Exit 42) |
| `test_e2e_array_indexed_write` | Indexed mutation `arr[1] = 99` | `-O0` and `-O1` | PASS (Exit 99) |
| `test_e2e_array_indexed_in_loop` | Loop traversal `while i < 3` | `-O0` and `-O1` | PASS (Stdout: `100\n200\n300\n`) |
| `test_e2e_array_expression_as_index` | Computed index `arr[1 + 2]` | `-O0` and `-O1` | PASS (Exit 4) |
| `test_e2e_array_reference_indexing` | Indexing through `&[i64; 3]` | `-O0` and `-O1` | PASS (Exit 25) |
| `test_e2e_string_slice_indexing` | Byte indexing on `&str` yielding `u8` | `-O0` and `-O1` | PASS (Stdout: `86 97`) |
| `test_e2e_panic_bounds_check_index_equals_len` | `arr[3]` on len 3 | `-O0` and `-O1` | PASS (Exit 101, Stderr matched) |
| `test_e2e_panic_bounds_check_index_greater_than_len` | `arr[10]` on len 3 | `-O0` and `-O1` | PASS (Exit 101, Stderr matched) |
| `test_e2e_panic_bounds_check_negative_index` | `arr[-1]` on len 3 | `-O0` and `-O1` | PASS (Exit 101, Stderr matched) |
| `test_e2e_panic_bounds_check_empty_array` | Indexing empty array `[][0]` | `-O0` and `-O1` | PASS (Exit 101, Stderr matched) |
| `test_e2e_panic_bounds_check_write_out_of_bounds` | Store `arr[5] = 99` on len 3 | `-O0` and `-O1` | PASS (Exit 101, Stderr matched) |
| `test_e2e_panic_bounds_check_in_helper_function` | OOB inside helper function | `-O0` and `-O1` | PASS (Exit 101, Stderr matched) |
| `test_e2e_panic_bounds_check_in_branch` | OOB inside conditional branch | `-O0` and `-O1` | PASS (Exit 101, Stderr matched) |
| `test_e2e_panic_intrinsic_explicit` | Built-in `panic("...")` | `-O0` and `-O1` | PASS (Exit 101, Stderr matched) |
| `test_compile_fail_mutating_immutable_array` | Assignment to immutable array | Compile check | PASS (Diagnostic `E0303`) |
| `test_compile_fail_mutating_string_slice_indexing` | Assignment to `s[0]` on `&str` | Compile check | PASS (Diagnostic `E0303`) |
| `test_compile_fail_redefining_panic_intrinsic` | Redefining `fn panic(...)` | Compile check | PASS (Diagnostic `E0103`) |

### Workspace Test Summary
- `vanta_codegen_x86_64`: 1 unit test passed.
- `vanta_diagnostics`: 1 unit test passed.
- `vanta_lexer`: 2 unit tests passed.
- `vanta_parser`: 1 unit test passed.
- `vanta_sema`: 2 unit tests passed.
- `vanta_mir`: 1 unit test + 12 optimization tests passed.
- `vanta_source`: 1 unit test passed.
- `vanta_driver`: 47 end-to-end integration tests passed.
- **Total**: 68 tests passing across the workspace. Zero warnings (`cargo clippy`), clean formatting (`cargo fmt`).

---

## 6. Documented Limitations and Next Milestone Roadmap

### 6.1 Limitations in Current Milestone
1. **Dynamic Slices (`&[T]`)**: Fixed-size arrays and string slices are fully bounds-checked; dynamic non-string slices (`&[i64]`, `&mut [T]`) will be supported in Milestone 5.
2. **Never Type (`!`)**: `panic` currently returns `Type::Unit` in the AST; full never-type bottom inference (`!`) is scheduled for future refinement.
3. **Array Bounds Range Analysis**: Dynamic loop induction variable range proofs (e.g. proving `i < 3` inside `while i < 3` eliminates bounds check on `arr[i]`) will be introduced in advanced optimization passes.

### 6.2 Next Milestone Roadmap
- **Milestone 5**: User-Defined Structs, Struct Methods, and First-Class Dynamic Slices (`&[T]`).
- ABI layout verification for nested structures and alignment padding.
