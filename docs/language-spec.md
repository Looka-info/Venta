# Vanta Language Specification
Version: 0.1.0-draft
Target Architecture: x86-64 (Linux ELF64, System V AMD64 ABI)

---

## 1. Introduction

**Vanta** is a statically typed, ahead-of-time compiled systems programming language designed for deterministic performance, zero-cost abstractions, predictable resource lifetimes, and memory safety by default without a tracing garbage collector.

This document defines the formal language specification for Vanta version 0.1.

---

## 2. Lexical Structure

### 2.1 Character Set & Encoding
Source code files must be UTF-8 encoded text.

### 2.2 Whitespace and Comments
- Whitespace consists of spaces (`U+0020`), horizontal tabs (`U+0009`), carriage returns (`U+000D`), and line feeds (`U+000A`).
- Single-line comments start with `//` and extend to the next newline.
- Block comments start with `/*` and extend to `*/`. Block comments may be nested.

### 2.3 Identifiers
Identifiers follow standard systems language syntax:
`[a-zA-Z_][a-zA-Z0-9_]*`
Identifiers must not collide with reserved keywords.

### 2.4 Keywords
The following keywords are reserved:
```
as, break, const, continue, else, false, fn, if, impl, let, loop,
mut, pub, return, struct, true, type, unsafe, while
```

### 2.5 Primitive Type Keywords
```
bool, i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, str, unit (or ())
```

### 2.6 Literals
- **Integer Literals**: Decimal (`1234`), Hexadecimal (`0x1F`), Binary (`0b1010`), Octal (`0o77`). Optional type suffixes: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`.
- **Boolean Literals**: `true`, `false`.
- **Character Literals**: `'a'`, `'\n'`, `'\x7F'`.
- **String Literals**: `"hello, world"`. Decoded UTF-8 byte sequences with escape sequences `\n`, `\r`, `\t`, `\\`, `\"`, `\0`. String literals evaluate to the immutable string slice type `&str`. The length equals the decoded byte length. Embedded NUL bytes (`\0`) are preserved.

### 2.7 Operators and Punctuation
- Arithmetic: `+`, `-`, `*`, `/`, `%`
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Logical: `&&`, `||`, `!`
- Bitwise: `&`, `|`, `^`, `<<`, `>>`
- Assignment: `=`, `+=`, `-=`, `*=`, `/=`, `%=`
- Punctuation: `(`, `)`, `{`, `}`, `[`, `]`, `;`, `:`, `::`, `,`, `.`, `->`
- References: `&`, `&mut`

---

## 3. Type System

Vanta is strictly and statically typed. Type inference is local to variable declarations.

### 3.1 Primitive Types
1. **Integers**:
   - Signed: `i8`, `i16`, `i32`, `i64` (two's complement).
   - Unsigned: `u8`, `u16`, `u32`, `u64`.
2. **Booleans**: `bool` (`true` or `false`, 1 byte representation).
3. **Unit**: `()` (zero-sized type, represents empty return or statement completion).

### 3.2 Compound Types
1. **Fixed-Size Arrays**: `[T; N]` where `T` is a type and `N` is an integer constant expression greater than or equal to 0. Elements reside contiguously in memory without per-element overhead.
2. **Dynamic Slices (`&[T]`)**: First-class immutable dynamically sized views over contiguous memory. A slice descriptor consists of two 64-bit words: `(data_pointer: *const T, length: u64)` (16 bytes, 8-byte aligned). Slices support:
   - Borrow coercion from array references: `let view: &[i64] = &arr;`.
   - Length query method: `view.len() -> i64`.
   - Bounds-checked indexing: `view[i] -> T` (panics with exit status 101 on out-of-bounds access).
   - Dynamic slices implement `Copy` and cannot be modified or re-sliced into mutable buffers in Milestone 5.
3. **String Slices (`&str`)**: First-class immutable string slice consisting of a two-word descriptor `(data_pointer: *const u8, byte_length: u64)` (16 bytes, 8-byte aligned). The referent type `str` is an unsized UTF-8 sequence that cannot be passed or stored by value. `&str` implements `Copy`. `&mut str` is disallowed.
4. **User-Defined Structs**: Nominal product types with named fields laid out with natural alignment and padding:
   ```vanta
   struct Point {
       x: i64,
       y: i64,
   }
   ```
   Structs can be initialized via named field initializers (`Point { x: 10, y: 20 }`), nested within other structs, and mutated field-by-field (`point.x = 42;`) if the root binding is mutable.
5. **References**:
   - Immutable Reference: `&T` (shared, read-only).
   - Mutable Reference: `&mut T` (exclusive, read-write).

---

## 4. Ownership, Borrowing, and Lifetimes

Vanta enforces memory safety at compile time using an affine type system and borrow checker.

### 4.1 Invariants
1. **Single Owner**: Every value in Vanta has a unique owner variable at any given point in program execution.
2. **Destruction on Scope Exit**: When an owner variable goes out of scope, the owned resource is deterministically dropped.
3. **Move vs Copy**:
   - Types implementing `Copy` (all primitive numeric types and `bool`, as well as arrays/structs composed solely of `Copy` types) are duplicated on assignment or parameter passing.
   - Non-`Copy` types are moved. After a value is moved, the source variable is considered uninitialized and cannot be read.
4. **Aliasing XOR Mutability**:
   - Any number of active immutable references (`&T`) may exist simultaneously to a value.
   - Exactly ONE active mutable reference (`&mut T`) may exist to a value at a time.
   - No immutable references may coexist with an active mutable reference to the same value.
5. **No Dangling References**: A reference cannot outlive the lifetime of the referent.

---

## 5. Declarations and Statements

### 5.1 Variable Bindings
- Immutable by default: `let x: i64 = 42;`
- Mutable binding: `let mut x: i64 = 42;`
- Optional type annotation with local inference: `let x = 42;` (defaults integer literals to `i64`).

### 5.2 Functions
Functions are declared using `fn`:
```vanta
fn add(a: i64, b: i64) -> i64 {
    return a + b;
}
```
If the return type is omitted, it defaults to `()`.
The final expression in a block without a semicolon is the block's value:
```vanta
fn square(x: i64) -> i64 {
    x * x
}
```

### 5.3 Control Flow
1. **Conditional (`if` / `else`)**:
   `if` is both a statement and an expression:
   ```vanta
   let val = if condition { 10 } else { 20 };
   ```
2. **Loops**:
   - `while <cond> { ... }`: Evaluates condition before each iteration.
   - `loop { ... }`: Infinite loop until `break`.
   - `break` and `continue` statements.

### 5.4 Structs and Inherent Methods

Structs declare user-defined types with named fields:
```vanta
struct Point {
    x: i64,
    y: i64,
}
```

Inherent methods are declared using `impl` blocks:
```vanta
impl Point {
    fn translate(self: &mut Self, dx: i64, dy: i64) {
        self.x = self.x + dx;
        self.y = self.y + dy;
    }

    fn sum(self: &Self) -> i64 {
        return self.x + self.y;
    }
}
```

#### Rules for Inherent Methods:
1. **Receiver Parameter**: The first parameter must be named `self` with type `Self`, `&Self`, or `&mut Self`.
2. **Static Resolution**: Method calls `p.translate(dx, dy)` resolve at compile time with zero virtual-table or dynamic dispatch overhead.
3. **Auto-Referencing and Auto-Dereferencing**: Calling `p.translate(dx, dy)` automatically borrows `&mut p` if the receiver expects `&mut Self`. Accessing fields on references (`self.x`) automatically dereferences.
4. **Mutability Enforcement**: Methods with receiver `&Self` cannot mutate fields (`self.x = 99` emits `E0303_MUTATE_IMMUTABLE`). Methods with receiver `&mut Self` require a mutable place binding at the call site.
5. **No Recursive Structs by Value**: A struct cannot directly or transitively contain an instance of itself by value (`E0208_RECURSIVE_STRUCT`), ensuring finite stack sizes.

---

## 6. Built-in Standard Output Intrinsics

Vanta provides fundamental built-in intrinsics available globally without import declarations:

### 6.1 `print_i64`
```vanta
fn print_i64(value: i64) -> ()
```
- Prints a signed 64-bit integer in base-10 ASCII representation to standard output (file descriptor 1).
- Supports negative numbers, zero, and positive numbers across the full `i64` range (`i64::MIN` to `i64::MAX`).
- Emits no implicit newline or surrounding whitespace.
- Allocates no heap memory; uses a stack-allocated buffer.
- Correctly retries interrupted writes (`EINTR`) and handles partial writes.

### 6.2 `print_str`
```vanta
fn print_str(value: &str) -> ()
```
- Writes the exact bytes of the string slice to standard output (file descriptor 1).
- Does not append a newline, quotation marks, or delimiters.
- Does not assume null-termination; uses the slice descriptor's `byte_length`.
- Handles empty strings (`""`) without issuing write calls.
- Preserves embedded NUL bytes (`\0`) and arbitrary UTF-8 multibyte sequences.
- Retries on `EINTR` and handles partial writes.

### 6.3 `panic`
```vanta
fn panic(msg: &str) -> ()
```
- Explicitly aborts execution with a panic message.
- Emits `"Vanta panic: "` followed by `msg` and a trailing newline `\n` directly to standard error (file descriptor 2).
- Immediately invokes `_exit(101)` without executing subsequent statements.
- Zero heap allocation; uses fixed stack-allocated buffers and direct unbuffered write syscalls.
- Protected keyword / globally reserved intrinsic that cannot be redefined.

---

## 7. Array and Slice Bounds Safety

Vanta enforces strict, deterministic runtime bounds checking for all indexed reads and writes on fixed-size arrays (`[T; N]`), array references (`&[T; N]`), and string slices (`&str`).

### 7.1 Bounds Validation Rules
1. **Valid Range**: An index `i` is valid if and only if `0 <= i < length`.
2. **Signed Index Handling**: Signed negative indices (`i < 0`) are explicitly caught and rejected without converting them to unsigned representations prior to validation.
3. **Upper Bound & Empty Collections**: Any index `i >= length` triggers an immediate bounds failure. Empty collections (`length == 0`) reject all indexing operations.
4. **CFG Dominance**: The bounds check conditional branch strictly dominates the memory read or write instruction in the control flow graph. No out-of-bounds address computation or memory access can execute.

### 7.2 Failure Behavior & Output
When a bounds check fails:
- Standard error (file descriptor 2) receives the exact formatted diagnostic:
  `Vanta panic: index out of bounds (index %ld, length %ld)\n`
- The process terminates immediately via `_exit(101)`.
- No heap memory is allocated; zero recursion, zero unwinding, zero signal handler risk.

### 7.3 Optimization Semantics
- At `-O0`: All bounds checks are generated and executed dynamically at runtime.
- At `-O1`: Bounds checks are eliminated if and only if the optimizer statically proves that `0 <= i < length`. Statically proven out-of-bounds accesses fold directly to deterministic panic calls, eliminating dead memory operations. Both optimization modes produce identical observable runtime behavior.

---

## 8. ABI & Target Platform

- **Architecture**: x86-64
- **Operating System**: Linux
- **Calling Convention**: System V AMD64 ABI:
  - First 6 integer/pointer arguments: `%rdi`, `%rsi`, `%rdx`, `%rcx`, `%r8`, `%r9`.
  - Return values: `%rax` (primary), `%rdx` (secondary).
  - Callee-saved registers: `%rbx`, `%rsp`, `%rbp`, `%r12`, `%r13`, `%r14`, `%r15`.
  - Caller-saved registers: `%rax`, `%rcx`, `%rdx`, `%rsi`, `%rdi`, `%r8`, `%r9`, `%r10`, `%r11`.
  - Stack alignment: `%rsp` must be 16-byte aligned immediately before a `call` instruction.

---

## 8. AI-First Design Guarantees & Machine Diagnostics

### 8.1 Core Principles
1. **Unambiguous Parsing & Semantic Rules**:
   - The grammar avoids context-sensitive ambiguities; all constructs are parseable without symbol-table feedback.
   - Function signatures require explicit parameter and return types to establish firm API contracts.
   - Local type inference is confined to local `let` bindings with deterministic default types.
2. **Deterministic Tooling Workflows**:
   - `vantac check <file>` performs semantic analysis and MIR verification without linking an executable.
   - Machine-actionable diagnostics (`--diagnostics=json`) provide structured JSON reports matching schema `1.0.0`.
   - Stable diagnostic codes (`E0001` - `E0401`) enable automated tools to parse errors and execute repairs with exact spans and non-hallucinating suggestions.
   - For full specifications and architectural rationale, refer to `docs/ai-first-design.md`.
