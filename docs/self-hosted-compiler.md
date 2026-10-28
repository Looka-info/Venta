# Vanta Self-Hosted Native Compiler (`vanta_c.va`)

## Overview

`vanta_c.va` is a self-hosting compiler written **100% in pure Vanta (`.va`)** that compiles Vanta source code directly into native x86-64 machine assembly targeting the System V AMD64 ABI on Linux.

It demonstrates language maturity, complete self-reliance, and verifies that Vanta's core primitives (functions, recursion, mutable locals, loops, conditionals, arrays, strings, standard I/O, and memory management) are robust enough to write a fully functioning systems compiler.

---

## Architecture & Components

The compiler lives in [`tools/self_host/vanta_c.va`](../tools/self_host/vanta_c.va) and consists of five core stages:

```
+--------------------+      +--------------------+      +---------------------+
|   Vanta Source     | ---> |       Lexer        | ---> |  Token Stream       |
|     (*.va)         |      | (char scanning)    |      | (keywords, idents)  |
+--------------------+      +--------------------+      +---------------------+
                                                                   |
                                                                   v
+--------------------+      +--------------------+      +---------------------+
| System V AMD64 ASM | <--- |   Code Generator   | <--- |   Recursive Descent |
|     (*.s)          |      |  (x86-64 emission) |      |   Parser & SymTab   |
+--------------------+      +--------------------+      +---------------------+
```

### 1. Lexer
- Scans characters from `&str` using UTF-8 / ASCII byte indexing.
- Recognizes language keywords (`fn`, `let`, `mut`, `return`, `if`, `else`, `while`).
- Identifies single and multi-character operators (`+`, `-`, `*`, `/`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `->`, `;`, `(`, `)`, `{`, `}`).
- Strips single-line comments (`// ...`) and whitespace.

### 2. Symbol Table & Stack Allocator
- Tracks function-local variable bindings, parameters, and stack slot offsets.
- Guarantees 16-byte stack frame alignment required by the System V AMD64 ABI.
- Maps function argument registers (`rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`) directly to reserved local stack slots during function entry.

### 3. Recursive Descent Parser
- **Functions:** Parses `fn name(param: type) -> ret_type { ... }`.
- **Statements:** Handles variable declarations (`let`, `let mut`), assignments, while loops, if-else conditional branches, function calls, and return statements.
- **Expressions:** Parses hierarchical binary expressions with correct precedence:
  - Multiplicative (`*`, `/`)
  - Additive (`+`, `-`)
  - Relational & Equality (`<`, `<=`, `>`, `>=`, `==`, `!=`)
  - Primary expressions (integer literals, variable identifiers, function calls, parenthesized expressions).

### 4. Code Generator
- Emits Intel-syntax x86-64 assembly instructions directly:
  - Function prologues: `push rbp; mov rbp, rsp; sub rsp, <frame_size>`
  - Epilogues: `mov rsp, rbp; pop rbp; ret`
  - Control flow: conditional jumps (`je`, `jne`, `jl`, `jle`, `jg`, `jge`), unconditional jumps (`jmp`), and uniquely generated branch labels.
  - Comparisons: `cmp`, `sete`/`setne`/`setl`/`setle`/`setg`/`setge`, `movzx`.
  - Arithmetic: `add`, `sub`, `imul`, `cqo` + `idiv`.
  - Function calls: System V AMD64 calling convention with argument passing in register sequence and result in `rax`.

### 5. CLI Driver & Standard I/O
- Reads command-line arguments using `std::env::args()`.
- Supports `<input.va> -o <output.s>` syntax.
- Performs file operations via `std::fs::read_to_string` and `std::fs::write`.

---

## Verification & Test Suite

The test suite in [`tools/self_host/tests/`](../tools/self_host/tests/) verifies each language feature through end-to-end compilation, native assembly, linking, and execution:

| Test Case | Description | Expected Output | Status |
|:---|:---|:---:|:---:|
| `01_return_42.va` | Minimal function exit code | Exit `42` | **PASS** |
| `02_arithmetic.va` | Nested arithmetic operations and precedence | Exit `40` | **PASS** |
| `03_while_loop.va` | Loop summation `1..10` with mutable variables | Exit `55` | **PASS** |
| `04_if_else.va` | Multi-function calls and conditional branches | Exit `10` | **PASS** |
| `05_fibonacci.va` | Recursive function calls and stack preservation | Exit `55` | **PASS** |

### Automated Test Runner

Execute the automated test suite with:

```bash
./scripts/test_self_hosted_compiler.sh
```

Sample output:
```
==========================================================
    Running Vanta Self-Hosted Compiler Test Suite        
==========================================================

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

==========================================================
  ALL SELF-HOSTED COMPILER TESTS PASSED SUCCESSFULLY!    
==========================================================
```
