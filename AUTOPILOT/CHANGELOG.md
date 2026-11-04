# Autopilot Changelog

## [Unreleased] - Iteration 6

### Optimized
- **Native Linker Section Contribution Indexing & Relocation Throughput**:
  - Implemented `ContributionLoc` and `ContributionMap` in `crates/vanta_linker/src/section.rs` with dual index lookups by `(object_id, source_section_idx)` and `(object_id, section_name)`.
  - Added `resolve_symbol_address_with_map` in `crates/vanta_linker/src/resolve.rs` for $O(1)$ symbol address lookup.
  - Replaced $O(R \times C)$ quadratic linear contribution searches across ELF (`elf/linker.rs`), PE/COFF (`coff/linker.rs`), and Mach-O (`macho/linker.rs`) relocation loops with instant $O(1)$ direct slice indexing.
  - Accelerated `compute_symbol_addresses` object lookups using direct index access.
  - Verified across all linker unit, fuzz, layout, and driver E2E tests (100% pass).

---

## [Unreleased] - Iteration 5

### Added
- **Standard Library Networking Client TCP Sockets (`std::net::connect`)**:
  - Implemented `vanta_net_connect(host, port)` in `runtime/vanta_rt.c` with IPv4 numeric address conversion and DNS hostname resolution fallback.
  - Registered `vanta_net_connect` built-in signature in `crates/vanta_sema/src/lib.rs`.
  - Exported `pub fn connect(host: &str, port: i64) -> i64` in `library/std/net.va`.
  - Added new integration test suite `crates/vanta_driver/tests/net_tests.rs` with 4 test scenarios: client data transfer against mock server, full Vanta TCP server echo, closed-port error reporting, and invalid port bounds under both `-O0` and `-O1`.

### Fixed
- **Socket Port Range Bounds Validation in C Runtime**:
  - Added bounds check `port <= 0 || port > 65535` in `vanta_net_listen()` in `runtime/vanta_rt.c`, preventing negative port values from being erroneously cast to unsigned short (e.g. port 65535) and binding unexpectedly.
  - Rebuilt runtime archive via `scripts/build_runtime.sh`.

---

## [Unreleased] - Iteration 4

### Added
- **Release Packaging & Installer Support for VentaNext Generator & Templates**:
  - Added `ventanext-gen` compilation and packaging to `scripts/package_release.sh`.
  - Added packaging of all official VentaNext website templates to `share/ventanext/templates/` with verified checksum manifests.
  - Added `ventanext-gen` and template tree installation to `scripts/install.sh` with manifest logging.
  - Added complete cleanup for generator binaries and template trees in `scripts/uninstall.sh`.
  - Added end-to-end integration tests in `crates/vanta_driver/tests/packaging_tests.rs` verifying that `vanta templates` and `vanta new` execute and scaffold complete projects from an isolated installed prefix.

### Fixed
- **Cross-Platform Template Root Discovery**:
  - Replaced hardcoded Windows OneDrive path in `crates/ventanext_codegen/src/main.rs` with `find_templates_root` in `template_registry.rs`.
  - Implemented multi-tier discovery: explicit arguments, environment variables (`VENTANEXT_TEMPLATES_DIR`, `VANTA_TEMPLATES_DIR`), sibling `share/ventanext/templates`, CWD ancestor tree search, user data directory (`~/.local/share/ventanext/templates`), and compile-time manifest fallback.
  - Verified with `test_find_templates_root_resolution` in `ventanext_codegen`.

---

## [Unreleased] - Iteration 3

### Added
- **Standard Library `HashMap` Full Dynamic CRUD Operations**:
  - Implemented open addressing with linear probing and tombstone slot reuse (`BucketState::Deleted`) in `library/std/collections/hash_map.va`.
  - Added public methods: `insert`, `get`, `contains_key`, `remove`, `clear`.
  - Re-exported full collections API in `library/std/collections/mod.va`.
  - Added comprehensive end-to-end integration tests in `crates/vanta_driver/tests/collections_tests.rs` verifying insertion, retrieval, deletion, tombstone reuse, and clearing under `-O0` and `-O1`.

### Fixed
- **ELF Dynamic Symbol Interposition on C-ABI Functions**:
  - Removed dummy weak `__errno_location()` and `vanta_errno_val` from `runtime/vanta_rt.c`.
  - Rebuilt `runtime/libvanta_rt.a` and `runtime/x86_64-unknown-linux-gnu/libvanta_rt.a`.
  - Fixed Curl HTTP connection failures (`CURLE_COULDNT_CONNECT`, code 7) caused by `libcurl.so` binding non-blocking socket error queries to the local zero variable instead of glibc's TLS `errno`.
  - Added TCP connection handling in `ffi_tests.rs` to filter out 0-byte probe packets from WSL2 port forwarding.
- **WSL2 Concurrent Test Runner `ETXTBUSY` Flakiness**:
  - Added retry logic with backoff on `std::io::ErrorKind::ExecutableFileBusy` in `mem2reg_tests.rs` and `differential_tests.rs` to handle filesystem flush latency during parallel test runs.
- **Workspace-wide Clippy Cleanliness**:
  - Resolved all linter warnings across all 15 crates and tests to achieve 100% compliance with `cargo clippy --workspace --all-targets -- -D warnings`.

---

## [Unreleased] - Iteration 1

### Fixed
- **ELF Static Runtime Machine Code Corruption (SIGILL)**:
  - Corrected `call_vfree_pos` in `crates/vanta_linker/src/elf/runtime.rs` from `vec_free_offset + 10` to `vec_free_offset + 14`, preventing the 4-byte call displacement from overwriting preceding instructions into illegal opcodes.
  - Corrected branch jump offsets in `vanta_vec_free` (`0x0C` -> `0x0D`), `vanta_free` (`0x18` -> `0x12`), `vanta_vec_init` (`0x18` -> `0x17`), and `print_str` (`0x14` -> `0x17`, `0x0F` -> `0x12`).
  - Hardened `vanta_realloc` null-pointer allocation path with a dynamic internal call relocation instead of a fragile relative jump, and fixed the failure path branch offset (`0x22` -> `0x1F`) to correctly preserve the `%rbx` register stack frame.
  - Verified fix with `collections_tests` executing cleanly with 6/6 tests passing.

- **WSL2 Cargo Build Configuration**:
  - Configured workspace-level `.cargo/config.toml` targeting `/home/mumer/.cargo_target` to eliminate Windows drive letter `:` delimiter collision in `$LD_LIBRARY_PATH`.
  - Added `.cargo/` to `.gitignore`.

---

## [Iteration 7] — 2026-11-04

### Added
- **RFC 6455 WebSocket Framing & Decoded Streaming**:
  - Implemented `WebSocketOpcode`, `WebSocketFrame`, and `WebSocketError`.
  - Implemented `decode_websocket_frame` with streaming buffer consumption, 7-bit/16-bit/64-bit payload parsing, and 4-byte client unmasking.
  - Implemented unified `encode_websocket_frame` and helpers for Text, Binary, Ping, Pong, and Close frames.
  - Added background client frame reader threads in `add_websocket_client` responding to Pings and handling Close frames.
- **VentaNext Dev Server Background Runner**:
  - Implemented `start_dev_server_background` returning `(LiveReloadBridge, JoinHandle)` for graceful lifecycle management on ephemeral ports.
- **Unit and Integration Tests**:
  - Added 8 dev server tests verifying RFC 6455 canonical test vectors, control frames, partial streaming, and loopback WebSocket handshakes.

### Improved
- **Broadcast Concurrency & Deadlock Elimination**:
  - Decoupled `LiveReloadBridge::broadcast` network writes from the client mutex via `std::mem::take`, eliminating head-of-line blocking for connecting clients.
  - Configured `TCP_NODELAY` and 500ms write timeouts on all client connections.
- **File Watcher Heap Allocation Efficiency**:
  - Replaced per-tick `HashMap` allocation in `start_file_watcher` with zero-allocation buffer swapping (`std::mem::swap`).
  - Added `IGNORED_DIRS` filter ignoring `target`, `out`, `dist`, `node_modules`, `.git`, `.cargo`, and `.ventanext`.
- **Fault-Tolerant Connection Handling**:
  - Replaced panicking `.unwrap()` in `handle_client_connection` with non-panicking `try_clone()` error recovery and 5-second HTTP request timeouts.

---

## [Iteration 8] — 2026-11-04

### Fixed
- **Compiler SSA ValueId Remapping Exhaustiveness**:
  - Fixed incomplete instruction match statements in `crates/vanta_mir/src/opt/inlining.rs` (Step 4 destination remapping), which omitted `Instruction::CallIndirect` and `Instruction::VtableAddr`.
  - Fixed identical incomplete destination mappings across 3 locations in `crates/vanta_mir/src/opt/loop_unroll.rs` and 2 locations in `crates/vanta_mir/src/opt/loop_peel.rs`.
  - Unified all SSA destination value assignment across inlining, loop unrolling, and loop peeling to invoke canonical `inst.defs()`, guaranteeing 100% variant coverage and eliminating SSA ValueId collisions on duplicated CFG paths.

### Added
- **Polymorphic Inlining & SSA Integrity Tests**:
  - Added `test_inline_callee_with_indirect_call_and_vtable_ssa_integrity` in `inlining_tests.rs`, verifying complete SSA uniqueness and `MirVerifier` validation on inlined polymorphic callee bodies.

