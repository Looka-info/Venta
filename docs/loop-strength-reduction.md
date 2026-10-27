# Loop Strength Reduction (LSR) in Vanta

## Overview

Loop Strength Reduction (LSR) replaces computationally expensive operations inside loops—predominantly integer multiplications by induction variables—with cheaper operations, specifically iterative additions.

In unoptimized code, calculating linear array offsets or scaled index expressions (e.g. `i * 9` or `i * stride`) requires a hardware multiplication instruction (`imul`) in every iteration of the loop. On modern x86-64 processors, `imul` incurs higher latency (3–4 cycles) and limited port execution throughput compared to simple single-cycle `add` instructions.

LSR detects these linear recurrences and synthesizes an equivalent additive recurrence.

---

## Transformation Mechanics

LSR is implemented in [`crates/vanta_mir/src/opt/strength_reduction.rs`](file:///crates/vanta_mir/src/opt/strength_reduction.rs).

### 1. Recurrence Candidate Identification
Given a natural loop with a basic induction variable $IV$:
- Initial value: $B_0$
- Loop step: $S$ (e.g. $+1$)
- Loop invariant stride/scale: $M$

Any multiplication inside the loop body of the form:
$$T = IV \times M$$
is recognized as a strength-reduction candidate.

### 2. Recurrence Synthesis
1. **Preheader Initialization:**
   In the loop's dedicated preheader, compute the initial value and recurrence step:
   $$R_0 = B_0 \times M$$
   $$\Delta = S \times M$$
   If $B_0 = 0$, $R_0 = 0$ is folded immediately into a constant.
2. **Loop Header Phi Insertion:**
   A new SSA value $R$ is added to the loop header:
   $$R = \phi(\text{preheader} \to R_0, \text{latch} \to R_{\text{next}})$$
3. **Usage Replacement:**
   All occurrences of $T = IV \times M$ inside the loop body are replaced with references to $R$.
4. **Latch Update Insertion:**
   In the loop latch block, compute the recurrence update using cheap addition:
   $$R_{\text{next}} = R + \Delta$$

---

## Disassembly Verification

Inspection of generated native x86-64 machine code confirms the complete elimination of multiplication inside the inner loop:

### Before LSR (`--no-strength-reduction`):
```x86asm
0000000000401250 <compute_lsr>:
  ...
  40128e:  mov    rdi, 0x9
  401295:  mov    r15, r9
  401298:  imul   r15, rdi        ; Expensive multiply in loop body!
  40129c:  add    rsi, r15
  40129f:  add    r9, 1
  ...
```

### After LSR (`-O1`):
```x86asm
0000000000401250 <compute_lsr>:
  40126e:  mov    r8, 0x9         ; Stride hoisted to preheader!
  ...
  40129c:  add    r14, r15        ; sum += recurrence
  4012ac:  add    r12, r13        ; i += 1
  4012b2:  add    rdi, r8         ; recurrence += 9 (NO IMUL!)
  ...
```

The inner loop body contains **zero** `imul` instructions.

---

## CLI Switch

LSR can be independently disabled across all compiler interfaces:
- `vantac --no-strength-reduction`
- `vanta compile --no-strength-reduction`
- `vanta run --no-strength-reduction`
