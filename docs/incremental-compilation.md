# Venta v2.1 Incremental Compilation Architecture

## Overview

Milestone 32 introduces a deterministic, content-addressed, fine-grained incremental compilation pipeline for the Venta programming language. Rather than repeatedly re-parsing, analyzing, lowering, optimizing, compiling, and linking entire programs from scratch, `ventac` tracks fine-grained cryptographic fingerprints across source modules, functions, object files, and link configurations.

### Core Principles

1. **Clean-Build Equivalence:** Cached artifacts are only reused when their complete transitive dependency fingerprints match. Incremental outputs must be indistinguishable in semantics and behavior from clean builds.
2. **Zero Invalidation Leakage:** Pure body changes do not invalidate consumers that depend strictly on public signatures or layouts.
3. **Crash Safety & Atomic Writes:** Artifacts are staged in lock-protected temporary locations, validated with SHA-256 digests, and published via atomic directory renames.
4. **Target Isolation:** Build artifacts are strictly segregated by target triple (`x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`), preventing any cross-platform artifact reuse.

---

## Command-Line Interface

Incremental compilation can be explicitly controlled via the CLI:

```bash
# Enable incremental compilation (default for 'ventac build')
ventac build main.va --incremental

# Force a clean rebuild, bypassing the cache
ventac build main.va --no-incremental

# Specify a custom cache location (default: .venta/cache/v1)
ventac build main.va --cache-dir=.venta/cache

# Print detailed build metrics (cache hits, misses, units recompiled)
ventac build main.va --build-stats

# Explain exact reasons for any rebuilt units
ventac build main.va --explain-rebuild
```

### Cache Management Subcommands

```bash
# Inspect cache entry statistics and on-disk size
ventac cache stats [--cache-dir=<dir>]

# Verify SHA-256 integrity of all cached artifacts
ventac cache verify [--cache-dir=<dir>]

# Garbage-collect and clean cached artifacts
ventac cache clean [--target=<triple>] [--cache-dir=<dir>]
```

---

## Build Statistics Reporting

When `--build-stats` is specified, `ventac` outputs real build execution metrics:

```text
Venta incremental build:
  Source files:
    Total:                 1
    Unchanged:             1
    Changed:                0
  Objects:
    Reused:                 1
    Regenerated:            0
  Linking:
    Status:                 Skipped (no-op)
    Reason:                 No link inputs changed
  Cache:
    Hits:                  2
    Misses:                 0
    Corrupted recovered:    0
```

---

## Benchmarks & Performance Verification

Running `python3 benchmarks/run_incremental_benchmarks.py` on a 20-module project with 202 functions demonstrates substantial compilation throughput gains:

| Scenario | Execution Time (ms) | Speedup vs Clean | Work Performed |
|---|---|---|---|
| **Clean Build (Baseline, 202 funcs)** | 66.36 ms | 1.00x | Full parse, sema, MIR, opt, codegen, link |
| **Cold Incremental Build** | 71.88 ms | 0.92x | Full build + atomic cache & fragment publication |
| **Warm No-Op Incremental** | 16.54 ms | **4.01x** | Zero compilation, zero codegen, zero relink |
| **Single-Function Edit** | 72.85 ms | 0.91x | Only modified function recompiled, cached fragments reused |
| **Link-Only Rebuild** | 17.70 ms | **3.75x** | Object reuse, relinking final executable |
| **Corruption Recovery** | 72.87 ms | 0.91x | Detected corrupted checksum, clean rebuild |

---

## Milestone 32.5 Correctness & Function Fragment Integration

Milestone 32.5 hardens the incremental compilation engine with complete cache fingerprints, true function-level fragment caching, and deterministic multi-module build graphs:

### 1. Build Configuration & Compiler Identity
- **Dynamic Git Revision & Source Digest:** `vanta_build/build.rs` dynamically captures the working tree git revision (`git rev-parse HEAD`) or computes a source tree digest fallback, embedding it via `cargo:rustc-env=VENTA_BUILD_GIT_REV`.
- **Complete Configuration Fingerprinting (`BuildConfiguration`):** Captures all 26+ compiler flags including `opt_level`, `emit_target`, `debug_info`, `enable_reg_alloc`, `enable_tail_calls`, `enable_block_layout`, `enable_code_align`, `enable_mem2reg`, `enable_gvn`, `enable_inline`, `enable_ipa_constprop`, `enable_loop_unroll`, `enable_vectorize`, `enable_instruction_schedule`, `enable_global_dce`, and CPU feature flags (`avx`, `avx2`, `fma`, `avx512f`). Any configuration change deterministically invalidates stale machine code.

### 2. Content-Based Archive & Link Fingerprinting
- **Pure Content Hashing:** `compute_link_fingerprint` reads the full byte streams of all linked static archives (`.a` / `.lib`) and additional inputs using SHA-256 (`Domain::Archive`), detecting content modifications even when file names and byte sizes remain identical.
- **Link Record Verification:** `LinkRecord` validates executable file existence, exact byte length, and SHA-256 digest before declaring a link cache hit.

### 3. Build Graph & Manifest Persistence
- **Persistent Manifest (`manifest.json`):** Persisted in `.venta/cache/v1/<triple>/manifest.json`, recording compiler ID, configuration fingerprint, target triple, source file digests, module interface fingerprints, function signatures, and type layout hashes.
- **Two-Level Fast Path:** Warm builds check the manifest directly, bypassing module tree parsing and semantic analysis when all inputs and compiler options are unchanged.

### 4. Function Fragment Caching & Object Reassembly
- **Granular Codegen Units (`FunctionFragment`):** Functions are compiled into relocatable binary fragments containing machine code, relative relocations, and DWARF 5 line entries.
- **Object Reassembly:** `assemble_object_from_fragments` stitches cached and freshly compiled fragments into aligned `.text` sections, shifts relocations and debug lines, constructs `.rodata` and vtables, and emits compliant ELF64 and PE/COFF objects.
- **Deterministic Hashing:** Structs and enums are sorted deterministically by symbol name before computing body fingerprints, eliminating HashMap iteration non-determinism across processes.

### 5. Invalidation Test Matrix (Tests A through J)
All 10 invalidation scenarios specified in Milestone 32.5 are verified in `crates/vanta_driver/tests/incremental_tests.rs`:
- **Test A (No-op):** 0 function compilations, 0 object regenerations, 0 relinks.
- **Test B (Private Function Edit):** Modifying a private helper recompiles only that function fragment; unrelated functions are reused from cache.
- **Test C (Public Signature Change):** Signature modification invalidates callers and triggers dependent semantic checks and recompilations.
- **Test D (Generic Template Change):** Template body modification invalidates affected concrete instantiations.
- **Test E (Type Layout Change):** Struct field addition changes layout fingerprint, invalidating dependent accessors.
- **Test F (Trait Implementation Change):** Modifying trait methods updates vtables and dynamic dispatch.
- **Test G (Link-Only Rebuild):** Deleting executable artifact triggers relink while reusing cached objects.
- **Test H (Same-Size Archive Change):** Content-based hash invalidates link when archive content changes despite identical size.
- **Test I (Compiler Identity Change):** Incompatible compiler ID or schema bump cleanly triggers clean rebuild.
- **Test J (Target Feature Change):** Toggling AVX2 invalidates cached machine code and regenerates with compatible instructions.
