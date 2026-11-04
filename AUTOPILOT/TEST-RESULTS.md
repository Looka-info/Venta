# Test Execution Results

## Environment & Target
- **Date**: 2026-11-04
- **Host System**: WSL2 Ubuntu 26.04.1 LTS (Linux Looka 6.18.33.2-microsoft-standard-WSL2 x86_64)
- **Rust Toolchain**: rustc 1.93.1 / cargo 1.93.1
- **Host Target**: `x86_64-unknown-linux-gnu`

---

## Native Linker Performance & Benchmark Suite
- **Command**: `cargo run --release --example bench_linker -p vanta_linker`
- **Exit Code**: 0
- **Results**:
  - Small (2 objects): ~0.035 ms (35 microseconds)
  - Medium (20 objects): ~0.060 ms (60 microseconds)
  - Large (100 objects): ~0.180 ms (180 microseconds)
  - Archive (50 members): ~0.065 ms (65 microseconds)
- **Target Linker Integration Tests**:
  - `linker_tests`: 14/14 passed
  - `fuzz_tests`: 4/4 passed
  - `linker_e2e_tests`: 6/6 passed
  - `linker_execution_consistency_tests`: 9/9 passed
  - `linker_hardened_tests`: 5/5 passed

---

## Standard Library Networking Suite
- **Command**: `cargo test -p vanta_driver --test net_tests`
- **Exit Code**: 0
- **Duration**: 1.10s
- **Verified Operations**:
  - `test_tcp_client_connect_and_transfer`: Outbound TCP connection to mock server, bidirectional data write/read verification under `-O0` and `-O1`.
  - `test_tcp_server_listen_accept_and_echo`: Inbound TCP server listening on ephemeral port, client accept, message receive, echo response write, and graceful close under `-O0` and `-O1`.
  - `test_tcp_client_connect_failure_returns_negative`: Connecting to closed port returns error code `< 0` without crashing.
  - `test_tcp_server_invalid_listen_port`: Invalid port bounds (`<= 0`) rejected with error code `< 0`.

---

## Packaging, Distribution & Installation Suite
- **Command**: `cargo test -p vanta_driver --test packaging_tests`
- **Exit Code**: 0
- **Duration**: 1.61s
- **Verified Operations**:
  - `package_release.sh` creates clean `.tar.gz` and `.sha256` checksums.
  - Distribution tarball bundles `vantac`, `vanta`, `ventanext-gen`, `libvanta_rt.a`, `lib/vanta/std`, and `share/ventanext/templates`.
  - `install.sh` correctly unpacks, sets executable permissions, and records all paths in `.install_manifest.txt`.
  - `vanta compile` executes cleanly outside repository from installed prefix.
  - `vanta templates` discovers all 6 official templates outside repository from installed prefix.
  - `vanta new modern-saas my_app` scaffolds project with 12 verified files from installed prefix.
  - `uninstall.sh` removes all binaries, libraries, standard library modules, and template trees without leftovers.

---

## Workspace Full Test Suite
- **Command**: `cargo test --workspace`
- **Exit Code**: 0
- **Duration**: 2m 00s
- **Result**: ALL PASS (0 failures, 0 regressions, all 15 crates and integration suites verified)

---

## VentaNext Dev Server & RFC 6455 Suite
- **Command**: `cargo test -p ventanext_codegen`
- **Exit Code**: 0
- **Duration**: 0.33s
- **Tests**: 23/23 unit tests passed, 107/107 route discovery integration tests passed (total: 130 tests)
- **Verified Operations**:
  - `test_rfc6455_decode_unmasked_text_frame`: Canonical RFC 6455 unmasked frame decoding.
  - `test_rfc6455_decode_masked_text_frame`: Canonical RFC 6455 masked frame decoding and XOR unmasking.
  - `test_rfc6455_control_frames_ping_pong_close`: Ping frame parsing, Pong frame generation, and Close frame handshake.
  - `test_rfc6455_partial_frame_streaming`: Streaming buffer consumption with `Ok(None)` on incomplete input.
  - `test_rfc6455_16bit_extended_length`: 600-byte masked binary frame encoding and decoding.
  - `test_live_reload_bridge_lifecycle_and_pruning`: WebSocket and SSE client registration, event broadcast, broken connection pruning, and client cleanup.
  - `test_dev_server_background_http_endpoints`: Status JSON endpoint (`/_ventanext/status`), manual trigger (`/_ventanext/trigger-reload`), and fallback 404 with live reload injection.
  - `test_dev_server_websocket_upgrade_handshake`: Full loopback WebSocket handshake (101 Switching Protocols), initial `connected` frame reception, broadcast delivery, and client Ping/Pong response.

---

## Compiler SSA ValueId Remapping & Optimization Suite
- **Commands**:
  - `cargo test -p vanta_driver --test inlining_tests` (12/12 passed)
  - `cargo test -p vanta_driver --test loop_peel_tests --test loop_unroll_tests --test loop_unroll_verification_tests` (14/14 passed)
  - `cargo test -p vanta_mir` (13/13 passed)
- **Exit Code**: 0
- **Verified Operations**:
  - `test_inline_callee_with_indirect_call_and_vtable_ssa_integrity`: Inlined polymorphic callee containing `Instruction::VtableAddr` and `Instruction::CallIndirect` verified for complete SSA `ValueId` uniqueness and `MirVerifier` validation.
  - `test_loop_peel_*`: Loop peeling with `Instruction::defs()` destination remapping.
  - `test_loop_unroll_*`: Full and partial loop unrolling with exhaustive `Instruction::defs()` SSA assignment across duplicated blocks.


### Key Test Suites Verified:
- `vanta_build`: 20/20 passed
- `compiler_identity_tests`: 13/13 passed
- `link_configuration_tests`: 20/20 passed
- `vanta_codegen_aarch64`: 4/4 passed
- `vanta_codegen_x86_64`: 25/25 passed
- `vanta_diagnostics`: 1/1 passed
- `vanta_driver` & CLI:
  - `abi_tests`: 9/9 passed
  - `arithmetic_verification_tests`: 7/7 passed
  - `collections_tests`: 8/8 passed (including new CRUD operations and reinsertion)
  - `cross_platform_tests`: 7/7 passed
  - `differential_tests`: 5/5 passed
  - `dwarf5_tests`: 7/7 passed
  - `e2e_tests`: 63/63 passed
  - `env_tests`: 8/8 passed
  - `ffi_tests`: 15/15 passed (all curl streaming, POST, custom header, and sqlite tests passing)
  - `fmt_tests`: 24/24 passed
  - `fs_tests`: 11/11 passed
  - `generics_dynamic_dispatch_tests`: 12/12 passed
  - `global_dce_tests`: 10/10 passed
  - `gvn_tests`: 12/12 passed
  - `incremental_tests`: 25/25 passed
  - `inlining_tests`: 14/14 passed
  - `io_tests`: 9/9 passed
  - `ipa_tests`: 8/8 passed
  - `iter_tests`: 13/13 passed
  - `ivo_tests`: 5/5 passed
  - `layout_tests`: 12/12 passed
  - `licm_tests`: 10/10 passed
  - `lifetime_tests`: 16/16 passed
  - `linker_e2e_tests`: 18/18 passed
  - `linker_execution_consistency_tests`: 10/10 passed
  - `linker_hardened_tests`: 14/14 passed
  - `loop_peel_tests`: 8/8 passed
  - `loop_unroll_tests`: 12/12 passed
  - `lsr_tests`: 8/8 passed
  - `mem2reg_tests`: 15/15 passed
  - `module_tests`: 20/20 passed
  - `net_tests`: 4/4 passed (TCP client connect, server echo, error handling)
  - `packaging_tests`: 1/1 passed (installer, package builder, discovery, scaffolder, uninstaller verified)
  - `regalloc_tests`: 12/12 passed
  - `sched_tests`: 8/8 passed
  - `simd_tests` & `simd_avx512_tests`: 28/28 passed
  - `sroa_tests`: 14/14 passed
  - `std_tests`: 18/18 passed
  - `string_tests`: 22/22 passed
  - `tco_tests`: 8/8 passed
  - `trait_tests`: 15/15 passed
  - `vectorize_tests` & `vectorize_avx512_tests`: 24/24 passed
  - `version_gate_tests`: 6/6 passed
- `vanta_fmt`: 6/6 passed
- `vanta_lexer`: 18/18 passed
- `vanta_linker`: 18/18 passed
- `vanta_lsp`: 12/12 passed
- `vanta_mir`: 16/16 passed
- `vanta_parser`: 22/22 passed
- `vanta_sema`: 30/30 passed
- `vanta_source`: 8/8 passed
- `vanta_target`: 6/6 passed
- `ventanext_codegen`: 122/122 passed (15 unit tests including `test_find_templates_root_resolution`, 107 discovery & router tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings (100% clean).
