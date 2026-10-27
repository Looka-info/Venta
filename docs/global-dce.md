# Global Dead Code Elimination (Global DCE) in Vanta

**Pass Module:** [`crates/vanta_mir/src/opt/global_dce.rs`](file:///crates/vanta_mir/src/opt/global_dce.rs)  
**CLI Switch:** `--no-global-dce`  
**Introduced in:** Milestone 24

---

## 1. Overview and Objectives

Global Dead Code Elimination (Global DCE) is an optimization pass designed to identify and eliminate unused code at three hierarchical levels in Vanta's custom compiler pipeline:

1. **Program-Level Function Elimination:** Prunes whole functions that are provably unreachable from the program's root set in executable builds.
2. **Function-Level Basic-Block Pruning:** Identifies and deletes unreachable basic blocks from the Control Flow Graph (CFG) and cleans up dangling `Phi` node incoming edges.
3. **Instruction-Level Dead Code Elimination:** Performs iterative fixed-point removal of pure computations whose defined SSA values are never used by any surviving instruction or terminator.

Unlike local dead-code passes that inspect isolated basic blocks, Global DCE operates globally across the entire MIR module and its call graph while strictly adhering to Vanta's memory safety, panic, and System V AMD64 ABI invariants.

---

## 2. Program-Level Reachability and Root Selection

### 2.1 Root Identification
When compiling a standalone executable (`emit_target == EmitTarget::Exe` and `has_main == true`), the root set of reachable functions comprises:
- The entry function `main`.
- Runtime constructor and initialization routines (`vanta_rt_ctor`, `vanta_rt_init`).
- Foreign export hooks prefixed with `__`.

### 2.2 Call Graph Traversal
The pass computes a directed call graph where edges $(F_A \to F_B)$ represent direct `Instruction::Call { callee: F_B, .. }` operations inside $F_A$. A breadth-first search (BFS) starting from the root set marks all reachable functions:

$$\text{Reachable} = \bigcup_{r \in \text{Roots}} \text{Descendants}(r)$$

Any function $F \notin \text{Reachable}$ is removed from `MirProgram::functions`. In library or test modes (`is_executable == false`), function pruning is automatically disabled so that exported unit tests and public module interfaces remain intact.

---

## 3. CFG Pruning and Phi Edge Simplification

Within each surviving function, Global DCE prunes basic blocks unreachable from the entry block (`BlockId(0)`):

1. **Reachability Traversal:** Beginning at entry block 0, the pass traverses successor edges derived from block terminators (`Branch`, `CondBranch`).
2. **Unreachable Block Removal:** Blocks not visited during traversal are removed from `MirFunction::blocks`.
3. **Phi Edge Maintenance:** For any surviving block containing `Instruction::Phi { inputs, .. }`, incoming edges originating from removed predecessor blocks are pruned:
   $$\text{inputs}' = \{ (b, v) \in \text{inputs} \mid b \in \text{ReachableBlocks} \}$$
4. **Phi Simplification:** If a `Phi` node is reduced to a single surviving incoming edge $(b_0, v_0)$, it is simplified into a direct SSA assignment:
   $$\text{Copy } \{ \text{dest}, \text{src}: v_0 \}$$

---

## 4. Fixed-Point Pure Instruction Elimination

After CFG pruning, Global DCE eliminates unused pure instructions within surviving blocks using fixed-point iteration:

### 4.1 Effect and Purity Classification
An instruction is considered **pure and eligible for elimination** only if:
- It defines a non-empty SSA `ValueId`.
- It performs no observable side-effects:
  - **No Memory Writes:** Excludes `Store`, `StoreIndirect`, `StoreIndirect32`, and `StoreSlice`.
  - **No Function Invocations:** Excludes all `Instruction::Call` operations (which may have I/O, allocation, or synchronization side-effects).
  - **No Arithmetic Traps:** Excludes division and remainder (`MirBinaryOp::Div`, `MirBinaryOp::Rem`), preserving runtime division-by-zero panics.

### 4.2 Fixed-Point Iteration
The pass gathers all used `ValueId`s across all surviving instructions and terminators. If a pure instruction's destination `dest` is not contained within the used set, the instruction is deleted. This process repeats until a fixed-point is reached:
$$\Delta \text{Instructions} = 0$$

---

## 5. Verification and Statistics

The pass collects comprehensive optimization statistics via `GlobalDceStats`:
- `functions_eliminated`: Count of dead functions pruned from `MirProgram`.
- `blocks_eliminated`: Count of unreachable CFG blocks removed.
- `instructions_eliminated`: Total pure dead instructions eliminated.

Verification is enforced by `MirVerifier::verify_program` immediately after the pass executes, ensuring valid SSA dominance and well-formed CFGs.
