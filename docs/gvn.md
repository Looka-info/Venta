# Global Value Numbering (GVN) and Common Subexpression Elimination (CSE) in Vanta MIR

## 1. Overview

Milestone 19 introduces a formal, deterministic **Global Value Numbering (GVN)** and **Common Subexpression Elimination (CSE)** optimization pass for Vanta's SSA-based Mid-Level Intermediate Representation (MIR).

Building upon the SSA form introduced by Mem2Reg (Milestone 18) and physical register allocation (Milestone 17), GVN detects identical computations across dominating basic blocks, eliminates redundant calculations, and replaces them with previously computed values before instruction selection and register allocation.

```
       ┌───────────────────┐
       │     AST / Sema    │
       └─────────┬─────────┘
                 │
       ┌─────────▼─────────┐
       │    MIR Lowerer    │
       └─────────┬─────────┘
                 │
       ┌─────────▼─────────┐
       │      Mem2Reg      │ (SSA Construction: Phis & Copies)
       └─────────┬─────────┘
                 │
   ┌─────────────┴─────────────┐
   │ Fixed-Point Opt Loop      │
   │ ┌───────────────────────┐ │
   │ │ Constant Propagation  │ │
   │ └───────────┬───────────┘ │
   │ ┌───────────▼───────────┐ │
   │ │        GVN / CSE      │ │ ◄── [NEW: Dominator-Tree Scoped Value Numbering]
   │ └───────────┬───────────┘ │
   │ ┌───────────▼───────────┐ │
   │ │ Dead Code Elimination │ │
   │ └───────────────────────┘ │
   └─────────────┬─────────────┘
                 │
       ┌─────────▼─────────┐
       │ Machine IR / ISel │
       └─────────┬─────────┘
                 │
       ┌─────────▼─────────┐
       │ Linear Scan RegAlloc │
       └─────────┬─────────┘
                 │
       ┌─────────▼─────────┐
       │   ELF64 Emission  │
       └───────────────────┘
```

---

## 2. Core Architecture and Algorithmic Design

GVN in Vanta operates directly on the control-flow graph (CFG) and dominator tree computed in [`crates/vanta_mir/src/opt/dom.rs`](file:///crates/vanta_mir/src/opt/dom.rs).

### 2.1 Dominator-Tree Preorder Traversal

To guarantee that any substituted expression is available at the use point without violating SSA dominance invariants:
1. GVN traverses basic blocks in **preorder** along the Dominator Tree starting from the function entry block.
2. A scoped **Available Expressions Table** (`HashMap<GvnExpr, ValueId>`) tracks expressions computed along the current dominance path.
3. When descending into a child block in the dominator tree, all expressions from dominating ancestor blocks remain available.
4. When exiting a basic block subtree, a reverse LIFO rollback stack (`Vec<(GvnExpr, Option<ValueId>)>`) restores previous entries or evicts newly registered expressions.

This mathematical property guarantees that an expression is available at point $P$ if and only if its definition **dominates** $P$.

---

### 2.2 Canonical Expression Representations (`GvnExpr`)

The compiler numbers and indexes pure computations using [`GvnExpr`](file:///crates/vanta_mir/src/opt/gvn.rs):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GvnExpr {
    ConstInt { val: i64, ty: Type },
    ConstBool { val: bool },
    Binary { op: MirBinaryOp, lhs: ValueId, rhs: ValueId, ty: Type },
    Unary { op: MirUnaryOp, operand: ValueId, ty: Type },
    Phi { block: BlockId, inputs: Vec<(BlockId, ValueId)>, ty: Type },
    Load { slot: SlotId },
    SlotAddr { slot: SlotId },
    FieldAddr { base_ptr: ValueId, offset: usize },
    IndexAddr { slot: SlotId, index: ValueId, elem_size: usize },
    IndexPtrAddr { base_ptr: ValueId, index: ValueId, elem_size: usize },
}
```

---

### 2.3 Commutative and Comparison Dual Normalization

To eliminate syntactically distinct but semantically identical operations, GVN applies strict canonical normalization:

1. **Commutative Operations:**
   For commutative binary operations (`Add`, `Mul`, `BitAnd`, `BitOr`, `BitXor`, `Eq`, `NotEq`, `And`, `Or`), operands are ordered deterministically by canonical `ValueId`:
   $$\text{lhs} \le \text{rhs}$$
   If $\text{lhs.0} > \text{rhs.0}$, the operands are swapped.
   - Example: `x + y` and `y + x` produce the exact same key `Binary(Add, min(x, y), max(x, y))`.

2. **Comparison Duals:**
   Greater-than comparisons are converted into their equivalent less-than counterparts:
   $$x > y \iff y < x$$
   $$x \ge y \iff y \le x$$
   - Example: `a > b` and `b < a` both normalize to `Binary(Lt, b, a)`.

3. **Constant Unification:**
   Integer and boolean constants are canonicalized so that identical literal values across the function share the same value number, enabling compound subexpressions such as `x * 13` and `13 * x` to recognize identical operands.

---

### 2.4 Read-Only Slot Load Elimination

Local memory variables modified in loops cannot be cached across control-flow joins without explicit SSA $\phi$ nodes. However, function parameters and local variables that are **never stored to anywhere in the function** are referentially transparent.

GVN audits function-wide store and address-of operations upfront:
$$\text{written\_slots} = \{ s \mid \exists \text{ Store}(s, \cdot) \lor \text{ StoreSlice}(s, \cdot) \lor \text{ SlotAddr}(s) \lor \text{ IndexAddr}(s, \cdot) \}$$

If $s \notin \text{written\_slots}$:
- `Load(s)` is treated as a pure, immutable expression.
- Repeated loads of parameter $s$ across the function or in nested loops are eliminated and replaced with the dominating loaded value.

---

### 2.5 SSA Copy Propagation and Phi Simplification

1. **Copy Elimination:**
   When encountering `Instruction::Copy { dest, src, .. }`, GVN records `set_leader(dest, find_leader(src))`. All downstream uses of `dest` are substituted with `src`, and DCE eliminates the dead copy.

2. **Trivial Phi Simplification:**
   For any `Instruction::Phi { dest, inputs, .. }`:
   $$\text{non\_self} = \{ \text{find\_leader}(v) \mid (\cdot, v) \in \text{inputs} \land \text{find\_leader}(v) \neq \text{dest} \}$$
   If $|\text{non\_self}| = 1$ (say $\{ V \}$), the $\phi$ node is redundant and simplified to $V$.

3. **Identical Phi Elimination:**
   Within the same basic block, if two phis have identical predecessor-value inputs, the second phi is replaced by the first.

---

## 3. Disassembly Verification

### 3.1 Workload: `arith_tree`
In `eval_tree(x, y)`:
```vanta
let a = (x + y) * (x - y) + (x * 3) - (y * 5);
let b = (a + x) * 2 - (a - y) * 3 + (x + y);
```

#### Without GVN (`--no-gvn`):
```x86asm
4012ee:  mov    -0x10(%rbp),%r8    # reload x
4012f2:  mov    -0x18(%rbp),%rsi   # reload y
4012f6:  mov    %r8,%r15
4012f9:  add    %rsi,%r15          # redundant recomputation of (x + y)
```

#### With GVN (Full Opt):
```x86asm
401272:  add    %r8,%rsi           # compute (x + y) once into %rsi
...
4012db:  add    %rsi,%rdi          # reuse %rsi directly! Zero reload, zero recomputation!
```
- **Assembly size:** Function shrunk from 245 bytes to 209 bytes.
- **Speedup:** 1.10x faster execution.

---

### 3.2 Targeted Workload: `cse_workload`
In `cse_kernel(x, y)`:
```vanta
let t1 = x * 13 + y * 17;
let t2 = y * 17 + x * 13;
let t3 = (x * 13 + y * 17) ^ (x * 13);
let t4 = t1 + t2 + t3;
let t5 = (x * 13 + y * 17) * 3;
let t6 = (y * 17 + x * 13) - 5;
return t4 + t5 + t6;
```

#### Result:
- **Instruction count:** Dropped from 70 instructions down to 30 instructions.
- **Runtime:** Dropped from **0.1002s** down to **0.0526s** (**1.91x speedup**).

---

## 4. Compiler CLI Usage

GVN is enabled by default at `-O1`. To disable GVN independently for differential testing or debugging:

```bash
# Compile with GVN disabled
vantac -O1 --no-gvn input.va -o bin_no_gvn

# Run with GVN disabled via toolchain runner
vanta run --no-gvn input.va

# Check compiler help
vantac --help
```
