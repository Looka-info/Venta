# System V AMD64 ABI, Runtime, and Memory Safety Audit

**Date:** October 28, 2026  
**Auditor:** Senior Compiler Engineer / x86-64 Systems Architect  
**Milestone:** 25 — v1.0 Release Readiness, Correctness Audit & Reproducible Benchmarking  

---

## 1. Executive Summary

This audit evaluates the compliance of Vanta's custom code generator, ELF64 emitter, and runtime library with the **System V AMD64 ABI (x86-64 Architecture Processor Supplement)**, evaluates the correctness of Tail-Call Optimization (TCO), and defines the exact boundary between Vanta's static borrow-checker guarantees and runtime checks.

Vanta does not rely on LLVM or Cranelift; all calling conventions, stack alignments, callee-saved register salvaging, and hidden aggregate return pointers are emitted directly by [`crates/vanta_codegen_x86_64/src/instruction_select.rs`](file:///crates/vanta_codegen_x86_64/src/instruction_select.rs) and [`crates/vanta_codegen_x86_64/src/frame.rs`](file:///crates/vanta_codegen_x86_64/src/frame.rs).

---

## 2. System V AMD64 ABI Conformance Audit

### 2.1 Argument Passing Conventions
The System V AMD64 ABI specifies that the first 6 integer or pointer arguments are passed in physical registers:
$$\text{INTEGER Registers} = [\%rdi, \%rsi, \%rdx, \%rcx, \%r8, \%r9]$$

- **Implementation:** Defined in [`Reg::ARG_REGS`](file:///crates/vanta_codegen_x86_64/src/register.rs).
- **Audit Findings:**
  - Standard scalar arguments ($n \le 6$) are loaded sequentially into `%rdi` through `%r9`.
  - Arguments exceeding 6 are evaluated and pushed onto the stack in reverse order (`arg_op` pushed from right to left).
  - Dynamic slices (`&[T]` and `&str`) are fat pointers comprising a pointer and length (2 scalar 64-bit words). In Vanta, fat pointers consume two consecutive argument registers (e.g. `%rdi` for pointer and `%rsi` for length), or split onto the stack if fewer than two argument registers remain.

### 2.2 Return Value Classification & `sret` Conventions
Return values are classified via [`classify_type`](file:///crates/vanta_codegen_x86_64/src/abi.rs) according to the ABI's 16-byte rule:

1. **Scalar & Small Aggregate Class ($\le 16$ bytes):**
   - Fits within 1 or 2 64-bit physical registers: `%rax` and `%rdx`.
   - Primitive types (`i64`, `bool`, enum discriminants) return directly in `%rax`.
   - Slices return pointer in `%rax` and length in `%rdx`.
2. **Memory Aggregate Class ($> 16$ bytes or Unaligned Structs):**
   - Large structs (such as `Vec<T>` at 24 bytes and `String` at 24 bytes) cannot be returned in registers.
   - The caller allocates a temporary return buffer on its stack frame and passes the address as a **hidden first argument** in `%rdi`.
   - The callee writes the return value directly into `*(%rdi)` and returns `%rax` containing the address of the return buffer.
   - User arguments are shifted right by one register (starting in `%rsi` instead of `%rdi`).
   - **Verification:** Verified by [`tests/abi_tests.rs`](file:///crates/vanta_driver/tests/abi_tests.rs) testing 24-byte vector returns, 24-byte string returns, and large user-defined structs.

### 2.3 Stack Alignment
The ABI mandates that the stack pointer `%rsp` must be **16-byte aligned** immediately before any `call` instruction:
$$(\%rsp + 8) \equiv 0 \pmod{16}$$

- **Implementation in `frame.rs`:**
  - In [`FrameLayout::compute`](file:///crates/vanta_codegen_x86_64/src/frame.rs), the frame size is computed such that:
    $$(\text{callee\_count} \times 8 + \text{frame\_size}) \equiv 0 \pmod{16}$$
  - For functions pushing an odd number of stack arguments, an explicit 8-byte padding adjustment (`sub rsp, 8`) is inserted prior to pushing arguments.

### 2.4 Callee-Saved vs. Caller-Saved Registers
- **Callee-Saved Registers:** `%rbx`, `%r12`, `%r13`, `%r14`, `%r15`, `%rbp`, `%rsp`.
- **Audit Findings:**
  - Vanta's linear-scan register allocator ([`reg_alloc.rs`](file:///crates/vanta_codegen_x86_64/src/reg_alloc.rs)) tracks all callee-saved registers assigned to virtual registers (`used_callee_saved`).
  - Prologue pushes only the used callee-saved registers:
    ```x86asm
    push %rbp
    mov %rbp, %rsp
    push %r15
    push %r14
    ...
    sub %rsp, <frame_size>
    ```
  - Epilogue restores them in strict reverse order before restoring `%rbp` and executing `ret`:
    ```x86asm
    lea -<callee_bytes>(%rbp), %rsp
    pop %r14
    pop %r15
    pop %rbp
    ret
    ```

---

## 3. Tail-Call Optimization (TCO) Safety Audit

### 3.1 Self-Tail Recursion Invariants
- Converts recursion into a loop backedge inside [`crates/vanta_mir/src/opt/tail_call.rs`](file:///crates/vanta_mir/src/opt/tail_call.rs).
- Parameter updates are scheduled via parallel `Phi` nodes and `Store` instructions.
- Guaranteed $O(1)$ stack usage verified by 500,000-deep recursion tests without stack exhaustion.

### 3.2 Sibling Tail-Call Lowering Invariants
Sibling tail-calls replace a `call; ret` sequence with a `jmp` after dismantling the caller's stack frame. To prevent memory corruption, the following guards are enforced in [`instruction_select.rs`](file:///crates/vanta_codegen_x86_64/src/instruction_select.rs):

1. **Stack Pointer Escape Guard (`stack_ptrs`):**
   - If an argument points to a stack slot in the caller's frame (e.g. `SlotAddr`, `FieldAddr`, or `IndexAddr`), tail-call optimization is **strictly rejected**.
   - Dismantling the stack frame before calling the callee would leave the callee reading invalidated memory (which would cause use-after-free or invalid pointer frees in drop glue).
2. **Aggregate Memory Return (`sret`) Guard:**
   - Functions with memory returns ($> 16$ bytes) cannot be sibling tail-called because the hidden return buffer belongs to the caller's frame.
3. **OS Entry (`main`) Guard:**
   - Sibling tail-calls from `main` are prohibited. The operating system expects the return status code of `main` in `%rax` (0 for void `main`). Direct jumping to external functions would leak arbitrary return values into the process exit code.
4. **Callee-Saved Restoration Order:**
   - Callee-saved registers (`%rbx`, `%r12`..`%r15`) are restored prior to dismantling `%rbp`.

---

## 4. Memory Safety Boundaries

Vanta combines static borrow-checking with dynamic runtime enforcement:

| Subsystem | Guarantee Type | Enforcement Mechanism | Failure Behavior |
|---|:---:|---|---|
| Use-after-move | Static | Compile-time affine type tracking in `vanta_sema` | Diagnostic `E0302` |
| Multiple mutable borrows | Static | Compile-time disjoint borrow tracking in `vanta_sema` | Diagnostic `E0305` |
| Mutation while borrowed | Static | Compile-time shared/exclusive lock analysis | Diagnostic `E0306` |
| Array indexing | Dynamic | Mandatory bounds check emitted before `IndexPtrAddr` | Runtime panic `vanta_bounds_check_fail` (exit 101) |
| Dynamic slice slicing | Dynamic | UTF-8 boundary validation & range check in `slice_bytes` | Returns `Option::None` |
| Heap memory deallocation | Dynamic | Drop glue emitted by compiler; `free` called once | Safe deallocation |
| Pointer arithmetic | Static | Raw pointer arithmetic is unsupported in safe Vanta | Disallowed by syntax |
| Integer division by zero | Dynamic | Native CPU hardware trap or panic check | Exit status 136 (SIGFPE) / Panic |

---

## 5. Audit Conclusion

The System V AMD64 ABI and native calling conventions in Vanta are conformant with Linux x86-64 standards. Tail-call optimizations enforce strict escape-analysis guards, and memory management guarantees are rigorously enforced by compile-time borrow checking and runtime bounds verification.
