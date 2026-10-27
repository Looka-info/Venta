# Vanta — AI-First Language Design Directive
**Document Version:** 1.0.0  
**Language Version:** 0.1.0-draft  
**Target Architecture:** x86-64 (Linux ELF64, System V AMD64 ABI)  
**Status:** Approved & Active Implementation Guide

---

## 1. Executive Summary & Vision

**Vanta** is an AI-first, performance-oriented systems programming language. It is engineered from first principles so that **autonomous AI coding agents** can generate, understand, inspect, debug, refactor, and maintain systems-level code with high precision and minimal hallucination, while remaining intuitive, readable, and ergonomically pleasant for human engineers.

Vanta is **not** a Rust clone, not a syntactic wrapper around C, and not a language that relies on an AI runtime or opaque neural compiler. Vanta is an independent, statically typed, ahead-of-time compiled programming language featuring:
- A custom native compiler (`vantac`) written in Rust.
- A custom x86-64 machine-code backend and pure ELF64 relocatable object writer.
- Deterministic, affine ownership and borrow-checking without a garbage collector.
- A Mid-Level Intermediate Representation (MIR) and optimization pipeline.
- C-ABI interoperability and Linux System V AMD64 compliance.

The AI-first designation signifies that **the language grammar, type system, compiler diagnostics, and tooling workflows are intentionally designed to maximize machine actionability and eliminate ambiguity**.

---

## 2. Core Architectural Principles & Trade-Offs

Every language feature and compiler subsystem in Vanta is evaluated against the following seven ranked priorities:

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Predictable code generation by AI agents                 │
│ 2. Unambiguous parsing and local type checking              │
│ 3. Machine-actionable diagnostics and deterministic repair   │
│ 4. Strong compile-time memory safety with simple invariants │
│ 5. Native execution performance & zero-cost abstractions   │
│ 6. Human readability and maintainability                   │
│ 7. Small cognitive surface & discoverable standard library │
└─────────────────────────────────────────────────────────────┘
```

### 2.1 Non-Negotiable Trade-Offs
1. **Safety and Correctness over Token Economy**:
   Vanta never sacrifices compile-time safety or runtime correctness to shave a few tokens from generated code. Terse, cryptic shortcuts (e.g., C-style comma operators, implicit conversions, arbitrary macro expansions) are rejected in favor of explicit, unambiguous constructs.
2. **Canonical Repetition over Redundant Flexibility**:
   AI code generation thrives when there is **one preferred canonical idiom** for any task. Providing five ways to express a loop or three ways to declare a variable increases agent divergence and syntax errors. Vanta prefers a single clear pattern.
3. **Local Reasoning over Global Inference**:
   Global type inference (e.g., Hindley-Milner with unbounded unification across module boundaries) creates catastrophic diagnostic cascades where an error in function $A$ manifests in function $Z$. Vanta enforces explicit function signatures and confines type inference to local variable initializers.
4. **Stable Structured Feedback over Free-Form Text**:
   Compilers designed for humans output free-form English paragraphs with ASCII carets. AI agents parsing such text often suffer from parsing failures or misinterpret multi-line messages. Vanta treats machine-readable JSON diagnostics as a first-class citizen alongside human terminal output.
5. **Core Language Decoupled from AI Frameworks**:
   Vanta's core compiler does not bake in tensor primitives, neural network operations, or vendor-specific GPU intrinsics. High-performance AI workloads will be supported through standard libraries and foreign-function interfaces (FFI), ensuring core language stability.

---

## 3. Syntax and Semantic Analysis for AI Agents

Systems languages such as C, C++, and Rust present recurring failure modes for automated agents. Vanta's design directly remedies these friction points.

### 3.1 Eliminating Grammar Ambiguities
- **Struct Literals vs. Block Bodies**:
  In Rust, `Foo { ... }` in certain statement contexts can be ambiguous with a block statement `{ ... }`. In Vanta, struct literals require the type name and braced field initializers with explicit identifiers `Point { x: 1, y: 2 }`, and statement contexts require explicit statement delimiters (semicolons) or keyword-led control flow (`if`, `while`, `loop`).
- **Context-Free Parsing**:
  Vanta's grammar is LALR(1) / LL(2) compatible and does not require type-feedback during lexing or parsing (unlike C's `typedef` ambiguity). An AI agent can reliably generate syntactically valid AST fragments without maintaining a full symbol table.

### 3.2 Predictable Local Type Inference
- **Explicit Function Contracts**:
  All function parameters and return types must be explicitly annotated (e.g., `fn calculate(a: i64, b: i64) -> i64`). Omitting the return type unambiguously specifies `()`.
- **Local Let Bindings**:
  `let x = 42;` infers `i64` from the literal default. If a binding has an explicit type `let x: i32 = 42;`, the compiler enforces that type. Uninitialized variables must specify their type `let x: i64;` and pass definite-initialization analysis before reading.

### 3.3 Explicit Ownership and Borrowing Semantics
- **Affine Types**: Values are owned by a single binding.
- **Copy Types**: Primitive integers (`i8`..`i64`, `u8`..`u64`), floats (`f32`, `f64`), `bool`, and immutable string slices (`&str`) are `Copy`. They duplicate bitwise upon assignment and parameter passing.
- **Borrow Exclusivity ($Aliasing \oplus Mutability$)**:
  - Multiple shared references: `&T` (read-only).
  - At most one exclusive reference: `&mut T` (read-write).
  - No shared reference may coexist with an active mutable reference to the same target.
- **Diagnostics with Clear State Tracing**: When a borrow conflict occurs, the diagnostic reports both the site of the original borrow and the site of conflicting mutation.

---

## 4. Machine-Actionable Diagnostics Specification

### 4.1 Specification & JSON Schema (`schema_version: "1.0.0"`)

Vanta defines a strict, versioned JSON schema for diagnostic output produced via `vantac check <file> --diagnostics=json` or `vantac compile <file> --diagnostics=json`.

```json
{
  "$schema": "https://vanta-lang.org/schema/v1/diagnostics.json",
  "version": "1.0.0",
  "success": false,
  "error_count": 1,
  "warning_count": 0,
  "diagnostics": [
    {
      "code": "E0201",
      "severity": "error",
      "message": "Type mismatch: expected 'i64', found '&str'",
      "file": "src/main.va",
      "span": {
        "start": 56,
        "end": 70,
        "start_line": 4,
        "start_col": 15,
        "end_line": 4,
        "end_col": 29
      },
      "expected": "i64",
      "actual": "&str",
      "labels": [
        {
          "file": "src/main.va",
          "span": {
            "start": 56,
            "end": 70,
            "start_line": 4,
            "start_col": 15,
            "end_line": 4,
            "end_col": 29
          },
          "message": "expected 'i64' due to function parameter"
        }
      ],
      "notes": [
        "function 'print_i64' parameter 'value' has type 'i64'"
      ],
      "suggestion": {
        "message": "use 'print_str' to print string slices",
        "replacement_span": {
          "start": 46,
          "end": 55,
          "start_line": 4,
          "start_col": 5,
          "end_line": 4,
          "end_col": 14
        },
        "replacement_text": "print_str"
      }
    }
  ]
}
```

### 4.2 Stable Diagnostic Code Catalog

All compiler diagnostics must emit a stable alphanumeric identifier following the `E[0-9]{4}` pattern:

| Code Range | Category | Description |
| :--- | :--- | :--- |
| **`E0001` - `E0099`** | **Lexical & Syntax** | Tokenizing and parsing errors |
| `E0001` | Lexer | Unexpected character encountered |
| `E0002` | Lexer | Unterminated string literal or block comment |
| `E0003` | Lexer | Invalid numeric literal format or overflow |
| `E0004` | Parser | Unexpected token; syntax error in statement or expression |
| `E0005` | Parser | Unexpected end of input (EOF) |
| **`E0101` - `E0199`** | **Symbol Resolution** | Scope, visibility, and name binding |
| `E0101` | Symbol | Duplicate symbol declared in the same scope |
| `E0102` | Symbol | Undefined identifier or unresolved name |
| `E0103` | Symbol | Attempt to redefine built-in intrinsic (`print_i64`, `print_str`) |
| `E0104` | Symbol | Unknown struct type name |
| `E0105` | Symbol | Unknown field name on struct |
| **`E0201` - `E0299`** | **Type System** | Type verification and conformance |
| `E0201` | Type | Type mismatch between expected and actual expressions |
| `E0202` | Type | Sized type required (e.g., bare unsized `str` used as value) |
| `E0203` | Type | Unsupported mutable string slice `&mut str` |
| `E0204` | Type | Invalid operand types for operator |
| `E0205` | Type | Function call argument count or type mismatch |
| `E0206` | Type | Invalid return of unsized type or invalid reference |
| `E0207` | Type | Function return type mismatch with declared signature |
| **`E0301` - `E0399`** | **Borrow & Ownership** | Memory safety and lifetime constraints |
| `E0301` | Borrow | Read of uninitialized variable |
| `E0302` | Borrow | Use of moved value |
| `E0303` | Borrow | Mutation of immutable variable or reference |
| `E0304` | Borrow | Cannot borrow as mutable while active shared borrow exists |
| `E0305` | Borrow | Cannot borrow as mutable while active mutable borrow exists |
| `E0306` | Borrow | Cannot mutate variable while active borrow exists |
| **`E0401` - `E0499`** | **IR & Backend** | Verification and machine code constraints |
| `E0401` | MIR | MIR verification invariant failure |
| `E0402` | Backend | Unsupported ABI target or machine relocation overflow |
| **`E0501` - `E0599`** | **Enums & Patterns** | Enums, variants, and pattern matching |
| `E0501` | Enum | Non-exhaustive pattern match |
| `E0502` | Enum | Unreachable match arm pattern |
| `E0503` | Enum | Unknown enum type name |
| `E0504` | Enum | Unknown or mismatched enum variant |
| `E0505` | Enum | Invalid payload type for enum variant |
| `E0506` | Enum | Variant arity mismatch |
| `E0507` | Enum | Recursive enum by value rejected |
| **`E0601` - `E0699`** | **Module System** | Modules, files, imports, and visibility |
| `E0601` | Module | Module file not found on disk |
| `E0602` | Module | Circular module dependency detected |
| `E0603` | Module | Private item access from outside module |
| `E0604` | Module | Unresolved import target |
| **`E0701` - `E0799`** | **Traits & Dispatch** | Traits, contracts, and static dispatch |
| `E0701` | Trait | Unknown trait in implementation or generic bound |
| `E0702` | Trait | Duplicate trait implementation for a type (coherence violation) |
| `E0703` | Trait | Missing required trait method implementation |
| `E0704` | Trait | Unknown trait method in implementation block |
| `E0705` | Trait | Trait method signature mismatch |
| `E0706` | Trait | Ambiguous trait method call |
| `E0707` | Trait | Generic trait bound not satisfied |
| `E0708` | Trait | Duplicate trait declaration |
| `E0709` | Trait | Invalid trait receiver mode |
| **`E0801` - `E0899`** | **Error Propagation** | Standard library and try operator |
| `E0801` | Try | Invalid try operand (not `Option` or `Result`) |
| `E0802` | Try | Incompatible function return type for `?` propagation |
| `E0803` | Try | Mismatched Result error types during propagation |
| `E0804` | Try | Postfix `?` operator used outside function |
| `E0805` | Try | Mixed Option/Result propagation without conversion |
| `E0806` | Try | Unsafe payload move during error propagation |
| **`E0901` - `E0999`** | **Iterators & Loops** | Iteration protocols and for loops |
| `E0901` | Iter | Type is not iterable (does not implement `Iterator<T>`) |
| `E0902` | Iter | Cannot infer iterator element type for loop |

---

## 5. Tooling and CLI for Iterative AI Workflows

### 5.1 Fast In-Memory Check (`vantac check`)
AI agents operating in automated repair loops require instant feedback without the overhead of disk I/O, code generation, and system linker invocation.
- `vantac check <file.va>`: Executes Lexing $\to$ Parsing $\to$ Semantic Analysis $\to$ MIR Lowering & Verification.
- Does not emit `.o` or binary files.
- Execution latency is under 5 milliseconds on modern workstations.
- Supports `--diagnostics=json` for direct automated consumption.

### 5.2 Deterministic Exit Codes
The `vantac` CLI adheres to deterministic POSIX exit codes:
- **`0`**: Success. Program is valid; check or compilation succeeded.
- **`1`**: Compilation or verification failed due to syntax, type, borrow, or IR error.
- **`2`**: Command-line usage error (invalid flag, missing input file, unrecognized option).

### 5.3 Inspectable Compiler Stages
For deep agent reasoning, `vantac` exposes its internal compiler artifacts:
- `--emit=tokens`: Outputs the lexed token stream with span coordinates.
- `--emit=ast`: Outputs the pretty-printed, validated Abstract Syntax Tree.
- `--emit=mir`: Outputs the Mid-Level Intermediate Representation with basic blocks, temporaries, and control-flow graphs.
- `--emit=obj`: Emits the relocatable ELF64 object file without invoking `gcc`.
- `--emit=exe`: Links the final native Linux executable.

---

## 6. Concrete Code & Diagnostic Examples

### 6.1 Example 1: Type Mismatch on Built-in Intrinsic
**Source (`invalid_type.va`):**
```vanta
fn main() -> i64 {
    print_i64("hello world");
    0
}
```
**Human Terminal Diagnostic:**
```text
invalid_type.va:2:15: error[E0201]: Type mismatch: expected 'i64', found '&str'
   2 |     print_i64("hello world");
     |               ^^^^^^^^^^^^^
  note: function 'print_i64' expects parameter of type 'i64'
  suggestion: did you mean to call 'print_str'?
```
**Machine JSON Diagnostic (`vantac check invalid_type.va --diagnostics=json`):**
```json
{
  "version": "1.0.0",
  "success": false,
  "error_count": 1,
  "warning_count": 0,
  "diagnostics": [
    {
      "code": "E0201",
      "severity": "error",
      "message": "Type mismatch: expected 'i64', found '&str'",
      "file": "invalid_type.va",
      "span": {
        "start": 37,
        "end": 50,
        "start_line": 2,
        "start_col": 15,
        "end_line": 2,
        "end_col": 28
      },
      "expected": "i64",
      "actual": "&str",
      "labels": [],
      "notes": [
        "function 'print_i64' expects parameter of type 'i64'"
      ],
      "suggestion": {
        "message": "did you mean to call 'print_str'?",
        "replacement_span": {
          "start": 23,
          "end": 32,
          "start_line": 2,
          "start_col": 5,
          "end_line": 2,
          "end_col": 14
        },
        "replacement_text": "print_str"
      }
    }
  ]
}
```

### 6.2 Example 2: Borrow Conflict
**Source (`invalid_borrow.va`):**
```vanta
fn main() -> i64 {
    let mut x: i64 = 10;
    let r: &i64 = &x;
    x = 20;
    *r
}
```
**Human Terminal Diagnostic:**
```text
invalid_borrow.va:4:5: error[E0306]: Cannot mutate variable 'x' while active borrow exists
   4 |     x = 20;
     |     ^
  note: active shared borrow exists
```
**Machine JSON Diagnostic:**
```json
{
  "version": "1.0.0",
  "success": false,
  "error_count": 1,
  "warning_count": 0,
  "diagnostics": [
    {
      "code": "E0306",
      "severity": "error",
      "message": "Cannot mutate variable 'x' while active borrow exists",
      "file": "invalid_borrow.va",
      "span": {
        "start": 68,
        "end": 69,
        "start_line": 4,
        "start_col": 5,
        "end_line": 4,
        "end_col": 6
      },
      "expected": null,
      "actual": null,
      "labels": [],
      "notes": [
        "active shared borrow exists"
      ],
      "suggestion": null
    }
  ]
}
```

---

## 7. Prioritized Gap Analysis

### Category A: Pre-Stabilization Mandates (Immediate)
1. **Machine-Readable Diagnostics (`--diagnostics=json`)**: Fully integrated into `vanta_diagnostics`, `vanta_sema`, `vanta_parser`, and `vanta_driver`.
2. **Fast Sub-Command `vantac check`**: Checking source semantics without linker invocation.
3. **Stable Error Codes**: Systematic assignment of `E0001` through `E0401` across all error paths.
4. **Explicit Suggestion Payloads**: Non-hallucinating suggestions with exact byte/line replacement spans.

### Category B: Current Milestone Enhancements (Milestone 3 & AI-First)
1. **String Slice Diagnostics**: Actionable errors on bare `str` usage (`E0202`) and mutable slices `&mut str` (`E0203`).
2. **Intrinsic Call Verification**: Clear diagnostic suggestions when passing strings to `print_i64` and vice versa.
3. **CLI Determinism**: Strict exit code compliance (`0` for success, `1` for compile error, `2` for CLI flags).

### Category C: Completed in Milestones 9 through 14
1. **Automated Code Formatter (`vantac fmt`)**: Canonical, deterministic AST-based formatter to prevent agent diff-churn (Completed in M7).
2. **Algebraic Sum Types & Pattern Matching**: Enums, exhaustive pattern matching, and variant constructors (Completed in M8).
3. **Module & Import System**: Explicit symbol scoping, file-based hierarchy (`mod name;`), selected imports (`import path::{name};`), visibility (`pub`), re-exports, and error codes `E0601`-`E0608` (Completed in M9).
4. **Traits, Interfaces, Generic Bounds & Static Dispatch**: Explicit trait contracts, static dispatch without vtables, inherent precedence, coherence enforcement, and error codes `E0701`-`E0709` (Completed in M10).
5. **Early-Return Error Propagation (`?`)**: Predictable `?` operator on `Option` and `Result` with deterministic LIFO cleanup and error codes `E0801`-`E0806` (Completed in M11).
6. **Iterator Protocols & `for` Loops**: Generic `Iterator<T>` trait, non-allocating `range_i64`, and error codes `E0901`-`E0902` (Completed in M12).
7. **Lifetime Parameters, Reference Safety, and Immutable Vector Iteration**: Named lifetime parameters (`'a`, `'b`, `'static`), compile-time provenance tracking, deterministic elision rules, immutable vector iteration (`vec.iter()`), and error codes `E0310`-`E0316` (Completed in M13).
8. **Owned Strings, UTF-8 Operations, and Safe Slicing**: First-class `String` type, 3-word descriptor layout, geometric growth with overflow and self-aliasing protection, safe UTF-8 character boundary slicing (`slice_bytes`), automatic destruction, and borrow checker integration (Completed in M14).

### Category D: Future Roadmap
1. **Interactive Documentation & Signature Inspection**: `vantac doc --json` emitting machine-readable API contracts and verified code examples.
2. **Optimization Pass Enhancements**: Inter-procedural constant propagation and loop invariant code motion.
3. **Consuming and Mutable Vector Iteration**: Dynamic partial drop flags and mutable disjoint element borrows.

---

## 8. Measurable Acceptance Criteria

1. **Schema Compliance**:
   All diagnostic outputs produced with `--diagnostics=json` validate cleanly against the `1.0.0` JSON specification.
2. **No External Linker Dependency for Validation**:
   `vantac check <file>` successfully diagnoses errors without invoking `gcc` or writing executables to disk.
3. **Exit Code Accuracy**:
   Zero compiler crashes (`panic!`), clean error codes (`0`, `1`, `2`) on all test scenarios.
4. **Test Suite Verification**:
   100% pass rate across workspace unit tests and end-to-end native execution tests in WSL2 Linux.
