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
as, break, const, continue, else, false, fn, if, let, loop,
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
1. **Fixed-Size Arrays**: `[T; N]` where `T` is a type and `N` is an integer constant expression greater than or equal to 0.
2. **Slices**: `&[T]` and `&mut [T]` representing a contiguous sequence `(pointer, length)`.
3. **String Slices (`&str`)**: First-class immutable string slice consisting of a two-word descriptor `(data_pointer: *const u8, byte_length: u64)` (16 bytes, 8-byte aligned). The referent type `str` is an unsized UTF-8 sequence that cannot be passed or stored by value. `&str` implements `Copy`. `&mut str` is disallowed.
4. **Structs**: Nominal product types with named fields:
   ```vanta
   struct Point {
       x: i64,
       y: i64,
   }
   ```
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

---

## 7. ABI & Target Platform

- **Architecture**: x86-64
- **Operating System**: Linux
- **Calling Convention**: System V AMD64 ABI:
  - First 6 integer/pointer arguments: `%rdi`, `%rsi`, `%rdx`, `%rcx`, `%r8`, `%r9`.
  - Return values: `%rax` (primary), `%rdx` (secondary).
  - Callee-saved registers: `%rbx`, `%rsp`, `%rbp`, `%r12`, `%r13`, `%r14`, `%r15`.
  - Caller-saved registers: `%rax`, `%rcx`, `%rdx`, `%rsi`, `%rdi`, `%r8`, `%r9`, `%r10`, `%r11`.
  - Stack alignment: `%rsp` must be 16-byte aligned immediately before a `call` instruction.
