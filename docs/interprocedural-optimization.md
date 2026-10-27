# Vanta Interprocedural Optimization (IPA)

## Overview

Milestone 20 introduces **Call Graph Analysis** (`crates/vanta_mir/src/opt/call_graph.rs`) and **Interprocedural Constant Propagation** (`crates/vanta_mir/src/opt/ipa_constprop.rs`) for Vanta MIR.

Interprocedural analysis extends the compiler's reasoning across function boundaries without requiring full function inlining. By analyzing caller-callee call graphs, recursion cycles, and function purity, Vanta can evaluate direct call sites with statically known scalar arguments at compile time.

---

## 1. Direct Call Graph Analysis

The `CallGraph` tracks direct invocation relationships across all functions in a `MirProgram`:
- **Function Summaries**:
  - `instruction_count`: Total instruction count across basic blocks.
  - `block_count`: Basic block count.
  - `is_leaf`: `true` if the function invokes no user functions.
  - `is_recursive`: `true` if the function participates in self-recursion or a mutual recursion cycle of any length.
  - `has_memory_return` & `has_aggregate_types`: `true` if parameter or return types involve structs, enums, arrays, slices, or strings.
- **Cycle Detection**:
  - `CallGraph::detect_cycle` traverses the callee graph using depth-first search to identify direct recursion ($A \to A$) and mutual recursion cycles ($A \to B \to A$, $A \to B \to C \to A$).
  - Functions flagged as recursive are excluded from inlining to prevent infinite expansion.

---

## 2. Interprocedural Constant Propagation (IPA-ConstProp)

`IpaConstProp` inspects direct call sites in caller functions where all argument expressions resolve to known compile-time scalar constants (`ConstInt`, `ConstBool`).

### Purity and Safety Rules
A callee is eligible for symbolic evaluation if:
1. **Purity**: Callee contains no I/O, no external runtime invocations (`vanta_panic`, `vanta_vec_*`), and no indirect heap memory stores.
2. **Instruction Budget**: Callee size is $\le 50$ instructions (`MAX_EVAL_CALLEE_SIZE`).
3. **Scalar Values**: Callee returns a scalar `i64` or `bool`.
4. **All Arguments Constant**: Every parameter value is proven constant in caller dataflow.

### Symbolic Evaluation Engine
When an eligible call site is identified:
1. The callee's MIR representation is cloned.
2. Constant values are assigned to parameter slots at the entry block.
3. `Mem2Reg` promotes parameter stores into SSA values.
4. `ConstantPropagation` and `DeadCodeElimination` iterate over the cloned function.
5. If the callee reduces to a single basic block returning an immediate constant:
   - The caller's `Instruction::Call` is replaced in-place with `Instruction::ConstInt` or `Instruction::ConstBool`.
   - The callee body is **not** duplicated or inlined into the caller, keeping code size minimal.

---

## Pipeline Ordering and Synergy

```
                      +-----------------------------+
                      |   Direct Call Graph Build   |
                      +-----------------------------+
                                     |
                                     v
                      +-----------------------------+
                      |       IPA-ConstProp         |
                      |  (Folds pure constant calls)|
                      +-----------------------------+
                                     |
                                     v
                      +-----------------------------+
                      |          Inliner            |
                      | (Splices eligible callees)  |
                      +-----------------------------+
                                     |
                                     v
                      +-----------------------------+
                      | Intraprocedural Fixed-Point |
                      |    (Mem2Reg + GVN + DCE)    |
                      +-----------------------------+
```

1. **IPA-ConstProp runs first**: Evaluates calls with constant arguments directly to constants. This avoids unnecessary inlining of pure calculations.
2. **Inliner runs second**: Splices remaining call sites that cannot be evaluated symbolically but benefit from caller context.
3. **Intraprocedural Fixed-Point Loop**: Optimizes inlined blocks alongside caller code.

---

## CLI Flags

- `--no-ipa-constprop`: Disables the IPA constant propagation pass across `vantac`, `vanta run`, and `vanta compile`.
- Example:
  ```bash
  vantac -O1 --no-ipa-constprop program.va -o program
  ```
