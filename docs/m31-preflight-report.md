# Milestone 31 Preflight & Platform ABI Audit Report

**Date**: 2026-10-30  
**Project**: Venta v2.0  
**Target Architecture**: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`  
**Base Commit**: `ff20713` (tag `v1.4.0-alpha.1`) on branch `main`  
**Working Tree Status**: Verified clean (0 uncommitted changes)

---

## 1. Repository State Verification
- **Commit History**: Verified that Milestone 30 (`ff20713`) introduced `vanta_target`, `vanta_codegen_aarch64`, PE/COFF `.obj` emission in `vanta_codegen_x86_64`, and cross-platform compilation options in `vanta_driver`.
- **Existing Crates**:
  - `vanta_target`: Target specification model (`Arch`, `Os`, `ObjectFormat`, `TargetAbi`, `TargetTriple`).
  - `vanta_codegen_x86_64`: x86-64 machine instruction encoder, ELF64 relocatable object writer, and COFF relocatable object writer.
  - `vanta_codegen_aarch64`: Pure-Rust AArch64 instruction encoder, AAPCS64 calling convention, and Mach-O 64-bit relocatable object writer.
  - `vanta_driver`: Compiler driver orchestrating lexing, parsing, type checking, MIR optimization, and code generation.

---

## 2. Platform ABI & Object File Integrity Audit

### 2.1 Microsoft x64 (`x86_64-pc-windows-msvc`)
- **Caller Shadow Space (Home Area)**:
  - Verified in `vanta_codegen_x86_64/src/instruction_select.rs`: Every outgoing call allocates `stack_arg_bytes = ((current_stack_offset + 15) & !15).max(32)`.
  - Stack arguments beyond the 4 positional slots start at `[rsp + 32]`, ensuring the 32-byte caller home area is preserved exclusively for the callee.
- **Positional Argument Registers**:
  - Parameter slot 1: `RCX` (or `XMM0` for vector/float)
  - Parameter slot 2: `RDX` (or `XMM1` for vector/float)
  - Parameter slot 3: `R8` (or `XMM2` for vector/float)
  - Parameter slot 4: `R9` (or `XMM3` for vector/float)
- **Non-Volatile (Callee-Saved) Preservation**:
  - `reg_alloc.rs` classifies `Rbx`, `Rbp`, `Rdi`, `Rsi`, `R12`, `R13`, `R14`, `R15` as callee-saved under `TargetAbi::MicrosoftX64`.
- **Hidden Structure Return (`sret`)**:
  - Memory returns pass buffer address in `RCX`, shifting normal argument registers right by one slot (`pos = 1`).
- **Unwind Metadata**:
  - Verified that COFF objects emit `.pdata` (`IMAGE_RUNTIME_FUNCTION_ENTRY`) and `.xdata` (`UNWIND_INFO`) with proper 18-byte symbol entries.

### 2.2 Apple Silicon ARM64 (`aarch64-apple-darwin`)
- **Reserved Platform Register X18**:
  - Verified strictly: Register `X18` is completely excluded from allocatable pools (`CALLER_SAVED_REGS`, `CALLEE_SAVED_REGS`) in `crates/vanta_codegen_aarch64/src/register.rs` and `reg_alloc.rs`.
- **Register Assignment**:
  - Arguments 1–8: `X0`–`X7`
  - Return: `X0` / `X1`
  - Indirect result (`sret`): `X8`
  - Scratch: `X16`, `X17`
  - Callee-saved: `X19`–`X28`, `X29` (`FP`), `X30` (`LR`)
- **Stack Alignment & Frame Record**:
  - 16-byte aligned stack allocation with `stp x29, x30, [sp, -frame_size]!` and `mov x29, sp`.
- **Mach-O Conventions**:
  - Leading underscore (`_`) enforced on C/runtime symbols in `macho_writer.rs`.

---

## 3. Linker Architecture Strategy
1. Create `crates/vanta_linker/` with:
   - Robust input readers for ELF64 (`.o`), AMD64 COFF (`.obj`), and Mach-O 64 (`.o`).
   - Archive reader and creator for Unix `ar` (`.a`) and Windows COFF (`.lib`).
   - Symbol resolution engine with strong/weak precedence, duplicate detection, and lazy archive extraction.
   - Section layout engine with page alignment and memory-protection separation.
   - Architecture-specific relocation engines (x86-64 ELF, AMD64 COFF, ARM64 Mach-O).
   - Linux ELF64 static executable generator (`ET_EXEC`) with self-contained startup (`_start`) and syscall runtime.
   - Windows PE32+ executable generator (`IMAGE_FILE_MACHINE_AMD64`) with minimal `KERNEL32.dll` imports and base relocations.
   - macOS Mach-O executable generator (`MH_EXECUTE`) with dyld loader and `libSystem.B.dylib` dependencies.
2. CLI and driver integration:
   - Provide `ventac archive create/list/inspect`.
   - Provide `ventac build ... --linker=internal`.
   - Zero invocations of external `gcc`, `clang`, `link.exe`, or `ld` in internal-linker mode.
