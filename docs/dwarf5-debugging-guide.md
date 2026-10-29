# Venta Native DWARF 5 Debugging Guide

This guide details the native DWARF 5 debug information emission architecture in **Venta**, explaining how to compile, step through, inspect, and debug Venta programs using standard Linux debuggers like GDB and LLDB.

---

## 1. Architecture & DWARF 5 Conformance

Venta compiles source code directly to ELF64 object files through its project-owned, pure-Rust code generation backend (`vanta_codegen_x86_64`) without using LLVM or Cranelift. When debug info is requested (`-g`), Venta synthesizes genuine DWARF 5 debug sections and associated ELF relocations directly into relocatable ELF object files (`.o`) and final executables.

### Emitted DWARF 5 Sections

| Section | Role | Relocation Section |
| :--- | :--- | :--- |
| `.debug_info` | Compilation unit, subprograms, parameters, local variables, and base types | `.rela.debug_info` |
| `.debug_abbrev` | Abbreviation table declarations for DWARF DIE layouts | *(None)* |
| `.debug_line` | Source line and column mapping program | `.rela.debug_line` |

### DWARF 5 Header & Language Encoding

- **Unit Type**: `DW_UT_compile` (`0x01`), 64-bit address size (`8`), 0-byte segment selector.
- **DWARF Version**: Strictly `version = 5` in both `.debug_info` and `.debug_line`.
- **Language Identifier**: `DW_LANG_Venta = 0x8001` (`32769`), conforming to the DWARF 5 user-defined language code range (`0x8000`–`0xffff`).
- **File Table**: 0-indexed DWARF 5 format descriptors with `DW_LNCT_path` (`DW_FORM_string`) and `DW_LNCT_directory_index` (`DW_FORM_udata`).
- **Frame Base**: `DW_AT_frame_base = [DW_OP_reg6]` (`%rbp`), allowing stack-relative resolution of locals and parameters via `DW_OP_fbreg (-offset)`.

---

## 2. Command-Line Usage

Debug information is strictly **opt-in**. When omitted, Venta produces clean, zero-overhead binaries with no debug sections.

### Compiling with Debug Symbols

```bash
# Compile and link to native executable with DWARF 5 debug info
ventac -g -o my_program my_program.va

# Or using the extended CLI
vanta build -g -o my_program my_program.va

# Compile to relocatable ELF object with debug info
ventac -g --emit=obj -o my_program.o my_program.va
```

### Unoptimized Debug Builds (Recommended for Maximum Variable Visibility)

```bash
# Disable MIR optimizations (-O0) so all variables retain dedicated stack slots
ventac -g -O0 -o my_program my_program.va
```

---

## 3. Debugging with GDB

Venta's DWARF 5 emissions are verified against GDB (version 12+ recommended).

### Starting a Debugging Session

```bash
gdb ./my_program
```

### Setting Breakpoints

You can set breakpoints either by function name or by source line:

```gdb
(gdb) break main
Breakpoint 1 at 0x4012d0: file my_program.va, line 10.

(gdb) break my_program.va:14
Breakpoint 2 at 0x4012ec: file my_program.va, line 14.
```

### Stepping Through Code

```gdb
(gdb) run
Starting program: /path/to/my_program

Breakpoint 1, main () at my_program.va:10
10      let x: i64 = 42;

(gdb) next
11      let y: i64 = calculate(x);

(gdb) step
calculate (val=42) at my_program.va:3
3       let res: i64 = val * 2;
```

### Inspecting Variables and Arguments

Venta maps function parameters and local variables to their respective stack locations relative to `%rbp`:

```gdb
(gdb) info args
val = 42

(gdb) info locals
res = 84

(gdb) print val
$1 = 42
```

### Call Stack Backtraces

Venta preserves standard System V AMD64 frame pointers (`push %rbp; mov %rsp, %rbp`), allowing GDB to reliably unwind nested function calls:

```gdb
(gdb) backtrace
#0  calculate (val=42) at my_program.va:3
#1  0x0000000000401314 in main () at my_program.va:11
```

---

## 4. Debugging with LLDB

Venta's DWARF 5 line tables and subprograms are compatible with LLDB:

```bash
lldb ./my_program
```

```lldb
(lldb) b calculate
Breakpoint 1: where = my_program`calculate + 15, address = 0x00000000004012df
(lldb) run
Process 1234 launched: './my_program' (x86_64)
* thread #1, stop reason = breakpoint 1.1
    frame #0: 0x00000000004012df my_program`calculate(val=42) at my_program.va:3:20
-> 3       let res: i64 = val * 2;
(lldb) bt
* thread #1, stop reason = breakpoint 1.1
  * frame #0: 0x00000000004012df my_program`calculate(val=42) at my_program.va:3:20
    frame #1: 0x0000000000401314 my_program`main at my_program.va:11:14
```

---

## 5. Known Limitations & Architectural Notes

1. **Optimized Register-Allocated Variables (`-O1`)**:
   Under `-O1` or standard optimizations, Venta's register allocator (`RegisterAllocator`) promotes frequently used scalar values to general-purpose registers (`%r12`–`%r15`, `%rbx`) or eliminates dead variables via GVN and mem2reg. Because Venta currently emits single-location DWARF attributes (`DW_AT_location = [DW_OP_fbreg]`), variables that have been promoted to registers or eliminated may display stale stack values or `<optimized out>`. For debugging local variables, compile with `-O0`.
2. **SIMD Vector Local Variables**:
   SIMD vectors (`v256`, `v512`) are managed by the specialized vector register allocator (`SimdRegisterAllocator`) across `%ymm0`–`%ymm15` and `%zmm0`–`%zmm31`. Vector type DIEs and vector register locations are planned for future DWARF extensions.
3. **Call Frame Information (`.eh_frame` / `.debug_frame`)**:
   Call stack unwinding relies on the compiler's strict frame-pointer discipline (`push %rbp; mov %rsp, %rbp`). Omission of frame pointers is never enabled when debug info is active. Full CFI instruction emission is scheduled for a future release.
