# Milestone 31.5 Baseline Preflight Report

**Project:** Venta  
**Milestone:** 31.5 — Native Platform Validation & Linker Correctness Hardening  
**Base Commit:** `6319f20` (`Implement Milestone 31: Venta v2.0 Self-Contained Native Linker & Multi-Object Static Archives`)  
**Base Release Tag:** `v1.4.0-alpha.1` (commit `ff20713`)  
**Date:** 2026-10-30  

---

## 1. Verified Baseline State

1. **Repository & Workspace**:
   - Working tree clean on branch `main` at commit `6319f20`.
   - `crates/vanta_linker` implemented as a zero-external-dependency library crate.
   - All workspace crates build cleanly (`cargo check --workspace` -> 0 errors, 0 warnings).
   - Linker unit tests pass (`cargo test -p vanta_linker` -> 11 passed).
   - Linker end-to-end tests pass (`cargo test --test linker_e2e_tests -p vanta_driver` -> 5 passed).
   - Language ABI tests pass (`cargo test --test abi_tests -p vanta_driver` -> 9 passed).

2. **Linux x86-64 ELF64**:
   - Pure-Rust static ELF64 executable generation with `ET_EXEC`, `PT_LOAD`, `PT_GNU_STACK` (NX).
   - Synthetic static runtime implements `_start`, `print_str`, `print_i64`, `vanta_panic`, `vanta_bounds_check_fail`, `vanta_alloc`, `vanta_free`, `vanta_realloc`, and `vanta_cpu_has_*` via direct Linux syscalls.
   - Tested natively on Linux (WSL2 Ubuntu) with exit code 42 for both single-object and multi-object static archive (`libmath.a`) scenarios. Zero dynamic dependencies.

3. **Windows x86-64 PE32+**:
   - Emits valid 64-bit PE32+ console executables (`0x140000000` ImageBase, `0x020B` Optional Header, `IMAGE_FILE_MACHINE_AMD64` `0x8664`).
   - Synthesizes `.idata` import table linking `ExitProcess`, `GetStdHandle`, and `WriteFile` from `KERNEL32.dll`.
   - Synthesizes `.reloc` base relocations in 4KB blocks with `IMAGE_REL_BASED_DIR64`.
   - Preserves `.pdata` and `.xdata` SEH unwind function tables.
   - Tested natively on Windows host: both single-object (`win_test.exe`) and multi-object with static archive (`libmath.lib` -> `win_multi.exe`) executed natively with exit code 42.

4. **Apple Silicon ARM64 Mach-O**:
   - Emits dyld-compatible `MH_EXECUTE` binaries (`0x100000000` base, 16KB page alignment) with `__PAGEZERO` (4GB), `__TEXT`, `__DATA`, `__LINKEDIT`, `LC_LOAD_DYLINKER` (`/usr/lib/dyld`), `LC_MAIN`, `LC_LOAD_DYLIB` (`/usr/lib/libSystem.B.dylib`), and `LC_SYMTAB`.
   - Validated binary structure and headers. Physical execution requires native Apple Silicon hardware.

5. **Static Archives**:
   - Pure-Rust `ar` and `.lib` parser and deterministic writer.
   - Handles GNU/SysV `/` symbol index, BSD `__.SYMDEF`, GNU `//` long filenames, and BSD `#1/len` extended filenames.
   - Implements lazy archive member extraction.

---

## 2. Identified Platform Gaps & M31.5 Hardening Goals

1. **Native Multi-Platform CI**:
   - Current CI (`.github/workflows/release-validation.yml`) only tests Ubuntu Linux.
   - Need comprehensive `.github/workflows/native-platform-ci.yml` running across `ubuntu-24.04`, `windows-2022`, and `macos-14` (Apple Silicon ARM64 runner).
2. **Linux ELF64 Linker Hardening**:
   - Expand multi-object tests to include global variables, duplicate locals across objects, weak/strong precedence, recursion, monomorphization, and trait objects with dynamic vtables.
   - Support GNU/SysV 64-bit symbol indexes (`/SYM64/`) for large archives.
   - Independent verification of all 6 ELF relocation types with positive/negative addends, range limits, and overflow checks.
   - GDB debugging validation on internally linked multi-object static binaries.
3. **Windows PE32+ Hardening**:
   - Native Windows console output verification via synthesized `WriteFile` / `GetStdHandle` or runtime hooks.
   - Verification of base relocations and `.pdata`/`.xdata` unwind structures under native Windows tooling.
4. **Apple Silicon Mach-O Hardening**:
   - Verification of paired `ARM64_RELOC_PAGE21` and `ARM64_RELOC_PAGEOFF12` relocations.
   - Clear distinction between cross-generated structure verification and physical Apple Silicon execution.
5. **Archive Conformance & Parser Robustness**:
   - Conformance test suite testing archives from GNU `ar`, BSD `ar`, and Venta `ArchiveWriter`.
   - Deterministic archive output test (bitwise hash comparison).
   - Malformed-input fuzzing and edge cases with zero panics.
6. **External Linker Subprocess Audit**:
   - Automated test verifying that `options.linker == LinkerKind::Internal` spawns 0 external compiler, linker, or archiver subprocesses.
7. **Linker Benchmarks**:
   - Reproducible benchmark across small, medium, and archive-heavy workloads.
