# Vanta Mid-Level Intermediate Representation (MIR) Specification

---

## 1. Overview

Vanta MIR is a control-flow-graph (CFG) based intermediate representation between the typed AST and target machine code.

MIR characteristics:
1. **Explicit Control Flow**: All loops (`while`, `loop`), conditionals (`if`/`else`), and short-circuiting operators are lowered into explicit basic blocks connected by branch terminators.
2. **Explicit Memory vs Value Representation**:
   - Variables and mutable allocations live in stack slots (`SlotId`).
   - Temporary values live in single-assignment virtual value IDs (`ValueId`).
3. **Target Independent**: MIR makes zero assumptions about x86-64 or register counts.
4. **Strictly Typed & Verifiable**: Every `ValueId` and `SlotId` possesses a defined `Type`. The MIR Verifier checks that all operands match expected types before lowering to Machine IR.

---

## 2. Structure

A `MirProgram` consists of global declarations and a set of `MirFunction` definitions.

```
MirFunction
├── name: String
├── params: Vec<(SlotId, Type)>
├── return_type: Type
├── local_slots: Vec<LocalSlot>
└── blocks: Vec<BasicBlock>
    ├── id: BlockId
    ├── instructions: Vec<Instruction>
    └── terminator: Terminator
```

---

## 3. Instructions

- `SlotAddr(ValueId, SlotId)`: Compute address of a local stack slot.
- `Load(ValueId, SlotId, Type)`: Load value from a local stack slot.
- `Store(SlotId, ValueId)`: Store value into a local stack slot.
- `ConstInt(ValueId, i64, Type)`: Materialize an integer constant.
- `ConstBool(ValueId, bool)`: Materialize a boolean constant.
- `Binary(ValueId, BinaryOp, ValueId, ValueId, Type)`: Binary arithmetic, comparison, or logical operation.
- `Unary(ValueId, UnaryOp, ValueId, Type)`: Negation, bitwise NOT, logical NOT.
- `Call(Option<ValueId>, String, Vec<ValueId>, Type)`: Direct function call.
- `IndexAddr(ValueId, SlotId, ValueId, usize)`: Compute address of element within a stack slot (`slot_addr + index * elem_size`).
- `IndexPtrAddr(ValueId, ValueId, ValueId, usize)`: Compute element address from a dynamic base pointer (`base_ptr + index * elem_size`).
- `LoadIndirect(ValueId, ValueId, Type)`: Load a typed value from an arbitrary memory address.
- `StoreIndirect(ValueId, ValueId)`: Store a value into an arbitrary memory address.
- `StrSlice(ValueId, ValueId, String, usize)`: Materialize a string slice descriptor `(data_ptr, byte_len)`.
- `StoreSlice(SlotId, ValueId, ValueId)`: Store two-word slice descriptor into a 16-byte local slot.
- `LoadSlice(ValueId, ValueId, SlotId)`: Load two-word slice descriptor from a 16-byte local slot.

### 3.1 Bounds Check Lowering Pattern
For indexed access `arr[i]`:
```text
  %zero = ConstInt(0, i64)
  %ge_zero = Binary(GtEq, %idx, %zero, bool)
  %lt_len = Binary(Lt, %idx, %len, bool)
  %in_bounds = Binary(And, %ge_zero, %lt_len, bool)
  CondBranch %in_bounds, bb_ok, bb_fail

bb_fail:
  Call vanta_bounds_check_fail(%idx, %len)
  Unreachable

bb_ok:
  %addr = IndexPtrAddr(%base_ptr, %idx, elem_size)
  %val = LoadIndirect(%addr, elem_type)
```
The check strictly dominates all address calculation and memory operations.

---

## 4. Terminators

Every basic block must end with exactly one terminator:
- `Branch(BlockId)`: Unconditional jump.
- `CondBranch(ValueId, BlockId, BlockId)`: Conditional branch (condition, then_block, else_block).
- `Return(Option<ValueId>)`: Return from function.
- `Unreachable`: Mark point unreachable after an infinite loop or abort.

---

## 5. Verification Rules

1. Every block referred to in a branch must exist in the function.
2. Every `ValueId` used as an operand must be defined prior to its use.
3. Terminator targets cannot be empty.
4. Every instruction operand type must match the signature of the operation.
5. Functions with a non-unit return type must return a value of the matching type along all terminating paths.
