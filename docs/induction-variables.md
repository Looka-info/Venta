# Induction Variable Optimization (IVO) in Vanta

## Overview

Induction Variable Optimization (IVO) detects and optimizes loop-carried recurrence variables in Vanta's SSA-based Mid-level Intermediate Representation (MIR).

In loop-intensive numerical and systems code, multiple variables often advance monotonically in step with a loop counter (e.g. `i = i + 1`, `j = j + 1`, `k = i * 4`). Identifying these patterns enables:
1. **Basic Induction Variable (BIV) Detection:** Identification of canonical loop counters with known initial values, loop headers, step amounts, and latch updates.
2. **Derived Induction Variable (DIV) Detection:** Identification of expressions linear in a BIV: $d = BIV \times S + C$.
3. **Redundant Induction Variable Elimination:** Replacing secondary induction variables advancing with identical initial values and steps by a single canonical IV.
4. **Dead Induction Variable Removal:** Eliminating induction counters and their latch increments when their values are never consumed outside or inside the loop body.

---

## Architecture & Algorithm

IVO is implemented in [`crates/vanta_mir/src/opt/induction_var.rs`](file:///crates/vanta_mir/src/opt/induction_var.rs) as an independent intraprocedural optimization pass.

### 1. Loop Identification
IVO leverages natural loop detection from [`crates/vanta_mir/src/opt/licm.rs`](file:///crates/vanta_mir/src/opt/licm.rs). For every loop in the control-flow graph (CFG), IVO discovers:
- Dedicated preheader block (created or verified).
- Header block containing merge `Phi` instructions.
- Latch block(s) with back-edges to the header.

### 2. Basic Induction Variable (BIV) Discovery
A basic induction variable is an SSA value defined by a `Phi` in the loop header:
$$\text{header\_phi} = \phi(\text{preheader} \to B_0, \text{latch} \to V_{\text{latch}})$$
where $V_{\text{latch}}$ is defined in a loop block as:
$$V_{\text{latch}} = \text{header\_phi} \pm \text{step}$$
where $\text{step}$ is invariant to the loop (defined outside the loop or a constant integer).

IVO records:
- `header_phi`: The SSA ValueId in the header.
- `init_val`: The incoming initial value from the preheader.
- `step_val`: The stride per iteration.
- `is_add`: Direction of the step (addition or subtraction).
- `latch_val`: The updated value fed back via the latch edge.

### 3. Redundant IV Elimination
When two BIVs $IV_1$ and $IV_2$ share:
- Equal initial values ($B_0^{(1)} = B_0^{(2)}$)
- Equal steps ($S^{(1)} = S^{(2)}$)
- Same direction ($\text{add/sub}$)

$IV_2$ is identical to $IV_1$ across all iterations. IVO rewrites all uses of $IV_2$ and its latch update to use $IV_1$, and subsequent Dead Code Elimination removes the redundant phi and update instructions.

### 4. Dead IV Elimination
If an IV's header phi and latch update are only used by each other (and not by any condition, computation, call, store, or return), IVO safely strips the latch update, allowing dead-code elimination to clean up the loop header phi.

---

## Correctness & Semantics Preservation

- **Integer Overflow:** Vanta semantics specify two's-complement wrapping on signed 64-bit integer overflow. All IV linear transformations preserve exact 64-bit modular arithmetic.
- **Back-edge Safety:** Only single-latch or normalized multi-latch loops with verified preheaders are transformed.
- **Isolation:** IVO operates strictly on SSA MIR before register allocation. It can be independently disabled via the `--no-indvar` CLI flag.

---

## Pass Statistics

The pass tracks fine-grained metrics via `InductionVarStats`:
- `bivs_found`: Total basic induction variables identified.
- `divs_found`: Total derived induction variables identified.
- `ivs_eliminated`: Redundant or dead induction variables removed.
