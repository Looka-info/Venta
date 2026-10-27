# Milestone 12 Engineering Report: Iterator Protocols, `for` Loops, Range Iteration, and Collection Integration

---

## Executive Summary

Milestone 12 introduces a robust, zero-cost, and memory-safe iteration protocol to the Vanta systems programming language. Building upon Vanta's established statically dispatched trait system, generic monomorphization, `Option<T>` enum, and ownership/drop architecture, this milestone delivers:

1. **Statically Dispatched Iterator Protocol**: Standardized `pub trait Iterator<T>` in `std::iter`, using a generic element parameter and exclusive receiver borrowing (`fn next(self: &mut Self) -> Option<T>`).
2. **Canonical `for ... in ...` Syntax**: Fully integrated grammar keyword `in`, AST statement `Stmt::For`, single-evaluation semantics, and deterministic MIR control-flow lowering with comprehensive LIFO scope unwinding.
3. **Standard Integer Range Iterator**: `std::iter::range_i64(start, end)` producing a 16-byte, non-allocating, half-open `[start, end)` integer range iterator (`RangeI64`) with signed overflow safety and negative boundary handling.
4. **User-Defined Iterators**: Full compiler and runtime support for user structs implementing `Iterator<T>` with internal mutable state (demonstrated via countdowns, step ranges, and Fibonacci sequence generators).
5. **Rigorous Collection Integration Analysis**: Architectural soundness evaluation of `Vec<T>` iteration. Confirmed that both borrowed iteration (`&Vec<T>`) and consuming iteration (`IntoIter<T>`) must be soundly deferred until lifetime parameters and dynamic partial drop flags are implemented in the language; emitting machine-readable diagnostic `E0901_TYPE_NOT_ITERABLE`.
6. **Tooling & Canonical Formatting**: Formatter integration in `vanta_fmt`, machine-readable diagnostic codes `E0901` and `E0902`, an end-to-end repository example in `examples/15_iterators/main.va`, and 13 new integration and differential tests verifying identical execution under `-O0` and `-O1`.

---

## 1. Iterator Protocol & Trait System Integration

### 1.1 Trait Declaration and Method Contract
The iteration protocol is implemented using Vanta's existing statically dispatched trait mechanism in `library/std/iter.va`:

```vanta
import std::option::Option;

pub trait Iterator<T> {
    fn next(self: &mut Self) -> Option<T>;
}
```

- **Output Representation**: Each advancement step returns `Option::Some(val)` when an element is produced, or `Option::None` when iteration terminates.
- **Receiver Mutability**: `next()` requires an exclusive borrow `&mut Self`, allowing iterators to safely modify internal state (e.g. indices, pointers, counters) across advancements without aliasing conflicts.
- **Zero-Cost Abstraction**: Monomorphization produces specialized concrete function implementations (e.g., `RangeI64__Iterator_next`). There are no virtual method tables (vtables), dynamic dispatch (`dyn Trait`), or indirect call overhead.

### 1.2 Generic Parameterization on Traits and Impls
To support `Iterator<T>`, the compiler's lexer, parser, semantic analyzer, and formatter were extended to accommodate generic parameters on traits and generic arguments in trait references:
- **Parser**: Extended `TraitDecl` with `type_params: Vec<TypeParam>` and `ImplBlock` with `trait_args: Vec<TypeExpr>`.
- **Semantic Analysis**: Implemented generic trait implementation verification in Pass 2a of `vanta_sema`, mapping trait type parameters to implementation type arguments and validating method signatures.
- **Canonical Formatting**: Extended `vanta_fmt` bracket classification to treat `<...>` brackets following trait identifiers and implementation targets as generic brackets rather than comparison operators.

---

## 2. Canonical `for ... in ...` Syntax and Semantic Lowering

### 2.1 Lexical and Grammar Extensions
- Added `TokenKind::In` to `vanta_lexer` recognizing the reserved keyword `"in"`.
- Extended `vanta_parser` AST with:
  ```rust
  Stmt::For {
      var: Ident,
      iter: Expr,
      body: Block,
      span: Span,
  }
  ```
- Formal grammar updated in `docs/grammar.ebnf`:
  ```ebnf
  ForStmt = "for", Identifier, "in", Expression, Block ;
  ```

### 2.2 Semantic Validation
During semantic analysis (`vanta_sema`):
1. **Iterable Verification**: Inspects the type of the `iter` expression and searches registered `trait_impls` for an implementation of `Iterator<T>` on the target type.
2. **Yielded Type Resolution**: Extracts the concrete type parameter `T` associated with `Iterator<T>`.
3. **Diagnostic Reporting**: If the target type does not implement `Iterator<T>`, the compiler halts and emits:
   - `E0901_TYPE_NOT_ITERABLE`: Explaining that the type is not iterable.
4. **Scope and Shadowing**: Establishes a nested lexical scope for the loop body and declares the iteration variable with the inferred element type `T`.

### 2.3 MIR Lowering & Single Evaluation
In `vanta_mir::lower::lower_for_stmt`:
1. **Single Evaluation**: The iterable expression is lowered into a dedicated stack slot `_for_iter_<id>` exactly once before entering the loop.
2. **Loop Condition Block (`for_cond`)**:
   - Takes address `SlotAddr` of `_for_iter_<id>`.
   - Emits a static `Call` to `<IterType>__Iterator_next`.
   - Reads the 32-bit discriminant tag from the returned `Option<T>` place.
   - Emits `CondBranch`: if tag == `Some` (1) jump to `for_body`; else jump to `for_exit`.
3. **Loop Body Block (`for_body`)**:
   - Extracts the payload value from the `Option<T>` instance into a local slot for the iteration variable.
   - Lowers all inner statements.
   - Cleans up inner iteration drop candidates in LIFO order.
   - Emits an unconditional `Branch(for_cond)`.
4. **Loop Exit Block (`for_exit`)**:
   - Drops the iterator instance itself if non-`Copy`.
   - Resumes normal sequential control flow.

### 2.4 Control Flow Integration (`break` and `continue`)
The MIR lowerer maintains a stack of loop entry depths (`loop_scope_depths`). When lowering control flow:
- `break`: Identifies all drop candidates registered within lexical scopes deeper than the loop scope, emits drop instructions in LIFO order, and branches to `for_exit`.
- `continue`: Drops all drop candidates registered in the current iteration body in LIFO order and branches to `for_cond`.

---

## 3. Standard Integer Range Iterator

### 3.1 Implementation in `library/std/iter.va`
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

### 3.2 Design Verification
1. **Zero Heap Allocations**: `RangeI64` occupies exactly 16 bytes on the stack. No heap allocation, heap fragmentation, or dynamic deallocation is incurred.
2. **Half-Open Interval**: Represents `[start, end)`. Empty ranges (`start >= end`) perform zero iterations and return `None` immediately.
3. **Overflow Soundness**: Because advancement only occurs when `self.current < self.end`, the maximum value `self.current` can reach before advancement is `end - 1`. If `end <= i64::MAX`, `self.current + 1` never overflows signed `i64`.
4. **Boundary & Zero-Crossing Handling**: Correctly handles negative boundaries, zero crossings, and large values near `i64::MAX` (e.g. `range_i64(-3, 3)` yields `-3, -2, -1, 0, 1, 2`).

---

## 4. User-Defined Iterators

Vanta allows any user-defined struct to participate in `for` loops by implementing `Iterator<T>`.

### 4.1 Fibonacci Generator Example
```vanta
struct Fibonacci {
    curr: i64,
    next: i64,
    remaining: i64,
}

fn fibonacci(n: i64) -> Fibonacci {
    Fibonacci { curr: 0, next: 1, remaining: n }
}

impl Iterator<i64> for Fibonacci {
    fn next(self: &mut Fibonacci) -> Option<i64> {
        if self.remaining > 0 {
            let res = self.curr;
            let new_next = self.curr + self.next;
            self.curr = self.next;
            self.next = new_next;
            self.remaining = self.remaining - 1;
            Option::Some(res)
        } else {
            Option::None
        }
    }
}
```

### 4.2 StepRange and Countdown Examples
Verified custom iterators in `crates/vanta_driver/tests/iter_tests.rs`:
- `Countdown`: Decrements an internal counter towards zero.
- `StepRange`: Advances a counter by an arbitrary step parameter (`step: i64`).

---

## 5. Collection Integration & Soundness Analysis

A foundational objective of Milestone 12 was evaluating collection iteration over `Vec<T>`.

### 5.1 Analysis of Borrowed Iteration (`&Vec<T>`)
- **Safety Requirement**: Yielding `&T` from an iterator over `&Vec<T>` requires that the elements cannot be read after the backing vector is mutated, reallocated, or freed.
- **Current Limitation**: Vanta's type system currently enforces lexical and non-lexical lifetimes on direct borrows of local variables, but does not yet support named lifetime parameters on structs (`struct Iter<'a, T>`).
- **Sound Decision**: Creating an iterator struct holding `&Vec<T>` without lifetime parameter tracking would allow the underlying `Vec<T>` to be modified (e.g. `vec.push()`), causing memory reallocations and dangling pointer use-after-free bugs. Borrowed `Vec` iteration is therefore deferred until lifetime parameters are added.

### 5.2 Analysis of Consuming Iteration (`IntoIter<T>`)
- **Safety Requirement**: Consuming iteration transfers ownership of elements one by one. If iteration terminates early via `break`, `return`, or panic, the remaining unconsumed elements must be dropped, while already consumed elements must not be double-dropped.
- **Current Limitation**: Vanta's compiler-generated destruction currently applies to whole types on scope exit and does not maintain runtime per-element drop flags for dynamic buffers.
- **Sound Decision**: Providing `IntoIter<T>` without dynamic drop flags would cause memory leaks on early exit or double-free bugs. Consuming `Vec` iteration is therefore deferred until dynamic drop flags or partial drop glue are introduced.

### 5.3 Diagnostic Behavior
Attempting to iterate over `Vec<T>` or `&Vec<T>` emits the stable diagnostic `E0901_TYPE_NOT_ITERABLE`, transparently communicating that the type does not implement `Iterator<T>`.

---

## 6. Tooling, Diagnostics, and Formatting

### 6.1 Diagnostic Codes Catalog
| Code | Category | Description |
|:---|:---|:---|
| `E0901` | Iter | Type is not iterable; it does not implement `Iterator<T>` |
| `E0902` | Iter | Cannot infer iterator element type for `for` loop |

### 6.2 Canonical Formatter Integration (`vantac fmt`)
- Enhanced `crates/vanta_fmt/src/lib.rs` with token formatting rules for `TokenKind::In` and `TokenKind::For`.
- Generic angle brackets `<...>` in `trait Iterator<T>` and `impl Iterator<i64> for RangeI64` are tagged and formatted without internal spaces.
- Verified idempotent formatting across all repository source files and examples with `test_all_repo_example_files_idempotent`.

---

## 7. Verification and Testing Evidence

### 7.1 Integration and Differential Test Suite (`iter_tests.rs`)
The test suite `crates/vanta_driver/tests/iter_tests.rs` contains 13 end-to-end tests:
1. `test_range_basic_iteration`: Standard half-open range `range_i64(0, 5)` yielding `01234`.
2. `test_range_empty_cases`: `range_i64(5, 5)` and `range_i64(10, 2)` performing 0 iterations.
3. `test_range_negative_zero_crossing`: `range_i64(-3, 3)` yielding `-3-2-1012`.
4. `test_range_single_element`: `range_i64(42, 43)` yielding `42`.
5. `test_range_near_i64_max`: Boundary test near `i64::MAX` verifying signed arithmetic safety without overflow.
6. `test_range_nested_loops`: Nested `for` loops producing matrix coordinates.
7. `test_for_break_and_continue`: Loop control flow skipping odds and terminating at boundary.
8. `test_user_defined_iterator_countdown`: Custom `Countdown` iterator struct yielding `4321`.
9. `test_user_defined_iterator_step`: Custom `StepRange` iterator struct advancing with step 5.
10. `test_single_evaluation_of_iterable`: Confirms side effects in the iterable expression run exactly once.
11. `test_diagnostic_e0901_non_iterable_primitive`: Rejects `for x in 42` with `E0901`.
12. `test_diagnostic_e0901_vec_deferred`: Rejects `for x in v` with `E0901`.
13. `test_diagnostic_e0901_vec_ref_deferred`: Rejects `for x in &v` with `E0901`.

### 7.2 Differential Execution Verification
Every execution test in `iter_tests.rs` compiles and executes the program twice:
1. **Unoptimized Build (`-O0`)**: Direct emission from unoptimized MIR.
2. **Optimized Build (`-O1`)**: Passes through constant folding, dead-code elimination, and copy propagation.
- In all 13 test cases, `-O0` and `-O1` produced identical stdout, exit codes, and runtime invariants.

---

## 8. Acceptance Criteria Status

| # | Acceptance Criterion | Status | Evidence |
|:---|:---|:---|:---|
| 1 | Baseline verification & clean Git tree | PASS | Workspace tests green before any edits |
| 2 | `Iterator<T>` trait in `library/std/iter.va` | PASS | Defined in `library/std/iter.va`, exported in `library/std/mod.va` |
| 3 | Lexer token `TokenKind::In` and `for` integration | PASS | Added to `vanta_lexer`, verified in `test_lex_trait_and_for` |
| 4 | Parser AST `Stmt::For` and grammar update | PASS | Added to `vanta_parser`, verified in `test_parse_for_loop_and_generic_trait` |
| 5 | Single evaluation of iterable expression | PASS | Verified in `test_single_evaluation_of_iterable` |
| 6 | Semantic checking & type inference | PASS | Implemented in `vanta_sema`, emits `E0901` and `E0902` |
| 7 | MIR lowering to structured CFG with LIFO drops | PASS | Implemented in `vanta_mir::lower::lower_for_stmt` |
| 8 | `break` and `continue` drop handling | PASS | Verified in `test_for_break_and_continue` |
| 9 | Zero-allocation range iterator `range_i64` | PASS | Verified in `test_range_basic_iteration`, `test_range_near_i64_max` |
| 10 | User-defined iterators with mutable receiver | PASS | Verified in `test_user_defined_iterator_countdown`, `test_user_defined_iterator_step` |
| 11 | Sound collection integration evaluation | PASS | `Vec<T>` deferred soundly; verified in `test_diagnostic_e0901_vec_deferred` |
| 12 | Differential verification `-O0` vs `-O1` | PASS | All 13 integration tests verified under both `-O0` and `-O1` |
| 13 | Canonical formatter support (`vantac fmt`) | PASS | Verified in `vanta_fmt` unit tests and `test_all_repo_example_files_idempotent` |
| 14 | Repository example `examples/15_iterators/` | PASS | Authored, formatted, compiled, and executed to code 0 |
| 15 | Zero external backend dependencies | PASS | Preserved pure-Rust custom x86-64 backend & ELF64 writer (no LLVM/Cranelift) |

---

## 9. Conclusion

Milestone 12 establishes a clean, predictable, and memory-safe iteration system in Vanta. It empowers developers and AI coding agents to write expressive, idiomatic loops with zero runtime overhead, while maintaining strict adherence to systems programming performance principles and memory safety guarantees.
