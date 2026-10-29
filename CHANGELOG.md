# Changelog

All notable changes to the Vanta programming language and compiler toolchain will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.1.0-alpha.3] - 2026-10-29 (Milestone 26.2 — System V AMD64 Vector ABI)

**Third Feature Milestone for v1.1 SIMD & Vectorization**

### Added
- **System V AMD64 256-Bit Vector ABI Classification & Calling Convention**:
  - Implemented `AbiClass::Vector256` classification for 256-bit SIMD vector types (`v256`, `Simd256`, `v256i`, `v256d`, and mangled variants) in `crates/vanta_codegen_x86_64/src/abi.rs`.
  - Configured independent register bank assignment: up to 8 vector arguments are passed in `%ymm0` through `%ymm7` without consuming general-purpose integer registers (`%rdi`, `%rsi`, `%rdx`, `%rcx`, `%r8`, `%r9`), enabling full compliance with Section 3.2.3 of the System V AMD64 ABI specification.
  - Implemented vector return value classification: functions returning 256-bit vectors place the result in `%ymm0`.
- **Vector Register Exhaustion & 32-Byte Aligned Stack Passing**:
  - Added stack fallback allocation when more than 8 vector parameters are passed.
  - Enforced 32-byte alignment for stack-passed vector arguments (`offset = (offset + 31) & !31; offset += 32`), guaranteeing that all vector arguments passed on the stack satisfy x86-64 32-byte memory alignment requirements.
- **Callee Frame Parameter Ingestion & Epilogue Return**:
  - Extended callee frame layout in `crates/vanta_codegen_x86_64/src/frame.rs` to detect incoming vector parameters, vector returns, and 32-byte stack slots, automatically setting `requires_32byte_alignment = true`.
  - Generated callee prologue stores to spill incoming register arguments (`%ymm0`..`%ymm7`) and reloads from 32-byte aligned stack slots.
  - Handled 256-bit vector returns in `crates/vanta_codegen_x86_64/src/instruction_select.rs` (`Terminator::Return`), loading from register, spill slots, or dereferencing pointers into `%ymm0`.
- **Direct Dereferencing for 32-Byte Vector Aggregate Stores**:
  - Updated `Instruction::Store` in `instruction_select.rs` so that assigning to a 32-byte vector slot dereferences pointer-backed source operands via `vmovups` (loading 32 bytes from `[ptr]` and storing 32 bytes to `[slot]`) instead of moving an 8-byte pointer address.
- **Bidirectional C Interoperability & Verification Suite**:
  - Verified cross-language calling convention with GCC: Venta caller invokes a C callee passing mixed integers and vectors, and C caller invokes an exported Venta callee returning 256-bit vectors in `%ymm0`.
  - Added 5 end-to-end integration tests in `crates/vanta_driver/tests/simd_tests.rs`:
    - `test_simd_vector_abi_direct_passing_and_returning`: Direct function call passing and returning 256-bit vectors in `%ymm0`.
    - `test_simd_vector_abi_multiple_args_fma`: Multiple vector arguments passed across `%ymm0`, `%ymm1`, `%ymm2` computing parallel FMA.
    - `test_simd_vector_abi_mixed_int_and_vector`: Interleaved integer (`%rdi`, `%rsi`) and vector (`%ymm0`, `%ymm1`) arguments confirming orthogonal register banks.
    - `test_simd_vector_abi_register_exhaustion_stack_fallback`: Passing 10 vector arguments (8 in `%ymm0`..`%ymm7`, 2 on 32-byte aligned stack).
    - `test_simd_vector_abi_c_oracle_bidirectional`: Bidirectional ABI interoperability between Venta and C compiled with GCC.

### Fixed
- **Instruction Scheduler RAW/WAR Memory Hazards**:
  - Resolved a critical bug in `crates/vanta_mir/src/opt/instruction_schedule.rs` where field loads (`LoadIndirect`) were reordered before preceding struct writes (`Store`), reading uninitialized memory. Added memory dependency edges between `WriteSlot` and `ReadIndirect` / `WriteIndirect` to enforce strict program order.
- **SROA Decomposition Protection**:
  - Excluded 256-bit vector types from SROA struct decomposition in `crates/vanta_mir/src/opt/sroa.rs` and protected direct vector stores/loads from improper slot removal.

---

## [1.1.0-alpha.2] - 2026-10-29 (Milestone 26.1 — Vector Register Allocation & Spilling)

**Second Feature Milestone for v1.1 SIMD & Vectorization**

### Added
- **Two-Tier Physical YMM Register Allocator**:
  - Implemented deterministic linear scan allocation for virtual SIMD values in pure Rust (`crates/vanta_codegen_x86_64/src/simd_alloc.rs`).
  - Introduced `SimdValueId` and `SimdOperand::Val(SimdValueId)` distinguishing virtual vector values before allocation from physical registers (`SimdOperand::Reg(SimdReg)`).
  - Configured physical allocatable pool over `ymm0` through `ymm13` (14 physical registers) with dedicated `ymm14` and `ymm15` scratch/reload registers.
  - Decoupled SIMD allocation from scalar GPR allocation (`reg_alloc.rs`), preventing scalar and vector live ranges from corrupting each other's register files.
- **Dynamic 32-Byte Stack Realignment & Aligned Vector Spilling**:
  - Implemented dynamic runtime stack realignment (`and rsp, -32`, encoded as `48 83 e4 e0`) in function prologues when SIMD spills are present.
  - Allocated 32-byte aligned spill slots (`[rsp + 32*k]`), completely eliminating unaligned 256-bit memory access faults.
  - Preserved `rbp` as unchanged frame anchor for scalar locals and parameter offsets, with clean stack restoration via `mov rsp, rbp` across all return paths.
- **System V AMD64 Volatility & Call Boundary Spilling**:
  - Enforced caller-saved volatility for all vector registers across function calls.
  - Automatically detected live intervals spanning call instructions and forced them to stack spill slots with reload generation upon subsequent use.
- **High Register Pressure Eviction**:
  - Implemented furthest-end-point spill eviction when simultaneous virtual SIMD values exceed the 14-register allocatable pool.
- **Verification Suites**:
  - Added 6 unit tests in `simd_alloc.rs` validating empty allocations, pool bounds, register reuse, call crossing spills, high-pressure eviction, and deterministic output.
  - Added 3 end-to-end integration tests in `crates/vanta_driver/tests/simd_tests.rs`: `test_simd_virtual_register_allocation`, `test_simd_call_preservation_and_spilling`, and `test_simd_high_register_pressure_spilling`.

---

## [1.1.0-alpha.1] - 2026-10-29 (Milestone 26 — Native SIMD Instruction Emission)

**First Feature Milestone for v1.1 SIMD & Vectorization**

### Added
- **Project-Owned Native SIMD Instruction Encodings**:
  - Implemented genuine machine code emission in pure Rust (`crates/vanta_codegen_x86_64/src/encoding.rs`) without external assembler, LLVM, or Cranelift dependencies.
  - Added support for 2-byte VEX (`0xC5`), 3-byte VEX (`0xC4`), and 4-byte EVEX (`0x62`) instruction prefixes.
  - Implemented `vmovups`: Unaligned packed-vector loads and stores across 128-bit (XMM), 256-bit (YMM / AVX), and 512-bit (ZMM / EVEX) vector widths.
  - Implemented `vpaddq`: Packed 64-bit integer quadword additions across 128-bit, 256-bit (AVX2), and 512-bit (AVX-512F EVEX) vector widths.
  - Implemented `vfmadd231pd`: Packed double-precision floating-point fused multiply-add ($dest = dest + (src1 \times src2)$) across 128-bit, 256-bit (AVX + FMA), and 512-bit (AVX-512F EVEX) vector widths.
- **CPU Target Feature Configuration & Gating**:
  - Introduced `TargetFeatures` modeling in `crates/vanta_codegen_x86_64/src/target.rs`: `avx`, `avx2`, `fma`, and `avx512f` feature flags.
  - Provided target presets: `baseline`, `avx`, `avx2`, `avx2_fma`, and `avx512`, as well as parsing `--target-cpu` (`x86-64-v1`..`x86-64-v4`, `native`) and `--target-feature` (`+avx2,+fma`).
  - Added compile-time validation in `CodeEmitter::encode_instruction_checked`, cleanly rejecting SIMD emission with structured diagnostics when target features are absent.
  - Baseline x86-64 builds strictly default to `baseline` without assuming AVX/AVX2/AVX-512, preserving deterministic baseline execution.
- **Driver and CLI Options**:
  - Added `--target-feature=<features>` and `--target-cpu=<cpu>` flags to both `vanta` and `vantac` CLI drivers.
  - Supported `-C target-feature=...` and `-C target-cpu=...` flags.
- **Runtime CPUID Probing & Standard Library**:
  - Added runtime CPUID detection in `runtime/vanta_rt.c`: `vanta_cpu_has_avx()`, `vanta_cpu_has_avx2()`, `vanta_cpu_has_fma()`, `vanta_cpu_has_avx512f()`.
  - Recompiled and updated native archive `runtime/libvanta_rt.a`.
  - Created `library/std/simd.va` exposing runtime feature checks and high-level 256-bit vector operations (`simd_add_i64_x4`, `simd_fma_f64_x4`, `simd_load_u256`, `simd_store_u256`).
- **Comprehensive Verification Suites**:
  - Direct machine-code oracle tests in `crates/vanta_codegen_x86_64/tests/simd_encoding_tests.rs` verifying exact byte sequences against GNU assembler oracles for VEX and EVEX forms.
  - Integration execution suite in `crates/vanta_driver/tests/simd_tests.rs` compiling and executing native programs with hardware gating and negative target feature rejection checks.

---

## [1.0.0] - 2026-10-28

**General Availability Release for Linux x86-64 (`x86_64-unknown-linux-gnu`)**

- **Status**: Formally certified and approved for General Availability following Milestone 27 evaluation and release gate audit.
- **Scope**: Verified on Linux x86-64, System V AMD64 ABI compliant, custom 14-pass SSA optimizer, pure Rust x86-64 instruction encoder and ELF64 object writer.

## [1.0.0-rc.2] - 2026-10-28 (Milestone 28 Hardening)

**Release Candidate 2 for Linux x86-64**

### Fixed
- **CI Version Gate Repair**: Fixed a build-blocking failure in `.github/workflows/release-validation.yml` where a hardcoded string check (`1.0.0-rc.1`) and brittle `.packages[0].version` metadata selector rejected release candidate 2 (`1.0.0-rc.2`).
- **Dynamic Release Artifact Parameterization**: Configured the workflow to dynamically derive archive filenames and checksum manifests from verified SemVer metadata.
- **CLI Driver Option Parsing**: Resolved an issue where `vanta check <file> --json` and `vanta compile <file> --json` rejected `--json` as an unknown option. Direct `--json`, `--diagnostics=json`, and `--format=json` are now accepted across all subcommands (`run`, `compile`, `check`) in both `vanta` and `vantac`.
- **Diagnostic Output Formatting**: Ensured structured JSON output containing exact diagnostic error codes, spans, and `success` boolean statuses is emitted directly to stdout.

### Added
- **Version Consistency Verifier**: Added `scripts/verify_version_consistency.py` validating that all 9 workspace crates have synchronized versions conforming to SemVer 2.0.0 release candidate (`1.0.0-rc.N`) or GA (`1.0.0`) formats.
- **Version Gate Regression Suite**: Added unit test suite `scripts/test_version_validation.py` (8 test cases) and integration test `crates/vanta_driver/tests/version_gate_tests.rs`.
- **Self-Hosted Compiler Verification & Hardening**:
  - Audited the Stage 0 -> Stage 1 bootstrap chain for `tools/self_host/vanta_c.va`.
  - Expanded test coverage from 5 to 9 native programs (adding 6-argument System V ABI calls, all comparison operators, nested loops, and operator precedence).
  - Published comprehensive milestone report in `docs/m28-verification-report.md`.

### Documentation
- Updated Evaluation Guide ([`docs/external-evaluation.md`](file:///docs/external-evaluation.md)) to accurately specify error detection schemas for `--json`.
- Established Evaluator Feedback Workflow ([`docs/external-feedback.md`](file:///docs/external-feedback.md)) and issue template ([`.github/ISSUE_TEMPLATE/evaluator-feedback.md`](file:///.github/ISSUE_TEMPLATE/evaluator-feedback.md)).
- Published Feedback Triage Report ([`docs/feedback-triage.md`](file:///docs/feedback-triage.md)), GA Decision Report ([`docs/v1.0-ga-decision.md`](file:///docs/v1.0-ga-decision.md)), and Post-v1.0 Roadmap ([`docs/post-v1.0-roadmap.md`](file:///docs/post-v1.0-roadmap.md)).

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
