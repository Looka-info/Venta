# Scalar Replacement of Aggregates (SROA) in Vanta

## Overview

Scalar Replacement of Aggregates (SROA) transforms eligible local struct aggregate storage on the stack into independent scalar local slots. When combined with Vanta's Memory-to-Register (`Mem2Reg`) SSA promotion pass, this completely eliminates stack allocation, indirect memory stores (`StoreIndirect`), and indirect memory loads (`LoadIndirect`), promoting struct fields directly into SSA registers and physical CPU registers.

---

## Eligibility & Safety Constraints

To preserve Vanta's memory safety, ownership, drop ordering, and System V AMD64 ABI invariants, SROA enforces strict eligibility gates:

1. **Local Allocations Only:** Function parameters (`param_slots`) are never scalarized; they adhere strictly to ABI calling conventions.
2. **Scalar Fields Only:** The aggregate must consist exclusively of primitive scalar fields (`i64`, `i32`, `u64`, `u32`, `i16`, `u16`, `i8`, `u8`, `bool`).
3. **No Destructor/Drop:** Structs containing non-trivial destructor fields (e.g. `String`, `Vec<T>`, or custom drop semantics) are strictly rejected.
4. **No Address Escapes:**
   - The slot address (`SlotAddr`) must never escape to external function calls (`Call`).
   - The address must never be stored as data into another pointer or memory slot (`Store`, `StoreIndirect`).
   - The address must never be returned from the function (`Return`).
   - The address is only permitted to be passed to `FieldAddr` with statically valid byte offsets corresponding to declared struct fields.
   - Field pointers produced by `FieldAddr` must only be consumed as target addresses in `LoadIndirect` or `StoreIndirect`.

---

## Implementation Details

SROA is implemented in [`crates/vanta_mir/src/opt/sroa.rs`](file:///crates/vanta_mir/src/opt/sroa.rs).

### 1. Decomposition of Aggregate Slots
When an aggregate slot `slot_agg` of type `Point { x: i64, y: i64 }` is identified as eligible and non-escaping:
1. `slot_agg` is removed from `func.slots`.
2. New individual scalar local slots are created for each field:
   - `slot_x`: `LocalSlot { name: "p_x", ty: Type::I64, size: 8, align: 8 }`
   - `slot_y`: `LocalSlot { name: "p_y", ty: Type::I64, size: 8, align: 8 }`
3. A byte-offset map `offset_to_slot` maps field offsets to their scalar slots.

### 2. Rewriting Memory Accesses
All instructions accessing the aggregate are rewritten:
- `FieldAddr { dest, base_ptr, offset }`:
  Recorded into an offset mapping: `field_ptr_map.insert(dest, offset_to_slot[offset])`. The `FieldAddr` instruction is replaced by an SSA no-op copy.
- `StoreIndirect { addr, val }`:
  Rewritten to a direct scalar store:
  ```rust
  Instruction::Store {
      slot: mapped_slot,
      val: *val,
  }
  ```
- `LoadIndirect { dest, addr, ty }`:
  Rewritten to a direct scalar load:
  ```rust
  Instruction::Load {
      dest: *dest,
      slot: mapped_slot,
      ty: ty.clone(),
  }
  ```

### 3. Immediate SSA Register Promotion
Immediately following SROA decomposition, `Mem2Reg` runs on the function. Because `slot_x` and `slot_y` are standard scalar slots, `Mem2Reg` converts all `Store` and `Load` instructions into SSA values and `Phi` nodes, completely removing all stack slots.

---

## Disassembly Verification

Inspection of the generated native machine code for `sroa_workload` demonstrates the transformation:

### Before SROA (`--no-sroa`):
```x86asm
00000000004012b0 <compute_sroa>:
  ...
  4012ef:  lea    rdi, [rbp-0x48]
  4012f9:  mov    QWORD PTR [r15+0x0], rsi   ; Store x to stack memory
  401317:  mov    QWORD PTR [r14+0x0], r12   ; Store y to stack memory
  401335:  mov    QWORD PTR [r13+0x0], rbx   ; Store z to stack memory
  40133e:  mov    rdi, QWORD PTR [r15+0x0]   ; Load x from stack memory
  401342:  mov    rsi, QWORD PTR [r14+0x0]   ; Load y from stack memory
  ...
```

### After SROA (`-O1`):
```x86asm
00000000004012b0 <compute_sroa>:
  ...
  4012ea:  mov    rdi, 0x1
  4012f1:  mov    r15, rsi
  4012f4:  add    r15, rdi        ; r15 = i + 1 (pure register arithmetic)
  4012f7:  mov    rdi, 0x2
  4012fe:  mov    r14, rsi
  401301:  add    r14, rdi        ; r14 = i + 2 (pure register arithmetic)
  401309:  mov    rdi, rsi
  40130c:  add    rdi, r15        ; rdi = x + y (register add)
  401312:  add    rsi, r14        ; rsi = (x + y) + z (register add)
  ...
```

**Every memory load and store has been eliminated from the loop body.**

---

## CLI Switch

SROA can be independently disabled:
- `vantac --no-sroa`
- `vanta compile --no-sroa`
- `vanta run --no-sroa`
