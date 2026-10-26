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

- `SlotAddr(ValueId, SlotId)`: Compute address of a local slot.
- `Load(ValueId, SlotId, Type)`: Load value from a local slot.
- `Store(SlotId, ValueId)`: Store value into a local slot.
- `ConstInt(ValueId, i64, Type)`: Load integer constant.
- `ConstBool(ValueId, bool)`: Load boolean constant.
- `Binary(ValueId, BinaryOp, ValueId, ValueId, Type)`: Binary arithmetic or logical operation.
- `Unary(ValueId, UnaryOp, ValueId, Type)`: Negation, bitwise NOT, logical NOT.
- `Call(ValueId, String, Vec<ValueId>, Type)`: Direct function call.
- `GetElementPtr(ValueId, ValueId, usize, Type)`: Compute field or element address.

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
