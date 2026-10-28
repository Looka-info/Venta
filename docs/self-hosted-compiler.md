# Vanta Self-Hosted Native Compiler (`vanta_c.va`) & Backend (`backend.va`)

## Overview

[`vanta_c.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/vanta_c.va) is a self-hosting compiler written **100% in pure Vanta (`.va`)** that compiles Vanta source code directly into native x86-64 machine assembly targeting the System V AMD64 ABI on Linux.

It pairs with a dedicated, modular native backend [`backend.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va), also written in pure Vanta, which handles physical machine registers, stack frame layout calculation, 16-byte stack alignment, instruction selection, and binary encoding primitives.

It demonstrates language maturity, complete self-reliance, and verifies that Vanta's core primitives (functions, recursion, mutable locals, loops, conditionals, arrays, strings, standard I/O, and memory management) are robust enough to write a modular, industrial-grade systems compiler.

---

## Architecture & Modular Design

The self-hosted compiler is split into clean, modular components:

```mermaid
flowchart TD
    subgraph Frontend ["Compiler Frontend (vanta_c.va)"]
        SRC["Vanta Source (.va)"] --> LEX["Lexer"]
        LEX --> TOKS["Token Stream"]
        TOKS --> PARSE["Recursive Descent Parser"]
        PARSE --> SYMS["Symbol Table & Scope Offsets"]
    end

    subgraph Backend ["Native Backend (backend.va)"]
        SYMS --> FRAME["FrameLayout (16-Byte Stack Alignment)"]
        FRAME --> EMIT["BackendEmitter (Instruction Selector)"]
        EMIT --> ABI["System V AMD64 ABI Calling Sequence"]
        ABI --> DIRECTIVES["GNU as / Intel Directives & .note.GNU-stack"]
    end

    subgraph BinaryReference ["Machine Code Representation"]
        EMIT -.-> REX["REX Prefix Calculator"]
        EMIT -.-> MODRM["ModR/M & SIB Byte Generator"]
        EMIT -.-> ELF["ELF64 Header Metadata"]
    end

    Backend --> OUT["System V AMD64 Assembly (.s)"]
    OUT --> GCC["Linker (gcc -no-pie)"]
    GCC --> BIN["Native Linux Executable"]
```

### 1. Lexer (`Lexer`)
- Scans characters from `&str` using UTF-8 / ASCII byte indexing.
- Recognizes language keywords (`fn`, `let`, `mut`, `return`, `if`, `else`, `while`).
- Identifies single and multi-character operators (`+`, `-`, `*`, `/`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `->`, `;`, `(`, `)`, `{`, `}`).
- Strips single-line comments (`// ...`) and whitespace.

### 2. Symbol Table (`SymbolTable`)
- Tracks function-local variable bindings, parameters, and stack slot offsets.
- Provides string pool resolution for variable identifiers.
- Calculates stack displacement slots for local variables relative to base pointer `rbp`.

### 3. Recursive Descent Parser (`Compiler`)
- **Functions:** Parses `fn name(param: type) -> ret_type { ... }`.
- **Statements:** Handles variable declarations (`let`, `let mut`), assignments, while loops, if-else conditional branches, function calls, and return statements.
- **Expressions:** Parses hierarchical binary expressions with correct precedence:
  - Multiplicative (`*`, `/`)
  - Additive (`+`, `-`)
  - Relational & Equality (`<`, `<=`, `>`, `>=`, `==`, `!=`)
  - Primary expressions (integer literals, variable identifiers, function calls, parenthesized expressions).

### 4. Pure Vanta Native Backend ([`backend.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va))

The backend encapsulates all architecture-specific and ABI details in pure Vanta:

- **Target Register Mappings (`MReg`):**
  - Physical register constants: [`REG_RAX`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L61) (0) through [`REG_R15`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L76) (15).
  - Register naming: 64-bit (`rax`, `rbx`, etc.) and 8-bit low byte (`al`, `bl`, etc.) via [`push_reg`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L162) and [`push_byte_reg`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L183).
  - System V AMD64 ABI Argument Sequence: [`abi_arg_reg`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L80) maps `0..5` to `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`.
  - Return Register: [`abi_ret_reg`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L91) (`rax`).

- **Stack Frame Layout & 16-Byte Alignment ([`FrameLayout`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L107)):**
  - System V AMD64 ABI mandates that `rsp + 8` is a multiple of 16 on function entry (after `call` pushes the return address).
  - The prologue executes `push rbp`, restoring 16-byte alignment (`rsp % 16 == 0`).
  - [`compute_frame_layout`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L121) computes an aligned stack allocation: `((raw_bytes + 15) / 16) * 16`.
  - Guarantees any subsequent `call` maintains the mandatory 16-byte alignment, preventing SSE/vector segfaults.

- **Instruction Emitter ([`BackendEmitter`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L143)):**
  - Prologue & Epilogue: [`emit_function_start`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L237) and [`emit_function_end`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L253).
  - Data movement: [`emit_mov_reg_imm`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L273), [`emit_mov_reg_reg`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L283), [`emit_mov_reg_mem`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L292), [`emit_mov_mem_reg`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L304), [`emit_push`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L316), [`emit_pop`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L323).
  - Arithmetic & Logic: [`emit_add`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L332), [`emit_sub`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L341), [`emit_imul`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L350), [`emit_idiv`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L359) (with `cqo`), [`emit_cmp`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L367), [`emit_setcc`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L384).
  - Control Flow: [`emit_jmp`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L400), [`emit_je`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L407), [`emit_jne`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L414), [`emit_call`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L421), [`emit_call_args`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L428), [`emit_bind_params`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L438).
  - Security / Modern Toolchain Compatibility: [`emit_footer`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L232) emits `.section .note.GNU-stack,"",@progbits`, explicitly disabling executable stack flags to prevent linker security warnings.

- **Direct Binary Machine Code Reference:**
  - [`rex_prefix(w, r, x, b)`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L457): Calculates x86-64 REX prefix bytes (`0x40 | (W<<3) | (R<<2) | (X<<1) | B`).
  - [`modrm_byte(mode, reg, rm)`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L467): Calculates ModR/M byte.
  - [`sib_byte(scale, index, base)`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L472): Calculates SIB addressing byte.
  - [`elf64_machine_x86_64()`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L477): Returns standard AMD64 ELF machine architecture `0x3E` (62).
  - ELF64 Magic Bytes: [`elf64_magic_0`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L482) through [`elf64_magic_3`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/backend.va#L485) (`0x7F`, `'E'`, `'L'`, `'F'`).

### 5. CLI Driver & Standard I/O
- Reads command-line arguments using `std::env::args()`.
- Supports `<input.va> -o <output.s>` syntax.
- Performs file operations via `std::fs::read_to_string` and `std::fs::write`.

---

## Verification & Test Suite

The test suite in [`tools/self_host/tests/`](file:///c:/Users/mumer/OneDrive/Documents/Venta/tools/self_host/tests/) verifies each language and backend feature through end-to-end compilation, native assembly, linking, and execution:

| Test Program | Focus & Validation | Expected Output | Status |
|:---|:---|:---:|:---:|
| `backend_test.va` | Pure Vanta backend unit tests (registers, ABI, frame alignment, REX/ModRM, ELF64) | Exit `100` | **PASS** |
| `01_return_42.va` | Minimal function exit code | Exit `42` | **PASS** |
| `02_arithmetic.va` | Nested arithmetic operations and precedence | Exit `40` | **PASS** |
| `03_while_loop.va` | Loop summation `1..10` with mutable variables | Exit `55` | **PASS** |
| `04_if_else.va` | Multi-function calls and conditional branches | Exit `10` | **PASS** |
| `05_fibonacci.va` | Recursive function calls and stack preservation | Exit `55` | **PASS** |
| `06_call_6_args.va` | Full 6-argument System V register calling sequence | Exit `21` | **PASS** |
| `07_comparisons.va` | Relational comparisons (`==`, `!=`, `<`, `<=`, `>`, `>=`) | Exit `63` | **PASS** |
| `08_nested_loops.va` | Nested loops & stack offset management | Exit `60` | **PASS** |
| `09_operator_precedence.va` | Multiplicative before additive precedence | Exit `28` | **PASS** |
| `10_stack_alignment_and_calls.va` | 16-byte stack alignment across nested function calls | Exit `44` | **PASS** |

### Automated Test Runner

Execute the automated test suite with:

```bash
./scripts/test_self_hosted_compiler.sh
```

Sample output:
```text
==========================================================
    Running Vanta Self-Hosted Compiler Test Suite        
==========================================================
Compiling self-hosted compiler (tools/self_host/vanta_c.va)...

--- Running Pure Vanta Backend Unit Tests (backend_test.va) ---
PASS: Pure Vanta native backend passed all verification tests (exit 100)

--- Testing 01_return_42 ---
PASS: 01_return_42 returned 42 (expected 42)

--- Testing 02_arithmetic ---
PASS: 02_arithmetic returned 40 (expected 40)

--- Testing 03_while_loop ---
PASS: 03_while_loop returned 55 (expected 55)

--- Testing 04_if_else ---
PASS: 04_if_else returned 10 (expected 10)

--- Testing 05_fibonacci ---
PASS: 05_fibonacci returned 55 (expected 55)

--- Testing 06_call_6_args ---
PASS: 06_call_6_args returned 21 (expected 21)

--- Testing 07_comparisons ---
PASS: 07_comparisons returned 63 (expected 63)

--- Testing 08_nested_loops ---
PASS: 08_nested_loops returned 60 (expected 60)

--- Testing 09_operator_precedence ---
PASS: 09_operator_precedence returned 28 (expected 28)

--- Testing 10_stack_alignment_and_calls ---
PASS: 10_stack_alignment_and_calls returned 44 (expected 44)

==========================================================
  ALL SELF-HOSTED COMPILER TESTS PASSED SUCCESSFULLY!    
==========================================================
```
