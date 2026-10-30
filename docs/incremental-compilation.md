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

Running `python3 benchmarks/run_incremental_benchmarks.py` on a multi-module benchmark demonstrates substantial compilation throughput gains:

| Scenario | Execution Time (ms) | Speedup vs Clean | Work Performed |
|---|---|---|---|
| **Clean Build (Baseline)** | 70.32 ms | 1.00x | Full parse, sema, MIR, opt, codegen, link |
| **Cold Incremental Build** | 72.32 ms | 0.97x | Full build + atomic cache publication |
| **Warm No-Op Incremental** | 17.41 ms | **4.04x** | Zero compilation, zero codegen, zero relink |
| **Single-Function Edit** | 72.01 ms | 0.98x | Invalidation of edited unit + relink |
| **Link-Only Rebuild** | 18.94 ms | **3.71x** | Object reuse, relinking final executable |
| **Corruption Recovery** | 71.63 ms | 0.98x | Detected corrupted checksum, clean rebuild |
