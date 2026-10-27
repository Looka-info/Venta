# Vanta: Native Performance Roadmap & Benchmark Specification
**Date:** October 27, 2026  
**Status:** Approved Engineering Directive  
**Target:** Closing the 1.0x–6.3x Compute Performance Gap Against Rust & C

---

## 1. Executive Summary & Measured Baseline

Vanta's custom x86-64 backend delivers remarkable compile-time efficiency (~0.25s per compilation) and minimal binary footprints (~22 KB). Furthermore, on memory-allocation-dominated workloads (`string_growth` and `vec_churn`), Vanta's lean C runtime wrapper performs on par with Rust and GCC (`1.00x` to `1.18x`).

However, on pure compute, loop-heavy, and recursive workloads (`collatz` and `fib`), **Vanta is between 3.4x and 6.3x slower than Rust (`-O3`) and GCC (`-O3`)**.

This document outlines the exact architectural reasons for this gap and establishes a four-phase performance engineering roadmap to achieve competitive parity with Rust without introducing LLVM or Cranelift.

---

## 2. Benchmark Reproduction & Empirical Evidence

### 2.1 Benchmark Environment
- **CPU:** AMD Ryzen 5 5600 6-Core Processor (12 Threads, 3.5 GHz base, 4.4 GHz boost)
- **OS:** Linux 6.18 x86_64 (WSL2 Ubuntu 24.04)
- **Compilers:** `vantac` 0.1.0, `rustc` 1.93.1 (`-O`), `gcc` 15.2.0 (`-O3`)
- **Harness:** `benchmarks/run_benchmarks.py` (5 measured runs after 1 warm-up run)

### 2.2 Measured Results

| Benchmark | Workload Characteristic | Vanta `-O0` | Vanta `-O1` | Rust `-O3` | GCC `-O3` | Vanta / Rust Ratio |
|---|---|---:|---:|---:|---:|:---:|
| `fib` | Recursive call overhead ($N=38$) | 0.5256s | 0.4959s | 0.1440s | 0.1426s | **3.44x slower** |
| `collatz` | Tight integer loop & branches ($N=500\text{k}$) | 0.6928s | 0.6825s | 0.1080s | 0.1275s | **6.32x slower** |
| `points` | Struct creation & Manhattan distance ($N=1\text{M}$) | 0.0944s | 0.0904s | 0.0793s | 0.0790s | **1.14x slower** |
| `vec_churn` | Heap vector growth & traversal ($200 \times 10\text{k}$) | 0.0906s | 0.0908s | 0.0770s | 0.0776s | **1.18x slower** |
| `string_growth` | In-place string reallocation ($2000 \times 10$) | 0.0792s | 0.0780s | 0.0782s | 0.0776s | **1.00x (Parity)** |
| **Binary Size** | Typical executable footprint | **21,952 B** | **21,952 B** | 11,809,232 B | 15,984 B | **538x smaller** |
| **Compile Time**| Latency to produce native executable | **0.25s** | **0.25s** | 0.40s–1.03s | 0.20s–0.23s | **1.6x–4.1x faster** |

---

## 3. Root Cause Analysis: Why Vanta Loses on Compute

Inspecting Vanta's MIR optimization pipeline (`crates/vanta_mir/src/opt`) and machine code generator (`crates/vanta_codegen_x86_64/src/instruction_select.rs`) reveals four structural bottlenecks:

### 3.1 Bottleneck 1: Stack-Spilled Values (No Register Allocator)
In Vanta's backend, every `ValueId` temporary is assigned a stack slot (`-offset(%rbp)`).
For example, a simple loop accumulation `sum = sum + i`:
```nasm
; Vanta emitted assembly
mov -16(%rbp), %r10      ; Load sum from stack
add -24(%rbp), %r10      ; Add i from stack
mov %r10, -32(%rbp)      ; Store intermediate to stack
mov -32(%rbp), %r10      ; Reload intermediate
mov %r10, -16(%rbp)      ; Store into sum slot
```
In contrast, Rust and GCC allocate `%rax`, `%rcx`, `%rdx`, `%rbx`, `%rsi`, `%rdi`, `%r8`-`%r15` exclusively to loop variables:
```nasm
; Rust / GCC emitted assembly
add %rsi, %rax           ; 100% register residency, zero memory reads/writes
inc %rsi
```
Every Vanta arithmetic operation incurs $2\times$ to $4\times$ memory roundtrips through the L1 cache.

### 3.2 Bottleneck 2: Lack of Function Inlining
In `fib` and `collatz_len`, calls always emit a raw `call` instruction preceded by full argument spilling. For small leaf functions, call/ret overhead, argument marshaling, and stack frame alignment dominate execution time.
Rust and GCC inline these calls completely, converting recursion into loops or unrolling them.

### 3.3 Bottleneck 3: Unoptimized Integer Arithmetic & Strength Reduction
In `collatz`:
- Checking `n % 2 == 0` emits `cqo` + `idivq` (cost: 15–20 cycles). Rust and GCC emit `test $1, %reg` (cost: 1 cycle).
- Dividing `n = n / 2` emits `cqo` + `idivq`. Rust and GCC emit `sar $1, %reg` (cost: 1 cycle).

### 3.4 Bottleneck 4: Redundant Bounds Checking in Sequential Loops
In array and slice indexing (`arr[i]`), Vanta emits a strict bounds-checking branch dominating every element access. When traversing an array sequentially (`0..len`), Rust and GCC prove statically that `i < len` is an invariant and eliminate the check entirely.

---

## 4. The Four-Phase Optimization Roadmap

```
┌────────────────────────────────────────────────────────────────────────┐
│ Phase 1: Linear-Scan Register Allocator (Estimated Gain: 2.5x–3.5x)    │
│ Allocate Callee-Saved & Scratch Registers (%rbx, %r12-%r15, %r10-%r11) │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│ Phase 2: MIR Inlining & Tail-Call Optimization (Estimated Gain: 1.5x)  │
│ Inline leaf functions (< 15 instructions); eliminate recursive frames │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│ Phase 3: Strength Reduction & Peephole Optimizer (Estimated Gain: 1.4x)│
│ Replace pow2 div/rem with bitwise AND/SAR; hoist loop invariants       │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼─────────────────────────────────────┐
│ Phase 4: Monotonic Bounds Check Elimination (Estimated Gain: 1.2x)     │
│ Eliminate bounds checks on induction loops over slices and vectors     │
└────────────────────────────────────────────────────────────────────────┘
```

### Phase 1: Linear-Scan Register Allocator (Target: Milestone 15)
- **Objective:** Eliminate stack memory spills for temporary virtual values (`ValueId`).
- **Implementation Strategy:**
  1. Compute live ranges for all `ValueId`s within each basic block and loop body.
  2. Reserve 6 callee-saved registers (`%rbx`, `%r12`, `%r13`, `%r14`, `%r15`) and 4 caller-saved registers (`%rcx`, `%rdx`, `%rsi`, `%rdi`) for high-frequency virtual values.
  3. Spill to stack slots only when physical register pressure exceeds 10 active values.
- **Expected Outcome:** Closes ~60% of the compute performance gap in `collatz` and `points`.

### Phase 2: MIR Function Inlining (Target: Milestone 16)
- **Objective:** Eliminate function call overhead for small helper methods and constructors.
- **Implementation Strategy:**
  1. Calculate AST / MIR instruction count per function.
  2. Automatically inline functions with fewer than 20 MIR instructions (e.g. `is_empty`, `len`, `manhattan`).
  3. Inline monomorphized trait methods.
- **Expected Outcome:** Brings `points` and `fib` closer to native Rust speeds.

### Phase 3: Strength Reduction & Peephole Machine Code Optimization (Target: Milestone 17)
- **Objective:** Replace high-latency CPU instructions with single-cycle operations.
- **Implementation Strategy:**
  1. In `crates/vanta_mir/src/opt/const_prop.rs`, transform `x % 2` to `x & 1` and `x / 2` to `x >> 1` for positive signed integers.
  2. Implement a machine-instruction peephole pass in `crates/vanta_codegen_x86_64` to eliminate redundant `mov %reg, %reg` and load-after-store sequences.
- **Expected Outcome:** 3x improvement on `collatz`.

### Phase 4: Monotonic Bounds Check Elimination (Target: Milestone 18)
- **Objective:** Eliminate redundant panic branches inside sequential loops.
- **Implementation Strategy:**
  1. Identify induction loops where loop variable `i` starts at 0 and increments monotonically to `len`.
  2. Emit a single loop-invariant assertion outside the loop header.
  3. Eliminate per-iteration conditional jumps and panic calls inside the loop body.
- **Expected Outcome:** Slice and vector traversal speed matches Rust `-O3`.

---

## 5. Performance Release Criteria

### Criteria for Public Alpha:
- **No workload slower than 3.0x of Rust `-O3`** (currently `collatz` is 6.32x slower).
- Linear-scan register allocation implemented for basic blocks.
- Compilation speed maintains < 0.35s latency.

### Criteria for Public Beta:
- **No workload slower than 1.5x of Rust `-O3`**.
- Function inlining implemented for functions < 20 instructions.
- Strength reduction for power-of-2 integer operations.

### Criteria for Vanta v1.0:
- **Measured parity with Rust `-O3` ($\pm 10\%$) on defined systems benchmark suite**.
- Selected memory and allocation workloads outperform Rust due to lean runtime and zero panic unwinding overhead.
- Binary sizes remain < 50 KB for standard CLI utilities.
