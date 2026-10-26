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

---

## 6. Struct Memory Layout and Alignment

Vanta guarantees deterministic, predictable struct layouts:

### 6.1 Field Ordering and Natural Alignment
1. **Declaration Order Preservation**: Struct fields are laid out in memory in their exact order of declaration. The compiler performs no hidden field reordering in Milestone 5.
2. **Natural Alignment**: Each field's byte offset is aligned to a multiple of its natural alignment (`align(T) = min(sizeof(T), 8)`). If the preceding field ends at an unaligned offset, the compiler inserts padding bytes.
3. **Trailing Padding and Struct Alignment**: The total size of a struct is rounded up to a multiple of the struct's maximum field alignment (`max_align = max(align(field_1), ..., align(field_n))`).
4. **Example**:
   ```vanta
   struct Mixed {
       a: u8,   // offset 0, size 1, align 1
                // padding: 7 bytes (offsets 1..7)
       b: i64,  // offset 8, size 8, align 8
       c: u8,   // offset 16, size 1, align 1
                // padding: 7 bytes (offsets 17..23)
   }            // total size: 24 bytes, align: 8 bytes
   ```

### 6.2 Nested Structs
Nested structs are laid out inline (by value) within the outer struct according to their own alignment and size. A nested struct field requires no heap allocations or indirection pointers.

---

## 7. Dynamic Slice Descriptor Layout and Borrow Safety

### 7.1 Fat Pointer Representation
A dynamic slice `&[T]` is an immutable 2-word fat pointer descriptor:
- Word 0: `data_ptr: *const T` (64-bit virtual memory address of the first element).
- Word 1: `len: u64` (64-bit element count).
- Size: 16 bytes. Alignment: 8 bytes.
- System V AMD64 ABI: When passed as function arguments or returned, dynamic slices occupy two consecutive 64-bit integer registers (e.g. `%rdi` for pointer and `%rsi` for length).

### 7.2 Borrow Checker Invariants
1. **Array to Slice Coercion**: Taking `&arr` where `arr: [T; N]` creates a shared borrow of `arr` with length `N`.
2. **Mutation Prevention**: While the slice borrow `&arr` is active, the underlying array `arr` cannot be modified, reassigned, or moved (`E0306_ASSIGN_BORROWED_VARIABLE`).
3. **Immutability of Slices**: Dynamic slice elements cannot be written through indexing (`s[i] = x` triggers `E0303_MUTATE_IMMUTABLE`). Mutating slices (`&mut [T]`) is deferred to a future milestone.
