# Milestone 13 Engineering Report: Lifetime Parameters, Reference Safety, and Immutable Collection Iteration

---

## Executive Summary

Milestone 13 introduces named lifetime parameters, compile-time reference provenance tracking, and sound immutable borrowed collection iteration to the Vanta systems programming language.

Building on Vanta's custom x86-64 backend, pure-Rust ELF64 object writer, statically dispatched trait system, affine ownership model, and the canonical `for ... in ...` loop construct from Milestone 12, this milestone achieves:

1. **Named Lifetime Parameters**: Explicit lexical and syntactic support for lifetime parameters (`'a`, `'b`, `'static`) in functions, structs, enums, traits, impl blocks, reference types (`&'a T`, `&'a mut T`), and slice types (`&'a [T]`).
2. **Explicit and Predictable Lifetime Elision**: Transparent elision rules that permit single-reference input elision while strictly rejecting ambiguous multi-reference origins at compile time without hidden heuristic guesses.
3. **Compile-Time Reference Provenance Tracking**: Comprehensive tracking connecting reference values to their underlying storage, rejecting references to local stack frames, outliving references, and unbound return lifetimes.
4. **Immutable Borrowed Vector Iteration (`vec.iter()`)**: Non-allocating, stack-allocated `Iter<'a, T>` struct implementing `Iterator<&'a T>` that yields `Option<&'a T>` through static dispatch and zero vtables.
5. **Borrow Conflict and Reallocation Protection**: Compile-time shared borrow tracking that strictly prevents vector mutation (`push`, `pop`) or moves while element references or iterators remain live.
6. **Sound Deferral of Complex Iteration Forms**: Actionable compile-time diagnostics (`E0316`) deferring consuming vector iteration (`into_iter()`) and mutable vector iteration (`iter_mut()`).
7. **Zero-Cost Compile-Time Erasure**: Complete erasure of lifetime parameters prior to MIR lowering, maintaining native 64-bit machine pointer representations and zero runtime overhead.
8. **Full Verification and AI Tooling**: 22 new differential and diagnostic tests in `crates/vanta_driver/tests/lifetime_tests.rs`, canonical formatting in `vanta_fmt`, clean workspace Clippy (`-D warnings`), and a complete end-to-end example in `examples/16_lifetimes_and_iter/main.va`.

---

## 1. Architectural Baseline & Audit

Prior to implementation, an audit of the compiler codebase confirmed:
- References (`&T`, `&mut T`) were represented in the type system as `Type::Ref { is_mut: bool, inner: Box<Type> }` with no lifetime parameter metadata.
- Function return types lacked lifetime annotations, and returning references was restricted or weakly validated.
- In Milestone 12, collection iteration (`Vec<T>`) was deliberately deferred because the language lacked lifetime parameters on structs (`Iter<'a, T>`) and borrow conflict tracking during iteration.
- Vector growth and reallocation (`push`) reallocated the underlying heap buffer via `vanta_realloc`, which would invalidate live element pointers if mutation were permitted during iteration.
- The compiler targets a custom x86-64 backend and pure-Rust ELF64 writer with zero LLVM or Cranelift dependencies.

---

## 2. Lifetime Syntax & Type System Integration

### 2.1 Lexical Analysis (`vanta_lexer`)
- Added `TokenKind::Lifetime(String)` recognizing identifiers prefixed by a single quote `'` (e.g. `'a`, `'b`, `'static`).
- Nested multi-character lifetimes and the reserved `'static` identifier are parsed and tokenized deterministically without conflict with character literals.

### 2.2 AST & Grammar (`vanta_parser`)
- Extended generic parameter declarations to support lifetime parameters preceding type parameters:
  - `FnDecl`: `lifetime_params: Vec<Ident>`
  - `StructDecl`: `lifetime_params: Vec<Ident>`
  - `EnumDecl`: `lifetime_params: Vec<Ident>`
  - `TraitDecl`: `lifetime_params: Vec<Ident>`
  - `ImplBlock`: `lifetime_params: Vec<Ident>`
- Extended type expressions:
  - `TypeExprKind::Reference { is_mut: bool, inner: Box<TypeExpr>, lifetime: Option<Ident> }`
  - `TypeExprKind::Slice { inner: Box<TypeExpr>, lifetime: Option<Ident> }`
- Updated formal grammar in `docs/grammar.ebnf`:
  ```ebnf
  GenericParamList = GenericParam { "," GenericParam } [ "," ] ;
  GenericParam     = Lifetime | TypeParam ;
  Lifetime         = "'" Identifier ;
  ReferenceType    = "&" [ Lifetime ] [ "mut" ] Type ;
  SliceType        = [ Lifetime ] "[" Type "]" ;
  ```

### 2.3 Semantic Validation (`vanta_sema`)
1. **Duplicate Lifetime Rejection (`E0310`)**:
   Ensures no duplicate lifetime identifiers are declared within a single generic parameter list.
2. **Undeclared Lifetime Rejection (`E0311`)**:
   Validates that all lifetime parameters appearing in signatures, fields, or reference types are in scope (declared in the enclosing item's header or equal to `'static`).
3. **Return Lifetime Outlives Input Origins (`E0312`)**:
   Ensures that any lifetime parameter on a returned reference appears in at least one input parameter; floating, unbound return lifetimes are rejected.
4. **Ambiguous Lifetime Elision Rejection (`E0313`)**:
   When a function returns a reference and has multiple reference parameters, explicit lifetime annotations are required; the compiler refuses to guess an elision origin.
5. **Local Reference Escape Rejection (`E0314`)**:
   Strictly rejects returning references to local stack-allocated variables or temporary expressions.
6. **Scope-Bound Storage Tracking (`E0315`)**:
   Associates references with their source symbol's scope, ensuring references cannot outlive their source storage.

---

## 3. Immutable Vector Iteration (`vec.iter()`)

### 3.1 The `Iter<'a, T>` Type
In `library/std/iter.va`:
```vanta
pub struct Iter<'a, T> {
    pub vec_ptr: i64,
    pub index: i64,
}
```
At runtime, `Iter<'a, T>` occupies exactly 16 bytes on the stack:
- Offset 0: 64-bit base pointer to the vector's heap buffer.
- Offset 8: 64-bit element cursor index (initialised to 0).
Zero heap allocation is required to instantiate or advance an iterator.

### 3.2 Method Call Typing & Monomorphization
When `vec.iter()` is invoked on `Vec<T>`:
1. The receiver `vec` is validated and placed into `BorrowState::Shared(1)`.
2. The monomorphized iterator struct `Iter__<T>` is instantiated in `self.ctx.structs`.
3. The monomorphized trait implementation `Iterator<&'a T>` is registered for `Iter__<T>`, with a specialized static function `{Iter__T}_std_iter_Iterator_next(self: &mut Iter__T) -> Option<&'a T>`.
4. The method call evaluates to `Type::Struct("Iter__<T>")`.

### 3.3 Borrow Conflict Protection (`E0306`)
While an iterator or derived reference is active:
- Calling `v.push(val)` or `v.pop()` is rejected at compile time with `E0306_MUTATE_WHILE_BORROWED`.
- Moving `v` (e.g. `let v2 = v;`) is rejected at compile time with `E0306_MUTATE_WHILE_BORROWED`.
- When the `for` loop concludes or the iterator goes out of scope, the borrow is released, permitting subsequent vector mutations or reallocations.

### 3.4 Deferral of Consuming & Mutable Iteration (`E0316`)
- `v.into_iter()` is rejected with `E0316_MUTATE_DURING_ITERATION`, explaining that partial moves of heap-allocated elements require per-element dynamic drop tracking.
- `v.iter_mut()` is rejected with `E0316_MUTATE_DURING_ITERATION`, guiding developers to use `vec.iter()` for immutable borrowed iteration.

---

## 4. MIR Lowering & Native x86-64 Execution

### 4.1 Lowering `vec.iter()`
In `crates/vanta_mir/src/lower.rs`:
1. When lowering `MethodCall` on a vector with method `"iter"`:
   - Allocates a stack slot `_iter_tmp` of type `Iter__<T>` (16 bytes, align 8).
   - Computes the vector address and stores it at offset 0.
   - Stores 64-bit integer 0 at offset 8.
   - Returns the slot address pointer.

### 4.2 Synthetic Next Function Generation
In `crates/vanta_mir/src/lower.rs::generate_iter_next_fn`:
Generates the specialized native next function `{Iter__T}_std_iter_Iterator_next`:
- Calls `vanta_vec_len` to obtain the buffer's active length.
- Compares `index < len`.
- If true: calls `vanta_vec_index` to compute the direct element address, increments `index`, and constructs `Option::Some(elem_ptr)`.
- If false: constructs `Option::None`.
- Returns the tagged union pointer directly.

### 4.3 Control Flow & Dereferencing
- `for x in v.iter()` desugars into the canonical loop CFG with single evaluation, condition testing, payload extraction, and loop-body drop cleanup.
- Dereferencing `*x` loads directly from the element pointer through `Instruction::LoadIndirect`.
- Loop control flow (`break` and `continue`) cleanly interacts with iteration and drop scopes.

---

## 5. Tooling, Canonical Formatting & AI Diagnostics

### 5.1 Canonical Source Formatter (`vantac fmt`)
- Updated `vanta_fmt` tokenizer and indentation engine to format named lifetimes:
  - Generics: `fn choose<'a, T>(...)`, `struct View<'a, T>`
  - References: `&'a T`, `&'a mut T`, `&'a [T]`
- Maintained idempotency across all repo source files:
  `format(format(code)) == format(code)`.

### 5.2 Machine-Readable Diagnostics (`--diagnostics=json`)
All new lifetime and iterator diagnostics emit structured JSON complying with schema v1.0.0:
- `E0310`: Duplicate lifetime parameter.
- `E0311`: Undeclared lifetime parameter.
- `E0312`: Return lifetime outlives input parameter origins.
- `E0313`: Ambiguous lifetime elision.
- `E0314`: Cannot return reference to local variable.
- `E0315`: Borrow outlives referenced storage scope.
- `E0316`: Deferred vector iteration form.

---

## 6. Verification & Test Evidence

### 6.1 Test Suite Breakdown (`crates/vanta_driver/tests/lifetime_tests.rs`)
The comprehensive integration test suite contains 22 tests verifying all phases of Milestone 13:

| Test Name | Category | Verified Invariant |
|---|---|---|
| `test_diagnostic_e0310_duplicate_lifetime_in_fn` | Compile-Fail | Rejects duplicate lifetime `'a, 'a` in functions (`E0310`) |
| `test_diagnostic_e0310_duplicate_lifetime_in_struct` | Compile-Fail | Rejects duplicate lifetime in structs (`E0310`) |
| `test_diagnostic_e0311_undeclared_lifetime_in_param` | Compile-Fail | Rejects undeclared lifetime `'a` in parameter (`E0311`) |
| `test_diagnostic_e0311_undeclared_lifetime_in_return` | Compile-Fail | Rejects undeclared lifetime `'b` in return type (`E0311`) |
| `test_diagnostic_e0312_return_lifetime_outlives_inputs` | Compile-Fail | Rejects return lifetime not matching any input (`E0312`) |
| `test_diagnostic_e0313_ambiguous_lifetime_elision` | Compile-Fail | Rejects ambiguous elision across multiple inputs (`E0313`) |
| `test_diagnostic_e0314_return_reference_to_local_var` | Compile-Fail | Rejects returning reference to local stack slot (`E0314`) |
| `test_diagnostic_e0314_return_ref_with_zero_ref_params`| Compile-Fail | Rejects returning reference without reference inputs (`E0314`)|
| `test_diagnostic_e0306_mutate_during_for_iteration` | Compile-Fail | Rejects `push` while vector is borrowed by iterator (`E0306`)|
| `test_diagnostic_e0306_move_vector_during_iteration` | Compile-Fail | Rejects moving vector while iterator is active (`E0306`)|
| `test_diagnostic_e0316_into_iter_deferred` | Compile-Fail | Emits actionable deferral diagnostic for `into_iter` (`E0316`)|
| `test_diagnostic_e0316_iter_mut_deferred` | Compile-Fail | Emits actionable deferral diagnostic for `iter_mut` (`E0316`)|
| `test_valid_lifetime_identity_function` | Differential | Identity function returning reference executes under -O0 and -O1 |
| `test_valid_lifetime_conditional_choice` | Differential | Conditional choice between two reference inputs under -O0 and -O1 |
| `test_valid_lifetime_elision_single_ref_param` | Differential | Single reference input elision executes identically under -O0 and -O1 |
| `test_vector_empty_iter` | Differential | Iteration over empty vector executes 0 iterations without panic |
| `test_vector_single_element_iter` | Differential | Iteration over single-element vector yields element reference |
| `test_vector_multi_element_iter` | Differential | Iteration over multiple elements yields all elements sequentially |
| `test_vector_reallocation_before_iter` | Differential | Vector reallocated before iteration preserves valid iteration |
| `test_vector_iter_break_and_continue` | Differential | Break and continue correctly interact with vector iteration |
| `test_vector_mutate_after_iteration` | Differential | Vector mutation is restored once iteration scope concludes |
| `test_vector_multiple_consecutive_iterations` | Differential | Multiple consecutive iterations over the same vector succeed |

### 6.2 Test Execution Results
All test suites executed under Ubuntu WSL2:
- `cargo test --test lifetime_tests`: **22/22 passed**
- `cargo test --test iter_tests`: **13/13 passed**
- `cargo test --test e2e_tests`: **62/62 passed**
- `cargo test --test fmt_tests`: **14/14 passed**
- `cargo test --test opt_tests`: **12/12 passed**
- `cargo test --test trait_tests`: **19/19 passed**
- `cargo test --test module_tests`: **20/20 passed**
- `cargo test --test std_tests`: **16/16 passed**
- `cargo test --workspace`: **100% passed (zero regressions)**
- `cargo fmt --check`: **Clean (code 0)**
- `cargo clippy --workspace --all-targets -- -D warnings`: **Clean (code 0)**
- `cargo build --release --bin vantac`: **Built successfully**

### 6.3 Canonical Example Execution
The milestone showcase program `examples/16_lifetimes_and_iter/main.va` compiles and executes cleanly with native output:
```text
=== Vanta Milestone 13: Lifetimes & Immutable Vector Iteration ===
1. Function with explicit lifetime 'a choose(&x, &y, true): 100
2. Lifetime elision on single borrowed parameter identity(&number): 42
3. Immutable borrowed vector iteration via numbers.iter():
  element: 10
  element: 20
  element: 30
  element: 40
  element: 50
  element: 60
4. Vector iteration with continue (skip 20) and break (at 50):
  item: 10
  item: 30
  item: 40
5. Vector mutation after iteration completes (new length): 8
6. Second iteration over updated vector:
  sum of all elements: 360
=== Milestone 13 Lifetimes and Iteration Verification Complete ===
```

---

## 7. Compliance Checklist

- [x] References have compile-time validity and provenance tracking.
- [x] References cannot outlive the storage they reference.
- [x] References cannot escape local stack frames.
- [x] Function input and returned reference lifetimes are validated.
- [x] Immutable borrowed iteration over `Vec<T>` supported via non-allocating `Iter<'a, T>`.
- [x] Vector mutation and reallocation are rejected while borrows/iterators are active.
- [x] Zero-cost static dispatch and native machine code generation preserved.
- [x] Zero LLVM / Cranelift dependencies introduced.
- [x] Canonical source formatting verified in `vantac fmt`.
- [x] Machine-readable diagnostics with clear codes (`E0310`-`E0316`).
- [x] Verified in WSL2 Linux with differential `-O0`/`-O1` tests.
