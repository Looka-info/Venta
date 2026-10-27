# Vanta Register Allocation Specification & Architecture

## Overview

Prior to Milestone 17, Vanta's custom x86-64 backend operated strictly in **stack-spill reference mode**: every MIR virtual register (`ValueId`) was assigned an 8-byte stack slot under `%rbp`, and every instruction read its inputs from `[rbp - offset]` through scratch register `%r10` and wrote its result back to `[rbp - offset]`. In tight computational loops, this generated hundreds of millions of redundant memory accesses, causing compute-heavy workloads to lag behind optimized C and Rust.

Milestone 17 introduces a **custom physical register allocator** directly into Vanta's native x86-64 backend (`crates/vanta_codegen_x86_64/src/reg_alloc.rs`). The allocator:
1. Performs CFG-aware dataflow liveness analysis.
2. Partitions physical x86-64 registers into distinct caller-saved and callee-saved pools.
3. Implements deterministic linear scan allocation with spill eviction.
4. Preserves the System V AMD64 ABI, stack alignment, and callee-saved register invariants.
5. Emits optimized register-to-register and register-to-memory instructions, eliminating intermediate stack traffic.

---

## 1. Register Architecture and Pool Partitioning

The x86-64 architecture provides 16 general-purpose 64-bit registers. Vanta partitions them into strict functional pools:

| Register | Classification | Role in Vanta Backend |
|---|---|---|
| `%rax` | Reserved / Return | Function return value, 1st operand of `idiv`/`cqo`, temporary for byte conversions |
| `%rdx` | Reserved / Math | 2nd operand of `idiv` (remainder / high 64 bits), not allocated to avoid math clobbers |
| `%rcx` | Reserved / Shift | Hardware shift counter (`%cl`) for `shl` / `shr` instructions |
| `%r10` | Reserved / Scratch | Primary scratch register for memory operand bridging, spills, and address calculations |
| `%r11` | Reserved / Scratch | Secondary scratch register for two-memory operations and indirect stores |
| `%rsp` | Reserved / Stack | Hardware stack pointer (strictly 16-byte aligned before `call`) |
| `%rbp` | Reserved / Frame | Hardware frame pointer |
| **`%rbx`** | **Callee-saved Pool** | General-purpose allocated register; preserved across calls |
| **`%r12`** | **Callee-saved Pool** | General-purpose allocated register; preserved across calls |
| **`%r13`** | **Callee-saved Pool** | General-purpose allocated register; preserved across calls |
| **`%r14`** | **Callee-saved Pool** | General-purpose allocated register; preserved across calls |
| **`%r15`** | **Callee-saved Pool** | General-purpose allocated register; preserved across calls |
| **`%rdi`** | **Caller-saved Pool** | General-purpose allocated register for non-call-crossing expressions |
| **`%rsi`** | **Caller-saved Pool** | General-purpose allocated register for non-call-crossing expressions |
| **`%r8`** | **Caller-saved Pool** | General-purpose allocated register for non-call-crossing expressions |
| **`%r9`** | **Caller-saved Pool** | General-purpose allocated register for non-call-crossing expressions |

### Invariant: Call-crossing vs Non-call-crossing
- **Callee-saved Pool (`Rbx, R12, R13, R14, R15`)**: Assigned to virtual registers whose live range spans one or more `call` instructions. Because System V AMD64 ABI guarantees callee functions preserve these registers, the values remain undisturbed across function calls.
- **Caller-saved Pool (`Rdi, Rsi, R8, R9`)**: Assigned to virtual registers whose live range is strictly contained within call-free regions. When a `call` occurs, none of these registers contains a live value, allowing arguments to be passed in `ARG_REGS` (`rdi, rsi, rdx, rcx, r8, r9`) without register shuffling or clobber hazards.

---

## 2. Dataflow Liveness Analysis

Liveness analysis operates over Vanta's control flow graph (CFG) in `MirFunction`:

### Step 1: Block-level Def and Use Sets
For each basic block $B$, compute:
- $\text{Def}(B)$: Set of `ValueId`s defined in $B$ before any use in $B$.
- $\text{Use}(B)$: Set of `ValueId`s used in $B$ before any definition in $B$.

### Step 2: Backward Fixed-Point Iteration
Dataflow equations are iterated backward until a global fixed point is reached:
$$\text{LiveOut}(B) = \bigcup_{S \in \text{Succs}(B)} \text{LiveIn}(S)$$
$$\text{LiveIn}(B) = \text{Use}(B) \cup (\text{LiveOut}(B) \setminus \text{Def}(B))$$

Because Vanta loops branch backward to loop headers, loop-carried liveness correctly propagates into $\text{LiveOut}$ of all loop blocks.

### Step 3: Linearization and Interval Construction
Program points are indexed monotonically ($0, 2, 4, \dots$). Each `ValueId` is assigned a `LiveInterval`:
- $\text{start}$: The point of definition.
- $\text{end}$: The maximum point of all uses or the end of any block where the value is live-out.
- $\text{does\_cross\_call}$: `true` if $\exists cp \in \text{CallPoints}$ such that $\text{start} < cp \le \text{end}$.

---

## 3. Linear Scan Register Allocation Algorithm

Given the sorted intervals ordered by $\text{start}$ ascending:

1. **Expire Active Intervals**:
   For all currently active intervals, if $\text{active.end} < \text{current.start}$, release the assigned physical register back to its corresponding pool (`free_callee_saved` or `free_caller_saved`).
2. **Allocation**:
   - If `does_cross_call`:
     - Attempt to acquire a register from `free_callee_saved`.
     - If unavailable, find the active callee-saved interval with the farthest $\text{end}$ point. If that $\text{end} > \text{current.end}$, evict it (spill it to stack) and give its register to `current`.
     - Otherwise, spill `current`.
   - If `!does_cross_call`:
     - Attempt to acquire from `free_caller_saved`.
     - If unavailable, attempt to acquire from `free_callee_saved`.
     - If both pools are exhausted, evict the active interval with the farthest $\text{end}$ point or spill `current`.
3. **Record Callee-saved Usage**:
   Track all distinct callee-saved registers utilized. Only the registers actually used are pushed/popped by the function.

---

## 4. Stack Layout and System V AMD64 ABI Compliance

When register allocation is active and $K$ callee-saved registers are used:

```
+------------------------------------+  <-- High Memory
| Caller Stack Frame                 |
+------------------------------------+
| Return Address (pushed by CALL)    |  <-- rsp % 16 == 8 at entry
+------------------------------------+
| Saved %rbp (pushed by function)    |  <-- rbp % 16 == 0
+------------------------------------+  <-- [rbp - 0]
| Saved Callee-saved Reg 1           |  <-- [rbp - 8]
| Saved Callee-saved Reg 2           |  <-- [rbp - 16]
| ...                                |
| Saved Callee-saved Reg K           |  <-- [rbp - (K * 8)]
+------------------------------------+
| Sret return pointer (if aggregate) |  <-- [rbp - (K * 8) - 8]
| Local Slot Variables (func.slots)  |
| Call Return Buffers                |
| Spill Slots (spill_0, spill_1...)  |
+------------------------------------+  <-- rsp % 16 == 0 (aligned total)
| Padding (0..8 bytes for 16B align) |
+------------------------------------+  <-- Low Memory
```

### Prologue
```asm
push rbp
mov rbp, rsp
push rbx          ; If used
push r12          ; If used
push r13          ; If used
sub rsp, frame_sz ; frame_sz = (aligned_total - K * 8)
```
Total bytes below `%rbp` is $(K \times 8) + \text{frame\_sz} = \text{aligned\_total}$, which is guaranteed to be a multiple of 16.

### Epilogue
```asm
lea rsp, [rbp - (K * 8)]
pop r13           ; In reverse order
pop r12
pop rbx
pop rbp
ret
```
Restoring `%rsp` with `lea rsp, [rbp - (K * 8)]` ensures that stack frame deallocation and callee-saved register restoration are 100% immune to dynamic stack adjustments or leaf calls.

---

## 5. Instruction Selection Lowerings

With register allocation active, `InstructionSelector` queries `get_val_op(val) -> Operand`:

1. **Immediate / Constant**:
   - Zero constant: `xor dst, dst` (2 bytes, zero latency).
   - Non-zero: `mov dst, imm`.
   - Spilled: `mov [rbp - spill], imm` via `%r10`.
2. **Binary Operations (`add`, `sub`, `imul`, `and`, `or`, `xor`)**:
   - Register destination:
     ```asm
     mov dst, lhs      ; Omitted if dst == lhs
     add dst, rhs_op   ; rhs_op can be Register or Memory!
     ```
   - Zero memory operations when `lhs`, `rhs`, and `dest` reside in registers.
3. **Conditionals (`test` / `cmp` / `jcc`)**:
   - Direct register testing: `test reg, reg; jnz <then>; jmp <else>`. Zero stack traffic.
4. **Function Calls**:
   - Arguments passed directly from their allocated callee-saved register or spill slot into `ARG_REGS` (`%rdi, %rsi, ...`).
   - Call returns (`%rax`) written directly to destination register.
