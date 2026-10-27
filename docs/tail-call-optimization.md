# Safe Tail-Call Optimization (TCO) in Vanta

**MIR Module:** [`crates/vanta_mir/src/opt/tail_call.rs`](file:///crates/vanta_mir/src/opt/tail_call.rs)  
**Codegen Lowering:** [`crates/vanta_codegen_x86_64/src/instruction_select.rs`](file:///crates/vanta_codegen_x86_64/src/instruction_select.rs)  
**CLI Switch:** `--no-tail-calls`  
**Introduced in:** Milestone 24

---

## 1. Overview and Objectives

Safe Tail-Call Optimization (TCO) provides two synergistic transformations in Vanta:

1. **Direct Self-Tail Recursion Elimination:** Transforms tail-recursive self-calls into an in-place $O(1)$ stack loop at the MIR level, completely avoiding call frame accumulation and preventing stack overflow.
2. **Sibling Direct Tail-Call Lowering:** Lowers tail-position calls to other functions into register moves, frame deallocation, and direct jump (`jmp`) instructions at the native machine-code level, reclaiming stack frames prior to invocation.

TCO guarantees that tail-recursive algorithms (such as accumulators, state machines, and parser loops) run with bounded, constant stack usage and minimal call/return overhead.

---

## 2. ABI Safety Invariants

Tail-call optimization requires strict adherence to the System V AMD64 ABI and Vanta's ownership model. An operation is eligible for tail-call optimization if and only if all of the following invariants hold:

### 2.1 Register Argument Constraint
The callee must take at most 6 scalar arguments:
$$\text{len}(\text{args}) \le 6 \quad (\%rdi, \%rsi, \%rdx, \%rcx, \%r8, \%r9)$$
Callees requiring stack-passed arguments ($> 6$ arguments) are rejected to prevent corrupting the caller's stack frame or overwriting return addresses.

### 2.2 Scalar and Void Returns Only
Both the caller and the callee must return either:
- A scalar register value (`i64`, `bool`, `enum`), or
- The `Unit` type (`void`).

Any call returning an aggregate via hidden memory pointer (`sret`) is rejected because the destination buffer belongs to the caller's stack or caller's parent.

### 2.3 Stack Pointer Escape Prohibition
A sibling tail call destroys the caller's stack frame *before* jumping to the callee. Therefore, **no argument may point into the caller's stack frame**.
The compiler computes the set of escaping stack addresses ($\text{stack\_ptrs}$) derived from:
- `Instruction::SlotAddr`
- `Instruction::IndexAddr`
- `Instruction::FieldAddr` / `Instruction::IndexPtrAddr`
- `Instruction::Copy` / `Instruction::Phi`

If any argument $a \in \text{args}$ satisfies $a \in \text{stack\_ptrs}$, sibling tail-call lowering is strictly prohibited.

### 2.4 Entry Function (`main`) Prohibition
Sibling tail-calls from `main` are prohibited (`caller.name != "main"`). The operating system runtime expects the return code of `main` in `%rax` (0 for void `main`). Jmp-tail-calling a void external function (such as `print_str`) from `main` would leave that function's internal return value in `%rax`, corrupting the process exit status.

---

## 3. Self-Tail Recursion Elimination (MIR Level)

Self-tail recursion is lowered directly inside [`TailCall::run_on_function`](file:///crates/vanta_mir/src/opt/tail_call.rs):

1. **Preheader Insertion:** An entry preheader block is synthesized (`.L_tco_preheader`) if the original entry block contains internal backedges.
2. **Loop Transformation:** The original entry block becomes the loop header.
3. **Parameter Updates:** In each block ending with `Call { callee == func.name, args, .. }` followed by `Return`:
   - Arguments are stored into the parameter slots:
     $$\text{Store } \{ \text{slot}: \text{params}[i], \text{val}: \text{args}[i] \}$$
   - The recursive `Call` and `Return` instructions are replaced with an unconditional `Terminator::Branch(header)`.
4. **Stack Guarantee:** The recursive call is converted into a standard loop that executes in $O(1)$ stack space.

---

## 4. Sibling Tail-Call Lowering (Machine Codegen Level)

When a function ends with a call to another function in tail position, [`InstructionSelector`](file:///crates/vanta_codegen_x86_64/src/instruction_select.rs) generates the following sequence:

```x86asm
# 1. Place arguments into System V AMD64 argument registers
mov %rdi, <arg0>
mov %rsi, <arg1>

# 2. Restore callee-saved registers (rbx, r12-r15)
lea -<callee_bytes>(%rbp), %rsp
pop %r15
pop %r14
pop %r13
pop %r12
pop %rbx

# 3. Dismantle caller stack frame
pop %rbp

# 4. Tail jump to callee
jmp <callee_symbol>@PLT
```

The tail jump is emitted as `MachineInst::TailJmp(callee)` and encoded in [`CodeEmitter::encode_instruction`](file:///crates/vanta_codegen_x86_64/src/encoding.rs) as an unconditional relative jump (`0xE9 rel32`) with relocation type `R_X86_64_PLT32`.

---

## 5. Verification and Acceptance

The implementation is verified by:
- Deep recursion tests executing 500,000 recursive self-tail calls in $O(1)$ stack memory without overflow ([`tco_tests.rs`](file:///crates/vanta_driver/tests/tco_tests.rs)).
- Sibling tail-calls verifying proper callee-saved register restoration and return value forwarding.
- Memory safety regression suites confirming stack address arguments are never passed to dismantled frames ([`string_tests.rs`](file:///crates/vanta_driver/tests/string_tests.rs)).
