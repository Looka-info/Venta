# Venta Content-Addressed Compiler Cache

## Overview

The Venta compiler cache persists intermediate and final artifacts on disk under a content-addressed, cryptographic structure.

---

## Directory Structure

By default, the cache is located in `.venta/cache/v1/`, namespaced by the compilation target triple:

```text
.venta/
  cache/
    v1/
      x86_64-unknown-linux-gnu/
        objects/       # Native relocatable objects (.o / .obj)
        interfaces/    # Module interface declarations (.vif)
        mir/           # Lowered and optimized MIR units (.vmir)
        functions/     # Reusable codegen function fragments (.vfn)
        archives/      # Generated static archives (.a / .lib)
        links/         # Link executable records (.link)
        metadata/      # Entry metadata and checksum manifests (.json)
        locks/         # Crash-safe atomic write staging directory
      x86_64-pc-windows-msvc/
        ...
      aarch64-apple-darwin/
        ...
```

---

## Cryptographic Fingerprint Algorithm

- **Algorithm:** Pure-Rust, FIPS 180-4 compliant SHA-256 implementation.
- **Domain Separation:** Every hash input is prefixed with a domain tag (e.g., `VENTA:SRC:v1`, `VENTA:OBJECT:v1`, `VENTA:LINK:v1`), preventing cross-artifact hash collisions.
- **Compiler Identity:** The fingerprint incorporates:
  - Compiler package version (`env!("CARGO_PKG_VERSION")`)
  - Cache schema version (`CACHE_SCHEMA_VERSION = 1`)
  - Git commit revision (`VENTA_GIT_REV`)
  - Target triple (`target_spec.triple`)
  - Optimization level (`-O0`, `-O1`)
  - CPU feature flags (`+avx2`, `+avx512f`, etc.)
  - Debug information flag (`-g`)

---

## Atomic Storage & Corruption Recovery

### Write Safety
1. New artifacts are written to temporary unique files in `locks/tmp_<key>_<pid>_<timestamp>.dat`.
2. Artifact bytes are flushed to disk.
3. Cryptographic checksum and metadata manifest are generated in `locks/tmp_<key>_<pid>_<timestamp>.meta`.
4. Staged files are moved via atomic rename (`fs::rename`) into their final destinations in `objects/` and `metadata/`.

### Validation & Recovery
When reading an artifact:
1. `metadata/<key>.json` is inspected for schema and compiler identity matches.
2. The data payload length is validated against `artifact_size`.
3. The SHA-256 digest of the data file is verified against `artifact_checksum`.
4. If truncation, bit rot, or schema mismatch is detected, the corrupted files are immediately removed, the entry is recorded as a cache miss, and the compiler cleanly regenerates the artifact from source without emitting corrupted machine code.
