# Vanta Memory-to-Register (Mem2Reg) and SSA Construction Architecture

## 1. Overview

**Mem2Reg** is Vanta's SSA construction and stack-to-register promotion optimization pass. Operating on Vanta Mid-Level Intermediate Representation (MIR), it promotes eligible mutable local variables (`SlotId`) into Static Single Assignment (`ValueId`) virtual registers.

By converting stack-allocated local variables into pure SSA values and inserting `Phi` nodes at control-flow join points, Mem2Reg eliminates redundant memory traffic (`Load` and `Store` instructions). These SSA values subsequently flow directly into Vanta's physical register allocator (Milestone 17 linear scan allocator), allowing hot loop accumulators and scalars to reside entirely in hardware registers (`%rbx`, `%r12`, `%r13`, `%r14`, `%r15`, `%rsi`, `%rdi`, `%r8`, `%r9`).

```
                +----------------------------+
                |     MIR Lowerer (AST)      |
                |   Variables as LocalSlots  |
                +--------------+-------------+
                               |
                               v
                +----------------------------+
                |    Unreachable Block DCE   |
                +--------------+-------------+
                               |
                               v
                +----------------------------+
                |          Mem2Reg           |
                |  - Conservative Filter     |
                |  - Critical Edge Splitting |
                |  - Dominance Frontiers (DF)|
                |  - Phi Placement (IDF)     |
                |  - Dom-Tree Renaming Walk  |
                |  - Load/Store Elimination  |
                +--------------+-------------+
                               |
                               v
                +----------------------------+
                |  Constant Propagation &    |
                |  Dead Code Elimination     |
                +--------------+-------------+
                               |
                               v
                +----------------------------+
                |  Linear Scan Reg Allocator |
                |    (Maps SSA to Hardware)  |
                +--------------+-------------+
                               |
                               v
                +----------------------------+
                |  Instruction Selection     |
                |  - Push/Pop Parallel Copies|
                |  - Native ELF64 Object     |
                +----------------------------+
```

---

## 2. Eligibility Rules & Conservative Fallback

Mem2Reg guarantees sound behavior by strictly promoting only eligible scalar local variables. If any variable fails eligibility criteria, it remains stack-allocated with its original `SlotId`, preserving memory semantics and destructor safety.

### Eligibility Criteria
1. **Scalar Primitive Types Only**:
   Only scalar types (`Type::I64`, `Type::I32`, `Type::U64`, `Type::U32`, `Type::I16`, `Type::U16`, `Type::I8`, `Type::U8`, `Type::Bool`) are eligible. Complex aggregates (`Struct`, `Enum`, `String`, `Vec`, `Array`) are never promoted.
2. **Non-Escaping / Non-Address-Taken**:
   A slot is rejected if its address is taken (`SlotAddr`) and that address is consumed by any downstream instruction, or if it is indexed (`IndexAddr`) or accessed via slice instructions (`StoreSlice`, `LoadSlice`).
3. **Non-Parameter**:
   Function parameters (`func.params`) are established by the caller via the System V AMD64 ABI and bound to stack slots; they are not promoted directly by Mem2Reg.
4. **Preserved Ownership & Drop Semantics**:
   Because only non-dropping primitive scalars are promoted, drop scopes, linear destructors, and move flags remain completely unaffected.

---

## 3. Algorithmic Pipeline

### Step 1: Critical Edge Splitting
A **critical edge** is a control flow edge from a basic block with multiple successors (`CondBranch`) to a basic block with multiple predecessors (a merge block containing `Phi` nodes).

If critical edges were not split, any parallel copy or phi move placed in the predecessor block would erroneously execute on both branch targets.

Vanta splits all critical edges upfront:
```
           [ CondBranch ]
              /        \
             /          \  (Critical Edge)
            v            v
     [ Then Block ]   [ Merge Block (Phis) ]

                      ||
                      \/

           [ CondBranch ]
              /        \
             /          \
            v            v
     [ Then Block ]   [ Trampoline Block ]  (Branch unconditionally)
            \            /
             \          /
              v        v
         [ Merge Block (Phis) ]
```
Every predecessor of a phi-bearing merge block is guaranteed to terminate with an unconditional `Terminator::Branch(target)`.

### Step 2: Dominator Tree & Dominance Frontiers (DF)
- **Dominance**: A block $D$ dominates block $B$ ($D \text{ dom } B$) if every execution path from the function entry to $B$ must pass through $D$.
- **Immediate Dominator ($idom(B)$)**: The unique strict dominator of $B$ that does not strictly dominate any other strict dominator of $B$.
- **Dominance Frontier ($DF(X)$)**: The set of nodes $Y$ such that $X$ dominates a predecessor of $Y$, but $X$ does not strictly dominate $Y$ itself:
  $$\forall B \text{ with } |Preds(B)| \ge 2: \quad \forall P \in Preds(B), \text{ walk } runner = P \text{ up to } idom(B): \quad DF(runner) \leftarrow DF(runner) \cup \{B\}$$

### Step 3: Iterated Dominance Frontier (IDF) & Phi Placement
For each eligible slot $S$, let $Defs(S)$ be the set of basic blocks containing a `Store` to $S$. The set of join blocks requiring a `Phi` node is computed using the Iterated Dominance Frontier ($IDF$):
$$Phis(S) = IDF(Defs(S))$$
A new `Instruction::Phi { dest: new_val, inputs: vec![], ty: slot.ty }` is inserted at the top of each block in $Phis(S)$.

### Step 4: Dominator Tree Preorder Renaming Walk
A recursive preorder traversal of the dominator tree visits blocks starting at `entry_block`:
1. **Reaching Definition Stacks**: A map `stacks: HashMap<SlotId, Vec<ValueId>>` maintains the current active SSA value for each slot. An initial default value (e.g. `0`) is seeded at entry.
2. **Phis Define Dest**: Each phi node in block $B$ defines `phi.dest`, which is pushed onto the corresponding slot stack.
3. **Stores Update Reaching Def**: An eligible `Store { slot, val }` pushes `val` onto `stacks[slot]` and marks the `Store` for removal.
4. **Loads Replaced**: An eligible `Load { dest, slot, .. }` is mapped to `stacks[slot].last()` in `val_subst` and marked for removal.
5. **Populate Successor Phis**: For each CFG successor block $Succ$, any phi in $Succ$ receives `(curr_block, stacks[slot].last())`.
6. **Recurse & Backtrack**: Dom-tree children are visited; upon exit from $B$, all values pushed by $B$ are popped from the stacks.

### Step 5: Instruction Pruning & Slot Removal
- All instruction uses throughout `func` are updated according to `val_subst`.
- Marked `Load` and `Store` instructions are deleted from basic blocks.
- Promoted slots are deleted from `func.slots`.

---

## 4. SSA Deconstruction & Hazard-Free Parallel Copies

In the x86-64 backend (`vanta_codegen_x86_64`), SSA values must be emitted to machine code.

### Instruction Selection for Phi
1. **Block Entry**: `Instruction::Phi` emits **no machine instructions** at the beginning of the block.
2. **Predecessor Edge**: On `Terminator::Branch(target)`, the selector inspects the target block's phis and collects all transfers:
   $$(dest\_op, val\_op)$$
   where $dest\_op$ is the allocated location of `phi.dest` and $val\_op$ is the allocated location of the incoming value from this predecessor.

### Parallel Copy Resolution
When multiple phi transfers execute at a block transition, a naive sequence of moves can suffer from dependency hazards or register cycles (e.g. swapping $A$ and $B$).

Vanta detects hazards and resolves them cleanly:
- **Zero Moves**: If `dest_op == val_op`, the value is already in the target physical register. Zero instructions are emitted!
- **Single Transfer / Hazard-Free Sequential Transfers**: Direct `mov dest_op, val_op` instructions are emitted (utilizing scratch `%r10` for memory-to-memory moves).
- **Hazardous Cycle / Swap**: If a destination register is read as a source by another transfer, Vanta uses staged stack evaluation:
  1. All sources are evaluated and pushed: `push src_op` (or `mov r10, src_op; push r10`).
  2. All destinations are popped in reverse order: `pop dest_op`.
  
  Because all local slots and spill offsets are relative to `%rbp`, adjusting `%rsp` during the push/pop cycle does not alter memory operand offsets. The net change to `%rsp` is exactly zero, guaranteeing 100% hazard-free swaps without register clobbering.

---

## 5. CLI Controls & Differential Testing

Mem2Reg is controlled by the `--no-mem2reg` CLI flag, available in both `vantac` and `vanta`:

```bash
# Compile with Mem2Reg enabled (default at -O1)
vantac -O1 program.va -o program

# Compile with Mem2Reg disabled (stack slots preserved)
vantac -O1 --no-mem2reg program.va -o program_nomem

# Compile with both Mem2Reg and physical register allocation disabled
vantac -O1 --no-mem2reg --no-regalloc program.va -o program_spill
```

Differential verification in `crates/vanta_driver/tests/mem2reg_tests.rs` tests each workload with and without `--no-mem2reg`, asserting byte-for-byte identical output and return codes across all branches, loops, and variable swaps.
