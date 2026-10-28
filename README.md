<div align="center">

# ⚡ Vanta Programming Language

**A Performance-First, Memory-Safe Systems Programming Language with a Custom SSA Optimizer and Native x86-64 Backend**

[![CI Status](https://github.com/Looka-info/Venta/actions/workflows/release-validation.yml/badge.svg)](https://github.com/Looka-info/Venta/actions)
[![Release Candidate](https://img.shields.io/badge/Release-v1.0--RC2-blue.svg)](https://github.com/Looka-info/Venta/releases)
[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-green.svg)](LICENSE)
[![Target Architecture](https://img.shields.io/badge/Target-Linux%20x86--64-orange.svg)](#)
[![Backend](https://img.shields.io/badge/Backend-Pure%20Rust%20(No%20LLVM)-red.svg)](#)
[![System V AMD64 ABI](https://img.shields.io/badge/ABI-System%20V%20AMD64-purple.svg)](#)

---

> *"Maximum execution speed, predictable resource management, and zero-cost safety — with sub-second compilation times and lightweight native binaries."*

</div>

---

## 📖 Table of Contents

- [Overview](#-overview)
- [Key Features](#-key-features)
- [System Architecture](#-system-architecture)
- [14-Pass SSA Optimization Pipeline](#-14-pass-ssa-optimization-pipeline)
- [Performance Benchmarks](#-performance-benchmarks)
  - [Optimization Speedup Highlights](#optimization-speedup-highlights)
  - [Compilation Speed & Binary Footprint Comparison](#compilation-speed--binary-footprint-comparison)
  - [Full Benchmark Execution Times](#full-benchmark-execution-times)
- [Self-Hosted Native Compiler (`vanta_c.va`)](#-self-hosted-native-compiler-vanta_cva)
- [Language Tour & Examples](#-language-tour--examples)
  - [1. Ownership and Affine Borrowing](#1-ownership-and-affine-borrowing)
  - [2. Enums and Pattern Matching](#2-enums-and-pattern-matching)
  - [3. Traits and Zero-Cost Polymorphism](#3-traits-and-zero-cost-polymorphism)
  - [4. Standard Library and I/O](#4-standard-library-and-io)
- [Command Line Interface (CLI)](#-command-line-interface-cli)
- [Building and Testing](#-building-and-testing)
- [Repository Structure](#-repository-structure)
- [License](#-license)

---

## 🚀 Overview

**Vanta** is a modern systems programming language designed from first principles. Unlike most new programming languages that rely on LLVM or Cranelift, Vanta implements its **own end-to-end toolchain in pure Rust**:

- **Custom Frontend**: Handcrafted recursive descent parser with precedence climbing, strict semantic type checking, and compile-time affine borrow checking.
- **Mid-Level IR (MIR)**: Static Single Assignment (SSA) form with control flow graphs (CFGs), basic blocks, and explicit terminator semantics.
- **14-Pass Optimizer**: Inlining, Global Value Numbering (GVN), Dead Code Elimination (DCE), Loop Invariant Code Motion (LICM), Scalar Replacement of Aggregates (SROA), Loop Unrolling, Loop Peeling, Instruction Scheduling, Tail-Call Optimization (TCO), and Trace-Scheduled Machine Code Layout.
- **Zero-Dependency Native Backend**: Linear-scan physical register allocator, direct x86-64 machine instruction encoder, and pure-Rust ELF64 relocatable object file writer.
- **Self-Hosting Compiler**: A native compiler written in 100% pure Vanta (`tools/self_host/vanta_c.va`) capable of compiling procedural Vanta code directly to System V AMD64 assembly.

---

## ✨ Key Features

| Capability | Description |
|:---|:---|
| **Memory Safety Without GC** | Affine ownership, move semantics, and lexical borrow checking (`&T`, `&mut T`) eliminate dangling pointers, use-after-free, and data races at compile time with zero runtime overhead. |
| **No LLVM / Cranelift Dependency** | Custom x86-64 instruction encoder and ELF64 writer compile directly from MIR to native Linux object files. |
| **Lightning Compilation Speed** | Compiles typical workloads in **~0.24 seconds** — over **8x faster** than `rustc` with LLVM. |
| **Minimal Binary Footprint** | Produces compact, standalone native ELF executables of **~22 KB** (over **500x smaller** than standard Rust release binaries). |
| **System V AMD64 ABI Native** | Direct register calling convention (`rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`, return in `rax`), enabling transparent C library interop and zero FFI marshaling penalties. |
| **AI-First Tooling** | Machine-readable JSON diagnostic output (`--json`) across `check`, `compile`, and `run` subcommands for seamless IDE, CI, and agentic AI pair programming workflows. |

---

## 🏗 System Architecture

```mermaid
flowchart TD
    subgraph Frontend ["Compiler Frontend"]
        SRC["Vanta Source (.va)"] --> LEX["Lexer (vanta_lexer)"]
        LEX --> PARSE["Precedence Parser (vanta_parser)"]
        PARSE --> AST["Abstract Syntax Tree"]
        AST --> SEMA["Semantic Analysis & Borrow Checker (vanta_sema)"]
    end

    subgraph MidEnd ["SSA Mid-Level IR & Optimization (vanta_mir)"]
        SEMA --> LOWER["MIR Lowering (CFG & SSA)"]
        LOWER --> VERIFY1["MIR Invariant Verifier"]
        VERIFY1 --> OPT["14-Pass Optimization Pipeline"]
        OPT --> OPT_PASSES["GVN • SCCP • DCE • Inlining • LICM • LSR • SROA • Unrolling • Peeling • Scheduling • TCO • Block Layout"]
        OPT_PASSES --> VERIFY2["Post-Optimization Verifier"]
    end

    subgraph Backend ["Native x86-64 Backend (vanta_codegen_x86_64)"]
        VERIFY2 --> REGALLOC["Linear Scan Register Allocator"]
        REGALLOC --> ENCODE["x86-64 Machine Code Instruction Encoder"]
        ENCODE --> ELF["ELF64 Relocatable Object Writer (.o)"]
        ELF --> LINK["System V Linker (libvanta_rt.a)"]
        LINK --> BIN["Native Linux ELF64 Binary (~22.5 KB)"]
    end
```

---

## ⚡ 14-Pass SSA Optimization Pipeline

Vanta features an aggressive, verifiable SSA optimization suite implemented in [`crates/vanta_mir/src/opt/`](file:///crates/vanta_mir/src/opt/):

```
       [ MIR Lowering ]
              |
              v
  +-----------------------+
  |  1. Inlining          | -> Interprocedural leaf & small-callee inlining
  |  2. SROA              | -> Scalar Replacement of Aggregates (destructures stack slots)
  |  3. GVN / CSE         | -> Global Value Numbering & Common Subexpression Elimination
  |  4. SCCP              | -> Sparse Conditional Constant Propagation
  |  5. LICM              | -> Loop Invariant Code Motion (hoists loop-invariant loads/ops)
  |  6. IVO               | -> Induction Variable Optimization & Canonicalization
  |  7. LSR               | -> Loop Strength Reduction (expensive ops -> recurrences)
  |  8. Loop Peeling      | -> Separates initial iterations to simplify loop body
  |  9. Loop Unrolling    | -> Replicates loop bodies (full or 2x/4x partial unrolling)
  | 10. Global DCE        | -> Worklist-driven Global Dead Code Elimination
  | 11. Instruction Sched | -> Dependency-aware latency instruction reordering
  | 12. TCO               | -> Self-tail recursion elimination & sibling call lowering
  | 13. Branch Inversion  | -> Fall-through conditional jump optimization
  | 14. Trace Layout      | -> Cache-aware basic block reordering & loop header alignment
  +-----------------------+
              |
              v
     [ MIR Verification ]
```

Every pass can be enabled, disabled, or tuned via dedicated compiler CLI switches (`--no-licm`, `--no-sroa`, `--no-tco`, `--unroll-factor=<N>`, etc.) for differential testing and performance regression analysis.

---

## 📊 Performance Benchmarks

All benchmark data below is independently reproduced and audited from the official test suite in [`benchmarks/results/milestone-25-release-audit.json`](file:///benchmarks/results/milestone-25-release-audit.json) run under controlled hardware isolation (AMD x86-64, Linux 6.6 kernel).

### Optimization Speedup Highlights

```
=================================================================================================
 Optimization Pass Performance Multipliers (Vanta -O1 vs Vanta -O0)
=================================================================================================
 Unroll & Peel Workload : [4.19x Speedup]  ████████████████████████████████████████  (76% faster)
 SROA Workload          : [2.61x Speedup]  ███████████████████████                   (62% faster)
 LICM Workload          : [2.49x Speedup]  ████████████████████                      (60% faster)
 CSE / GVN Workload     : [1.87x Speedup]  ███████████████                           (47% faster)
 Inlining (Leaf Calls)  : [1.85x Speedup]  ██████████████                            (46% faster)
 Loop Strength Red.     : [1.56x Speedup]  ████████████                              (36% faster)
 Tail-Call Opt (TCO)    : [1.49x Speedup]  ███████████                               (33% faster)
 Register Pressure      : [1.25x Speedup]  ████████                                  (20% faster)
 Inlining (Loop Calls)  : [1.22x Speedup]  ███████                                   (18% faster)
 Arithmetic Tree        : [1.18x Speedup]  ██████                                    (15% faster)
=================================================================================================
```

### Compilation Speed & Binary Footprint Comparison

Vanta provides an exceptional balance of rapid iteration, compact executable footprints, and high runtime performance:

| Metric | Vanta (v1.0-RC2) | Rust (`rustc 1.93.1 -O3`) | GCC (`gcc 15 -O3`) | Vanta Advantage |
|:---|:---:|:---:|:---:|:---|
| **Compilation Speed** | **~0.24 s** | ~1.91 s | ~0.28 s | **8x faster compile times than Rust** |
| **Binary Executable Size** | **~22.5 KB** | ~11.8 MB | ~16.0 KB | **520x smaller binary footprint** |
| **Backend Dependencies** | **Zero (Pure Rust)** | LLVM (~100M lines C++) | GCC Backend | **No massive external toolchain** |
| **Memory Safety Model** | **Affine Borrow Check** | Affine Borrow Check | None (Manual Unsafe) | **Memory safety by default** |
| **Intermediate Representation** | **Custom SSA MIR** | MIR + LLVM IR | GIMPLE + RTL | **Fully verifiable custom SSA** |

### Full Benchmark Execution Times

*Median execution time across 5 independent runs per workload (lower is better):*

| Benchmark Workload | Vanta -O0 | Vanta -O1 (Full Opt) | GCC -O3 | Rust -O3 | Optimization Gain |
|:---|:---:|:---:|:---:|:---:|:---:|
| `unroll_peel_workload` | 81.08 ms | **19.36 ms** | 6.12 ms | 7.89 ms | **4.19x** |
| `sroa_workload` | 77.57 ms | **29.75 ms** | 6.03 ms | 7.86 ms | **2.61x** |
| `licm_workload` | 279.83 ms | **112.23 ms** | 58.96 ms | 83.03 ms | **2.49x** |
| `cse_workload` | 86.88 ms | **46.40 ms** | 30.67 ms | 30.80 ms | **1.87x** |
| `inline_leaf` | 80.23 ms | **43.47 ms** | 20.99 ms | 19.52 ms | **1.85x** |
| `lsr_workload` | 71.71 ms | **45.98 ms** | 5.69 ms | 7.90 ms | **1.56x** |
| `tco_workload` | 63.68 ms | **42.70 ms** | 37.00 ms | 43.83 ms | **1.49x** |
| `reg_pressure` | 43.52 ms | **34.88 ms** | 23.03 ms | 19.23 ms | **1.25x** |
| `inline_loop` | 8.97 ms | **7.37 ms** | 5.97 ms | 8.04 ms | **1.22x** |
| `arith_tree` | 17.85 ms | **15.16 ms** | 12.35 ms | 13.44 ms | **1.18x** |
| `collatz` | 336.86 ms | **310.75 ms** | 76.83 ms | 59.89 ms | **1.08x** |
| `points` | 12.63 ms | **11.76 ms** | 9.42 ms | 11.72 ms | **1.07x** |
| `vec_churn` | 22.89 ms | **21.54 ms** | 7.17 ms | 9.81 ms | **1.06x** |
| `string_growth` | 6.26 ms | **6.24 ms** | 6.31 ms | 8.25 ms | **1.00x** |
| `fib` | 325.14 ms | **335.86 ms** | 91.88 ms | 95.62 ms | **0.97x** |

---

## 🛠 Self-Hosted Native Compiler (`vanta_c.va`) & Backend (`backend.va`)

Located in [`tools/self_host/vanta_c.va`](file:///tools/self_host/vanta_c.va) and [`tools/self_host/backend.va`](file:///tools/self_host/backend.va), this is a complete native compiler and modular code generation backend written **100% in pure Vanta**:

```mermaid
flowchart LR
    subgraph Bootstrap ["Stage 0: Bootstrap Compiler"]
        RUST["Rust Reference Compiler (vantac)"]
        VA_SRC["tools/self_host/vanta_c.va"]
        VA_BACKEND["tools/self_host/backend.va"]
        RUST -->|Compiles| VA_SRC
        VA_BACKEND -.->|Modular Import| VA_SRC
        VA_SRC -->|Produces| S1_BIN["Stage 1 Compiler Binary (vanta_c_bin)"]
    end

    subgraph NativePipeline ["Stage 1: Pure Vanta Native Backend Pipeline"]
        S1_BIN -->|Parses AST & Symbols| FRONT["Compiler Frontend"]
        FRONT -->|Delegates to| BACKEND["Vanta-C Native Backend (backend.va)"]
        BACKEND -->|FrameLayout (16-Byte Align)| STACK["System V AMD64 Stack Manager"]
        BACKEND -->|BackendEmitter| ASM["Valid System V AMD64 Assembly (.s)"]
        ASM -->|gcc -no-pie| PROG["Native Standalone Linux Executables"]
    end
```

### Pure Vanta Native Backend Features:
- **Physical Register Map & Calling Convention:** Full System V AMD64 ABI registers (`rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`, `rax`, `rsp`, `rbp`) and condition codes.
- **Strict 16-Byte Stack Frame Alignment:** Dynamic frame layout calculation (`FrameLayout`) guaranteeing compliance across nested function calls without SSE/vector ABI faults.
- **Security Compliant Directives:** Emits `.section .note.GNU-stack,"",@progbits` ensuring non-executable stack flags and zero linker warnings.
- **Machine Code & ELF64 Reference:** Pure Vanta encoding helpers for x86-64 REX prefix bytes, ModR/M addressing bytes, SIB bytes, and ELF64 header metadata.

### Verified Test Suite:
Run via [`scripts/test_self_hosted_compiler.sh`](file:///scripts/test_self_hosted_compiler.sh):
- `backend_test.va`: Pure Vanta native backend unit tests (**PASS**, exit `100`)
- `01_return_42.va`: Basic exit code return (**PASS**, exit `42`)
- `02_arithmetic.va`: Binary expressions & parentheses (**PASS**, exit `40`)
- `03_while_loop.va`: Mutable iteration & accumulation (**PASS**, exit `55`)
- `04_if_else.va`: Multi-function calls & conditional branching (**PASS**, exit `10`)
- `05_fibonacci.va`: Deep recursive function stack preservation (**PASS**, exit `55`)
- `06_call_6_args.va`: Full 6-argument System V register calling sequence (**PASS**, exit `21`)
- `07_comparisons.va`: All relational comparison operations (`==`, `!=`, `<`, `<=`, `>`, `>=`) (**PASS**, exit `63`)
- `08_nested_loops.va`: Nested loops & stack offset management (**PASS**, exit `60`)
- `09_operator_precedence.va`: Multiplicative before additive precedence (**PASS**, exit `28`)
- `10_stack_alignment_and_calls.va`: 16-byte stack alignment across nested function calls (**PASS**, exit `44`)

---

## 💻 Language Tour & Examples

### 1. Ownership and Affine Borrowing
Vanta strictly enforces ownership rules at compile time:

```vanta
struct Buffer {
    capacity: i64,
    size: i64,
}

fn process_buffer(b: &Buffer) -> i64 {
    return b.size;
}

fn consume_buffer(b: Buffer) {
    // b is moved and dropped at end of scope
}

fn main() -> i64 {
    let buf = Buffer { capacity: 1024, size: 42 };
    
    // Immutable borrow: buf remains valid
    let s = process_buffer(&buf);
    
    // Move: ownership transferred
    consume_buffer(buf);
    
    // Compile error if accessed here: E0301 (Use of moved value)
    return s;
}
```

### 2. Enums and Pattern Matching
Algebraic Data Types with payload support:

```vanta
import std::option::Option;

enum ResultStatus {
    Success(i64),
    Failure(i64),
    Pending,
}

fn evaluate(res: ResultStatus) -> i64 {
    match res {
        ResultStatus::Success(val) => {
            return val;
        }
        ResultStatus::Failure(err_code) => {
            return -err_code;
        }
        ResultStatus::Pending => {
            return 0;
        }
    }
}
```

### 3. Traits and Zero-Cost Polymorphism

```vanta
trait Formatter {
    fn format(self: &Self) -> i64;
}

struct Metric {
    value: i64,
}

impl Formatter for Metric {
    fn format(self: &Self) -> i64 {
        return self.value * 10;
    }
}
```

### 4. Standard Library and I/O

```vanta
import std::fs;
import std::io;
import std::string::String;
import std::env;

fn main() -> i64 {
    // Read command line arguments
    match env::args() {
        Result::Ok(args) => {
            if args.len() > 1 {
                print_str("Input: ");
                print_str(args[1].as_str());
                print_str("\n");
            }
        }
        Result::Err(_) => {}
    }

    // File I/O
    match fs::read_to_string("config.txt") {
        Result::Ok(content) => {
            print_str("Read file: ");
            print_str(content.as_str());
        }
        Result::Err(err) => {
            print_str("Error reading file\n");
        }
    }
    return 0;
}
```

---

## 💻 Command Line Interface (CLI)

The `vanta` driver provides a unified interface for checking, compiling, and running code:

```bash
# Type check and borrow check without compiling (ultra fast)
vanta check src/main.va

# Structured JSON diagnostics for IDEs and agentic tools
vanta check src/main.va --json

# Compile to native Linux executable with full optimizations (-O1)
vanta compile src/main.va -O1 -o my_app

# Compile and execute immediately
vanta run src/main.va

# Independent optimization switches for differential testing
vantac src/main.va -O1 --no-licm --no-tco --unroll-factor=4 -o my_app
```

---

## 🔨 Building and Testing

### Prerequisites
- **Linux x86-64** or **WSL2** (Ubuntu 22.04 / 24.04 / 26.04)
- **Rust Toolchain**: 1.80+ (pinned to 1.93.1 in `rust-toolchain.toml`)
- **GCC**: standard C toolchain for linking (`gcc`, `ar`, `make`)
- **Python 3**: for version validation and benchmark harnesses

### Build from Source
```bash
# Clone the repository
git clone https://github.com/Looka-info/Venta.git
cd Venta

# Build compiler binaries in release mode
cargo build --release --bin vantac --bin vanta

# Run full workspace test suite (150+ tests)
cargo test --workspace

# Run specialized ABI and differential tests
cargo test --test abi_tests
cargo test --test differential_tests
cargo test --test version_gate_tests

# Run self-hosted compiler test suite (9 native tests)
./scripts/test_self_hosted_compiler.sh

# Build distribution release package
./scripts/package_release.sh
```

---

## 📁 Repository Structure

```
Venta/
├── Cargo.toml                       # Virtual workspace root
├── rust-toolchain.toml              # Toolchain pinning (stable 1.93.1)
├── crates/
│   ├── vanta_source                 # Source file mapping, Spans, FileId
│   ├── vanta_diagnostics            # Terminal & JSON diagnostic emission
│   ├── vanta_lexer                  # Tokenizer & comment stripper
│   ├── vanta_parser                 # AST & precedence climbing parser
│   ├── vanta_sema                   # Type checker & affine borrow checker
│   ├── vanta_mir                    # SSA MIR, CFG, verifier & 14-pass optimizer
│   ├── vanta_codegen_x86_64         # Linear scan regalloc, x86-64 encoder, ELF64 writer
│   ├── vanta_driver                 # CLI drivers: vantac and vanta
│   └── vanta_fmt                    # Source code formatter
├── runtime/
│   ├── vanta_rt.c                   # Lightweight C runtime (memory, panic, strings)
│   └── libvanta_rt.a                # Static runtime library
├── library/std/                     # Standard library (fs, io, env, string, iter)
├── tools/self_host/
│   ├── vanta_c.va                   # Self-hosting compiler in pure Vanta
│   └── tests/                       # 9 native assembly verification programs
├── benchmarks/                      # 18 benchmark suites & JSON results
├── scripts/                         # Packaging, CI verification, clean-prefix scripts
└── docs/                            # Formal language specs, release gates, reports
```

---

## 📄 License

Vanta is dual-licensed under either:
- **MIT License** ([LICENSE-MIT](LICENSE) or http://opensource.org/licenses/MIT)
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option.
