# Vanta Milestone 5 Engineering Report
## User-Defined Structs, Methods, Dynamic Slices, and ABI Correctness

**Date**: October 2026  
**Compiler Version**: `vantac 0.1.0`  
**Target Architecture**: Linux x86-64, System V AMD64 ABI, ELF64  
**Optimization Profiles**: `-O0` (No optimization), `-O1` (Constant propagation & Dead code elimination)

---

## 1. Executive Summary

Milestone 5 expands the expressive power of **Vanta** by introducing user-defined product types (structs), inherent method syntax with compile-time static dispatch, and immutable dynamically sized slices (`&[T]`), all while maintaining strict memory safety, deterministic resource layout, and zero-overhead native execution performance.

Key capabilities delivered in Milestone 5:
1. **User-Defined Structs**:
   - Declarations (`struct Point { x: i64, y: i64 }`).
   - Named field initializers (`Point { x: 10, y: 20 }`).
   - Field member access (`point.x`) and assignment (`point.x = 42`).
   - Nested structs (`player.pos.x = 50`) stored inline by value without pointer indirection.
   - Deterministic memory layout using natural alignment and padding rules without hidden reordering.
   - Cycle detection rejecting infinitely sized recursive structs (`E0208`).
   - Semantic checks enforcing field completeness (`E0107`), field uniqueness (`E0106`), field existence (`E0105`), and place mutability (`E0303`).
2. **Inherent Methods**:
   - `impl StructName { ... }` blocks with `self: &Self`, `self: &mut Self`, or `self: Self` receiver semantics.
   - Zero-cost static dispatch: method calls (`p.translate(dx, dy)`) resolve at compile time to mangled symbol references without vtables or dynamic dispatch overhead.
   - Automatic referencing and auto-dereferencing for receiver field access and method calls.
   - Receiver validation requiring `self` as the first parameter (`E0109`) and method resolution verification (`E0108`).
3. **Dynamic Slices (`&[T]`)**:
   - Immutable slice representation via a 16-byte, 2-word fat pointer descriptor: `(data_ptr: *const T, len: u64)`.
   - Array-to-slice borrow coercion: `let view: &[i64] = &arr;`.
   - Built-in `.len()` query method.
   - Full bounds-checked element indexing via `vanta_bounds_check_fail`.
   - Borrow checker integration enforcing shared borrow lifetimes on the backing array, preventing mutation or reassignment while borrowed (`E0306`).
4. **ABI and Code Generation**:
   - Alignment-aware stack frame layout respecting each slot's natural size and alignment requirements.
   - System V AMD64 ABI compliance: slices passed as two 64-bit integer registers, struct references passed in standard argument registers.
5. **Differential Verification**:
   - 100% pass rate across all 62 end-to-end integration tests under both `-O0` and `-O1`.
   - Full workspace test suite pass (lexer, parser, sema, MIR, codegen, diagnostics, driver, opt).
   - Zero warnings under `cargo clippy --workspace --all-targets -- -D warnings`.
   - Formatted cleanly according to `cargo fmt --check`.

---

## 2. User-Defined Structs and Memory Layout

### 2.1 Declaration and Instantiation
Structs are declared as nominal types with named fields:
```vanta
struct Point {
    x: i64,
    y: i64,
}

struct Player {
    pos: Point,
    health: i64,
}
```

Instantiation uses explicit named field initializers:
```vanta
let mut p = Player {
    pos: Point { x: 15, y: 30 },
    health: 100,
};
```

### 2.2 Memory Layout and Natural Alignment
Vanta implements a deterministic C-compatible layout algorithm (`compute_layout` in `vanta_sema::types`):
1. **Declaration Order Preservation**: Fields are laid out in the exact sequence declared by the programmer. The compiler performs no reordering.
2. **Natural Alignment**: Every field's offset is aligned to a multiple of its natural alignment (`align(T) = min(sizeof(T), 8)`). If preceding fields leave the offset unaligned, natural padding bytes are inserted.
3. **Trailing Padding and Total Size**: The struct's overall size is rounded up to a multiple of its maximum field alignment:
   ```text
   current_offset = (current_offset + field_align - 1) & !(field_align - 1);
   field_offset = current_offset;
   current_offset += field_size;
   total_size = (current_offset + max_align - 1) & !(max_align - 1);
   ```
4. **Padding Example**:
   ```vanta
   struct Mixed {
       a: u8,   // offset 0, size 1, align 1
                // 7 padding bytes inserted (offsets 1..7)
       b: i64,  // offset 8, size 8, align 8
       c: u8,   // offset 16, size 1, align 1
                // 7 padding bytes inserted (offsets 17..23)
   }            // total size: 24 bytes, align: 8 bytes
   ```

### 2.3 Nested Structs
Nested structs are laid out inline (by value) within the outer struct's memory allocation. For instance, `Player` occupies `16 (Point) + 8 (health) = 24 bytes`. No secondary heap pointers or indirection steps are created.

### 2.4 Cycle Detection and Recursive Structs
To guarantee finite stack frame sizes and compile-time determinism, by-value recursive structs are strictly forbidden:
```vanta
struct Node {
    val: i64,
    next: Node, // Compile Error: E0208
}
```
During semantic analysis Pass 1, `vanta_sema` constructs a dependency graph of all declared structs and applies depth-first search cycle detection. Any circular by-value containment emits `E0208_RECURSIVE_STRUCT` with the exact recursive cycle path.

---

## 3. Inherent Methods and Static Dispatch

### 3.1 Method Syntax and Receivers
Inherent methods are declared inside `impl` blocks:
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

Vanta supports three receiver forms:
- `self: &Self`: Shared, immutable reference to the struct instance.
- `self: &mut Self`: Exclusive, mutable reference allowing in-place field updates.
- `self: Self`: By-value receiver consuming or copying the struct instance.

### 3.2 Receiver Validation and Mutability
1. **Receiver Parameter Enforcement**: The first parameter must be named `self` and typed `Self`, `&Self`, or `&mut Self`. Omission or incorrect naming emits `E0109_INVALID_RECEIVER`.
2. **Compile-Time Mutability Check**: Methods with `&Self` cannot assign to struct fields (`self.x = 99` emits `E0303_MUTATE_IMMUTABLE`). Methods with `&mut Self` require a mutable place binding at the call site.
3. **Auto-Referencing and Auto-Dereferencing**:
   - Calling `p.translate(dx, dy)` on a mutable binding `let mut p` automatically borrows `&mut p` without requiring the programmer to write `(&mut p).translate(dx, dy)`.
   - Accessing fields on references (`self.x`) automatically dereferences the pointer.

### 3.3 Zero-Cost Static Dispatch
Method calls are resolved entirely at compile time:
- Method names are deterministically mangled as `<StructName>_<method_name>` (e.g. `Point_translate`).
- In MIR lowering, method invocations are converted to direct `Instruction::Call` operations passing the receiver address as the first argument in `%rdi`.
- No virtual tables (vtables), dynamic dispatch pointers, or runtime metadata lookups are generated. Method calls execute at the exact same speed as free functions.

---

## 4. Dynamic Slices (`&[T]`)

### 4.1 Fat Pointer Representation
Dynamic slices (`&[T]`) provide uniform, bounds-safe views over contiguous sequences of data:
- **Descriptor Layout**:
  - `data_ptr: *const T` (8 bytes, memory address of element 0).
  - `length: u64` (8 bytes, element count).
  - Total size: 16 bytes. Alignment: 8 bytes.
- **Copy Semantics**: `&[T]` implements `Copy` and can be freely copied between variables, parameters, and return values without heap allocation.

### 4.2 Array-to-Slice Coercion
An immutable reference to a fixed array (`&[T; N]`) implicitly coerces into an immutable dynamic slice (`&[T]`):
```vanta
let arr: [i64; 4] = [10, 20, 30, 40];
let view: &[i64] = &arr;
```
During MIR lowering, `lower_slice_expr` extracts the array place address and materializes a `ConstInt` for length `N`, producing a 2-word slice descriptor without runtime cost.

### 4.3 Slice Operations
1. **Length Query**: `view.len()` lowers to reading the descriptor's length word directly.
2. **Bounds-Checked Indexing**: `view[i]` performs runtime verification that `0 <= i < len`. If index `i` is out of bounds, execution diverts to `vanta_bounds_check_fail` and halts with exit status `101`.
3. **Borrow Safety**: While `view` borrows `&arr`, the backing array `arr` cannot be modified or reassigned (`arr = [4, 5, 6]` emits `E0306_ASSIGN_BORROWED_VARIABLE`).
4. **Immutability**: Slices in Milestone 5 are strictly read-only (`view[0] = 5` emits `E0303_MUTATE_IMMUTABLE`).

---

## 5. Compiler Intermediate Representation and Backend Codegen

### 5.1 MIR Additions
1. **`Instruction::FieldAddr`**:
   ```rust
   FieldAddr { dest: ValueId, base_ptr: ValueId, offset: u32 }
   ```
   Computes the address of a struct field by adding `offset` to `base_ptr`. Selected to:
   ```asm
   mov r10, base_ptr
   add r10, offset
   mov dest, r10
   ```
2. **`LocalSlot` Alignment and Sizing**:
   `LocalSlot` holds explicit `size: usize` and `align: usize` fields, allowing stack frame layout to correctly allocate and align arbitrary struct sizes.
3. **Recursive Struct Field Initialization**:
   `lower_init_struct_fields` traverses nested struct literals and recursively initializes fields in-place at their calculated offsets, eliminating intermediate temporary copies.
4. **Whole-Struct Copying**:
   `copy_struct_bytes` copies structs byte-by-byte in 8-byte, 4-byte, or 1-byte chunks during struct assignment and pass-by-value.

### 5.2 System V AMD64 ABI Conformance
- **Slices**: Passed across function call boundaries in two 64-bit integer registers (e.g. `%rdi` for pointer and `%rsi` for length), matching the System V ABI classification for two-word composite structures.
- **References**: Reference receivers (`&Self`, `&mut Self`) pass as standard 64-bit memory addresses in the first available argument register (`%rdi`).
- **Stack Alignment**: The stack frame maintains strict 16-byte alignment prior to every `call` instruction.

---

## 6. Verification and Empirical Test Results

### 6.1 Test Suite Breakdown
All tests run natively under Linux x86-64 in WSL2 Ubuntu using `cargo test --workspace`:

| Test Suite Component | Total Tests | Passed | Failed | Status |
|---|---|---|---|---|
| **E2E Integration Tests (`vanta_driver`)** | 62 | 62 | 0 | **100% Pass** |
| **MIR Optimization Tests (`opt_tests`)** | 12 | 12 | 0 | **100% Pass** |
| **Semantic Analysis (`vanta_sema`)** | 6 | 6 | 0 | **100% Pass** |
| **Lexer Unit Tests (`vanta_lexer`)** | 2 | 2 | 0 | **100% Pass** |
| **Parser Unit Tests (`vanta_parser`)** | 1 | 1 | 0 | **100% Pass** |
| **MIR Verification (`vanta_mir`)** | 1 | 1 | 0 | **100% Pass** |
| **Codegen & ELF Generation (`vanta_codegen_x86_64`)** | 1 | 1 | 0 | **100% Pass** |
| **Diagnostics Serialization (`vanta_diagnostics`)** | 1 | 1 | 0 | **100% Pass** |
| **Source Location Map (`vanta_source`)** | 1 | 1 | 0 | **100% Pass** |
| **Total Workspace Tests** | **87** | **87** | **0** | **100% Pass** |

### 6.2 Milestone 5 Specific E2E Tests
The following 15 new end-to-end tests verify struct, method, and slice semantics:
1. `test_e2e_struct_declaration_and_field_access`: Struct declaration, initialization, and field read.
2. `test_e2e_struct_field_mutation`: Mutation of fields on a mutable struct binding.
3. `test_e2e_struct_layout_and_padding`: Mixed byte/int struct verifying natural alignment padding.
4. `test_e2e_nested_structs`: Multilevel nested structs with field mutations.
5. `test_e2e_inherent_methods_mut_and_ref`: `impl` block with `&mut Self` mutator and `&Self` reader methods.
6. `test_e2e_dynamic_slice_borrow_and_len`: Dynamic slice coercion from array and `.len()` invocation.
7. `test_e2e_dynamic_slice_passed_to_function`: Passing `&[i64]` to a function calculating sum in a while loop.
8. `test_e2e_dynamic_slice_out_of_bounds_panic`: Dynamic slice indexing triggering bounds panic (exit 101).
9. `test_compile_fail_recursive_struct`: Rejection of recursive struct with `E0208`.
10. `test_compile_fail_missing_field`: Rejection of missing field in struct literal with `E0107`.
11. `test_compile_fail_duplicate_field`: Rejection of duplicate field in struct literal with `E0106`.
12. `test_compile_fail_unknown_method`: Calling undefined method on struct emits `E0108`.
13. `test_compile_fail_invalid_receiver`: Inherent method without `self` emits `E0109`.
14. `test_compile_fail_mutating_through_immutable_ref`: Mutating struct field through `&Self` emits `E0303`.
15. `test_compile_fail_mutating_array_while_borrowed_by_slice`: Reassignment of array while borrowed by slice emits `E0306`.

### 6.3 Differential Optimization Verification
All E2E tests are executed using differential testing:
- Each source program is compiled and executed under `-O0` (no optimization) and `-O1` (constant folding, constant branch elimination, dead-code elimination).
- Exit codes, stdout bytes, and stderr output are compared; both optimization levels yield 100% identical observable behavior.

### 6.4 Clean Build and Linter Status
- `cargo clippy --workspace --all-targets -- -D warnings`: Passed with zero warnings.
- `cargo fmt --check`: Passed with zero diffs.
- `cargo build --release --bin vantac`: Compiled release binary without warnings in 7.67 seconds.

---

## 7. AI-First Diagnostic Design

Milestone 5 preserves Vanta's AI-first developer experience by extending structured diagnostics:
- **Stable Error Codes**:
  - `E0105`: Unknown struct field name.
  - `E0106`: Duplicate field in struct instantiation.
  - `E0107`: Missing field in struct instantiation.
  - `E0108`: Unknown method on struct.
  - `E0109`: Invalid receiver parameter in inherent method.
  - `E0208`: Recursive struct with cycle explanation.
  - `E0303`: Mutation through immutable reference or immutable slice.
  - `E0306`: Assignment to variable while actively borrowed by a slice.
- **Machine-Actionable JSON**: Diagnostics provide precise source spans, expected/actual types, and code suggestions, enabling automated AI coding agents to detect and fix syntax or type errors in a single turn without hallucinated assumptions.

---

## 8. Milestone 5 Deliverables and Next Steps

### 8.1 Deliverables Checklist
- [x] Struct declarations, field access, field mutation, and nested structs.
- [x] Deterministic struct memory layout with natural alignment and padding.
- [x] Cycle detection for recursive structs (`E0208`).
- [x] Inherent `impl` blocks with `&Self`, `&mut Self`, and `Self` receivers.
- [x] Zero-cost static method call resolution.
- [x] Dynamic slices `&[T]` with 2-word fat pointer descriptors.
- [x] Array-to-slice borrow coercion.
- [x] Dynamic slice `.len()` method and bounds-checked indexing.
- [x] Borrow checker preventing mutation of arrays while borrowed by slices.
- [x] System V AMD64 ABI classification and stack frame alignment.
- [x] Differential verification passing 100% under `-O0` and `-O1`.
- [x] Updated language specification, memory model, IR specification, and grammar.

### 8.2 Roadmap for Milestone 6
With structs, inherent methods, and dynamic slices established, the next recommended compiler milestone encompasses:
1. **Dynamic Heap Allocation & Standard Vectors (`Vec<T>`)**: Safe heap allocator integration (`vanta_alloc`, `vanta_free`), RAII destructor hooks, and dynamically resizable buffers.
2. **Generic Types and Functions (`struct Pair<T> { first: T, second: T }`)**: Monomorphized compile-time generics.
3. **Mutable Dynamic Slices (`&mut [T]`)**: In-place slice mutation and slicing subranges (`&arr[1..3]`).
4. **Pattern Matching & Tagged Unions (`enum Result<T, E>`)**: Algebraic data types and exhaustiveness checking.
