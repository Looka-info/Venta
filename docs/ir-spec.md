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

A `MirProgram` consists of global declarations, struct layout definitions (`structs: HashMap<String, StructDef>`), interned string literals, and a set of `MirFunction` definitions.

```
MirFunction
├── name: String
├── params: Vec<(SlotId, Type)>
├── return_type: Type
├── local_slots: Vec<LocalSlot> (id, name, ty, size, align)
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
- `FieldAddr(ValueId, ValueId, u32)`: Compute field address from a struct base pointer (`base_ptr + offset`).
- `LoadIndirect(ValueId, ValueId, Type)`: Load a typed value from an arbitrary memory address.
- `StoreIndirect(ValueId, ValueId)`: Store a value into an arbitrary memory address.
- `StoreIndirect32(ValueId, ValueId)`: Store a 32-bit value into an arbitrary memory address (used for discriminant tag writes).
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

---

## 6. Monomorphization and Automatic Destruction in MIR

### 6.1 Generic Specialization
- Generic functions and structs are expanded prior to MIR lowering into concrete instances.
- Each concrete specialization becomes a distinct `MirFunction` with mangled name `name__T1_T2`.
- Calls to generic functions are lowered as direct calls to their monomorphized counterparts.

### 6.2 Drop Scope Lowering
- `MirLowerer` maintains a stack of lexical drop scopes `drop_scopes: Vec<Vec<DropCandidate>>`.
- When a variable whose type satisfies `needs_drop` is declared in a `let` statement or function parameter, it is tracked as an active candidate.
- When an initialized variable of a drop type is assigned to another variable, passed by value, or returned, it is marked as moved (`is_moved = true`).
- At lexical scope exit (block termination) and immediately preceding any `Return` terminator, calls to destructors (e.g. `vanta_vec_free`) are emitted in reverse order of declaration (LIFO) for all un-moved candidates.
- Structs containing fields needing destruction recursively emit field-level drops.

### 6.3 Enum Construction, Pattern Matching, and Tagged Union Drop Glue
- **Variant Construction**:
  - `lower_init_variant` allocates an aggregate slot sized according to `EnumDef::compute_layout`.
  - The 32-bit discriminant tag is written to offset 0 using `StoreIndirect32`.
  - Payload fields are stored at `align_to(4, max_payload_align)` using aggregate/scalar stores.
- **Match Expression Lowering**:
  - `lower_match` evaluates the scrutinee into a dedicated slot.
  - A sequence of basic blocks is created: each testing an arm's pattern against the discriminant tag or literal value.
  - On match, control flows to an arm block where payload fields are bound to local arm variables.
  - The arm body expression is evaluated and stored into a merge `result_slot`.
  - Exiting the arm scope automatically executes drop glue for any non-`Copy` arm bindings.
  - All arm exit paths branch to a unified join block.
- **Enum Drop Glue**:
  - `emit_drop_for_place` checks if the enum type satisfies `needs_drop(&structs, &enums)`.
  - If any variant payload requires destruction, it loads the 32-bit tag using `LoadIndirect(%base, Type::I32)` and emits conditional branches testing each drop-needing variant tag.
  - Each variant branch drops the payload fields recursively and joins at the post-drop continuation block. Unit variants and drop-free variants jump directly to the continuation block.

---

## 7. Multi-Module MIR Lowering and Symbol Management

### 7.1 Module Tree Lowering
- `MirLowerer::lower_module_tree(&mut self, module_tree: &ModuleTree) -> MirProgram`:
  - Iterates over all modules in the compilation unit.
  - Lowers each top-level function and struct method using its canonical mangled name.
  - Appends all monomorphized generic function instances.
  - Consolidates string literals into the global `MirProgram::string_literals` table.

### 7.2 Namespaced Symbol Identifiers
- Modules are flattened into the global symbol table using canonical namespacing:
  - Submodule functions: `<module_path>__<function_name>` (e.g. `math__add`, `math__geometry__dist_sq`).
  - Inherent Methods: `<canonical_struct_name>_<method_name>` (e.g. `models__Point_sum`).
  - Trait Methods: `<canonical_target>__<canonical_trait>_<method_name>` (e.g. `User__Describe_describe`, `Square__geometry_Shape_perimeter`).
  - Generic specializations: `<canonical_prefix>__<type_args>` (e.g. `math__identity__i64`).
  - Entry point: `main` in the root module always retains unmangled symbol name `"main"`.
  - Intrinsics: Built-in runtime intrinsics (`print_i64`, `print_str`, `panic`, `vanta_alloc`, `vanta_free`, etc.) are never mangled and map directly to ELF symbol entries.

### 7.3 Trait Method Lowering and Static Dispatch
- `MirLowerer` lowers trait implementations (`Item::Impl` with `trait_name`) into standard `MirFunction` definitions using the mangled symbol format `<Target>__<Trait>_<method>`.
- Method calls `val.method(...)` resolve statically at compile time:
  - The receiver is evaluated to its stack place address using `lower_place_addr(val)`.
  - The receiver pointer is passed as the first argument in `Instruction::Call`.
  - Zero vtables, zero function pointer tables, zero indirect jumps (`CALL *%rax`), and zero runtime trait objects (`dyn Trait`).
- Bounded generic functions `<T: Trait>` are monomorphized for each distinct concrete type argument during semantic analysis, lowering to separate statically specialized MIR functions that invoke concrete trait methods directly.

---

## 8. Postfix `?` Error Propagation and Standard Library Lowering

### 8.1 Postfix `?` Operator Lowering
The postfix `?` operator on expression `inner?` is lowered into CFG branches with early return and drop scope cleanup:
1. **Operand Slot**: `inner` is evaluated into a dedicated local slot `_try_inner_<id>`.
2. **Discriminant Extraction**: The 32-bit tag is loaded via `LoadIndirect(%inner_ptr, Type::I32)` and compared against the success tag (`Some` or `Ok`) using `Binary(MirBinaryOp::Eq)`.
3. **Control-Flow Split**: `Terminator::CondBranch` branches to `try.success` or `try.error`.
4. **Success Block**:
   - The success payload offset is obtained from the enum variant layout.
   - For aggregate payloads (structs/enums), payload bytes are copied to a temporary result slot. For primitives, values are loaded indirectly.
   - Execution continues sequentially in `try.success`.
5. **Error Block (Early Return)**:
   - Allocates the function's return slot `_try_ret_<id>` with the enclosing function's return enum type (`Option` or `Result`).
   - Writes the error discriminant tag (`None` or `Err`) using `StoreIndirect32`.
   - If `Result`, copies the error payload from `inner` to the error return payload slot.
   - **Scope Unwinding & Drops**: Iterates in reverse over all active `drop_scopes`, finding still-owned non-moved variables, and invokes `emit_drop_for_place` for each.
   - Terminates with `Terminator::Return(Some(ret_ptr))`.

### 8.2 Inherent `Option` and `Result` Method Lowering
Built-in helper methods on `Option` and `Result` are inlined directly into MIR instructions without function call overhead:
- **`is_some` / `is_none` / `is_ok` / `is_err`**: Loads the 32-bit tag and performs an equality comparison with the variant tag.
- **`unwrap_or(default)`**: Evaluates into `unwrap_or.some`/`unwrap_or.ok` (copying/loading payload) and `unwrap_or.none`/`unwrap_or.err` (storing default value) into a pre-allocated result slot before merging.
- **`unwrap()`**: Branches on the tag: success path extracts payload; failure path loads panic message string slice, emits `vanta_panic`, and terminates with `Terminator::Unreachable`.

---

## 9. Iterator Protocols and `for` Loop Lowering

### 9.1 `for` Loop CFG Structure
The canonical loop construct `for var in iter_expr { body }` is lowered into structured basic blocks with explicit drop tracking:

```text
[Pre-loop Block]
  %iter_val = lower_expr(iter_expr)
  _for_iter = Slot(iter_ty)
  Store(_for_iter, %iter_val)
  Branch(for_cond)

[for_cond Block]
  %iter_ptr = SlotAddr(_for_iter)
  %next_res = Call("<IterTy>__Iterator_next", [%iter_ptr])
  %tag = LoadIndirect(%next_res, Type::I32)
  %is_some = Binary(Eq, %tag, 1)  // Option::Some tag == 1
  CondBranch(%is_some, for_body, for_exit)

[for_body Block]
  // Enter drop scope
  %payload = ExtractPayload(%next_res, elem_ty)
  _var = Slot(elem_ty)
  Store(_var, %payload)
  lower_stmts(body)
  // Exit drop scope (drops _var if non-Copy)
  Branch(for_cond)

[for_exit Block]
  // Post-loop continuation
```

### 9.2 Loop Control Flow & Scope Unwinding
- **`break`**: Drops all candidate variables declared within the current iteration body (scopes deeper than the loop scope) in LIFO order and emits `Terminator::Branch(for_exit)`.
- **`continue`**: Drops all candidate variables declared within the current iteration body in LIFO order and emits `Terminator::Branch(for_cond)`.
- **Single Evaluation**: `iter_expr` is lowered exclusively in the pre-loop block; repeated evaluations are impossible.
- **Static Dispatch**: The invocation of `next` statically dispatches to the monomorphized `Iterator` trait implementation for the concrete iterator type. Zero vtables or runtime trait objects are involved.

---

## 10. Lifetimes and Borrowed Vector Iteration Lowering

### 10.1 Lifetime Erasure in MIR
Lifetimes (`'a`, `'b`, `'static`) exist exclusively in the frontend type system and borrow checker. During MIR lowering:
- All lifetime annotations are erased: `&'a T` and `&'a mut T` lower directly to `Type::Ref { is_mut, inner }`.
- No lifetime tokens, epoch counters, or reference counts are emitted into MIR.
- Stack slot layouts and calling conventions for references remain pure 64-bit machine pointers.

### 10.2 Lowering of `vec.iter()`
When a method call `vec.iter()` is lowered:
1. Allocates a temporary stack slot `_iter_tmp` of type `Iter__<T>` (size 16 bytes, align 8).
2. Computes the address of the underlying vector buffer and stores it at byte offset 0 (`vec_ptr`).
3. Stores a 64-bit constant integer 0 at byte offset 8 (`index`).
4. Yields a pointer to `_iter_tmp` as the iterator value.

### 10.3 Synthetic Iterator Next Function Generation
When `vec.iter()` is lowered, the MIR lowerer dynamically generates the specialized next function `{Iter__T}_std_iter_Iterator_next(self: &mut Iter__T) -> Option<&'a T>`:
```text
[entry Block]
  %vec_ptr = LoadIndirect(%self + 0, Type::I64)
  %idx = LoadIndirect(%self + 8, Type::I64)
  %len = Call("vanta_vec_len", [%vec_ptr])
  %in_bounds = Binary(Lt, %idx, %len)
  CondBranch(%in_bounds, some_block, none_block)

[some_block Block]
  %elem_addr = Call("vanta_vec_index", [%vec_ptr, %idx])
  %next_idx = Binary(Add, %idx, 1)
  StoreIndirect(%self + 8, %next_idx)
  %res_some = ConstructEnum(Option, "Some", [%elem_addr])
  Return(%res_some)

[none_block Block]
  %res_none = ConstructEnum(Option, "None", [])
  Return(%res_none)
```
This zero-cost lowering translates directly into native x86-64 machine code via direct static calls, operating with zero virtual dispatch, zero runtime lifetime overhead, and zero heap allocation.

---

## 11. Owned String Lowering and Destruction

### 11.1 String MIR Representation
A `String` in MIR is represented as a 24-byte composite structure (`data_ptr: i64`, `len: i64`, `cap: i64`). Stack allocations allocate a 24-byte `SlotId` aligned to 8 bytes.

### 11.2 String Operation Lowering
1. **`String::new()`**:
   - Emits `Call("vanta_str_init", [dest_slot_addr])`.
   - Initializes `dest->data = NULL`, `dest->len = 0`, `dest->cap = 0`. Zero allocations.
2. **`String::from_str(s)`**:
   - Extracts `(src_ptr, src_len)` via slice lowering.
   - Emits `Call("vanta_str_from_str", [dest_slot_addr, src_ptr, src_len])`.
3. **`s.push_str(text)`**:
   - Extracts `(text_ptr, text_len)`.
   - Emits `Call("vanta_str_push_str", [s_addr, text_ptr, text_len])`.
4. **`s.as_str()` / `s.as_bytes()`**:
   - Loads `data_ptr` (offset 0) and `len` (offset 8) from `s_addr`.
   - Returns a 2-word slice value `(data_ptr, len)`.
5. **`s.clone()`**:
   - Emits `Call("vanta_str_clone", [dest_slot_addr, s_addr])`.
6. **`s.equals(other)`**:
   - Extracts pointers and lengths for both slices.
   - Emits `Call("vanta_str_equals", [s_data, s_len, other_ptr, other_len])`.
7. **`s.slice_bytes(start, end)`**:
   - Evaluates `start` and `end`.
   - Allocates a 24-byte `Option<&str>` stack slot (`tag: i32`, `_pad: i32`, `ptr: i64`, `len: i64`).
   - Emits `Call("vanta_str_slice_bytes", [opt_addr, s_data, s_len, start, end])`.

### 11.3 Destruction and Drop Scopes
- When a `String` slot exits its scope or is reassigned, `emit_drop_for_place` emits `Call("vanta_str_free", [slot_addr])`.
- In `Vec<String>`, `vanta_vec_free_strings` is invoked, iterating over all active strings and freeing each heap buffer before freeing the backing array.





