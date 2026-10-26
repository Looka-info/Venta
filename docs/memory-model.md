# Vanta Memory Model and Safety Specification

---

## 1. Core Safety Tenet

Vanta provides **memory safety by default**:
1. No use-after-free.
2. No double-free.
3. No dangling references.
4. No data races in safe code.
5. No uninitialized memory reads.
6. Bounds-checked indexing by default.

---

## 2. Ownership Semantics

### 2.1 Unique Ownership
Every value residing in memory has an unambiguous owner variable at any given moment during runtime.
When the owner variable's lexical scope ends:
- The resource's destructor (if defined) is invoked.
- The stack slot or allocated memory is reclaimed.

### 2.2 Copy Semantics
Types that represent small, self-contained bit patterns implement `Copy`:
- Primitive integers: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`
- Booleans: `bool`
- Fixed-size arrays of `Copy` elements: `[T; N]` where `T: Copy`
- Structs composed exclusively of `Copy` fields.

Assigning or passing a `Copy` value duplicates its bits; the source variable remains initialized and valid.

### 2.3 Move Semantics
Types with unique identity or heap resources (e.g. dynamic buffers, non-Copy structs) have move semantics:
- Transfer of ownership occurs on assignment (`let b = a;`) or function call (`foo(a)`).
- After transfer, the source variable `a` enters an **uninitialized / moved-out** state.
- Reading from an uninitialized or moved-out variable is rejected at compile time by the borrow checker.

---

## 3. Borrowing Rules (Aliasing XOR Mutability)

References allow observing or mutating data without transferring ownership.

### 3.1 Immutable Borrows (`&T`)
- Multiple immutable borrows of the same value may coexist:
  `let r1 = &x; let r2 = &x; // Valid`
- While an immutable borrow is active:
  - The underlying value `x` cannot be mutated directly (`x = 5; // Compile error`).
  - No mutable borrow (`&mut x`) may be taken (`let m = &mut x; // Compile error`).

### 3.2 Mutable Borrows (`&mut T`)
- A mutable borrow grants exclusive read-write access to the referenced value:
  `let m = &mut x; *m += 1;`
- While an active mutable borrow exists:
  - No other references (mutable or immutable) to `x` may exist.
  - Direct access to `x` is prohibited until the borrow terminates.

### 3.3 Non-Lexical Lifetimes (NLL)
A borrow ends at the point of its last use, rather than strictly at the closing curly brace of its enclosing scope.

---

## 4. Definite Initialization

Variables in Vanta must be definitely assigned along all execution paths before their first read.
Example:
```vanta
let x: i64;
if cond {
    x = 10;
} else {
    x = 20;
}
// Guaranteed initialized here
let y = x + 1;
```
If an execution branch fails to initialize `x`, the compiler reports a compile-time diagnostic.

---

## 5. Array and Slice Bounds Safety Guarantees

In accordance with Core Safety Tenet #6, all indexed element accesses (`arr[i]`) on fixed arrays (`[T; N]`), array references (`&[T; N]`), and string slices (`&str`) are checked for spatial validity before memory is accessed.

### 5.1 Validity Range & Sign Preservation
1. Access is defined if and only if `0 <= i < length`.
2. Negative signed integers (`i < 0`) are evaluated under signed comparison semantics. Negative values are never cast or coerced into unsigned integers before checking, preventing wrapping vulnerabilities.
3. Indexing an empty collection (`length == 0`) unconditionally panics.

### 5.2 Control-Flow Dominance
The bounds check conditional branch strictly dominates the memory place calculation and read/write instructions in the Control Flow Graph. An out-of-bounds index immediately diverts control flow to a cold runtime panic path, ensuring that out-of-bounds pointer arithmetic and memory loads/stores are never executed.

### 5.3 Deterministic Panic and Exit
1. Bounds violations invoke `vanta_bounds_check_fail(index, length)`.
2. Diagnostic output is written directly to standard error (fd 2):
   `Vanta panic: index out of bounds (index %ld, length %ld)\n`
3. The process exits with exit status `101` (`_exit(101)`).
4. Zero heap memory is allocated during panic handling; stack buffers and direct POSIX write syscalls ensure deterministic termination under out-of-memory or stack constraints.

### 5.4 Optimizer Proofs
Optimizers (`-O1`) may remove runtime checks if and only if constant propagation or range analysis proves `0 <= index < length` along all reaching execution paths. If an index is proven statically out of bounds, the optimizer preserves the deterministic panic path and removes the dead downstream memory access. Both `-O0` and `-O1` maintain identical observable behavior.
