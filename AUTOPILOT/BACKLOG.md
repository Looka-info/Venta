# Engineering Backlog

## P0 — Critical Correctness and Safety

### [RESOLVED-001] ELF Static Runtime `vanta_vec_free` / `vanta_free` Corrupted Machine Code
- **Severity**: P0 (Invalid executable output / SIGILL on Linux)
- **Root Cause**: Hand-calculated instruction offsets and displaced relative jumps in `crates/vanta_linker/src/elf/runtime.rs`.
- **Resolution**: Replaced incorrect hardcoded displacements with exact instruction offsets and dynamic relative relocations.
- **Verification Method**: `cargo test -p vanta_driver --test collections_tests`.
- **Status**: Closed (Verified).

---

### [RESOLVED-002] WSL2 / Windows Host `.cargo/config.toml` Path Collision
- **Severity**: P0 (Build system failure)
- **Root Cause**: Cargo ascended into Windows user directory with Windows drive letter in `target-dir`.
- **Resolution**: Added workspace `.cargo/config.toml` directing `target-dir` to `/home/mumer/.cargo_target`.
- **Verification Method**: `cargo check --workspace` and `cargo test --workspace`.
- **Status**: Closed (Verified).

---

### [RESOLVED-004] ELF Dynamic Symbol Interposition on C-ABI `__errno_location`
- **Severity**: P0 (Silent failure / Network connection failures across dynamic libraries)
- **Root Cause**: Weak stub `__errno_location` in `runtime/vanta_rt.c` interposed glibc TLS errno.
- **Resolution**: Removed `__errno_location` from `runtime/vanta_rt.c` and recompiled runtime archive.
- **Verification Method**: `cargo test -p vanta_driver --test ffi_tests`.
- **Status**: Closed (Verified).

---

### [RESOLVED-007] Broken Release Packaging & Installer Template Scaffolding
- **Severity**: P0 / P1 (Broken installation / Template generation failure / Cross-platform path crash)
- **Root Cause**: `find_templates_root` relied on hardcoded Windows OneDrive path; `package_release.sh` and `install.sh` omitted `ventanext-gen` and `templates/`.
- **Resolution**: Implemented multi-tier cross-platform discovery; bundled `ventanext-gen` and templates in release package and installer with full manifest tracking.
- **Verification Method**: `cargo test -p vanta_driver --test packaging_tests`.
- **Status**: Closed (Verified).

---

## P1 — Major Functionality & Compiler Correctness

### [RESOLVED-003] Standard Library Collections End-to-End Dynamic Operations
- **Severity**: P1
- **Description**: Provide full dynamic CRUD operations on `std::collections::HashMap` (`insert`, `get`, `contains_key`, `remove`, `clear` with tombstone slot reuse).
- **Resolution**: Implemented linear probing with tombstone slot reuse in `library/std/collections/hash_map.va`. Added `test_hash_map_crud_and_reinsertion` under `-O0` and `-O1`.
- **Verification Method**: `cargo test -p vanta_driver --test collections_tests`.
- **Status**: Closed (Verified).

---

### [RESOLVED-008] Standard Library Networking (`std::net`) Verification and Client Sockets
- **Severity**: P1 (Missing TCP client connection / unvalidated port range in socket primitives)
- **Root Cause**: Missing `vanta_net_connect` primitive; missing port bounds check in `vanta_net_listen`.
- **Resolution**: Implemented `vanta_net_connect` with address parsing/DNS resolution; added port bounds checks; exported `std::net::connect`; created `net_tests.rs`.
- **Verification Method**: `cargo test -p vanta_driver --test net_tests` (4/4 passed).
- **Status**: Closed (Verified).

---

## P2 — Reliability and Quality

### [RESOLVED-005] Clean Workspace Clippy Warnings & Warnings Gate
- **Severity**: P2
- **Resolution**: Refactored minor warnings across all 15 crates and integration test suites.
- **Verification Method**: `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- **Status**: Closed (Verified).

---

### [RESOLVED-006] Transient `ETXTBUSY` on Rapid Binary Execution in WSL2
- **Severity**: P2
- **Resolution**: Added retry backoff on `ErrorKind::ExecutableFileBusy` in test harnesses.
- **Verification Method**: Parallel test runs pass consistently.
- **Status**: Closed (Verified).

---

## P3 — Performance and Optimization

### [RESOLVED-009] Native Linker Contribution Scanning and Relocation Hot Path
- **Severity**: P3 (Linear scan in inner relocation loop: $O(R \times C)$ quadratic overhead)
- **Evidence**: In `elf/linker.rs`, `coff/linker.rs`, and `macho/linker.rs`, applying each relocation performed a linear scan over all `merged_sections` and an inner linear scan over all `merged_sec.contributions`. In `resolve.rs`, `resolve_symbol_address` performed two passes over all merged sections and contributions for every local symbol or section reference.
- **Root Cause**: Lack of an indexed contribution table mapping `(object_id, source_section_idx)` and `(object_id, section_name)` directly to `(merged_section_idx, offset_in_merged)`.
- **Affected Files**:
  - `crates/vanta_linker/src/section.rs`
  - `crates/vanta_linker/src/resolve.rs`
  - `crates/vanta_linker/src/elf/linker.rs`
  - `crates/vanta_linker/src/coff/linker.rs`
  - `crates/vanta_linker/src/macho/linker.rs`
- **Resolution**:
  - Implemented `ContributionLoc` and `ContributionMap` in `section.rs`, providing $O(1)$ section and contribution lookups.
  - Added `resolve_symbol_address_with_map` in `resolve.rs` to bypass linear contribution scans during symbol virtual address computation.
  - Replaced $O(C)$ linear scans in the relocation application loops across ELF, PE/COFF, and Mach-O linkers with $O(1)$ direct indexing into `merged_sections`.
- **Verification Method**: `cargo test -p vanta_linker` (18/18 passed), `cargo test -p vanta_driver --test linker_e2e_tests` (6/6 passed), `cargo run --release --example bench_linker -p vanta_linker`.
- **Status**: Closed (Verified).

---

### [RESOLVED-010] VentaNext Live Dev Server RFC 6455 Framing & Decoupled Non-Blocking Broadcast
- **Severity**: P3 / P2 (Reliability, RFC compliance, and concurrency blocking)
- **Root Cause**:
  - `dev_server.rs` lacked RFC 6455 frame decoding, masking/unmasking, and Ping/Pong/Close control frame handling.
  - `handle_client_connection` called `stream.try_clone().unwrap()` which panicked worker threads upon rapid client disconnections.
  - `LiveReloadBridge::broadcast` performed network writes while holding the global mutex, creating head-of-line blocking for connecting clients.
  - File watcher reallocated and destroyed `HashMap<PathBuf, SystemTime>` every 80ms without scanning filters for build/temp directories.
- **Resolution**:
  - Implemented RFC 6455 frame decoder `decode_websocket_frame` supporting 7-bit, 16-bit, and 64-bit payload lengths, 4-byte client unmasking, and control frame validation.
  - Implemented unified frame encoder `encode_websocket_frame` with dedicated helpers for Ping, Pong, and Close frames.
  - Replaced panicking `unwrap()` with non-panicking `try_clone()` error recovery and 5-second HTTP request parsing timeouts.
  - Decoupled `broadcast` network writes: drained client snapshot outside the mutex, set 500ms write timeouts and `TCP_NODELAY`, pruned closed connections, and merged surviving clients lock-free.
  - Optimized file watcher with directory exclusion filters (`IGNORED_DIRS`) and zero-allocation hash map swapping (`std::mem::swap`).
  - Added background client frame listener threads responding to Ping frames with Pong frames and cleanly servicing Close frames.
  - Added `start_dev_server_background` helper and 8 integration tests covering RFC 6455 vectors, status endpoints, WebSocket handshake, and live reload broadcasting.
- **Verification Method**: `cargo test -p ventanext_codegen` (23/23 unit tests passed, 107/107 discovery tests passed, total 130 tests).
- **Status**: Closed (Verified).

---

### [RESOLVED-011] Compiler SSA ValueId Remapping Exhaustiveness in MIR Inliner, Loop Unroller, and Loop Peeler
- **Severity**: P0 / P1 (Compiler correctness / Invalid SSA / Duplicate ValueId definitions)
- **Evidence**:
  - In `crates/vanta_mir/src/opt/inlining.rs`, Step 4 destination remapping manually matched 15 instruction variants but omitted `Instruction::CallIndirect` and `Instruction::VtableAddr`.
  - In `crates/vanta_mir/src/opt/loop_unroll.rs` (3 locations) and `crates/vanta_mir/src/opt/loop_peel.rs` (2 locations), identical hand-rolled match arms omitted `CallIndirect` and `VtableAddr`.
  - When inlining or unrolling loops containing indirect calls or virtual table lookups, duplicated instructions retained original callee `ValueId`s, causing SSA collisions in the caller.
- **Resolution**:
  - Replaced manual, incomplete match statements across `inlining.rs`, `loop_peel.rs`, and `loop_unroll.rs` with canonical calls to `inst.defs()`.
  - Ensured all defined SSA values are systematically and exhaustively assigned fresh, unique `ValueId`s across all CFG duplications.
  - Added `test_inline_callee_with_indirect_call_and_vtable_ssa_integrity` verifying SSA uniqueness and `MirVerifier` validation on polymorphic callee inlining.
- **Verification Method**: `cargo test -p vanta_driver --test inlining_tests` (12/12 passed), `cargo test -p vanta_driver --test loop_peel_tests --test loop_unroll_tests --test loop_unroll_verification_tests` (14/14 passed), `cargo test -p vanta_mir` (13/13 passed).
- **Status**: Closed (Verified).

---

### [TASK-012] DWARF Debug Line and Symbol Info Generation Review in x86_64 & AArch64 Backends
- **Severity**: P2
- **Description**: Audit DWARF .debug_line and .debug_info emission in `crates/vanta_codegen_x86_64` and `crates/vanta_codegen_aarch64` to verify source location mappings, stepping accuracy in GDB/LLDB, and stack unwinding frame descriptions (.eh_frame / CFI).
- **Affected Files**: `crates/vanta_codegen_x86_64/src/frame.rs`, `crates/vanta_codegen_aarch64/src/frame.rs`
- **Verification Method**: Debugger stepping and CFI layout integration tests.
- **Status**: Ready.
