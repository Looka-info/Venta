# Loop Peeling Optimization in Vanta

**Pass Module:** [`crates/vanta_mir/src/opt/loop_peel.rs`](file:///crates/vanta_mir/src/opt/loop_peel.rs)  
**Flag:** `--no-loop-peel` (alias: `--no-peel`)  
**Introduced in:** Milestone 23

---

## 1. Overview and Objectives

Loop peeling separates the initial iteration ($k = 1$) of an eligible natural loop from the remaining iterations, placing the peeled body directly prior to the steady-state loop header.

Loop peeling addresses three key optimization goals:
1. **Constant Propagation from Initial Invariants:** Loop induction variables and accumulators typically have known constant initial values ($i = 0, \text{acc} = 0$). In the peeled first iteration, these constants propagate through the loop body, folding operations and eliminating dead branches.
2. **Strength Reduction & Invariant Exposure:** Special-case checks on the first iteration (such as boundary handling or initialization logic) are resolved outside the loop.
3. **Steady-State Loop Simplification:** The remaining loop executes from iteration 1 onwards with warm caches and simplified invariant conditions.

---

## 2. Eligibility & Safety Constraints

Loop peeling mutates the control-flow graph (CFG) by introducing an alternate entry path to the loop header and an alternate path to the loop exit. To guarantee semantic safety:

1. **Single Latch and Single Exit:** The loop must possess exactly one backedge latch and exactly one exit edge leaving the loop.
2. **Purity of Header Instructions:** All non-Phi instructions in the header must be pure and side-effect free (`Load`, `Binary`, `Unary`, `ConstInt`, `SlotAddr`, `Copy`). No `Call`, `Store`, or memory mutation may occur in the header prior to the branch decision.
3. **Purity of Loop Body:** Loops containing `Call`, `Return`, or `Unreachable` are rejected to avoid code bloat and preserve call-barrier invariants.
4. **Instruction Count Threshold:** Only loops whose bodies contain $\le 40$ instructions are peeled to prevent excessive code expansion.

---

## 3. Critical Edge Splitting Architecture

A fundamental challenge in SSA loop peeling arises at the loop exit block:
- Control may reach the exit block either because the loop executed 0 iterations (the zero-trip path from the preheader), OR
- Because the loop executed $\ge 1$ iterations and terminated normally at the loop header.

### 3.1 The Critical Edge Problem
In Vanta's custom x86-64 backend, blocks with SSA `Phi` nodes receive their incoming parallel transfers during the processing of unconditional `Terminator::Branch` instructions. If a conditional branch (`CondBranch`) targets a block with `Phi` nodes directly, parallel copy registers cannot be safely staged without clobbering condition registers or conflicting with alternate branch targets.

### 3.2 Trampoline Splitting Solution
To eliminate critical edges, Vanta's Loop Peeling pass inserts two dedicated trampoline basic blocks:
```
           [ Preheader ]
             /        \
 (if true)  /          \  (if false: zero trips)
           v            v
    [ Peeled Body ]  [ zero_trip_trampoline ]
           |                    |
           v                    | (Branch)
     [ Loop Header ] <----+     |
      /           \       |     |
(exit)\            \ (body)     |
       v            v     |     |
[ loop_exit_trampoline ] [ Body ] -+
       |                        |
       | (Branch)               |
       +---------> [ Exit Block ] <----+
```
1. `zero_trip_trampoline`: An empty block terminating with `Terminator::Branch(exit_block_id)`.
2. `loop_exit_trampoline`: An empty block terminating with `Terminator::Branch(exit_block_id)`.

Both edges enter `exit_block_id` via clean, unconditional `Branch` terminators. In `exit_block_id`, SSA `Phi` nodes are inserted for all loop-carried variables:
$$\text{exit\_phi} = \phi\left( (\text{zero\_trip\_trampoline}, \text{init\_val}), (\text{loop\_exit\_trampoline}, \text{phi\_dest}) \right)$$

This guarantees:
- **Zero-trip execution** preserves initial values without executing the peeled body or reading uninitialized loop registers.
- **Normal exit execution** receives the final loop output with 100% SSA and physical register allocation fidelity.

---

## 4. CLI Controls & Statistics

The pass can be disabled independently via:
```bash
vantac -O1 --no-peel main.va -o main
```

Pass statistics are tracked via `LoopPeelStats`:
- `loops_analyzed`: Total candidate loops evaluated.
- `loops_peeled`: Loops successfully peeled and trampolined.
- `candidates_rejected`: Loops rejected due to calls, multiple exits, or size limits.
