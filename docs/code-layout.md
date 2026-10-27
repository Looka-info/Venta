# Branch-Aware Machine-Code Layout and Alignment in Vanta

**Module:** [`crates/vanta_codegen_x86_64/src/instruction_select.rs`](file:///crates/vanta_codegen_x86_64/src/instruction_select.rs)  
**Encoding:** [`crates/vanta_codegen_x86_64/src/lib.rs`](file:///crates/vanta_codegen_x86_64/src/lib.rs)  
**CLI Switches:** `--no-block-layout`, `--no-code-align`  
**Introduced in:** Milestone 24

---

## 1. Overview and Objectives

Branch-Aware Machine-Code Layout and Alignment reorganizes physical basic blocks and machine instructions to minimize branch penalties and maximize execution efficiency on modern x86-64 superscalar pipelines:

1. **Trace-Scheduling Block Layout:** Reorders CFG basic blocks so that frequently taken execution paths fall through directly to the physically next basic block, eliminating taken-branch penalties and instruction cache breaks.
2. **Conditional Branch Inversion:** Inverts branch conditions when the "then" target matches the physical successor, converting jumps into straight-line fall-throughs.
3. **Fall-Through Jump Elimination:** Omits redundant unconditional `jmp` instructions when the jump target is the immediately adjacent physical basic block or the function epilogue.
4. **Loop Header Code Alignment:** Aligns loop headers to 16-byte instruction cache boundaries using standard NOP padding (`0x90`), ensuring optimal instruction fetch and decode window alignment.

---

## 2. Trace-Scheduling Basic-Block Ordering

In the naive emitter, basic blocks are emitted in their arbitrary definition order in `MirFunction::blocks`. Under `--enable-block-layout` (enabled by default in `-O1`), [`InstructionSelector`](file:///crates/vanta_codegen_x86_64/src/instruction_select.rs) employs a deterministic trace-scheduling heuristic:

1. **Entry Block Anchor:** Block 0 (`BlockId(0)`) is always placed first.
2. **Greedy Successor Tracing:** From the current block $B$, the algorithm inspects the terminator:
   - For `Branch(target)`: If `target` is unvisited, it is selected as the immediate physical successor.
   - For `CondBranch { then_block, else_block, .. }`: The pass prioritizes unvisited successors, preferring `else_block` (the standard fall-through path), followed by `then_block`.
3. **Residual Block Placement:** Any unvisited blocks (e.g., exception or panic cold paths) are placed in ascending `BlockId` order.

---

## 3. Branch Inversion and Jump Elimination

During machine instruction selection, the relative physical positions of blocks dictate the branch instruction emission:

### 3.1 Conditional Branch Inversion
Given `Terminator::CondBranch { cond, then_block, else_block }` and the physical next block `next_block_id`:

- **Case 1: `then_block == next_block_id` (Inversion):**
  Instead of emitting a jump to `then_block` followed by a jump to `else_block`, the condition is inverted to `ConditionCode::Equal` (`je else_label`), allowing the "then" path to fall through directly:
  ```x86asm
  # Branch inverted to jump to else_label on false, falling through to then_block
  test %rax, %rax
  je .L_func_bb_else
  # fall-through directly into .L_func_bb_then
  ```

- **Case 2: `else_block == next_block_id` (Standard Fall-Through):**
  A standard conditional jump to `then_block` is emitted on true (`jne then_label`), falling through directly into `else_block`:
  ```x86asm
  test %rax, %rax
  jne .L_func_bb_then
  # fall-through directly into .L_func_bb_else
  ```

- **Case 3: Neither block is physical next:**
  Emits `jne then_label` followed by an explicit `jmp else_label`.

### 3.2 Unconditional Jump Elimination
For `Terminator::Branch(target)`:
- If `Some(target) == next_block_id`, **no jump instruction is emitted**. Execution falls through directly into the successor.
- Otherwise, `MachineInst::Jmp(target_label)` is emitted.

### 3.3 Epilogue Fall-Through
For `Terminator::Return(val)`:
- If the current block is the final physical block preceding the epilogue, the unconditional `jmp .L_func_epilogue` is omitted, falling through directly into the frame epilogue.

---

## 4. Loop Header 16-Byte Alignment

Hot inner loop performance is heavily influenced by instruction fetch boundary alignment. When `--enable-code-align` is active (the default in `-O1`), [`X86_64Backend::compile_to_elf_full`](file:///crates/vanta_codegen_x86_64/src/lib.rs) aligns loop header labels:

```rust
if enable_code_align {
    if let MachineInst::Label(lbl) = inst {
        if lbl.contains("loop_header") {
            while !emitter.bytes.len().is_multiple_of(16) {
                emitter.emit_u8(0x90); // nop padding
            }
        }
    }
}
```

This prevents the loop entry instruction from straddling a 16-byte or 64-byte cache line boundary, preventing instruction prefetch stalls.

---

## 5. Verification and CLI Controls

The optimizations can be independently toggled:
- `--no-block-layout`: Disables trace scheduling and branch inversion, preserving linear block order.
- `--no-code-align`: Disables 16-byte loop header NOP padding.

Correctness is verified by [`layout_tests.rs`](file:///crates/vanta_driver/tests/layout_tests.rs), which validates jump elimination and execution equivalence across nested loops, breaks, and complex branch graphs.
