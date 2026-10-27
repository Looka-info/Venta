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

---

## 8. Heap Allocation, Monomorphization, and Automatic Destruction

### 8.1 Heap Memory Architecture
Heap memory is allocated via `runtime/vanta_rt.c`:
- **Alignment**: Every allocation is aligned to 16 bytes.
- **Overflow & OOM**: Allocations exceeding the positive 63-bit signed integer range or failing `malloc` panic immediately via `vanta_panic` (`_exit(101)`).
- **Leak Detection**: Active allocations are tracked dynamically via `vanta_active_allocs() == g_vanta_alloc_count - g_vanta_free_count`.

### 8.2 Move Tracking & Linear Ownership
- The compiler tracks each variable's initialization state as `Uninitialized`, `Initialized`, or `Moved`.
- Transfer of a non-Copy type (e.g. `Vec<T>`) marks the source variable as `Moved`.
- Re-accessing a moved variable emits `E0302_USE_OF_MOVED_VALUE`.
- Reassigning a variable whose current value is not moved invokes the destructor on the old value before storing the new value and marking the variable as `Initialized`.

### 8.3 Deterministic Destructor Generation
- Destructors are emitted in reverse order of declaration (LIFO) at lexical scope exit and before any `return` statement.
- Only variables that are in the `Initialized` state (not moved) have destructor code generated.
- When an outer struct contains fields needing destruction, destruction recurses member-by-member.

### 8.4 `Vec<T>` Memory Representation
`Vec<T>` is laid out as a 3-word struct:
- Word 0 (offset 0): `data_ptr: *mut T` (heap buffer or NULL when empty).
- Word 1 (offset 8): `len: i64` (number of active elements).
- Word 2 (offset 16): `cap: i64` (total capacity of allocated buffer).
Total size: 24 bytes, 8-byte aligned. Geometric growth (0 -> 4 -> 8 -> 16 -> 2*N) minimizes reallocations. All indexed reads and writes perform bounds checking.

---

## 9. Sum Types, Tagged Unions, and Variant Destruction

### 9.1 Tagged Union Memory Layout
Enums are laid out as tagged unions adhering to strict deterministic rules:
1. **Discriminant Tag**: A 32-bit unsigned integer (`u32`, `Type::I32` in MIR) residing at byte offset 0. Tags are assigned sequentially (`0, 1, 2, ...`) in order of variant declaration.
2. **Payload Offset**: The payload region begins immediately following the tag, aligned to the maximum alignment among all variant payloads:
   `payload_offset = align_to(4, max_payload_align)`
3. **Payload Size**: The payload storage size equals the maximum payload size across all variants:
   `max_payload_size = max(variant_payload_size)`
4. **Enum Alignment**:
   `enum_align = max(4, max_payload_align)`
5. **Total Size**:
   `total_size = align_to(payload_offset + max_payload_size, enum_align)`

#### Example Layouts:
- Unit enum `enum Color { Red, Green, Blue }`:
  - Tag at offset 0 (4 bytes). Max payload: 0 bytes. Total size: 4 bytes, alignment: 4 bytes.
- Mixed payload enum `enum Message { Quit, Move(i64, i64), Text(&str) }`:
  - Tag at offset 0 (4 bytes).
  - Padding: 4 bytes (offsets 4..7).
  - Payload starts at offset 8 (alignment 8).
  - `Move` payload: 16 bytes (offsets 8..23).
  - Total size: 24 bytes, alignment: 8 bytes.
- Generic enum `enum Option<i64> { Some(i64), None }`:
  - Tag at offset 0 (4 bytes), padding: 4 bytes, payload at offset 8 (8 bytes).
  - Total size: 16 bytes, alignment: 8 bytes.

### 9.2 Machine-Level 32-Bit Tag Operations
In the custom x86-64 backend:
- **Tag Read**: Uses 32-bit memory-to-register load `Mov32Mem` (`0x8B` with REX.W=0). In x86-64, a 32-bit register write automatically zeroes the upper 32 bits (bits 32..63) of the 64-bit destination register.
- **Tag Write**: Uses 32-bit register-to-memory store `MovMem32` / `StoreIndirect32` (`0x89` with REX.W=0). Writes strictly 4 bytes, preserving adjacent payload memory.

### 9.3 Dynamic Drop Glue for Enums
When an enum instance goes out of scope or is dropped, compiler-generated drop glue executes:
1. Loads the 32-bit discriminant tag from offset 0.
2. Branches conditionally to a tag-specific block for each variant that contains non-`Copy` payloads (`needs_drop`).
3. Executes recursive destructors on the active variant's owned payload fields.
4. Variants without owned payloads (or unit variants) bypass drop execution.
5. Guarantees that enum-wrapped heap structures (such as `Vec<T>`) are completely reclaimed without double destruction or memory leaks (`vanta_active_allocs() == 0`).

### 9.4 Move Tracking in Pattern Matching
When matching an enum by value (`match expr { Enum::Variant(x) => ... }`):
- The scrutinee is evaluated into an aggregate slot.
- Matching into bound variables moves non-`Copy` payload fields to the arm bindings.
- Bound variables are allocated local slots in the arm scope and registered for destruction at arm exit.
- Ignored wildcard subpatterns (`_`) for non-`Copy` fields are bound to temporary slots and dropped immediately at arm exit.
- After matching, the scrutinee variable is marked as moved, preventing use-after-move.

---

## 10. Cross-Module Ownership and Destruction Guarantees

### 10.1 Uniform ABI Across Module Boundaries
- Because Vanta compiles a complete declared module tree into a single compilation unit, module boundaries introduce zero runtime ABI differences.
- Types defined in submodules have deterministic, canonical layouts computed identically in all modules that import them.
- Passing owned values (such as `Vec<T>` or structs with non-`Copy` fields) across module boundaries preserves strict linear ownership:
  - If a function takes an owned parameter, ownership transfers to the callee's drop scope.
  - If a function returns an owned value, ownership transfers to the caller's drop scope.
  - References (`&T` and `&mut T`) enforce identical borrow-checker lifetimes across modules.

### 10.2 Zero-Leak Guarantee
- Destructors for types defined in foreign modules are generated directly by the compiler frontend and lowerer.
- Drop glue calls `vanta_free` or recursive component destructors at lexical scope exit regardless of which module originally declared the type.
- Verified by automated tests: active allocations tracked by `vanta_active_allocs()` strictly return to zero when cross-module heap values exit scope.

---

## 11. Traits, Static Dispatch, and Zero-Overhead Abstractions

### 11.1 Zero-Cost Memory Layouts
- **No Vtable Overhead**: In Vanta, traits are purely compile-time contracts. Types implementing traits contain **zero** runtime overhead:
  - No virtual method table pointers (`__vptr` / `vptr`).
  - No runtime type information (RTTI) metadata.
  - Struct and enum layouts are bit-for-bit identical regardless of how many traits they implement.
- **Reference Passing**: Trait receivers (`self: &Self` and `self: &mut Self`) pass the direct 64-bit stack or heap address of the receiver in register `%rdi` (per System V AMD64 ABI), identical to ordinary reference parameters.

### 11.2 Compile-Time Static Dispatch
- Calls on trait-implementing types and trait-bounded generic functions compile to direct, unpadded machine-code `CALL` instructions:
  - Target symbol: `<Target>__<Trait>_<method>` (e.g. `User__Describe_describe`).
  - No indirect jumps (`CALL *%rax`), branch predictor pollution, or dynamic method lookup.
- Generic functions with trait bounds `<T: Trait>` are monomorphized at compile time for each distinct concrete type. Each specialization receives its own optimized machine code and independent stack frame layout.

---

## 12. Memory Model for `Option`, `Result`, and Early-Return Error Propagation

### 12.1 Layout and Payload Protection
- `Option<T>` is laid out as a standard tagged union:
  - Tag (offset 0): `u32` (0 for `None`, 1 for `Some`).
  - Payload (offset 8 for 8-byte aligned `T`): `T`.
  - When `None` is active, the payload memory is inactive and never accessed or dropped.
- `Result<T, E>` is laid out as a tagged union:
  - Tag (offset 0): `u32` (0 for `Ok`, 1 for `Err`).
  - Payload (offset 8): Maximum of `sizeof(T)` and `sizeof(E)`.
  - Alignment: Maximum alignment of `tag`, `T`, and `E`.

### 12.2 Drop Mechanics During `?` Early Return
When a `?` operation fails (i.e. operand is `Option::None` or `Result::Err`):
1. **Unwinding Active Drop Scopes**: The compiler tracks all stack slots allocated within active lexical scopes (`drop_scopes`). Before emitting the early return instruction, drop glue executes in LIFO order for all still-owned local variables.
2. **Resource Integrity**: If a function allocated heap vectors or owned structs prior to the failing `?` expression, those resources are guaranteed to be freed (`vanta_free`) on the early return path without leaks.
3. **Moved Return Payload**: The error value transferred into the function's return slot is marked as transferred/moved, ensuring it is NOT destroyed during stack cleanup, but instead safely delivered to the caller.
4. **Move Invalidation**: Attempting to reuse an owned value passed into a `?` operand is rejected at compile time (`E0302`).

---

## 13. Iteration Memory Model, Range Semantics, and Collection Safety

### 13.1 Iterator Lifecycle and Stack Allocation
1. **Local Iterator Slot**:
   When entering a `for` loop, the iterable expression is evaluated and stored into a hidden local stack slot (`_for_iter`). The iterator instance is owned by the loop's outer scope.
2. **Short-Lived Receiver Borrow**:
   Each advancement cycle borrows the iterator mutably (`&mut _for_iter`) solely for the duration of the `next()` method call. No persistent borrow remains active during the execution of the loop body.
3. **Zero Heap Overhead**:
   Standard iterators such as `RangeI64` allocate zero heap memory. A `RangeI64` instance consists of two 64-bit integer fields (`current` and `end`), totaling 16 contiguous bytes on the stack.

### 13.2 Scope Drops and Control Flow
1. **Iteration Body Scope**:
   Each iteration of the loop body establishes a distinct drop scope. Values allocated or constructed inside the loop body that require destruction (e.g. `Vec<T>`) are deterministically destroyed at the conclusion of that iteration before the next `next()` invocation.
2. **Control Flow Destruction**:
   - `break`: Identifies all drop candidates allocated in nested drop scopes within the loop body, emits drop glue in reverse declaration order, and transfers control to the post-loop continuation block.
   - `continue`: Identifies all drop candidates allocated in the current iteration body, emits drop glue, and branches to the loop condition block.
   - Loop Exit: Upon exiting the loop, the iterator instance itself is dropped if its type requires destruction.

### 13.3 Historical Context: Deferral of `Vec<T>` in Milestone 12
Prior to Milestone 13, iterating over `Vec<T>` was deferred because the language lacked named lifetime parameters (`Iter<'a, T>`) and borrow-conflict tracking to prevent vector mutation or reallocation while element references were live.

---

## 14. Lifetime Parameters, Reference Safety, and Immutable Collection Iteration

### 14.1 Zero-Cost Compile-Time Erasure
Named lifetime parameters in Vanta (`'a`, `'b`, `'static`) are entirely erased during compilation:
- **No Runtime Metadata**: A reference `&'a T` has the exact same representation as an unannotated reference `&T` or raw memory address: an 8-byte pointer in register `%rdi` or on the stack.
- **Zero Space Overhead**: Structs parameterised by lifetimes (e.g., `Iter<'a, T>`, `View<'a, T>`) contain no runtime lifetime tags, generational counters, or epoch descriptors.
- **Static Monomorphization**: Concrete types are substituted during compilation, and all lifetime validation occurs exclusively during static semantic analysis.

### 14.2 Reference Provenance and Escape Prevention
Vanta's borrow checker enforces strict compile-time provenance:
1. **Stack Frame Escape Prevention**:
   - Returning a reference to a local variable or temporary stack slot is detected and rejected at compile time (`E0314`).
   - If a function returns a reference, its lifetime must be tied to at least one reference parameter (`E0312`).
2. **Provenance Propagation**:
   - The origin of every reference value (input parameter vs. local storage) is tracked through assignments, let-bindings, and pattern matching.
   - Borrowing local storage associates the reference with that local symbol's lifetime, preventing the reference from surviving beyond the scope of that storage (`E0315`).

### 14.3 Memory Layout and Lifecycle of `Iter<'a, T>`
Invoking `numbers.iter()` on a vector `Vec<T>` produces an immutable borrowed iterator:
1. **Layout**:
   - Offset 0 (8 bytes): Pointer to the vector's underlying contiguous heap buffer (`vec_ptr`).
   - Offset 8 (8 bytes): 64-bit unsigned integer element cursor (`index`, initialised to 0).
   - Total Size: Exactly 16 bytes on the stack, 8-byte aligned. Zero heap allocations.
2. **Advancement (`next`)**:
   - Loads vector length via `vanta_vec_len`.
   - If `index < len`, computes element address via `vanta_vec_index(vec_ptr, index)`.
   - Increments `index` by 1.
   - Returns `Option::Some(&'a T)` containing the direct pointer to the element in the vector buffer.
   - If `index >= len`, returns `Option::None`.
3. **Borrow Invalidation Protection**:
   - While `Iter<'a, T>` or any element reference derived from it is live, the parent `Vec<T>` variable is held in `BorrowState::Shared`.
   - Attempting to push (`push`), pop (`pop`), or move the vector invalidates the borrow and is rejected at compile time (`E0306`). This guarantees that vector buffer reallocation cannot create dangling pointers to elements.
   - When the `for` loop scope concludes or the iterator goes out of scope, the borrow count is decremented, restoring full mutability to the vector.

### 14.4 Sound Deferral of Consuming and Mutable Iteration
- **`into_iter()`**: Moving elements out of a heap vector one by one requires per-element drop flags to ensure that early loop exits (via `break` or `return`) drop unconsumed elements while avoiding double-frees for already-moved elements. Explicitly deferred (`E0316`).
- **`iter_mut()`**: Yielding mutable disjoint element references `&'a mut T` across iteration steps requires compile-time disjointness proofs. Explicitly deferred (`E0316`).

---

## 15. Owned String Memory Model and UTF-8 Safety Architecture

### 15.1 Physical Representation
An owned `String` descriptor occupies exactly 24 contiguous bytes on the stack (3 quadwords, 8-byte aligned on x86-64):
- Word 0 (`data_ptr: *mut u8`): Address of the heap-allocated UTF-8 buffer managed by `vanta_alloc`, `vanta_realloc`, and `vanta_free`. Empty strings use `NULL` (`0x0`).
- Word 1 (`len: i64`): Number of valid bytes in the buffer.
- Word 2 (`cap: i64`): Total allocated capacity in bytes.

```
       Stack Descriptor (24 bytes)                      Heap UTF-8 Buffer
+-------------------+---------+---------+        +---+---+---+---+---+---+---+---+
| data_ptr (8B)     | len(8B) | cap(8B) | ---->  | H | e | l | l | o |   |   |   |
+-------------------+---------+---------+        +---+---+---+---+---+---+---+---+
                                                 |<----- len = 5 ---->|
                                                 |<---------- cap = 8 ---------->|
```

### 15.2 Invariant Guarantees
1. **UTF-8 Correctness by Construction**: The bytes stored in `data_ptr[0..len]` are guaranteed to be valid UTF-8. There are no unchecked public constructors or byte-level mutation APIs.
2. **Deterministic Drop Mechanics**:
   - Destruction invokes `vanta_str_free(ptr)`. If `data_ptr != NULL`, `vanta_free(data_ptr)` is called, reclaiming heap memory and zeroing fields.
   - Variable reassignment (`s = new_s`) drops the previous buffer before acquiring the new descriptor.
   - Container destruction (`Vec<String>`) traverses each element via `vanta_vec_free_strings` before deallocating the vector spine.
3. **Borrow Invalidation and Aliasingxor-Mutability**:
   - Borrowing a string via `.as_str()` or `.as_bytes()` ties the resulting slice reference to the string's provenance.
   - Attempting to call `.push_str()` or `.clear()` while an immutable view is live is caught at compile time (`E0306`).
   - Self-aliasing in runtime primitives (e.g. `s.push_str(s.as_str())`) computes relative byte offsets to preserve pointer validity even if `vanta_realloc` relocates the backing buffer.
4. **Boundary Validation on Safe Slicing**:
   - `s.slice_bytes(start, end)` enforces `0 <= start <= end <= len` and verifies that `start` and `end` do not land on continuation bytes (`(b & 0xC0) == 0x80`).
   - Returns `Option<&str>` without copying or allocating heap memory.






