# Vanta: Release Stage Gates & Quality Criteria (v0.1 to v1.0)
**Date:** October 27, 2026  
**Status:** Approved Quality Standard  
**Current Milestone:** Milestone 14 Complete (Owned Strings & UTF-8 Slices)  
**Current Release Stage:** Private Developer Preview (Experimental Compiler)

---

## 1. Executive Summary & Purpose

To prevent premature release and ensure engineering rigor, this document defines the explicit, verifiable **Release Gates** that Vanta must satisfy to advance through each stage of its lifecycle toward a production-grade **v1.0 General Availability**.

Each gate specifies mandatory pass/fail criteria across six architectural dimensions:
1. **Language & Compiler Correctness** (Type checker, borrowing, ABI, drop glue)
2. **Native Performance & Codegen** (Throughput, register allocation, latency)
3. **Standard Library & Runtime** (I/O, collections, networking, FFI)
4. **Developer & AI Experience** (Diagnostics, CLI, LSP, formatting)
5. **Installation & Distribution** (Zero-dependency binary distribution)
6. **Platform Support** (Linux, macOS, Windows native targets)

A release cannot advance to a higher tier while any mandatory gate criteria remain blocked or unverified.

---

## 2. Release Stage Definitions & Gate Matrices

```
+----------------------------+      Gate 1      +----------------------------+
|  Private Developer Preview |  ------------->  |        Public Alpha        |
|      (Current State)       |                  | (Controlled Early Adopters)|
+----------------------------+                  +----------------------------+
                                                              |
                                                              | Gate 2
                                                              v
+----------------------------+      Gate 3      +----------------------------+
|   v1.0 General Release     |  <-------------  |        Public Beta         |
|  (Production / Rust Rival) |                  | (API Freeze & Performance) |
+----------------------------+                  +----------------------------+
```

---

### Gate 1: Progression from Private Preview to Public Alpha

**Target Audience:** Advanced systems developers, compiler enthusiasts, AI agent researchers.  
**Objective:** Allow third-party developers to install and run Vanta without cloning or compiling the Rust compiler source tree.

| Dimension | Mandatory Gate Requirement | Verification Method | Status |
|---|---|---|:---:|
| **Installation** | Standalone installer (`install.sh`) downloading precompiled static `vantac` + `libvanta_rt.a`. No Rust/Cargo required. | Clean Docker container install test (`ubuntu:latest`) | **Blocked** |
| **CLI Usability** | `vantac --version` and `vantac --help` return 0 with structured metadata. Empty `main() -> ()` exits with 0. | CLI regression test suite | **Blocked** |
| **ABI Correctness** | Support System V AMD64 `sret` (hidden return pointer) for aggregates >16 bytes (`String` returned by value). | Unit tests returning `String` / large structs by value | **Blocked** |
| **Standard Library** | `std::io` (print, println, stdin reading), `std::fs` (read_to_string, write), `std::env::args`. | Real CLI file-processing application | **Blocked** |
| **Diagnostics** | Machine-readable JSON diagnostics (`--diagnostics=json`) conforming to JSON Schema v1.0. | Schema validation test against compile-fail suite | **Passed** |
| **Memory Safety** | 100% leak-free heap vectors and strings verified by Valgrind/ASan in integration tests. | Automated Valgrind CI run | **Passed** |
| **Compiler Test Suite** | >= 250 automated compiler and runtime tests passing with zero failures. | `cargo test --workspace` | **Passed** (130/130) |

---

### Gate 2: Progression from Public Alpha to Public Beta

**Target Audience:** Production software teams, automated AI code generation agents, library authors.  
**Objective:** Stable language semantics, competitive performance (within 1.5x of Rust), and rich developer tooling.

| Dimension | Mandatory Gate Requirement | Verification Method | Status |
|---|---|---|:---:|
| **Performance** | Basic Linear-Scan Register Allocator implemented; compute benchmarks (`fib`, `collatz`) within **1.5x of Rust `-O3`**. | `benchmarks/run_benchmarks.py` CI tracking | **Blocked** |
| **Safety** | Runtime bounds checks on all slice and array index operations (`panic` on out-of-bounds). | Negative runtime test cases in test suite | **Blocked** |
| **C FFI** | `extern "C"` foreign function declarations and C ABI calling convention support. | Integration with `libcurl` or `sqlite3` | **Blocked** |
| **Tooling** | Language Server Protocol (`vanta-lsp`) supporting diagnostics, hover types, and go-to-definition. | VS Code extension automated test suite | **Blocked** |
| **Packaging** | Multi-file package manager and dependency resolver (`vanta-pkg` or built-in `vanta build`). | Building multi-crate test workspace | **Blocked** |
| **Platform Coverage** | Linux x86-64 and macOS AArch64 (Apple Silicon) tier-1 support. | Native CI runners on Linux & macOS | **Blocked** |
| **Optimizer** | Function inlining for small leaf functions and dead code elimination at `-O1`. | MIR optimization pass verification | **Partial** |

---

### Gate 3: Progression from Public Beta to v1.0 General Availability

**Target Audience:** Enterprise systems, high-concurrency infrastructure, mission-critical AI backend pipelines.  
**Objective:** Production memory safety guarantees, performance parity or superiority over Rust/C, full ecosystem.

| Dimension | Mandatory Gate Requirement | Verification Method | Status |
|---|---|---|:---:|
| **Language Stability** | Formal 1.0 Language Specification with backwards-compatibility guarantee and semantic versioning. | RFC process and spec freeze review | **Blocked** |
| **Performance** | Graph-coloring / advanced linear-scan register allocator, loop-invariant code motion, SIMD vectorization. **0.95x–1.05x Rust `-O3`**. | Industry benchmark suite (Computer Language Benchmarks Game) | **Blocked** |
| **Concurrency** | Safe concurrency model (thread spawn, channels, atomics, Send/Sync trait equivalents). | Multithreaded stress test suite (TSan clean) | **Blocked** |
| **Self-Hosting / Backend** | Standalone object linking without requiring host `gcc` (pure-Rust ELF linker or direct LLVM-free binary emission). | Clean environment binary generation | **Blocked** |
| **Native Windows** | Native Windows x86-64 PE/COFF backend and Microsoft x64 ABI support (no WSL2 required). | Windows 11 native build and test | **Blocked** |
| **Standard Library** | Comprehensive `std`: networking (`TcpStream`, `UdpSocket`), time, threading, collections (`HashMap`, `BTreeMap`). | Standard library unit test coverage >= 90% | **Blocked** |
| **Security Audit** | Independent formal verification and security audit of the memory model, borrow checker, and runtime allocator. | Third-party security audit report | **Blocked** |

---

## 3. Prioritized Issue & Implementation Roadmap

To systematically advance Vanta from its current state through Gate 1 (Public Alpha) and Gate 2 (Public Beta), work is divided into concrete engineering milestones ordered strictly by risk reduction and user impact.

```mermaid
flowchart TD
    M14[Milestone 14: Owned Strings - COMPLETE] --> M15[Milestone 15: ABI Fixes & CLI Polish]
    M15 --> M16[Milestone 16: Precompiled Runtime & Standalone Installer]
    M16 --> M17[Milestone 17: Linear-Scan Register Allocator]
    M17 --> M18[Milestone 18: Standard I/O, File System & CLI Args]
    M18 --> Alpha{Gate 1: Public Alpha Release}
    Alpha --> M19[Milestone 19: Language Server Protocol - LSP]
    M19 --> M20[Milestone 20: Bounds Checks & C FFI]
    M20 --> Beta{Gate 2: Public Beta Release}
```

---

### Phase 1: Near-Term Gate 1 Milestones (Path to Public Alpha)

#### Milestone 15: ABI Stability, Process Exit & CLI Correctness (Immediate Priority)
- **Problem:**
  - Returning structs >16 bytes by value (e.g. `String`, 24 bytes) corrupts caller memory because Vanta lacks the System V AMD64 `sret` convention.
  - `vantac --version` exits with an error.
  - `fn main() -> ()` does not zero `%rax` before returning to libc, leaking arbitrary syscall results as the exit code.
- **Tasks:**
  1. Implement `sret` in `crates/vanta_codegen_x86_64/src/frame.rs` and `instruction_select.rs`: caller allocates stack space and passes pointer in `%rdi`; callee copies return struct to `(%rdi)` and returns `%rdi` in `%rax`.
  2. Implement proper flag handling for `--version` and `-V` in `crates/vanta_cli/src/main.rs`.
  3. Emit `xor %eax, %eax` before `ret` in entry point functions that return unit `()`.
- **Exit Criteria:** `String` returned by value compiles and runs cleanly; `vantac --version` prints `vantac 0.1.0`; CLI programs exit with status 0.

#### Milestone 16: Precompiled Runtime & Standalone Installer
- **Problem:**
  - Building any Vanta program invokes host `gcc` on `vanta_rt.c`. If `gcc` is missing, compilation fails.
  - Users cannot install Vanta without Rust and Cargo.
- **Tasks:**
  1. Add build script in `crates/vanta_driver` to precompile `runtime/vanta_rt.c` into `libvanta_rt.a`.
  2. Bundle `libvanta_rt.a` into release tarballs or embed binary archive bytes into `vantac`.
  3. Create GitHub Actions release workflow publishing `vantac-x86_64-unknown-linux-gnu.tar.gz`.
  4. Write `scripts/install.sh` for one-command curl-to-sh installation in `/usr/local/bin` and `/usr/local/lib/vanta`.
- **Exit Criteria:** A clean Ubuntu Docker container with no Rust/Cargo installs and builds a Vanta project with one curl command.

#### Milestone 17: Linear-Scan Register Allocator
- **Problem:**
  - Every single `ValueId` is spilled to stack memory (`-offset(%rbp)`). Pure compute loops (`collatz`, `fib`) are 3.5x–6.3x slower than Rust and GCC.
- **Tasks:**
  1. Implement liveness analysis on MIR control flow graphs (`crates/vanta_mir/src/liveness.rs`).
  2. Implement linear-scan register allocator allocating general-purpose registers (`%rbx`, `%r12`, `%r13`, `%r14`, `%r15`) across basic blocks.
  3. Emit callee-saved register push/pop in function prologues and epilogues.
- **Exit Criteria:** `collatz` benchmark executes within **1.5x of Rust `-O3`**; `fib` benchmark executes within **1.8x of Rust `-O3`**.

#### Milestone 18: Basic Standard I/O, File System & CLI Arguments
- **Problem:**
  - Vanta cannot read files, receive command line arguments, or read user input, preventing the development of practical tools.
- **Tasks:**
  1. Add runtime C primitives: `vanta_fs_read_to_string`, `vanta_fs_write`, `vanta_io_read_line`, `vanta_env_args_count`, `vanta_env_arg_get`.
  2. Implement Vanta modules: `std::fs`, `std::io`, `std::env`.
  3. Add automated tests reading and writing files and parsing arguments.
- **Exit Criteria:** An end-to-end grep-like utility written in pure Vanta compiles and runs against real files.

---

### Phase 2: Medium-Term Gate 2 Milestones (Path to Public Beta)

#### Milestone 19: Language Server Protocol (`vanta-lsp`)
- **Deliverable:** Real-time semantic checking, hover documentation, and auto-formatting in VS Code, Neovim, and Cursor.
- **Architecture:** Standalone JSON-RPC 2.0 daemon built on `tower-lsp` or custom pure-Rust server wrapping `crates/vanta_frontend`.
- **AI Agent Benefit:** AI coding agents (Cline, Cursor, Copilot) receive instant diagnostic feedback during editing cycles without running batch compilation.

#### Milestone 20: Array Bounds Checks & Foreign Function Interface (C FFI)
- **Deliverable:** Safe array/slice indexing with guaranteed panic on out-of-bounds (`SIGABRT` with source line context); `extern "C"` blocks enabling direct invocation of standard C dynamic and static libraries.
- **Safety Impact:** Closes the final primary memory safety loophole in Vanta's array handling.

---

## 4. Release Evaluation Scorecard Summary

| Evaluated Dimension | Weight | Score (1–5) | Weighted Score | Stage Classification |
|---|:---:|:---:|:---:|---|
| **Language & Safety Semantics** | 25% | **3.8** / 5.0 | 0.95 | Solid memory model, affine ownership, borrow checking, drop glue. |
| **Native Performance & Optimization** | 20% | **2.2** / 5.0 | 0.44 | Stack spill bottleneck (1.0x–6.3x gap vs Rust); lacks regalloc. |
| **Standard Library & Real Usability** | 20% | **1.8** / 5.0 | 0.36 | No file I/O, stdin, networking, or FFI; only 5 std modules. |
| **AI-First Design & Diagnostics** | 15% | **4.2** / 5.0 | 0.63 | Outstanding deterministic formatting and structured JSON diagnostics. |
| **Installation & Distribution** | 10% | **1.2** / 5.0 | 0.12 | Requires Rust, Cargo, git, and WSL; no binary packages or scripts. |
| **Platform & Tooling Ecosystem** | 10% | **2.6** / 5.0 | 0.26 | Linux x86-64 only; no LSP, debugger integration, or native Windows. |
| **TOTAL WEIGHTED SCORE** | **100%** | — | **2.76 / 5.00** | **Private Developer Preview / Experimental** |
