# Vanta Conservative Function Inlining

## Overview

Milestone 20 introduces a size-aware, conservative **Function Inlining** optimization pass for Vanta's Intermediate Representation (`crates/vanta_mir/src/opt/inlining.rs`). 

Inlining eliminates direct call overhead (call/ret instructions, register spills across call boundaries, and prologue/epilogue stack frame operations) by splicing the callee body directly into the caller's control-flow graph (CFG). Once inlined, the callee's instructions become directly visible to Vanta's existing intraprocedural optimization pipeline:
- **Mem2Reg** promotes inlined parameter stores and local variables to SSA values.
- **Global Value Numbering (GVN) & Common Subexpression Elimination (CSE)** reuse caller values and eliminate redundant computations across the caller-callee boundary.
- **Constant Propagation** propagates caller constant arguments into the inlined callee expressions.
- **Dead Code Elimination (DCE)** removes unused calculations and pruned branch paths.

---

## Architecture & Algorithm

### 1. Direct Call Candidate Selection & Growth Budgeting

The inliner operates under strict size and safety budgets:
- **Leaf Function Budget**: Max 60 instructions for leaf functions (functions that make no subsequent calls).
- **Non-Leaf Function Budget**: Max 35 instructions for non-leaf functions.
- **Caller Growth Cap**: Max 250 additional instructions per caller function (`MAX_CALLER_GROWTH`).
- **Inline Depth / Rounds**: At most 4 inlining rounds per function (`MAX_INLINE_ROUNDS`).
- **Mutual and Self-Recursion Rejection**: Functions in call cycles are identified by `CallGraph::detect_cycle` and strictly rejected.
- **Aggregate and Memory-Class Rejection**: Functions accepting or returning aggregates (`Type::Struct`, `Type::Enum`, `Type::Array`, `Type::Slice`, `Type::Str`) are strictly excluded to preserve System V AMD64 ABI hidden return pointer (`sret`) conventions and memory lifetimes.

### 2. CFG Splice and Block Splitting

For each eligible call site at instruction index $k$ in block $B_{\text{call}}$:
1. **Identifier Remapping**:
   - Callee local slots are cloned into caller local slots with unique names (`format!("{}_inlined_{}", callee.name, slot.name)`).
   - Callee `ValueId`s are remapped to fresh IDs guaranteed to exceed `caller.max_value_id()`.
   - Callee `BlockId`s are remapped to fresh IDs guaranteed to exceed `caller.max_block_id()`.
2. **Block Splitting**:
   - $B_{\text{call}}$ is split into:
     - $B_{\text{before}}$ (retaining $B_{\text{call}}$'s original ID and instructions $0 \dots k-1$).
     - $B_{\text{continuation}}$ (a new block holding instructions $k+1 \dots N$ and original terminator).
   - $B_{\text{before}}$ appends `Store` instructions storing each call argument `arg_val` into the cloned callee parameter slot.
   - $B_{\text{before}}$'s terminator becomes `Branch(callee_entry_id)`.
3. **Callee Return Redirect & Value Joining**:
   - Every `Return(opt_val)` terminator in the cloned callee is replaced with `Branch(continuation_block_id)`.
   - If the call produces a result (`dest: Some(dest_val)`):
     - Single return path: an `Instruction::Copy { dest: dest_val, src: ret_val, ty }` is placed at the entry of $B_{\text{continuation}}$.
     - Multiple return paths: an SSA `Instruction::Phi { dest: dest_val, inputs: return_entries, ty }` is placed at the entry of $B_{\text{continuation}}$.
4. **Contiguous Block Placement**:
   - Cloned callee blocks and $B_{\text{continuation}}$ are spliced immediately following $B_{\text{before}}$ using `caller.blocks.splice(insert_idx..insert_idx, splice_blocks)`.
   - This ensures strict topological ordering, preventing use-before-definition errors in linear passes and verification.
5. **Successor Phi Update**:
   - Any Phi node in caller successor blocks that originally referenced $B_{\text{call}}$ is updated to reference $B_{\text{continuation}}$.

---

## Compiler Pipeline Integration

Inlining runs as part of `OptimizationPipeline::optimize_program_with_options`:
1. **Pre-IPA / Pre-Inlining Mem2Reg & DCE**: Cleans up early dead code and promotes straightforward locals.
2. **Interprocedural Constant Propagation (IPA-ConstProp)**: Evaluates pure callees symbolically with constant arguments and folds calls to immediate constants without inlining.
3. **Inliner**: Inlines eligible direct call sites within size and recursion budgets.
4. **Intraprocedural Fixed-Point Optimization Loop**:
   - `ConstantPropagation`
   - `Mem2Reg` (promotes parameter stores and inlined slots to SSA)
   - `Gvn` (CSE and value numbering across inlined boundaries)
   - `DeadCodeElimination`

---

## CLI Flags

- `--no-inline`: Disables the function inliner pass across `vantac`, `vanta run`, and `vanta compile`.
- Example:
  ```bash
  vantac -O1 --no-inline program.va -o program
  ```
