# Changelog

All notable changes to the Vanta programming language and compiler toolchain will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased] - 2026-10-30 (Milestone 27 — Native DWARF 5 Debug Information for GDB/LLDB)

**First Feature Milestone for v1.2 Debugging & Tooling**

### Added
- **Native DWARF 5 Section Generation (`crates/vanta_codegen_x86_64/src/dwarf.rs`)**:
  - Implemented pure-Rust DWARF 5 generator emitting `.debug_info`, `.debug_abbrev`, `.debug_line`, and associated relocations (`.rela.debug_info`, `.rela.debug_line`) directly into relocatable ELF64 objects and executables without external backends.
  - Implemented standard DWARF 5 compilation unit header (`DW_UT_compile = 0x01`, version 5, pointer size 8).
  - Assigned registered language code `DW_LANG_Venta = 0x8001` (32769) within the DWARF 5 user-defined vendor range (`0x8000`–`0xffff`).
- **DWARF 5 Line Number Program (`DwarfLineTable`)**:
  - Emitted DWARF 5 line table with 0-indexed directory and file format descriptors (`DW_LNCT_path` via `DW_FORM_string`, `DW_LNCT_directory_index` via `DW_FORM_udata`).
  - Implemented standard and extended line program opcodes: `DW_LNE_set_address`, `DW_LNS_advance_pc`, `DW_LNS_advance_line`, `DW_LNS_set_file`, `DW_LNS_set_column`, `DW_LNS_copy`, and `DW_LNE_end_sequence`.
  - Mapped machine instruction code offsets directly to source lines and columns.
- **Function & Variable DIE Emission (`DwarfInfoWriter`)**:
  - Emitted `DW_TAG_subprogram` DIEs for functions with `DW_AT_name`, `DW_AT_decl_file`, `DW_AT_decl_line`, `DW_AT_low_pc`, `DW_AT_high_pc`, `DW_AT_type`, and `DW_AT_frame_base = [DW_OP_reg6]` (`%rbp`).
  - Emitted `DW_TAG_formal_parameter` and `DW_TAG_variable` DIEs with `DW_AT_name`, `DW_AT_decl_file`, `DW_AT_decl_line`, `DW_AT_type`, and stack-relative evaluation via `DW_AT_location = [DW_OP_fbreg, -offset]`.
  - Emitted `DW_TAG_base_type` DIEs for primitive scalar types (`i64`, `i32`, `i16`, `i8`, `u64`, `u32`, `u16`, `u8`, `f64`, `f32`, `bool`).
- **Source Span Propagation in MIR & Machine IR**:
  - Extended `MirFunction` to retain `span`, `slot_spans`, `val_spans`, and `terminator_spans`.
  - Added `MachineInst::SourceLoc { file_id, line, column }` pseudo-instruction, emitting source coordinates into the line table at function entry, statement boundaries, and block terminators without emitting bytes into `.text`.
- **ELF64 Writer Debug Extensions (`crates/vanta_codegen_x86_64/src/elf_writer.rs`)**:
  - Added support for `.debug_info`, `.debug_abbrev`, `.debug_line`, `.rela.debug_info`, and `.rela.debug_line` sections with proper section symbols and relocation linkages.
  - Zero debug overhead when debug info is disabled, preserving identical release ELF outputs.
- **CLI Options**:
  - Added `-g` and `--debug` flags to `ventac`, `vanta build`, and `vanta run`.
- **Comprehensive Debugger Verification Test Suite (`crates/vanta_driver/tests/dwarf5_tests.rs`)**:
  - 7 automated tests covering ELF section structure, `readelf --debug-dump=info` DWARF 5 validation, line table format verification, default no-debug omission, GDB source line breakpoints, GDB stepping, GDB local/argument inspection (`print`, `info args`, `info locals`), GDB 3-frame call stack unwinding (`bt`), and LLDB batch debugging with source listing.
- **Documentation (`docs/dwarf5-debugging-guide.md`)**:
  - Comprehensive debugging guide for GDB and LLDB covering breakpoints, source stepping, variable inspection, stack backtraces, and known limitations.

---

## [1.1.0-alpha.6] - 2026-10-30 (Milestone 26.5 — AVX-512 8-Lane Loop Auto-Vectorization & Strip-Mining)

**Sixth Feature Milestone for v1.1 SIMD & Vectorization**

### Added
- **512-bit AVX-512 8-Lane Loop Auto-Vectorization (`crates/vanta_mir/src/opt/vectorize.rs`)**:
  - Extended conservative MIR loop vectorizer to detect eligible counted loops and strip-mine them into 8-wide 512-bit EVEX vector operations under `--target-feature=+avx512f`.
  - Added `VectorPattern::IntegerAdd512` (8-lane 64-bit packed integer addition emitting `venta_simd_vpaddq512`).
  - Added `VectorPattern::FloatFma512` (8-lane double-precision FMA emitting `venta_simd_vfmadd231pd512`).
  - Integrated `has_avx512f` into `VectorizeTargetConfig` wired directly to `CompilerOptions::target_features.avx512f`.
- **8-Lane Strip-Mining & Scalar Remainder Handling**:
  - Implemented dynamic threshold check $\text{count} \ge 8$ in `vector.check` block; short loop counts ($N < 8$) branch directly to `scalar_header` without executing vector code.
  - Formulated vector bound $\text{vector\_end} = \text{init} + (\text{count} \;\&\; -8)$, preventing out-of-bounds array access in the final vector iteration.
  - Implemented induction variable step of 8 ($i_{\text{vec}} \mathrel{+}= 8$) in `vector.body`.
  - Wired vector termination into scalar remainder loop, correctly computing leftovers $N \pmod 8 \in \{1..7\}$.
- **Compile-Time Trip-Count Width Adaptation**:
  - Statically known loops of length $4 \le N < 8$ adaptively downgrade to 4-lane 256-bit AVX2 vectorization (`IntegerAdd256`) when `has_avx2` is available.
  - Statically known loops with $N \ge 8$ or dynamic bounds vectorize to 8-lane AVX-512.
- **Sound Memory Legality & Alias Verification**:
  - Formulated runtime non-overlap guard for 8-lane operations: $\text{out} + 8 \times \text{count} \le a \;\lor\; a + 8 \times \text{count} \le \text{out}$.
  - Explicitly permitted exact in-place updates ($\text{out} == a$).
  - Rejected loops with loop-carried cross-iteration recurrences or unsupported internal calls.
- **Hierarchical Target Gating & Fallback**:
  - `--target-feature=+avx512f`: emits EVEX 512-bit ZMM vector loops.
  - `--target-feature=+avx2`: emits VEX 256-bit YMM vector loops.
  - Baseline target: preserves scalar execution with zero SIMD instructions.
  - `--no-vectorize`: forces scalar execution regardless of target CPU features.
- **Integration Test Suite (`crates/vanta_driver/tests/vectorize_avx512_tests.rs`)**:
  - 15 automated integration tests covering eligibility, EVEX disassembly, fallback to AVX2, `--no-vectorize` flag, exact 8 lanes, short boundaries (0, 1, 7), remainders 1, 7, 17, in-place updates, negative/overflow semantics, FMA MIR transformation, large lengths (67), baseline fallback, recurrence rejection, and multi-length differential validation (0..33).
- **Benchmark Suite (`benchmarks/run_vectorize_benchmarks.py`)**:
  - Expanded benchmark comparing Scalar vs AVX2 256-bit vs AVX-512 512-bit across Short (8 lanes), Medium (64 lanes), and Long (1024 lanes).
  - Truthfully detects host CPU hardware capabilities, skipping native ZMM execution with clear diagnostic reporting when run on non-AVX-512 hosts.

---

## [1.1.0-alpha.5] - 2026-10-30 (Milestone 26.4 — AVX-512 ZMM Register Allocation, Spilling & System V Vector ABI)

**Fifth Feature Milestone for v1.1 SIMD & Vectorization**

### Added
- **Expanded Physical ZMM Vector Register File & Allocation (`crates/vanta_codegen_x86_64/src/simd_alloc.rs`)**:
  - Extended linear scan register allocator to allocate virtual 512-bit vector values across 30 physical ZMM registers (`zmm0`–`zmm29`).
  - Allocated dedicated scratch registers `zmm30` (`SCRATCH_A_512`) and `zmm31` (`SCRATCH_B_512`) for spilling and reloads.
  - Retained strict physical register aliasing: `xmmN`, `ymmN`, and `zmmN` share the unified physical register index `SimdReg(N)` ($0 \le N < 32$).
  - Added width-aware vector allocation (`VectorWidth::V512` vs `VectorWidth::V256`) and dynamic spill slot sizing.
- **Dynamic 64-Byte Stack Frame Realignment & Epilogue Tear-down (`crates/vanta_codegen_x86_64/src/frame.rs`)**:
  - Implemented dynamic 64-byte stack alignment in function prologues (`and rsp, -64`, opcode `48 83 e4 c0`) whenever 512-bit parameters, local variables, return values, or spills are active.
  - Enforced 64-byte frame size alignment (`(total + 63) & !63`) and sequential 64-byte spill slot placement.
  - Protected caller stack arguments (`[rbp + 16 + off]`) from dynamic `rsp` changes, with robust epilogue frame recovery via `rbp` (`lea rsp, [rbp - callee_bytes]` or `mov rsp, rbp`).
- **Intel EVEX Compressed Displacement ($Disp8 \times N$) (`crates/vanta_codegen_x86_64/src/encoding.rs`)**:
  - Implemented full EVEX memory operand encoder (`emit_evex_mem_operand`) adhering to the Intel EVEX compressed displacement specification ($N = 64$ for 512-bit full vectors, $N = 32$ for 256-bit vectors).
  - Automatically emits 1-byte ModR/M disp8 when offsets are divisible by $N$ within $[-128, 127]$, and uncompressed ModR/M disp32 (mod=10) when offsets are not divisible by $N$ (e.g. caller stack parameter ingestion at `16(%rbp)`).
- **System V AMD64 512-bit Vector Calling Convention (`crates/vanta_codegen_x86_64/src/abi.rs`)**:
  - Implemented `AbiClass::Vector512` classification for 64-byte vector types (`v512`, `Simd512`, and 64-byte aligned structs).
  - First 8 vector arguments passed in physical registers `zmm0` through `zmm7`.
  - 9th and subsequent vector arguments passed on stack with guaranteed 64-byte alignment (`offset = (offset + 63) & !63; offset += 64`).
  - Single vector return in `zmm0`.
  - Independent integer and vector register assignment counters, allowing arbitrary interleaving of scalar and 512-bit vector parameters.
- **Intrinsic Lowering & Standard Library Primitives (`library/std/simd.va`, `crates/vanta_sema`)**:
  - Added `pub struct v512` (64-byte aligned struct with 8 64-bit lanes `q0`..`q7`).
  - Added direct vector ABI intrinsics: `venta_simd_vec_load512`, `venta_simd_vec_store512`, `venta_simd_vec_add512`, `venta_simd_vec_fma512`.
  - Added virtual SIMD register value intrinsics: `venta_simd_reg_load512`, `venta_simd_reg_store512`, `venta_simd_reg_add512`, `venta_simd_reg_fma512`.
- **Runtime CPUID & OSXSAVE/XCR0 Validation (`runtime/vanta_rt.c`)**:
  - Extended `vanta_cpu_has_avx512f()` to verify CPUID Leaf 7 EBX bit 16, CPUID Leaf 1 ECX bit 27 (OSXSAVE), and XCR0 mask `0xE6` via `xgetbv` before executing 512-bit vector instructions.
  - Implemented bidirectional C ABI oracle function `vanta_c_vector_add512(__m512i a, __m512i b) -> __m512i`.
- **Comprehensive Integration Test Suite (`crates/vanta_driver/tests/simd_avx512_tests.rs`)**:
  - 7 automated tests covering code generation, dynamic 64-byte alignment, 30-register ZMM utilization, 64-byte aligned spill slots, 9-vector parameter exhaustion, mixed scalar/vector arguments, GCC C ABI oracle interoperability, and negative target-feature gating.

---

## [1.1.0-alpha.4] - 2026-10-29 (Milestone 26.3 — Loop Auto-Vectorization & Strip-Mining)

**Fourth Feature Milestone for v1.1 SIMD & Vectorization**

### Added
- **Conservative Loop Auto-Vectorization Pass (`crates/vanta_mir/src/opt/vectorize.rs`)**:
  - Implemented analysis of single-latch natural loops, canonical induction variables with step `+1`, loop bounds (`<` and `<=`), and independence.
  - Added loop body instruction analysis recognizing contiguous array/slice element operations without loop-carried dependencies.
  - Implemented vectorization patterns:
    - 256-bit packed 64-bit integer addition (`VectorPattern::IntegerAdd256`, emitting `venta_simd_vpaddq256`).
    - 256-bit packed double-precision fused multiply-add (`VectorPattern::FloatFma256`, emitting `venta_simd_vfmadd231pd256`).
  - Added loop-invariant input hoisting (`ensure_available` and `ensure_base_available`), moving read-only slot loads and constants to preheaders/check blocks to maintain SSA dominance invariants.
- **Dynamic Vector Strip-Mining & Remainder Loop Architecture**:
  - Split eligible loops into a dynamically gated vector pre-check (`vector.check`), 4-wide SIMD vector loop (`vector.header`, `vector.body`), and scalar remainder loop (`scalar_header`, `scalar_body`, `scalar_latch`).
  - Dynamic iteration threshold check: evaluates `count >= 4` and dynamically routes short trip counts ($N < 4$) and remainders ($N \pmod 4 \in \{1, 2, 3\}$) to the scalar fallback.
  - Scalar remainder loop preserves exact language semantics, including array bounds checking and overflow handling.
- **Comprehensive Memory Dependency & Alias Analysis**:
  - Emitted runtime pointer non-overlap checks (`out + count*8 <= a || a + count*8 <= out`) in `vector.check` to guarantee vectorization safety across independent array slices.
  - Verified in-place vector updates (`out == a`) as safe for parallel lane evaluation.
  - Protected independent stack slots from cross-aliasing.
- **CPU Target-Feature Gating & Differential CLI Control**:
  - Strictly gated SIMD loop emission on target features: AVX2 for `vpaddq` and AVX+FMA for `vfmadd231pd`. Baseline targets (`x86_64-v1`) preserve scalar execution.
  - Added `--no-vectorize` (and `--no-vec`) compiler flags in `crates/vanta_driver/src/main.rs` and `crates/vanta_driver/src/vanta_cli.rs` for differential verification.
- **Throughput Verification & Benchmark Suite**:
  - Built comprehensive test suite in `crates/vanta_driver/tests/vectorize_tests.rs` with 13 automated tests covering exact lengths, remainders 1..3, short/zero trip counts, in-place updates, negative/boundary values, disabled flags, baseline targets, and machine-code disassembly inspection.
  - Implemented reproducible performance benchmark (`benchmarks/run_vectorize_benchmarks.py`) demonstrating a **6.3x to 6.7x speedup** over scalar loops on array addition kernels.

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
