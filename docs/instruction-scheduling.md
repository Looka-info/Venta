# Dependency-Aware Instruction Scheduling in Vanta

**Pass Module:** [`crates/vanta_mir/src/opt/instruction_schedule.rs`](file:///crates/vanta_mir/src/opt/instruction_schedule.rs)  
**Flag:** `--no-instruction-schedule` (alias: `--no-sched`)  
**Introduced in:** Milestone 23

---

## 1. Overview and Motivation

Modern x86-64 microarchitectures execute instructions out-of-order across multiple execution ports, but pipeline bubbles and stall cycles still arise when an instruction depends directly on a multi-cycle high-latency operation (such as an integer multiplication with latency 3-4, or a memory load with L1 latency 4-5).

Vanta's **Dependency-Aware Instruction Scheduler** is an intra-basic-block DAG list scheduler that reorders independent operations. By interleaving independent computations between high-latency producers and their consumers, the scheduler allows memory loads and arithmetic multiplications to hide their latency without stalling the CPU pipeline.

---

## 2. Dependency Graph Construction

For each basic block, the scheduler constructs a Directed Acyclic Graph (DAG) whose nodes represent instructions and whose edges represent strict ordering constraints.

### 2.1 Dependency Types
1. **RAW (Read-After-Write / True Data Dependencies):** If instruction $A$ defines `ValueId(x)` and instruction $B$ consumes `ValueId(x)`, an edge $A \to B$ is added with weight equal to the latency of instruction $A$.
2. **WAR (Write-After-Read / Anti-Dependencies):** If instruction $A$ reads memory or slot $M$ and instruction $B$ overwrites $M$, an edge $A \to B$ is enforced with latency 1.
3. **WAW (Write-After-Write / Output Dependencies):** If instruction $A$ stores to memory address $M$ and instruction $B$ subsequently stores to $M$, edge $A \to B$ is enforced.
4. **Memory Aliasing Barriers:** Because Vanta conservative MIR does not assume whole-program pointer disambiguation across indirect stores, every `StoreIndirect` or `Call` acts as a memory barrier with respect to subsequent `LoadIndirect` operations.
5. **Call Barriers:** Function calls (`Instruction::Call`) may observe memory, trigger side effects, or modify global state. All instructions preceding a call must complete before the call, and all instructions following a call cannot be scheduled prior to the call.

---

## 3. Microarchitectural Latency Modeling

The scheduler models representative latency and throughput characteristics for standard x86-64 microarchitectures (Intel Skylake/Golden Cove, AMD Zen 3/4):

| Instruction Category | MIR Instructions | Modeled Latency | Issue Throughput |
|---|---|---:|---:|
| Simple Arithmetic / Logic | `Add`, `Sub`, `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr` | 1 cycle | 1 / cycle |
| Constants & Copies | `ConstInt`, `ConstBool`, `Copy`, `SlotAddr` | 1 cycle | 1 / cycle |
| Integer Multiplication | `Binary { op: Mul, .. }` | 3 cycles | 1 / cycle |
| Memory Loads | `Load`, `LoadIndirect`, `LoadSlice` | 4 cycles | 1 / cycle |
| Memory Stores | `Store`, `StoreIndirect`, `StoreSlice` | 1 cycle | 1 / cycle |
| Integer Division / Remainder | `Binary { op: Div | Rem, .. }` | 15 cycles | Non-pipelined |
| Function Calls | `Call` | 20 cycles | Call barrier |

---

## 4. List Scheduling Algorithm

Instruction scheduling uses a backward critical-path height heuristic:

### 4.1 Critical Path Height
For each node $u$ in the DAG:
$$\text{Height}(u) = \text{latency}(u) + \max_{(u, v) \in \text{Edges}} \text{Height}(v)$$
Leaves with no outgoing edges have $\text{Height}(u) = \text{latency}(u)$.

### 4.2 Priority Selection
1. A **Ready List** tracks all instructions whose incoming dependency count has reached 0.
2. At each scheduling step, the ready instruction with the **highest critical path height** is selected.
3. If heights are equal, tie-breaking chooses the instruction with the **earliest original program index**, preserving original source order stability.
4. As each instruction is scheduled, incoming dependency counters of successor nodes are decremented, and newly ready instructions enter the Ready List.

---

## 5. CLI Controls & Statistics

The pass can be disabled independently via:
```bash
vantac -O1 --no-sched main.va -o main
```

Pass statistics are tracked via `InstructionScheduleStats`:
- `blocks_scheduled`: Total basic blocks evaluated and rescheduled.
- `instructions_reordered`: Individual instructions moved relative to their prior position.
- `max_critical_path`: Deepest critical path height discovered across all basic blocks.
