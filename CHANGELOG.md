# Changelog

All notable changes to the Vanta programming language and compiler toolchain will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.0-rc.1] - 2026-10-28

**Release Candidate 1 for Linux x86-64 (`x86_64-unknown-linux-gnu`)**

This release marks the feature-complete release candidate of the Vanta language, custom compiler pipeline, optimization framework, and native runtime.

### Added
- **Language Features**:
  - Strongly typed systems language with affine ownership, move semantics, and borrow checker.
  - Primitive scalar types: `i64`, `f64`, `bool`, `()`.
  - Aggregates: Structs with named fields, nested struct layouts, and enum tagged unions.
  - Pattern matching: Exhaustive `match` expressions with destructuring and wildcard patterns.
  - Inherent methods: `impl Type { fn method(self: &Self) ... }` and `self: &mut Self` mutation.
  - Modules & Imports: Hierarchical file-based module system (`import path::module;`).
  - Standard Library: `std::io` (stdout write, flush), `std::fs` (read_to_string, write), `std::env` (args), `std::result::Result`, `std::option::Option`, `std::string::String`, `std::iter::Iterator`.
  - Owned heap strings and UTF-8 string slices (`&str`).
  - Heap-allocated dynamic vectors (`Vec<T>`).
- **Custom Native Compiler Backend (No LLVM / No Cranelift)**:
  - Custom SSA-based Mid-Level Intermediate Representation (MIR).
  - Graph-coloring physical register allocator (Chaitin-Briggs) with spill weight estimation.
  - System V AMD64 ABI compliant function calling convention (register packing, XMM0, hidden `sret` returns, 16-byte stack alignment).
  - Custom x86-64 instruction selector, machine code encoder, and ELF64 object writer.
  - C static runtime archive (`libvanta_rt.a`) providing allocator, drop glue, and panic handlers.
- **Optimization Pipeline (`-O1`)**:
  - Memory-to-Register promotion (`Mem2Reg`) with dominance frontier phi placement.
  - Bounded leaf and direct call function inlining with interprocedural analysis.
  - Global Value Numbering (GVN) and Common Subexpression Elimination (CSE).
  - Constant Propagation and compile-time arithmetic folding.
  - Scalar Replacement of Aggregates (SROA) for non-escaping structs.
  - Loop Invariant Code Motion (LICM) with clean preheader creation.
  - Induction Variable Optimization (IVO) and Loop Strength Reduction (LSR).
  - Loop Unrolling (factor 4) and Loop Peeling for constant bounds.
  - Dependency-aware Instruction Scheduling.
  - Self and Sibling Tail-Call Optimization (TCO).
  - Global Dead Code Elimination (Global DCE) pruning unreferenced static symbols and unreachable functions.
  - Trace-scheduled basic block layout and 16-byte loop header alignment (`.p2align 4`).
- **Developer Tooling & AI-First Diagnostics**:
  - Unified CLI driver `vanta` (`run`, `compile`, `check`, `fmt`).
  - Low-level compiler CLI `vantac`.
  - Machine-actionable JSON diagnostic output (`--json`) with precise line, column, byte offset, and error codes (`E0001`–`E0607`).
  - Source code formatter `vanta fmt` with canonical formatting rules.
- **Verification & Testing Infrastructure**:
  - Strengthened MIR verifier enforcing slot bounds, phi predecessors, value defs, and phi-before-non-phi ordering.
  - Multi-paradigm differential test suite verifying bit-identical output against GCC `-O2`.
  - Bounded deterministic fuzzing harness testing lexer, parser, semantic analyzer, and MIR verifier over 1,800 synthetic malformed inputs.
  - Standalone release packaging script and automated prefix installation test suite.

### Changed
- Unified workspace versioning to `1.0.0-rc.1` (SemVer 2.0.0 compliant) mapped to Git tag `v1.0-RC1`.
- Made tarball creation deterministic and bit-for-bit reproducible (`--sort=name --mtime --owner=0 --group=0 gzip -n`).
- Standardized CLI flag handling (`-h`, `--help`, `-V`, `--version`) across all subcommands.

### Known Limitations
- Supported host and target platform is Linux x86-64 (`x86_64-unknown-linux-gnu`). Windows and macOS native codegen remain experimental.
- SIMD / auto-vectorization is not yet implemented; the backend emits scalar 64-bit integer instructions.
