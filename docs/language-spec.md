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
as, break, const, continue, else, enum, false, fn, for, if, impl, in, let, loop,
match, mut, pub, return, struct, true, type, unsafe, while
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
- Punctuation: `(`, `)`, `{`, `}`, `[`, `]`, `;`, `:`, `::`, `,`, `.`, `->`, `=>`, `_`
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

### 5.5 Enums and Algebraic Sum Types

Enums define algebraic sum types representing one of several possible variants:
```vanta
enum Color {
    Red,
    Green,
    Blue,
}

enum Message {
    Quit,
    Move(i64, i64),
    Echo(i64),
}

enum Option<T> {
    Some(T),
    None,
}
```

#### Rules for Enums:
1. **Variants**: May be unit variants (`Color::Red`, `Message::Quit`) or tuple-style payload variants (`Message::Move(i64, i64)`).
2. **Generics**: Enums support type parameters (`Option<T>`, `Result<T, E>`) and are monomorphized at compile time.
3. **Tagged Union Memory Layout**:
   - Offset 0 contains a 32-bit unsigned discriminant tag (`u32`).
   - The payload begins at `align_to(4, max_payload_align)`.
   - The overall size is `align_to(tag_plus_max_payload, enum_align)`.
4. **No Recursive Enums by Value**: Directly or transitively recursive enum variants without an indirection/pointer are rejected at compile time with `E0507_RECURSIVE_ENUM`.

### 5.6 Pattern Matching and Exhaustiveness Checking

Pattern matching is performed via `match` expressions:
```vanta
let res = match msg {
    Message::Quit => 0,
    Message::Move(x, y) => x + y,
    Message::Echo(val) => val * 2,
};
```

#### Rules for Pattern Matching:
1. **Scrutinee Evaluation**: Evaluated exactly once into a temporary place.
2. **Exhaustiveness Guarantee**: Every match expression must cover all possible values. Missing variants emit `E0501_NON_EXHAUSTIVE_MATCH` with a precise list of missing variants.
3. **Unreachable Pattern Rejection**: Patterns that can never match due to earlier catch-all arms or overlapping patterns emit `E0502_UNREACHABLE_PATTERN`.
4. **Patterns Supported**:
   - Wildcard (`_`): Matches any value.
   - Identifier binding (`x`): Binds the value to a local variable.
   - Variant pattern (`Enum::Variant`, `Enum::Variant(p1, p2)`): Discriminant check and recursive subpattern matching.
   - Literals: Boolean (`true`, `false`) and integer literals (`0`, `42`).
5. **Ownership and Destruction**:
   - Matching by value moves non-`Copy` payload fields into arm bindings.
   - At arm scope exit, any active owned bindings are dropped in LIFO order.
   - Dropping an enum value executes runtime drop glue that inspects the 32-bit tag and destroys only the active variant's payload, guaranteeing zero memory leaks.

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

## 9. Generics, Heap Allocation, and Vector Abstraction

### 9.1 Monomorphized Generics
- Generic functions (`fn identity<T>(value: T) -> T`) and generic structs (`struct Pair<T1, T2> { first: T1, second: T2 }`) are monomorphized strictly at compile time.
- No dynamic dispatch, boxing, or runtime overhead.
- Names are mangled deterministically as `<symbol>__<types>` (e.g., `identity__i64`, `Vec__i64`).
- Recursive instantiation depth is guarded by a hard limit (16) emitting diagnostic `E0211` to prevent infinite expansion.
- Arity mismatch emits `E0209`, and inference failure on unconstrained parameters emits `E0212`.

### 9.2 Heap Allocator Runtime Hooks
Low-level runtime allocation hooks in `runtime/vanta_rt.c`:
- `vanta_alloc(size: u64) -> *mut void`: Allocates `size` bytes with 16-byte alignment. Panics on overflow (> 63-bit signed limit) or OOM.
- `vanta_realloc(ptr: *mut void, new_size: u64) -> *mut void`: Reallocates memory buffer; frees if `new_size == 0`.
- `vanta_free(ptr: *mut void)`: Reclaims allocated memory. Safe against NULL pointers.
- `vanta_active_allocs() -> i64`: Returns `alloc_count - free_count` for leak detection and memory verification.

### 9.3 Move Semantics and Automatic Destruction
- Non-Copy types (e.g. `Vec<T>` and structs containing fields needing drop) adhere to strict linear ownership and move semantics.
- Initializing another variable (`let y = x;`) or passing by value transfers ownership, moving `x` into an uninitialized state.
- Reading or mutating a moved variable is rejected at compile time with diagnostic `E0302_USE_OF_MOVED_VALUE`.
- Deterministic destruction is compiler-generated at block scope exit and `return` in LIFO order, ensuring exact `vanta_vec_free` invocation without double frees.

### 9.4 `Vec<T>` Abstraction
Built-in 24-byte generic vector (`data_ptr: i64`, `len: i64`, `cap: i64`):
- `Vec<T>::new()` / `Vec_new()`: Initializes an empty vector with NULL data and zero capacity.
- `.len() -> i64`: Returns current element count.
- `.capacity() -> i64`: Returns allocated capacity.
- `.push(val: T)`: Appends an element, geometrically growing capacity (0 -> 4 -> 8 -> 16).
- `.pop() -> T`: Removes and returns the last element, panicking with status code 101 if empty.
- `v[i]` / `v[i] = val`: Bounds-checked indexed read and write. Rejects out-of-bounds access with deterministic runtime panic.

---

## 10. AI-First Design Guarantees & Machine Diagnostics

### 10.1 Core Principles
1. **Unambiguous Parsing & Semantic Rules**:
   - The grammar avoids context-sensitive ambiguities; all constructs are parseable without symbol-table feedback.
   - Function signatures require explicit parameter and return types to establish firm API contracts.
   - Local type inference is confined to local `let` bindings with deterministic default types.
2. **Deterministic Tooling Workflows**:
   - `vantac check <file>` performs semantic analysis and MIR verification without linking an executable.
   - Machine-actionable diagnostics (`--diagnostics=json`) provide structured JSON reports matching schema `1.0.0`.
   - Stable diagnostic codes (`E0001` - `E0608`) enable automated tools to parse errors and execute repairs with exact spans and non-hallucinating suggestions.
   - For full specifications and architectural rationale, refer to `docs/ai-first-design.md`.

---

## 11. Modules, Imports, Namespaces, and Visibility

### 11.1 Module Declarations
Vanta organizes multi-file programs via file-backed modules:
```vanta
mod math;
pub mod models;
```
- `mod <name>;` declares a private child module. It is accessible only within the declaring module.
- `pub mod <name>;` declares a public child module, accessible hierarchically outside the declaring module.

### 11.2 Deterministic File Resolution
The entry file provided to `vantac` serves as the root module of the compilation unit (e.g. `src/main.va`).
Child modules resolve deterministically on the filesystem:
1. For root `main.va`:
   - `mod math;` looks for `src/math.va` or `src/math/mod.va`.
2. For non-root submodule `src/math.va` (or `src/math/mod.va`):
   - `mod geometry;` looks for `src/math/geometry.va` or `src/math/geometry/mod.va`.
3. Resolution Diagnostics:
   - If neither candidate exists, the compiler emits `E0601_MODULE_NOT_FOUND`.
   - If both `name.va` and `name/mod.va` exist, the compiler emits `E0607_AMBIGUOUS_MODULE_FILE`.
   - If a module file transitively loads itself, the compiler detects the cycle and emits `E0602_MODULE_CYCLE`.
   - If a module is declared multiple times in the same source file, the compiler emits `E0605_DUPLICATE_MODULE`.
   - If a module file cannot be read from disk, the compiler emits `E0608_UNREADABLE_MODULE`.

### 11.3 Import Syntax and Selected Imports
Vanta requires explicit selected imports rather than wildcard imports:
```vanta
import math::{add, multiply};
import models::Point;
pub import internal::helper;
```
- Multi-item imports use braces: `import path::{item1, item2};`.
- Single-item imports can omit braces: `import path::item;`.
- Re-exports: `pub import ...` brings items into scope and re-exports them to downstream modules.
- Submodules can also be imported directly: `import math::geometry;`.
- Resolution uses fixed-point analysis to propagate re-exports across arbitrary declaration orders without circularities.
- If an imported name does not exist in the target module, `E0603_UNKNOWN_IMPORT` is emitted.

### 11.4 Visibility and Encapsulation
- **Private by Default**: All top-level declarations (functions, structs, enums, methods, and child modules) are private unless prefixed with `pub`.
- **Private Item Access**: Calling a private function or method outside its defining module emits `E0604_PRIVATE_ITEM`.
- **Field Visibility**: Struct fields are private unless marked `pub`. Reading or writing a private field or initializing it in a struct literal outside its defining module emits `E0606_PRIVATE_FIELD`.
- **Private Type Leakage**: Public function signatures or public method signatures that expose private types in their parameter or return types emit `E0604_PRIVATE_ITEM`.

### 11.5 Collision-Free Symbol Mangling
- Double-underscore (`__`) separation namespaces symbols across modules:
  - Function `add` in `math`: symbol name `math__add`.
  - Function `dist_sq` in `math::geometry`: symbol name `math__geometry__dist_sq`.
  - Struct `Point` in `models`: symbol name `models__Point`.
  - Method `sum` on `models::Point`: symbol name `models__Point_sum`.
  - Generic specialization `identity<i64>` in `math`: symbol name `math__identity__i64`.
- Unmangled Intrinsics & Entry:
  - Root `main` function always generates machine symbol `"main"`.
  - Compiler intrinsics (`print_i64`, `print_str`, `panic`, `vanta_alloc`, etc.) remain globally reserved and unmangled.

## 12. Traits, Interfaces, Generic Constraints, and Static Dispatch

### 12.1 Trait Declarations
A trait defines an abstract contract of method signatures that concrete types must satisfy:
```vanta
pub trait Describe {
    fn describe(self: &Self) -> i64;
}

pub trait Mutator {
    fn update(self: &mut Self);
}
```
- **Visibility**: `pub trait` makes the trait accessible across modules via imports (`import module::TraitName;`). Unmarked traits are private to their defining module.
- **Receivers**: Trait methods require an explicit `self` receiver with type `&Self` (shared reference) or `&mut Self` (mutable reference). Missing `self` or non-reference receivers emit `E0709_INVALID_TRAIT_RECEIVER`.
- **Signatures**: Methods declare explicit parameters and optional return types. Trait methods do not declare bodies.

### 12.2 Trait Implementations and Inherent Implementations
Types fulfill trait contracts via explicit `impl` blocks:
```vanta
struct User {
    id: i64,
}

impl Describe for User {
    fn describe(self: &User) -> i64 {
        self.id
    }
}
```
- **Inherent vs Trait Impls**:
  - `impl Target { ... }` declares inherent methods for `Target`.
  - `impl Trait for Target { ... }` implements a trait contract for `Target`.
- **Supported Targets**: Both `struct` and `enum` types can implement traits.
- **Completeness and Conformance**:
  - All methods declared in the trait must be implemented; missing methods emit `E0703_MISSING_TRAIT_METHOD`.
  - Extra methods not in the trait emit `E0704_UNKNOWN_TRAIT_METHOD`.
  - Parameter types, receiver mutability, and return types must match the trait signature exactly; mismatches emit `E0705_TRAIT_METHOD_SIGNATURE_MISMATCH`.

### 12.3 Global Coherence Policy
To guarantee predictable execution and prevent conflicting behaviors:
- At most one implementation of a trait is permitted for a given concrete type across the entire compilation unit.
- If duplicate implementations exist for the same `(Trait, Target)` pair, the compiler emits `E0702_DUPLICATE_IMPLEMENTATION`.

### 12.4 Method Resolution Precedence
When invoking a method `val.method(...)`:
1. **Inherent Methods First**: The compiler searches inherent methods on `val`'s concrete type. If found, the inherent method is selected unconditionally.
2. **In-Scope Trait Methods**: If no inherent method matches, the compiler searches all traits currently imported or declared in the current module scope that are implemented for `val`'s type.
3. **Ambiguity Rejection**: If multiple in-scope traits provide the method, the call is ambiguous and rejected with `E0706_AMBIGUOUS_TRAIT_METHOD`.
4. **Unknown Method**: If no inherent or in-scope trait method matches, the compiler emits `E0108_UNKNOWN_METHOD`.

### 12.5 Generic Trait Constraints
Generic functions can restrict type parameters using trait bounds:
```vanta
fn print_description<T: Describe>(item: &T) {
    let id = item.describe();
    print_i64(id);
}
```
- **Bound Validation**: When declaring `<T: Trait>`, `Trait` must resolve to a known trait in scope, otherwise emitting `E0701_UNKNOWN_TRAIT`.
- **Static Monomorphization**: When `print_description(&u)` is called with concrete type `User`, the compiler verifies that `User` implements `Describe`. If not, `E0707_TRAIT_BOUND_NOT_SATISFIED` is emitted.
- **Zero Vtables, Zero Dynamic Dispatch**: Every call to a bounded generic method compiles directly to a concrete, statically dispatched call to `<Target>__<Trait>_<method>` in the generated x86-64 machine code. No vtables, function pointers, or runtime trait objects (`dyn Trait`) exist.

### 12.6 Milestone 10 Diagnostics Catalog
- `E0701`: Unknown trait in `impl` or generic bound.
- `E0702`: Duplicate trait implementation for a type (coherence violation).
- `E0703`: Missing required trait method in `impl` block.
- `E0704`: Unknown trait method in `impl` block.
- `E0705`: Trait method signature mismatch (receiver mutability, param types, or return type).
- `E0706`: Ambiguous trait method call from multiple in-scope traits.
- `E0707`: Trait bound not satisfied during generic specialization.
- `E0708`: Duplicate trait declaration in the same scope.
- `E0709`: Invalid trait receiver (must be `&Self` or `&mut Self`).

---

## 13. Standard Library Foundation, Option<T>, Result<T, E>, and Error Propagation

### 13.1 Standard Library Organization and Discovery
Vanta features a self-contained, documented standard-library source tree rooted at `library/std/`:
- `library/std/mod.va`: Declares public submodules `pub mod option;` and `pub mod result;`.
- `library/std/option.va`: Defines the generic `Option<T>` enum and associated helper functions.
- `library/std/result.va`: Defines the generic `Result<T, E>` enum and associated helper functions.

#### Discovery and Availability
The compiler automatically loads the standard library root using the following precedence:
1. Explicit CLI argument: `--std-path <DIR>`.
2. Environment variable: `VANTA_STD_PATH`.
3. Relative file traversal: Searching upward from the root source file's directory for `library/std/mod.va`.
4. Process working directory: `library/std/mod.va`.
5. Cargo manifest root: `../../library/std/mod.va`.
6. Compile-time embedded fallback: Guaranteed embedded source fallback via `include_str!` for isolated environments.

Standard library items are imported explicitly without compiler magic:
```vanta
import std::option::Option;
import std::result::Result;
```

### 13.2 `Option<T>`
`Option<T>` models optional absence or presence of a value without sentinel values or null pointers:
```vanta
pub enum Option<T> {
    None,
    Some(T),
}
```

#### Inherent Methods
Vanta supports method syntax directly on `Option<T>` values:
- `opt.is_some() -> bool`: Returns `true` if `opt` is `Some`, `false` otherwise.
- `opt.is_none() -> bool`: Returns `true` if `opt` is `None`, `false` otherwise.
- `opt.unwrap_or(default: T) -> T`: Returns the inner `T` if `Some`, or `default` if `None`.
- `opt.unwrap() -> T`: Returns the inner `T` if `Some`, or triggers a panic (`called Option::unwrap() on a None value`) if `None`.

### 13.3 `Result<T, E>`
`Result<T, E>` models recoverable failures and success outcomes:
```vanta
pub enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

#### Inherent Methods
- `res.is_ok() -> bool`: Returns `true` if `res` is `Ok`, `false` otherwise.
- `res.is_err() -> bool`: Returns `true` if `res` is `Err`, `false` otherwise.
- `res.unwrap_or(default: T) -> T`: Returns the inner `T` if `Ok`, or `default` if `Err`.
- `res.unwrap() -> T`: Returns the inner `T` if `Ok`, or triggers a panic (`called Result::unwrap() on an Err value`) if `Err`.

### 13.4 Postfix `?` Error Propagation Operator
The postfix `?` operator provides ergonomic, concise, and safe error propagation:
```vanta
fn parse_number(text: &str) -> Result<i64, ParseError> { ... }

fn compute_total(a: &str, b: &str) -> Result<i64, ParseError> {
    let x = parse_number(a)?;
    let y = parse_number(b)?;
    Result::Ok(x + y)
}
```

#### Semantics and Guarantees:
1. **Single Evaluation**: The operand expression is evaluated exactly once into a temporary place.
2. **Discriminant Check**: The 32-bit tag is tested against the success variant (`Some` or `Ok`).
3. **Success Path**: The success payload is unwrapped and becomes the value of the try expression.
4. **Early Return Path**:
   - For `Result<T, E>`, returns `Result::Err(e)` early from the enclosing function.
   - For `Option<T>`, returns `Option::None` early from the enclosing function.
5. **Precise Destructor Invocations**: On the early return path, all still-owned local variables in active drop scopes are destroyed in LIFO order before the function returns. The returned error payload itself is transferred into the return value slot and is not destroyed on early return.
6. **Move Invalidation**: Moving an owned value into a `?` operand marks the local variable as moved, preventing subsequent use (`E0302`).

### 13.5 Milestone 11 Diagnostics Catalog
- `E0801`: Invalid try operand (used `?` on a type that is neither `Option` nor `Result`).
- `E0802`: Incompatible function return type for `?` propagation (enclosing function must return `Option` or `Result`).
- `E0803`: Mismatched Result error types (operand `Result<T, E1>` error type does not match function return error type `E2`).
- `E0804`: Postfix `?` used outside a function body.
- `E0805`: Incompatible try propagation (mixing `Option` and `Result` without explicit conversion).
- `E0806`: Unsafe payload move during error propagation.

---

## 14. Iterator Protocols, `for` Loops, Range Iteration, and Collection Integration

### 14.1 The Generic Iterator Trait
Iteration in Vanta is governed by a statically dispatched, monomorphized trait defined in `library/std/iter.va`:

```vanta
import std::option::Option;

pub trait Iterator<T> {
    fn next(self: &mut Self) -> Option<T>;
}
```

#### Protocol Guarantees:
1. **Advancement**: Calling `next(&mut self)` yields `Option::Some(value)` when the next item is available, and advances the internal iterator cursor.
2. **Termination**: When iteration terminates, `next(&mut self)` produces `Option::None`.
3. **Monomorphization**: Calls to `next` are monomorphized and statically dispatched with zero virtual method overhead (`dyn Trait` and vtables are prohibited).
4. **Deterministic Side Effects**: Advancing the iterator occurs exactly once per iteration cycle in a `for` loop.

### 14.2 Canonical `for ... in ...` Syntax and Semantic Lowering
Vanta provides idiomatic loop iteration using the canonical syntax:

```vanta
for <pattern> in <iterable_expr> {
    <statements>
}
```

#### Desugaring and Evaluation Rules:
1. **Single Evaluation**: The `<iterable_expr>` is evaluated exactly once into a temporary stack location before the loop begins.
2. **Mutable Receiver Binding**: A unique mutable iterator variable `__iter` is allocated in a dedicated scope.
3. **Loop Condition & Advancement**: At the head of the loop, `__iter.next()` is called via static dispatch.
4. **Pattern Matching**: The yielded `Option<T>` is inspected:
   - `Option::Some(val)`: Binds `val` to the iteration variable `<pattern>` and executes the loop body.
   - `Option::None`: Jumps directly to the loop exit block.
5. **Drop Cleanup**: When exiting the loop (normally, via `break`, or via early return), the iterator instance and all loop-scoped variables are cleanly destroyed in reverse declaration order.

### 14.3 Integer Range Iteration
The standard library provides an efficient, non-allocating half-open integer range iterator in `std::iter`:

```vanta
pub struct RangeI64 {
    pub current: i64,
    pub end: i64,
}

pub fn range_i64(start: i64, end: i64) -> RangeI64 {
    RangeI64 {
        current: start,
        end: end,
    }
}

impl Iterator<i64> for RangeI64 {
    fn next(self: &mut RangeI64) -> Option<i64> {
        if self.current < self.end {
            let val = self.current;
            self.current = self.current + 1;
            Option::Some(val)
        } else {
            Option::None
        }
    }
}
```

#### Range Properties:
- **Half-Open Interval**: Represents `[start, end)`. If `start >= end`, 0 iterations are performed.
- **Zero Allocations**: `RangeI64` occupies exactly 16 bytes on the stack (two 64-bit integers) and requires zero heap allocation.
- **Overflow Safety**: Since `self.current < self.end`, `self.current` never exceeds `end - 1`. For all `end <= i64::MAX`, incrementing `self.current + 1` is safe from signed integer overflow.
- **Negative Boundaries**: Correctly handles negative boundaries and zero crossings (e.g., `range_i64(-3, 3)` yields `-3, -2, -1, 0, 1, 2`).

### 14.4 User-Defined Iterators
Any user-defined struct may participate in `for` loop iteration by implementing `Iterator<T>`:

```vanta
struct Countdown {
    curr: i64,
}

impl Iterator<i64> for Countdown {
    fn next(self: &mut Countdown) -> Option<i64> {
        if self.curr > 0 {
            let v = self.curr;
            self.curr = self.curr - 1;
            Option::Some(v)
        } else {
            Option::None
        }
    }
}
```

### 14.5 Collection Integration Evaluation (`Vec<T>`)
A comprehensive architectural audit evaluated iterating over `Vec<T>`:
1. **Borrowed Iteration (`&Vec<T>`)**:
   Iterating over a borrowed vector requires yielding borrowed references `&T`. In Vanta's current affine type system, references cannot have arbitrary lifetime annotations tying them to the collection. Mutating or reallocating `Vec<T>` during iteration would violate memory safety without lifetime-parametric iterator types (`Iter<'a, T>`).
2. **Consuming Iteration (`IntoIter<T>`)**:
   Consuming a `Vec<T>` moves elements out one by one. If iteration terminates early via `break` or `return`, the remaining unconsumed elements must be dropped while previously moved elements must not be double-freed. Vanta's destructor generation currently applies to whole types on scope exit and does not maintain dynamic partial drop flags.
3. **Sound Decision**:
   To uphold Vanta's zero-compromise memory safety guarantees, `Vec<T>` and `&Vec<T>` are not iterable in Milestone 12. Attempting to iterate over them emits `E0901_TYPE_NOT_ITERABLE` with explicit compiler diagnostics.

### 14.6 Control Flow Integration
- `break`: Cleans up loop-body drop candidates and jumps immediately to the loop exit block.
- `continue`: Cleans up loop-body drop candidates and jumps to the loop condition block, ensuring the next element is fetched.
- LIFO Scope Drops: Local variables declared inside the loop body are dropped at the end of each iteration before the next `next()` call.

### 14.7 Milestone 12 Diagnostics Catalog
- `E0901`: Type is not iterable (target type does not implement `Iterator<T>`).
- `E0902`: Cannot infer iterator element type for `for` loop.

---

## 15. Lifetime Parameters, Reference Safety, and Immutable Collection Iteration

### 15.1 Named Lifetime Parameters
Milestone 13 introduces named lifetime parameters to Vanta's type system to express compile-time relationships between referenced storage and borrowed values.

Lifetimes are purely compile-time constructs: they introduce **zero runtime overhead**, no garbage collection, and no reference-counting metadata. Lifetimes are validated during semantic analysis and erased before MIR lowering.

#### Syntax:
- **Declarations**: Lifetimes begin with an apostrophe `'` followed by an identifier (e.g., `'a`, `'b`, `'static`).
- **Generic Headers**: Lifetimes precede type parameters in generic parameter lists:
  ```vanta
  fn pick<'a>(first: &'a i64, second: &'a i64, choose_first: bool) -> &'a i64 { ... }
  struct View<'a, T> { item: &'a T }
  ```
- **Reference Types**: Reference and slice types accept explicit lifetimes:
  `&'a T`, `&'a mut T`, `&'a [T]`, `Iter<'a, T>`.
- **The `'static` Lifetime**: Reserved identifier indicating storage valid for the entire duration of program execution (e.g., string literals `&'static str`).

### 15.2 Lifetime Elision Rules
To maintain ergonomic development while avoiding unsound inferences, Vanta defines deterministic lifetime elision rules:
1. **Single Reference Input**: If a function takes exactly one reference parameter and returns a reference, the output reference is unambiguously bound to the input parameter's lifetime:
   ```vanta
   fn identity(x: &i64) -> &i64 { x } // Elided: fn identity<'a>(x: &'a i64) -> &'a i64
   ```
2. **Multiple Reference Inputs**: If a function takes multiple reference parameters and returns a reference, elision is rejected with `E0313_AMBIGUOUS_LIFETIME_ELISION`. Named lifetime parameters are required to disambiguate the returned reference's origin.
3. **Zero Reference Inputs**: A function returning a reference with zero reference parameters is rejected with `E0314_RETURN_LOCAL_REFERENCE` because it cannot return local or temporary storage.

### 15.3 Reference Provenance & Safety Rules
The semantic analyzer tracks reference provenance to ensure references never outlive the storage they borrow:
1. **Local Stack Rejection**: Returning a reference to a local variable or temporary stack slot is strictly rejected at compile time (`E0314`).
2. **Origin Validation**: A returned reference's lifetime parameter must appear in at least one input parameter; unbound lifetimes are rejected (`E0312`).
3. **Undeclared Lifetimes**: Using any lifetime name not declared in the enclosing generic parameter list is rejected (`E0311`).
4. **Duplicate Lifetimes**: Declaring duplicate lifetime names in a single signature is rejected (`E0310`).

### 15.4 Immutable Vector Iteration (`vec.iter()`)
Milestone 13 enables sound, non-allocating immutable borrowed iteration over heap-allocated vectors:
```vanta
let mut v: Vec<i64> = Vec_new();
v.push(10);
v.push(20);

for elem in v.iter() {
    print_i64(*elem); // elem has type &'a i64
}
```

#### Invariants and Mechanics:
1. **Shared Borrow**: Invoking `v.iter()` places vector `v` in `BorrowState::Shared`, disallowing mutations (`v.push`, `v.pop`, `E0306`) and moves (`E0306`) while the iterator or references derived from it are active.
2. **Stack Allocation**: `v.iter()` creates an `Iter<'a, T>` struct containing the base buffer pointer and a 64-bit element index cursor (16 bytes on the stack, zero heap allocation).
3. **Yielding References**: `Iter<'a, T>` implements `Iterator<&'a T>`, where `next(&mut self)` yields `Option<&'a T>`.
4. **Loop Release**: Upon loop termination (normal, `break`, or early return), the borrow is released, permitting subsequent vector mutations or reallocations.

### 15.5 Deferred Vector Iteration Forms
- **Consuming Vector Iteration (`v.into_iter()`)**: Consuming a heap vector requires tracking dynamic partial-drop flags across loop breaks and panics. Explicitly deferred with actionable diagnostic `E0316_MUTATE_DURING_ITERATION`.
- **Mutable Vector Iteration (`v.iter_mut()`)**: Yielding mutable disjoint element references `&'a mut T` is deferred with `E0316_MUTATE_DURING_ITERATION`.

### 15.6 Milestone 13 Diagnostics Catalog
- `E0310`: Duplicate lifetime parameter in declaration.
- `E0311`: Undeclared lifetime parameter in type expression or signature.
- `E0312`: Return lifetime outlives input parameter origins.
- `E0313`: Ambiguous lifetime elision with multiple reference parameters.
- `E0314`: Cannot return reference to local variable or temporary storage.
- `E0315`: Borrow outlives referenced storage scope.
- `E0316`: Vector iteration form deferred (use `iter()` for immutable borrowed iteration).

---

## 16. Owned Strings, UTF-8 Operations, Safe Slicing, and String Memory Safety

### 16.1 First-Class `String` Type (`std::string::String`)
Milestone 14 introduces a first-class, heap-backed, affine `String` type in the standard library (`import std::string::String;`).

#### Internal Memory Layout:
A `String` descriptor consists of three 64-bit words (24 bytes total, 8-byte aligned on x86-64):
1. `data_ptr: i64`: Pointer to the heap-allocated UTF-8 byte buffer (or `NULL` if empty and unallocated).
2. `len: i64`: Current byte length (measures bytes, not Unicode scalar values or grapheme clusters).
3. `cap: i64`: Total allocated byte capacity.

#### Invariants:
1. **UTF-8 Validity by Construction**: Every safe `String` contains valid UTF-8. Constructing via `from_str`, appending via `push_str`, or cloning copies validated UTF-8 text. Arbitrary unchecked byte writes are disallowed.
2. **Deterministic Geometric Growth**: When appending exceeds capacity, capacity doubles (`cap * 2`, minimum 8 bytes). Allocation size calculations check for signed 64-bit overflow and panic deterministically (`vanta_panic`).
3. **Interior Aliasing Protection**: Appending a substring of the target string (`s.push_str(s.as_str())`) is fully safe. If reallocation moves the buffer, the source pointer is recalculated before copying.
4. **Zero Allocation for Empty Strings**: `String::new()` initializes with `data_ptr = NULL, len = 0, cap = 0`, requiring 0 heap allocations until the first non-empty append.

### 16.2 Public Canonical API
| Method / Function | Receiver | Returns | Description |
|---|---|---|---|
| `String::new()` | None | `String` | Creates an empty owned string (0 capacity, 0 allocations). |
| `String::from_str(s: &str)` | None | `String` | Allocates a heap buffer and copies UTF-8 bytes from slice `s`. |
| `s.push_str(text: &str)` | `&mut self` | `()` | Appends slice `text` onto the end of `s`, growing capacity if needed. |
| `s.as_str()` | `&self` | `&str` | Returns an immutable zero-cost slice view tied to `s`'s lifetime. |
| `s.as_bytes()` | `&self` | `&[u8]` | Returns an immutable zero-cost slice view over the raw UTF-8 bytes. |
| `s.len_bytes()` | `&self` | `i64` | Returns the length of the string in bytes. |
| `s.capacity()` | `&self` | `i64` | Returns the allocated byte capacity. |
| `s.is_empty()` | `&self` | `bool` | Returns `true` if `len == 0`, else `false`. |
| `s.clear()` | `&mut self` | `()` | Sets length to 0 while retaining allocated capacity for reuse. |
| `s.clone()` | `&self` | `String` | Creates an independent heap allocation with identical byte content. |
| `s.equals(other: &str)` | `&self` | `bool` | Checks exact byte equality without NUL-terminator assumptions. |
| `s.slice_bytes(start, end)` | `&self` | `Option<&str>` | Safe byte-range slicing validated against UTF-8 character boundaries. |

### 16.3 Safe UTF-8 Byte Slicing (`slice_bytes`)
Both `String` and `&str` provide `.slice_bytes(start: i64, end: i64) -> Option<&str>`:
1. **Range Bounds Check**: Verifies that `0 <= start <= end <= len`. Out-of-range bounds return `Option::None`.
2. **UTF-8 Character Boundary Validation**: Verifies that neither `start` nor `end` points to a UTF-8 continuation byte (`(byte & 0xC0) == 0x80`). If either boundary splits a multi-byte sequence, returns `Option::None`.
3. **Zero-Copy Sub-slice**: If valid, returns `Option::Some(&str)` pointing directly into the existing buffer.

### 16.4 Ownership, Destruction, and Container Nesting
- **Drop Glue**: `String` is an affine non-Copy type (`needs_drop == true`). On scope exit, `vanta_str_free` deallocates the buffer if non-null and resets fields.
- **Reassignment**: Assigning to an existing mutable `String` variable (`s = ...`) drops the previous buffer before storing the new value.
- **Nested Containers**:
  - `Vec<String>`: Automatically invokes `vanta_vec_free_strings` on destruction, freeing each element buffer before freeing the vector storage array.
  - `Option<String>` / `Result<String, E>`: Payload destruction runs when the discriminant contains `String`.
  - User Structs: Structs containing `String` fields recursively drop string fields on scope exit.
- **Borrow Safety**: While `s.as_str()` or `s.slice_bytes()` is live, `s` is immutably borrowed (`BorrowState::Shared`). Mutating (`s.push_str`, `s.clear`) or moving `s` emits `E0306_MUTATE_WHILE_BORROWED` or `E0307_MOVE_WHILE_BORROWED`.

## 17. User-Defined Generics, Multiple Trait Bounds, Monomorphization & Dynamic Dispatch

### 17.1 User-Defined Generic Types & Functions
Venta supports user-defined generic functions, structs, and enums parameterized by type parameters enclosed in angle brackets:
- Generic functions: `fn identity<T>(x: T) -> T { x }`
- Generic structs: `struct Pair<A, B> { first: A, second: B }`
- Generic enums: `enum Either<L, R> { Left(L), Right(R) }`
- Turbofish syntax: `identity::<i64>(42)` explicitly specifies type arguments at call sites.

### 17.2 Trait Bounds & Multiple Constraints
Type parameters may declare one or more trait bounds separated by `+`:
```venta
fn display_and_compare<T: Printable + Comparable>(a: &T, b: &T) {
    ...
}
```
During semantic analysis, the compiler validates that every concrete type argument satisfies all declared bounds, emitting diagnostic `E0707_TRAIT_BOUND_NOT_SATISFIED` if an implementation is missing.

### 17.3 Monomorphization Engine
Generic functions and types undergo full static monomorphization at compile-time:
- Symbol mangling: `func__Type1_Type2` produces deterministic, collision-free symbols.
- Instantiation limits: Monomorphization recursion depth is bounded to 64 and total instantiated functions to 1024, preventing infinite expansion and emitting `E0211_RECURSIVE_GENERIC_INSTANTIATION`.
- Zero runtime overhead: Specialized code paths are optimized independently by the MIR optimization passes.

### 17.4 Trait Objects & Dynamic Dispatch
Where runtime polymorphism is required, Venta provides first-class dynamically dispatched trait objects via `&dyn Trait` and `&mut dyn Trait`:
- **Object Safety**: A trait is object-safe if every method has `self` (`&Self` or `&mut Self`) as its first parameter, does not return `Self`, and has no generic type parameters. Using `&dyn Trait` on a non-object-safe trait emits `E0710_NOT_OBJECT_SAFE`.
- **Memory Representation**: Trait objects are 16-byte wide fat references:
  - First word (offset 0): 64-bit pointer to concrete data instance (`data_ptr`).
  - Second word (offset 8): 64-bit pointer to static vtable (`vtable_ptr`).
- **System V AMD64 ABI Classification**: Classified as `AbiClass::Integer2 { size: 16 }`, passed in two 64-bit integer registers (`rdi`, `rsi` etc.) and returned in `rax` and `rdx`.
- **Vtable Layout**: The compiler emits static vtables (`_vtable_{trait}_{target}`) in `.rodata` containing 64-bit function pointers corresponding to each trait method in declaration order.
- **Dynamic Dispatch Emission**: Method invocations on `&dyn Trait` calculate `vtable_ptr + method_idx * 8`, load the function pointer via `Instruction::LoadIndirect`, set `data_ptr` as the first receiver argument, and emit `Instruction::CallIndirect` (`call *%reg`).





