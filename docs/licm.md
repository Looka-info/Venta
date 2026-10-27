# Loop Invariant Code Motion (LICM) Architecture in Vanta

## 1. Overview and Design Objectives

Loop Invariant Code Motion (LICM) is a foundational compiler optimization pass that identifies computations inside natural loops whose evaluated results remain identical across all iterations of the loop, and safely hoists them into a newly synthesized or reused preheader block dominating the loop header.

In Vanta's custom native compilation pipeline (SSA-based MIR, Mem2Reg, GVN/CSE, physical register allocation, and x86-64 machine-code backend), LICM serves three critical architectural goals:
1. **Reduces Dynamic Loop Latency:** Eliminates redundant arithmetic and memory operations from repeatedly executing within hot loop bodies.
2. **Exposes Global Value Numbering Opportunities:** Hoisting invariant computations into preheaders clusters them with outer scope definitions, enabling Global Value Numbering (GVN) and Common Subexpression Elimination (CSE) to fold cross-loop redundancies.
3. **Preserves Register Allocation Budgets:** Trivial constants (immediates) and pure values are carefully filtered to prevent excessive register pressure across long loop live ranges that could induce stack spilling.

---

## 2. Natural Loop Discovery via Dominance Analysis

A loop is defined as a natural loop if and only if:
1. It possesses a single entry point (the *header* block $H$) that dominates every block in the loop.
2. There exists at least one *backedge* $(L, H)$ where block $L$ is a predecessor of $H$ and $H$ dominates $L$ ($H \ \text{dom} \ L$).

### Backedge Detection
Vanta utilizes its existing dominator tree analysis (`vanta_mir::opt::DominatorTree`) to identify back edges:
```rust
for block in &func.blocks {
    for &succ_id in &block.terminator.successors() {
        if dom_tree.dominates(succ_id, block.id) {
            // Found backedge: block.id (latch L) -> succ_id (header H)
        }
    }
}
```

### Loop Body Construction
Given a header $H$ and latch $L$, the natural loop body is discovered via reverse depth-first traversal of predecessors from $L$, stopping at $H$:
```rust
let mut loop_blocks = HashSet::new();
loop_blocks.insert(header);
let mut stack = vec![latch];

while let Some(block_id) = stack.pop() {
    if loop_blocks.insert(block_id) {
        if let Some(preds) = cfg_preds.get(&block_id) {
            for &p in preds {
                if !loop_blocks.contains(&p) {
                    stack.push(p);
                }
            }
        }
    }
}
```

### Innermost-First Processing
To support nested loops correctly, discovered loops are sorted in ascending order of block count:
```rust
loops.sort_by_key(|l| l.blocks.len());
```
Innermost loops are analyzed and hoisted first. When an inner loop invariant is hoisted to its preheader, that preheader may reside within an enclosing outer loop. In subsequent rounds or outer passes, the hoisted instruction is naturally recognized as an outer loop candidate if its operands are also invariant to the outer loop.

---

## 3. Loop Preheader Synthesis and CFG Rewriting

For an invariant instruction to dominate the loop body without altering control-flow semantics, it must be placed in a dedicated *preheader* block $P$ that:
1. Dominates the loop header $H$.
2. Is entered exclusively from the external predecessors of $H$.
3. Has $H$ as its sole successor.

### Preheader Insertion Algorithm
1. **Reuse vs Creation:** If $H$ already has exactly one predecessor $P_{ext}$ that is outside the loop, and $P_{ext}$ unconditionally jumps to $H$ with no other successors, $P_{ext}$ may serve directly as the preheader. Otherwise, a synthetic preheader block $P$ is instantiated.
2. **External Predecessor Redirection:** For every predecessor $P_i$ of $H$ that is outside the loop, its terminator target $H$ is rewritten to $P$:
   - `Terminator::Jump { target: H }` $\to$ `Terminator::Jump { target: P }`
   - `Terminator::Branch { cond, then_target, else_target }` $\to$ branches targeting $H$ are remapped to $P$.
3. **Preheader Insertion Order:** In Vanta's linear machine-code emitter, basic blocks are emitted in definition order. To guarantee that SSA value definitions hoisted to $P$ dominate uses in $H$ and the loop body under linear verification, $P$ is inserted in `func.blocks` immediately preceding $H$:
   ```rust
   let header_idx = func.blocks.iter().position(|b| b.id == header_id).unwrap();
   func.blocks.insert(header_idx, preheader_block);
   ```
4. **Header Phi Node Rewriting:** If $H$ contains SSA Phi instructions, their input tuples `(pred_block, value)` are classified into internal loop inputs and external inputs:
   - If there is exactly 1 external input `(P_ext, val)`, it is rewritten to `(P, val)`.
   - If there are multiple external inputs, a new Phi instruction is synthesized in $P$, merging those external inputs into a single SSA value, which is then passed from $P$ into $H$'s Phi.

---

## 4. Speculation Safety and Invariance Analysis

Hoisting an instruction speculatively out of a loop that might never execute (e.g. zero-trip loop `while 0 > 1`) must never introduce undefined behavior or observable crashes into a valid program.

### Instruction Classification Rules

| Instruction Kind | Safe to Hoist? | Architectural Reason |
|:---|:---:|:---|
| **ALU Operations** (`Add`, `Sub`, `Mul`, `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr`) | **YES** | Pure arithmetic; does not trap or mutate state on x86-64. |
| **Comparisons** (`Eq`, `NotEq`, `Lt`, `LtEq`, `Gt`, `GtEq`) | **YES** | Pure comparison; outputs boolean flag; zero side effects. |
| **Unary Ops** (`Neg`, `Not`) | **YES** | Pure bitwise / numeric inversion. |
| **SSA Copies** (`Copy`) | **YES** | Register transfer / alias simplification. |
| **Integer Division / Modulo** (`Div`, `Rem`) | **NO (STRICT)** | May trigger hardware CPU Divide Error (`#DE`, `SIGFPE`) if divisor is zero. Hoisting out of guarded or zero-trip loops introduces crashes. |
| **Function Calls** (`Call`, `CallIndirect`) | **NO** | May have arbitrary side effects, I/O, panic, or non-termination. |
| **Memory Stores** (`Store`) | **NO** | Mutates heap/stack state across iterations. |
| **Pointer Dereferences** (`Load` on arbitrary addresses) | **NO** | Memory address could be NULL, dangling, or unmapped. |
| **Read-Only Unescaped Slot Loads** | **YES** | Parameter or local stack slot whose address is never taken (`SlotAddr`/`IndexAddr` absent) and which is never stored to inside the loop. Speculation is 100% memory-safe. |

### Invariance Discovery Fixed-Point
LICM discovers invariants using an iterative worklist:
1. All values defined outside the loop are seeded as invariant.
2. In each iteration, an instruction $I$ inside the loop is marked invariant if:
   - It satisfies `is_speculation_safe_instruction(I)`.
   - Every operand $v \in \text{operands}(I)$ is already known to be invariant.
3. Steps repeat until no further invariant values can be added (fixed-point reached).

---

## 5. Register Pressure & Constant Pruning

A major pitfall of naive LICM in non-LLVM native backends is **hoisting trivial literal constants (`ConstInt`, `ConstBool`)**.

### The Register Starvation Problem
In x86-64, small integer literals can be embedded directly as immediate operands (e.g. `add rbx, 5` or `cmp rax, 20000000`) or materialized with a 1-cycle `mov reg, imm`. Materializing them inside the loop has near-zero latency.

When a bare constant is hoisted into a preheader:
- A new SSA `ValueId` is created before the loop.
- The live range of that value spans the **entire duration of the loop**.
- It monopolizes a physical register throughout all iterations.
- In tight loops with multiple accumulators (e.g. `loop_accum`), holding constants in registers exhausts the physical register pool and forces the linear-scan allocator to spill hot accumulators into stack memory.

### Constant Pruning Algorithm
To prevent register starvation while still enabling compound invariant arithmetic to be hoisted, Vanta enforces a **two-phase prune**:
1. Discover all candidate invariant instructions.
2. Identify all non-constant hoisted computations (e.g. `Mul`, `Add`, `Load`).
3. Retain a `ConstInt` / `ConstBool` if and only if it is required as an operand to a hoisted non-constant computation.
4. Discard standalone constants whose only consumers remain inside the loop. Those constants stay inside the loop where they are encoded as immediate operands or allocated temporary scratch registers without spilling hot loop variables.

---

## 6. Verification and Diagnostics

LICM can be controlled via the compiler CLI:
- Enabled by default under `-O1`.
- Disabled using `--no-licm`.

Verification is enforced across:
1. `crates/vanta_mir/src/verify.rs`: Validates SSA dominance, block connectivity, and Phi input counts after transformation.
2. `crates/vanta_driver/tests/licm_tests.rs`: Comprehensive integration test suite covering backedge discovery, division rejection, nested loop traversal, dependent invariant chains, boolean hoisting, and CLI flag disabling.
