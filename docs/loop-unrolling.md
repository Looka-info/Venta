# Loop Unrolling Optimization in Vanta

**Pass Module:** [`crates/vanta_mir/src/opt/loop_unroll.rs`](file:///crates/vanta_mir/src/opt/loop_unroll.rs)  
**Flag:** `--no-loop-unroll` (alias: `--no-unroll`)  
**Introduced in:** Milestone 23

---

## 1. Overview and Objectives

Loop unrolling is a classic loop transformation that replicates the operations of a loop body across multiple iterations. By duplicating loop bodies and eliminating or reducing branch overhead, loop unrolling achieves two primary goals:
1. **Loop Control Overhead Reduction:** Eliminates test-and-branch instructions, reduces pipeline stalls, and increases basic-block length.
2. **Cross-Iteration Optimization Exposure:** Exposes straight-line computation chains across adjacent iterations to downstream optimizations, including Constant Propagation, Global Value Numbering (GVN), Dead Code Elimination (DCE), and Instruction Scheduling.

In Vanta's custom compiler pipeline (with its custom SSA MIR, linear-scan physical register allocator, and ELF64 machine code emitter), Loop Unrolling operates strictly on the SSA representation while preserving all System V AMD64 ABI invariants.

---

## 2. Architecture & Eligibility

Loop unrolling targets natural loops identified by [`NaturalLoop::discover_loops`](file:///crates/vanta_mir/src/opt/licm.rs) using dominator trees.

### 2.1 Eligibility Invariants
To ensure sound SSA construction and prevent code bloat or register spill explosion, a candidate loop must satisfy:
1. **Single Latch and Canonical Form:** The loop must possess exactly one backedge latch block targeting the header.
2. **Call Barrier:** Loops containing `Instruction::Call` are rejected to prevent excessive stack frame allocation and register spill pressure.
3. **Canonical Header Branch:** The header terminator must be a `CondBranch { cond, then_block, else_block }` where `then_block` is inside the loop and `else_block` is the single exit block outside the loop.
4. **Induction Variable & Bound Provability:** The loop condition must be evaluated against a detectable basic induction variable ($IV$) with a constant initial value ($IV_0$), constant latch step ($S > 0$), and constant upper limit ($L$).

The trip count $N$ is provably computed via:
$$N = \begin{cases} \frac{L - IV_0 + S - 1}{S} & \text{for } IV < L \\ \frac{L - IV_0}{S} + 1 & \text{for } IV \le L \\ 0 & \text{for } L < IV_0 \end{cases}$$

---

## 3. Transformation Strategies

Vanta implements two distinct, bounded unrolling strategies:

### 3.1 Full Unrolling ($N \le 16$, Total Instructions $\le 256$)
For loops with small, constant trip counts:
- The entire loop is unrolled $N$ times.
- Loop body blocks are duplicated for each iteration $0 \le k < N$, remapping internal definitions to fresh SSA `ValueId`s in topological block order.
- The preheader branches directly to iteration 0, each iteration's latch branches to the subsequent iteration's entry, and the final iteration's latch branches directly to the loop exit block.
- **Zero-Trip Loop Elimination ($N = 0$):** If the loop condition is statically false on entry ($N = 0$), the preheader unconditionally branches to the exit block, eliminating all loop body blocks.
- **Loop Elimination:** The original loop header and body blocks are removed from `MirFunction::blocks`. All header `Phi` nodes used outside the loop are mapped to their final iteration values (`curr_phi_vals`).

### 3.2 Bounded Partial Unrolling (Factor $K = 2$)
For loops where $N > 16$ and $N$ is an exact multiple of 2 (with body instruction count $\le 120$):
- The loop body is duplicated once ($K = 2$).
- The first copy executes odd/even steps and chains directly into the second copy.
- The second copy's latch updates the induction variables and branches back to the loop header.
- The loop trip count is effectively halved, cutting the branch-and-decrement penalty by 50% without residual loop epilogues.

---

## 4. SSA Soundness and Topological Invariants

A critical invariant discovered during compiler verification is that basic blocks within `MirFunction::blocks` must remain in **topological order**. Arbitrary `HashSet` iteration during loop unrolling can invert dominating definitions and dominated uses (e.g. `while.body` defining an array address before `bounds_ok` performs `IndexPtrAddr`).

To guarantee SSA correctness:
```rust
let body_block_ids: Vec<BlockId> = func.blocks.iter()
    .map(|b| b.id)
    .filter(|b_id| *b_id != loop_info.header && loop_info.blocks.contains(b_id))
    .collect();
```
All cloned basic blocks maintain the function's topological order, ensuring every value is defined before its linear or dominated use.

---

## 5. CLI Controls & Statistics

The pass can be independently toggled:
```bash
vantac -O1 --no-unroll main.va -o main
```

Pass statistics are tracked via `LoopUnrollStats`:
- `loops_analyzed`: Total candidate loops inspected.
- `full_unrolls`: Number of loops completely eliminated via full unrolling.
- `partial_unrolls`: Number of loops partially unrolled by factor 2.
- `loops_rejected`: Candidate loops rejected due to unbounded trips, calls, or size thresholds.
