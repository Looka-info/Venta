# VANTA MILESTONE 6 ENGINEERING REPORT
## Heap Allocation, Monomorphized Generics, Automatic Destruction, and Vec<T>

---

## 1. Executive Summary

Milestone 6 introduces dynamic heap memory management, compile-time monomorphized generics, automatic linear destruction, and a safe heap-backed `Vec<T>` abstraction to the Vanta programming language.

All four core development stages were implemented and verified with zero reliance on LLVM or Cranelift:
1. **Runtime Heap Allocator Hooks**: 16-byte aligned allocation, reallocation, deallocation, overflow/OOM safety panics, and active allocation tracking in [`runtime/vanta_rt.c`](file:///c:/Users/mumer/OneDrive/Documents/Venta/runtime/vanta_rt.c).
2. **Compile-Time Monomorphized Generics**: Generic functions (`fn identity<T>(value: T) -> T`) and structs (`struct Pair<T1, T2> { first: T1, second: T2 }`), speculative parser lookahead with isolated diagnostic bags, type argument inference, recursion depth guards (`E0211`), arity checking (`E0209`), and inference diagnostics (`E0212`).
3. **Automatic Destruction & Move Semantics**: Linear ownership tracking (`InitState::Moved`), compile-time use-after-move rejection (`E0302`), and deterministic LIFO drop code generation at lexical scope exits and return terminators.
4. **`Vec<T>` Abstraction**: Standard 24-byte vector layout `(data_ptr: i64, len: i64, cap: i64)`, geometric capacity growth (0 -> 4 -> 8 -> 16), methods `.len()`, `.capacity()`, `.push(val)`, `.pop()`, and bounds-checked indexing `vec[i]` and `vec[i] = val`.

All 103 workspace tests pass cleanly under both unoptimized (`-O0`) and optimized (`-O1`) modes, `cargo clippy` emits zero warnings, and `cargo fmt` passes.

---

## 2. Stage 1 — Runtime Heap Allocator Hooks

### 2.1 Implementation Details
The runtime memory hooks were implemented in [`runtime/vanta_rt.c`](file:///c:/Users/mumer/OneDrive/Documents/Venta/runtime/vanta_rt.c) and exported with C linkage:
- `vanta_alloc(uint64_t size) -> void*`: Allocates memory aligned to 16 bytes. Checks for signed 63-bit integer overflow (`size > 0x7fffffffffffffffULL`) and NULL return from `malloc`, invoking `vanta_panic` on error. Increments global counter `g_vanta_alloc_count`.
- `vanta_realloc(void *ptr, uint64_t new_size) -> void*`: Safely reallocates a dynamic buffer. If `ptr == NULL`, delegates to `vanta_alloc`. If `new_size == 0`, delegates to `vanta_free`. Guards against overflow and allocation failure.
- `vanta_free(void *ptr)`: Frees the buffer and increments `g_vanta_free_count`. Safely handles NULL pointers without crashing.
- `vanta_active_allocs() -> int64_t`: Computes `g_vanta_alloc_count - g_vanta_free_count`, enabling leak detection in testing and runtime verification.

### 2.2 Semantic Integration
The allocation hooks were registered in `vanta_sema`'s intrinsic symbol table:
- `vanta_alloc`: `(size: i64) -> i64`
- `vanta_realloc`: `(ptr: i64, new_size: i64) -> i64`
- `vanta_free`: `(ptr: i64) -> ()`
- `vanta_active_allocs`: `() -> i64`
- User programs are protected from redefining these intrinsics with `E0103_REDEFINE_INTRINSIC`.

---

## 3. Stage 2 — Compile-Time Monomorphized Generics

### 3.1 AST & Parser Architecture
- **AST Nodes**: Extended [`crates/vanta_parser/src/ast.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_parser/src/ast.rs) with:
  - `TypeExprKind::Generic { name: String, type_args: Vec<TypeExpr> }`
  - `FnDecl.type_params: Vec<Ident>`
  - `StructDecl.type_params: Vec<Ident>`
  - `ExprKind::Call.type_args: Vec<TypeExpr>`
  - `ExprKind::StructLiteral.type_args: Vec<TypeExpr>`
- **Speculative Parsing Lookahead**: In [`crates/vanta_parser/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_parser/src/lib.rs), disambiguating generic `<TypeArgs>` from binary less-than expressions (`i < 10`) was implemented using speculative parsing with an isolated scratch diagnostic bag (`scratch_diags`). This completely isolates potential parse errors during speculative lookahead and prevents spurious error emission.

### 3.2 Monomorphization Engine
In [`crates/vanta_sema/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_sema/src/lib.rs):
- **Name Mangling**: Generic instantiations are mangled deterministically as `<symbol>__<types>` (e.g., `identity__i64`, `Pair__i64_bool`, `Vec__i64`).
- **AST Substitution**: Helper routines (`subst_type_expr`, `subst_expr`, `subst_stmt`, `subst_block`, `subst_fn_decl`) clone and rewrite AST nodes by replacing type parameter identifiers with concrete types.
- **Type Argument Inference**: When type arguments are omitted in function calls (`identity(42)`) or struct literals (`Pair { first: 1, second: 2 }`), `unify_type_arg` infers concrete types from argument expressions.
- **Diagnostics & Cycle Protection**:
  - `E0209_GENERIC_ARITY_MISMATCH`: Emitted when provided type arguments do not match parameter arity.
  - `E0212_CANNOT_INFER_TYPE_PARAM`: Emitted when unconstrained type parameters cannot be deduced.
  - `E0211_RECURSIVE_GENERIC_INSTANTIATION`: A recursion guard limit (depth 16) halts recursive monomorphization cycles (`recurse<T>(x: T) -> recurse(BoxNode { val: x })`) and prevents stack overflow.

---

## 4. Stage 3 — Automatic Destruction & Move Semantics

### 4.1 Linear Ownership & Move Tracking
- **Drop Requirements**: `Type::needs_drop(&self.structs)` recursively detects whether a type contains resources requiring deallocation:
  - `Vec` and `Vec__<T>` return `true`.
  - User structs containing fields satisfying `needs_drop` return `true`.
  - Primitive integers, booleans, and references (`&T`, `&mut T`) return `false`.
- **Borrow Checker Move State**: In `vanta_sema`, local variable states track `InitState::Uninitialized`, `InitState::Initialized`, and `InitState::Moved`.
  - Moving a non-Copy variable (via `let y = x;`, `y = x;`, `consume(x)`, or struct field initialization `Container { v: x }`) transitions `x` to `InitState::Moved`.
  - Subsequent access to `x` is rejected with `E0302_USE_OF_MOVED_VALUE`.

### 4.2 LIFO Destructor Code Generation
In [`crates/vanta_mir/src/lower.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/lower.rs):
- `MirLowerer` maintains lexical drop scopes: `drop_scopes: Vec<Vec<DropCandidate>>`.
- Moving a variable marks its `DropCandidate.is_moved = true`.
- At block scope exit and immediately preceding any `Terminator::Return`, the lowerer emits destructor calls (`vanta_vec_free`) in reverse order of declaration (LIFO) for all candidates where `!candidate.is_moved`.
- Structs with fields needing destruction recursively emit field-level drops via `Instruction::FieldAddr`.
- Handled function parameters: when a function takes a non-Copy type by value, the parameter is tracked in the outermost function scope and freed when the function exits.

---

## 5. Stage 4 — Safe Heap-Backed `Vec<T>` Abstraction

### 5.1 Memory Representation & ABI
`Vec<T>` is defined as a 24-byte struct with 8-byte alignment:
```text
+-----------------------+-----------------------+-----------------------+
|  data_ptr (offset 0)  |     len (offset 8)    |    cap (offset 16)    |
|       (8 bytes)       |       (8 bytes)       |       (8 bytes)       |
+-----------------------+-----------------------+-----------------------+
```
- In System V AMD64 ABI, when a 24-byte struct is passed by value to a function, the compiler passes its memory address in register `%rdi` / arg registers, and the callee's prologue copies all 24 bytes into its local frame stack slot, guaranteeing true by-value ownership transfer.

### 5.2 Operations & Safety Guarantees
- `Vec<T>::new()` / `Vec_new()`: Initializes `data_ptr = NULL`, `len = 0`, `cap = 0`.
- `.len() -> i64`: Returns vector length from offset 8 via `vanta_vec_len`.
- `.capacity() -> i64`: Returns allocated capacity from offset 16 via `vanta_vec_capacity`.
- `.push(val: T)`: Reallocates geometrically (4 -> 8 -> 16 -> 2*cap) when capacity is exhausted, copies element bytes, and increments length.
- `.pop() -> T`: Decrements length and returns the popped element. Panics with exit code 101 (`"Cannot pop from an empty Vec"`) if `len <= 0`.
- Indexed Read (`v[i]`) and Write (`v[i] = val`): Emits `emit_bounds_check(idx, len)` strictly dominating `IndexPtrAddr`. Rejects indices `< 0` or `>= len` with exit code 101 (`"Vanta panic: index out of bounds"`).

---

## 6. Verification and Acceptance Evidence

### 6.1 Test Suite Results
All 103 tests in the workspace pass (100% success rate):
- **78 End-to-End Tests**:
  - `test_e2e_allocator_hooks`: Verified allocation, reallocation, and zero active allocations.
  - `test_e2e_generic_function`: Monomorphized generic identity function.
  - `test_e2e_generic_struct`: Monomorphized generic `Pair<i64, bool>`.
  - `test_e2e_vec_basic_ops`: Verified `Vec_new()`, `.len()`, `.capacity()`, `.push()`, `.pop()`, read indexing `v[i]`, and write indexing `v[i] = 99`.
  - `test_e2e_vec_automatic_destruction`: Verified `vanta_active_allocs()` transitions from 0 -> 1 -> 0 on scope exit.
  - `test_e2e_vec_move_to_function`: Verified by-value vector move into function and cleanup.
  - `test_e2e_vec_move_assignment`: Verified `let b = a;` move transfer and single free.
  - `test_e2e_struct_with_nested_vec_drop`: Verified recursive destruction of structs containing vectors.
  - `test_e2e_vec_geometric_growth`: Verified capacity scaling across 10 pushes.
  - `test_e2e_vec_passed_by_ref`: Verified `&Vec<i64>` reference borrowing.
  - `test_compile_fail_vec_use_after_move`: Verified `E0302_USE_OF_MOVED_VALUE`.
  - `test_compile_fail_generic_arity_mismatch`: Verified `E0209_GENERIC_ARITY_MISMATCH`.
  - `test_compile_fail_cannot_infer_type_param`: Verified `E0212_CANNOT_INFER_TYPE_PARAM`.
  - `test_compile_fail_recursive_generic_instantiation`: Verified `E0211_RECURSIVE_GENERIC_INSTANTIATION`.
  - `test_e2e_vec_pop_empty_panic`: Verified panic on empty pop.
  - `test_e2e_vec_out_of_bounds_panic`: Verified panic on index out of bounds.
- **12 Optimizer Tests**: All constant folding, branch elimination, and dead code elimination tests pass.
- **13 Component Unit Tests**: Lexer, parser, MIR verifier, sema layout, codegen, source, and diagnostics tests pass.

### 6.2 Code Quality & Formatting
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings, clean.
- `cargo fmt --check`: 0 formatting diffs, clean.
- `gcc -c -Wall -Wextra -Werror runtime/vanta_rt.c`: 0 warnings, clean.

---

## 7. Changed Files Summary

| File | Changes Made |
| :--- | :--- |
| [`runtime/vanta_rt.c`](file:///c:/Users/mumer/OneDrive/Documents/Venta/runtime/vanta_rt.c) | Added `vanta_alloc`, `vanta_realloc`, `vanta_free`, `vanta_active_allocs`, `vanta_vec_init`, `vanta_vec_push`, `vanta_vec_pop`, `vanta_vec_index`, `vanta_vec_free`, `vanta_vec_len`, `vanta_vec_capacity`. |
| [`crates/vanta_parser/src/ast.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_parser/src/ast.rs) | Added generic parameters to `FnDecl`, `StructDecl`, `TypeExprKind::Generic`, and type arguments to `Call` and `StructLiteral`. |
| [`crates/vanta_parser/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_parser/src/lib.rs) | Implemented generic parsing with speculative lookahead and isolated scratch diagnostics bag. Added `::` associated call parsing. |
| [`crates/vanta_sema/src/types.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_sema/src/types.rs) | Added `Type::needs_drop`, `is_vec`, `vec_elem_type`, and `from_mangled_string`. |
| [`crates/vanta_sema/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_sema/src/lib.rs) | Registered allocator intrinsics and `Vec` struct template. Implemented AST substitution, generic function/struct monomorphization, type inference, recursion guard (`E0211`), move tracking (`InitState::Moved`), and `Vec` method/index typechecking. |
| [`crates/vanta_mir/src/lower.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/lower.rs) | Added `DropCandidate` and `drop_scopes`. Implemented LIFO drop generation at block exit and `return`, move detection, `lower_index_base_and_len` for vectors, vector methods (`len`, `capacity`, `push`, `pop`), and `Vec_new`. |
| [`crates/vanta_codegen_x86_64/src/frame.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/frame.rs) | Added support for by-value struct parameter spilling in function prologue copying all struct words from argument register. |
| [`crates/vanta_driver/tests/e2e_tests.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_driver/tests/e2e_tests.rs) | Added comprehensive E2E tests for generics, allocator hooks, `Vec<T>`, automatic destruction, move semantics, nested struct drops, and panics. |
| [`docs/grammar.ebnf`](file:///c:/Users/mumer/OneDrive/Documents/Venta/docs/grammar.ebnf) | Updated formal grammar with generic parameters, arguments, and associated call paths. |
| [`docs/language-spec.md`](file:///c:/Users/mumer/OneDrive/Documents/Venta/docs/language-spec.md) | Documented Milestone 6 semantics: allocator hooks, monomorphized generics, move semantics, and `Vec<T>`. |
| [`docs/memory-model.md`](file:///c:/Users/mumer/OneDrive/Documents/Venta/docs/memory-model.md) | Added Section 8 detailing heap allocation, linear ownership states, LIFO destructor generation, and `Vec<T>` layout. |
| [`docs/ir-spec.md`](file:///c:/Users/mumer/OneDrive/Documents/Venta/docs/ir-spec.md) | Added Section 6 covering generic monomorphization and drop scope lowering in MIR. |
