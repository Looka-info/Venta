# Venta Build Graph Model

## Overview

The `vanta_build` crate implements an explicit dependency graph representing every input, intermediate compilation representation, and final output artifact within the Venta compilation pipeline.

---

## Graph Node Hierarchy

Every node in the `BuildGraph` has a stable, deterministic `NodeId`:

| Node Kind | Example Identity | Description |
|---|---|---|
| `Workspace` | `ws:workspace` | Top-level project container |
| `SourceFile` | `src:main.va` | Source file path and content fingerprint |
| `Module` | `mod:math` | Module declaration and interface fingerprint |
| `Declaration` | `decl:math::PI` | Module-level constant or variable declaration |
| `TypeInterface` | `type:Point` | Struct or enum layout (size, alignment, field offsets) |
| `GenericTemplate` | `tmpl:identity` | Generic AST/MIR template definition |
| `ConcreteInstantiation` | `inst:identity<i64>` | Monomorphized specialization for concrete types |
| `Function` | `fn:main::compute` | Function body and signature fingerprints |
| `CodegenFragment` | `frag:x86_64:compute` | Machine code instructions, relocations, local symbols |
| `ObjectFile` | `obj:x86_64:main.o` | Target relocatable object file (`.o` or `.obj`) |
| `Archive` | `arch:x86_64:librt.a` | Static archive with member index |
| `Executable` | `exe:x86_64:app` | Linked executable binary image |

---

## Dependency Kinds & Edge Invalidation

Edges in the graph carry explicit semantic strengths:

- **`Body`**: Consumer executes or inlines provider implementation.
  - *Invalidation rule:* Changing a function's body invalidates only `Body`, `Codegen`, and `Link` dependents. Callers depending solely on `Signature` remain valid.
- **`Signature`**: Consumer relies on parameter types, return types, or calling conventions.
  - *Invalidation rule:* Changing a signature invalidates all calling sites and dependent ABI boundaries.
- **`TypeLayout`**: Consumer embeds or accesses struct/enum fields.
  - *Invalidation rule:* Adding, removing, or reordering fields invalidates dependent layouts and all functions calculating offsets.
- **`TraitResolution`**: Consumer performs static method dispatch or constructs dynamic vtables.
  - *Invalidation rule:* Adding or modifying trait implementations updates affected call sites and vtables.
- **`GenericTemplate`**: Concrete instantiation depends on AST/MIR template code.
  - *Invalidation rule:* Template changes propagate to all concrete instantiations.
- **`Codegen`**: Machine code depends on MIR, optimization level, and CPU features.
- **`Link`**: Executable depends on object members, archives, and link configuration.
- **`Debug`**: DWARF 5 tables depend on source paths and debug flags.

---

## Cycle Handling & Traversal

1. **Deterministic Topological Sorting:** Uses a Kahn-style topological sort with lexicographical BTree tie-breaking to guarantee deterministic execution orders.
2. **Recursive Function Safety:** Mutual recursion between functions is recognized and permitted via `Body` and `Signature` edges without being falsely flagged as illegal cyclic compilation graphs.
3. **Stale Node Pruning:** Active node tracking ensures that deleted source files or removed functions have their corresponding nodes and reverse dependency edges safely pruned.
