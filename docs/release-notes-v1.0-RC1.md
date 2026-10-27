# Vanta v1.0-RC1 Release Notes

**Release Identifier:** `v1.0-RC1`  
**SemVer Version:** `1.0.0-rc.1`  
**Date:** October 28, 2026  
**Auditor / Release Manager:** Vanta Release Engineering  
**Primary Supported Target:** Linux x86-64 (`x86_64-unknown-linux-gnu`)

---

## 1. Release Overview & Candidate Purpose

Vanta **v1.0-RC1 (Release Candidate 1)** is the first feature-complete release candidate of the Vanta systems programming language, compiler toolchain, and runtime environment.

Vanta is engineered specifically as a performance-first, memory-safe systems programming language featuring its own complete compiler backend:
- Custom High-Level / Mid-Level Intermediate Representation (MIR) in SSA form.
- Custom Chaitin-Briggs graph-coloring register allocator.
- Custom x86-64 instruction selector, machine code encoder, and ELF64 object writer.
- **Zero dependency on LLVM, Cranelift, or external compiler backends.**

The purpose of this candidate is to freeze the core language specification, ABI, and optimization pipeline for external evaluation by systems developers and AI tooling integrators.

---

## 2. Supported Platforms & System Requirements

### 2.1 Target Architecture
- **Supported Target**: Linux x86-64 (`x86_64-unknown-linux-gnu`) with System V AMD64 ABI.
- **Minimum Operating System**: Linux kernel $\ge 4.15$ (Ubuntu 20.04+, Debian 11+, Fedora 34+, Arch Linux, or WSL2).
- **Required System Tools**: `gcc` or `clang` (used strictly as an ELF linker), `libc.so.6`.

### 2.2 Host Build Requirements (Building from Source)
- **Rust Toolchain**: `rustc` $\ge 1.80.0$, `cargo` (edition 2021).
- **C Compiler**: `gcc` $\ge 9.0$ or `clang` $\ge 10.0$.
- **Python**: $\ge 3.8$ (optional, required only for running the automated benchmark suite).

---

## 3. Installation & Uninstallation Guide

### 3.1 Pre-Built Binary Installation
1. Download the release archive and checksum:
   ```bash
   # Download archive and sha256 checksum
   vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz
   vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz.sha256
   ```
2. Verify archive integrity:
   ```bash
   sha256sum -c vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz.sha256
   # Output must be: vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz: OK
   ```
3. Extract the release tarball:
   ```bash
   tar -xzf vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz
   cd vanta-v1.0-RC1-x86_64-unknown-linux-gnu
   ```
4. Run the installer (defaults to `~/.local`):
   ```bash
   ./install.sh
   # Or install to a custom path:
   ./install.sh --prefix=/opt/vanta
   ```
5. Ensure `bin` is on your `$PATH`:
   ```bash
   export PATH="$HOME/.local/bin:$PATH"
   vanta --version
   # Emits: vanta 1.0.0-rc.1
   ```

### 3.2 Uninstallation
To cleanly purge all installed binaries, libraries, and standard library files:
```bash
./uninstall.sh
# Or with a custom prefix:
./uninstall.sh --prefix=/opt/vanta
```
The uninstallation script reads `.install_manifest.txt` and removes only files that were placed during installation.

---

## 4. Implemented Features in v1.0-RC1

### 4.1 Language Semantics & Type System
- **Affine Ownership & Borrowing**: Single-owner semantics with affine moves. References (`&T` and `&mut T`) statically enforced by compile-time borrow checker preventing data races and iterator invalidation.
- **Primitive Types**: 64-bit signed integers (`i64`), 64-bit IEEE floats (`f64`), booleans (`bool`), unit type (`()`).
- **Composite Types**:
  - Structs with named fields and explicit memory layout.
  - Enums with variant payloads (tagged unions).
  - Dynamic slices (`&[T]`) with runtime bounds checking.
  - Heap-allocated strings (`String`) and string slices (`&str`).
  - Heap-allocated vectors (`Vec<T>`).
- **Pattern Matching**: Complete pattern matching with exhaustive variant verification, tuple destructuring, and wildcards (`_`).
- **Inherent Methods**: Methods declared on structs via `impl StructName { fn method(self: &Self) ... }` and `self: &mut Self`.
- **Module System**: Hierarchical file-based modules via `import module;` and `import std::io;`.

### 4.2 Standard Library (`std`)
- `std::io`: Standard output writing (`write_stdout`, `flush_stdout`).
- `std::fs`: File system operations (`read_to_string`, `write`).
- `std::env`: Command-line argument retrieval (`args() -> Result<Vec<String>, i64>`).
- `std::result::Result`: `Result::Ok(T)` and `Result::Err(E)`.
- `std::option::Option`: `Option::Some(T)` and `Option::None`.
- `std::string::String`: Owned string construction, slicing, and length queries.
- `std::iter::Iterator`: Iterable abstractions.

### 4.3 Native SSA Optimization Pipeline (`-O1`)
- **Mem2Reg**: Promotes stack allocations into SSA virtual registers with dominance frontier phi placement.
- **Function Inlining**: Inlines direct leaf functions and call candidates under instruction budget limits.
- **GVN & CSE**: Global Value Numbering folds congruent subexpressions.
- **Constant Propagation**: Propagates compile-time constants through arithmetic trees and basic blocks.
- **SROA**: Scalar Replacement of Aggregates decomposes non-escaping structs into scalar SSA variables.
- **LICM**: Hoists loop-invariant computations into preheader blocks.
- **IVO & LSR**: Normalizes canonical induction variables and converts multiplications into increments.
- **Loop Unroll & Peel**: Replicates loop bodies (factor 4) for constant trip counts.
- **Instruction Scheduling**: Dependency-aware instruction reordering.
- **TCO**: Eliminates self-tail-recursion and sibling tail-calls.
- **Global DCE**: Prunes dead functions and static symbols, reducing binary footprint by ~17%.
- **Branch-Aware Layout & Alignment**: Emits fall-through branches and aligns loop headers to 16 bytes.

### 4.4 Toolchain & Developer Experience
- `vanta run <file.va> [-- <args>...]`: Compiles and executes code in memory.
- `vanta compile <file.va> [-o <out>]`: Produces a native ELF64 standalone binary.
- `vanta check <file.va> [--json]`: Rapid validation without linking.
- `vanta fmt [files...] [--check]`: Canonical source code formatter.

---

## 5. Verified Test Suites & Validation Evidence

All components have been tested across 30 test suites (290+ tests) with 100% pass rate:
- **Full Workspace Tests**: `cargo test --workspace` (0 failures).
- **ABI Conformance Tests**: `cargo test --test abi_tests` (System V AMD64 calling conventions).
- **Differential Tests**: `cargo test --test differential_tests` (bit-for-bit equivalence against GCC `-O2`).
- **Fuzzing Harness**: `cargo test --test fuzz_tests` (1,800 fuzzed inputs across lexer, parser, sema, and MIR verifier).
- **Packaging Tests**: `cargo test --test packaging_tests` (clean-prefix install, build, run, and uninstall).
- **Benchmark Suite**: 18 workloads evaluated over 5 trials with Median and MAD statistics.

---

## 6. Known Limitations & Roadmap

1. **Host and Target Platform Scope**:
   - Vanta v1.0-RC1 exclusively targets **Linux x86-64**.
   - macOS (Mach-O) and Windows (PE/COFF) native codegen remain experimental and require WSL2 or Linux virtualization.
2. **SIMD & Auto-Vectorization**:
   - Vanta v1.0-RC1 emits scalar 64-bit integer and floating-point instructions. It does not implement auto-vectorization for 256-bit AVX2/AVX-512 instructions. On numerical loop kernels where GCC and LLVM vectorize, GCC and Rust will outperform Vanta by $5\times\text{--}8\times$.
3. **Generics**:
   - Generic types (`Vec<T>`, `Option<T>`, `Result<T, E>`) are currently supported for standard library types. Arbitrary user-defined generic type parameters and trait bounds are planned for v1.1.
4. **External Linker Dependency**:
   - Vanta emits valid ELF64 relocatable object files (`.o`), but relies on the host system linker (`ld` or `gcc`) to resolve libc symbols and emit the final executable header.

---

## 7. Reporting Issues & Contributing

We welcome reproducible issue reports from external evaluators:
- **Repository**: `https://github.com/vanta-lang/vanta`
- **Issue Tracker**: Submit bug reports with the following template:
  1. Operating system and kernel version (`uname -srm`).
  2. Package checksum (`sha256sum`).
  3. Compiler version (`vantac --version`).
  4. Minimal reproducible Vanta source code (`repro.va`).
  5. Exact command executed (`vanta run repro.va` or `vantac -O1 repro.va`).
  6. Observed output vs. expected output.
