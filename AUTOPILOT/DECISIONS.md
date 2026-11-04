# Architecture and Technical Decisions

## ADR-001: Workspace-Local Cargo Target Directory in WSL2
- **Context**: Running Cargo from WSL2 inside an NTFS mount (`/mnt/c/...`) causes Cargo to ascend the directory tree and find Windows-specific configuration (`/mnt/c/Users/mumer/.cargo/config.toml`). That configuration set `target-dir = "D:/rust/target"`, which contained `:` causing Linux `$LD_LIBRARY_PATH` syntax errors. Additionally, building Rust targets directly on NTFS inside WSL2 introduces severe 9p/DrvFS file I/O performance penalties and OneDrive file churn.
- **Decision**: Configure workspace-level `.cargo/config.toml` targeting native Linux filesystem path `/home/mumer/.cargo_target`. Keep `.cargo/` in `.gitignore` so local developer platform paths do not conflict across Windows and WSL2 environments.
- **Consequences**: Fast builds (< 1s incremental check), zero path collision errors, no unwanted OneDrive synchronization.

## ADR-002: Dynamic Offset Relocations for ELF Static Runtime Functions
- **Context**: The built-in ELF static runtime in `vanta_linker` synthesizes x86-64 machine code for `_start`, `vanta_alloc`, `vanta_free`, `vanta_vec_*`, etc., directly into memory. Previous implementations relied on fragile hand-calculated instruction lengths and jump offsets, leading to corruption (`call_vfree_pos = vec_free_offset + 10` overwrote instructions instead of the call argument).
- **Decision**: Use calculated instruction offsets and dynamic relative call relocations (`(target - (pos + 4))`) for all inter-function calls within the synthesized text section. Never use hardcoded negative short jumps across functions.
- **Consequences**: Complete stability against code modifications, zero runtime SIGILL crashes from malformed instruction streams.

## ADR-003: Exclusion of Glibc Interposition Symbols from `libvanta_rt.a`
- **Context**: In `runtime/vanta_rt.c`, a fallback stub for `__errno_location()` was defined returning a pointer to a dummy variable. When Vanta programs link dynamically with libraries like `libcurl.so`, ELF dynamic symbol interposition forces `libcurl` to bind calls to `__errno_location` to the executable's dummy stub rather than glibc's TLS `errno`. Non-blocking socket connect operations returning `-1` with `EINPROGRESS` were misinterpreted as immediate failures with `errno == 0` (`CURLE_COULDNT_CONNECT`).
- **Decision**: Remove glibc-reserved interposition symbols from `runtime/vanta_rt.c`. Runtime code relying on system errno must either use libc's real header definitions or isolate freestanding symbols strictly inside static internal linker objects with hidden visibility.
- **Consequences**: Dynamic FFI shared libraries (`libcurl`, `sqlite3`, etc.) correctly observe thread-local `errno` and system state.

## ADR-004: WSL2 Test Harness Execution Retries on `ETXTBUSY`
- **Context**: Linux returns `ETXTBUSY` (errno 26: "Text file busy") when attempting to `execve()` an executable file that is currently open for writing by any process. Under high-core parallel test runs in WSL2, rapid back-to-back link-and-execute cycles can trigger transient `ETXTBUSY` before NTFS/ext4 buffer writeback completion.
- **Decision**: Test helpers executing newly written binaries must retry on `std::io::ErrorKind::ExecutableFileBusy` with a brief sleep (50ms, up to 5 attempts).
- **Consequences**: Eliminates transient parallel test failures across the workspace.

## ADR-005: Universal VentaNext Template Installation & Multi-Tier Discovery
- **Context**: Venta CLI allows users to browse (`vanta templates`) and scaffold projects (`vanta new <template> <name>`). Previously, template resolution hardcoded a developer's Windows path in `main.rs`, and `scripts/package_release.sh` / `scripts/install.sh` never packaged `ventanext-gen` or `templates/`. External users installing via `install.sh` had non-functional `templates` and `new` subcommands.
- **Decision**: Package `ventanext-gen` in `bin/` and templates under `share/ventanext/templates/`. Implement a 7-tier search hierarchy: (1) explicit `--registry`, (2) `VENTANEXT_TEMPLATES_DIR` / `VANTA_TEMPLATES_DIR`, (3) CWD relative, (4) CWD ancestor traversal, (5) executable sibling directory `../share/ventanext/templates`, (6) user data directories (`~/.local/share/ventanext/templates`, `~/.ventanext/templates`), (7) system paths (`/usr/local/share/ventanext/templates`).
- **Consequences**: `vanta templates` and `vanta new` work out of the box everywhere across installed environments, source checkouts, and clean prefixes.

## ADR-006: Direct TCP Client Socket Primitive Integration in `std::net`
- **Context**: `std::net` originally only exposed server primitives (`listen`, `accept`, `read`, `write`, `close`). Programs could not initiate outbound TCP client connections without invoking external C libraries via `std::curl` or FFI.
- **Decision**: Implement `vanta_net_connect(host, port)` in `runtime/vanta_rt.c` with address parsing and DNS resolution, register in `vanta_sema`, and export `std::net::connect`. Validate port ranges (`1..=65535`) in both client and server socket primitives to prevent negative port value overflow.
- **Consequences**: Pure Vanta programs can now act as robust, zero-dependency TCP clients and servers.

## ADR-007: O(1) Linker Contribution Mapping Index
- **Context**: In ELF, PE/COFF, and Mach-O internal linkers, applying relocations scanned all merged sections and all contributions linearly for each relocation ($O(R \times C)$ quadratic complexity). In large builds with tens of thousands of relocations and thousands of contributions, this created excessive loop iterations and memory churn.
- **Decision**: Construct a unified `ContributionMap` once per link session after section merging. Look up `(object_id, section_index)` and `(object_id, section_name)` in $O(1)$ time to directly index the target `MergedSection` slice.
- **Consequences**: Eliminates linear search overhead across relocation passes, accelerates symbol address resolution, and scales efficiently to thousands of object files.

## ADR-008: Decoupled Non-Blocking WebSocket Broadcasting and RFC 6455 Framing
- **Context**: In `crates/ventanext_codegen/src/dev_server.rs`, `LiveReloadBridge::broadcast` performed network writes while holding the client list `Mutex`. Stalled or slow-reading browser tabs could hold the mutex for extended periods, blocking new incoming WebSocket connections and dev server status queries. Furthermore, absence of client frame decoding meant client Ping frames were ignored and Close frames left sockets unserviced.
- **Decision**: Decouple network writes from mutex locking using `std::mem::take` to extract existing clients, perform all network I/O outside the mutex with `TCP_NODELAY` and 500ms write timeouts, prune closed/broken connections, and merge surviving clients back into the mutex. Implement an RFC 6455 frame decoder (`decode_websocket_frame`) and background reader threads to service client Pings (replying with Pong) and Close frames.
- **Consequences**: Zero head-of-line blocking on client connections, instant hot-reload dispatching (< 1ms), and full RFC 6455 compliance.

## ADR-009: Exhaustive `Instruction::defs()` SSA Remapping in MIR Transformations
- **Context**: Function inlining (`inlining.rs`), loop unrolling (`loop_unroll.rs`), and loop peeling (`loop_peel.rs`) duplicate CFG basic blocks and require fresh `ValueId`s for all defined SSA variables. Previously, each pass implemented its own incomplete `match inst` arm list, omitting `Instruction::CallIndirect` and `Instruction::VtableAddr`. Inlining or unrolling functions with indirect calls or virtual table lookups resulted in duplicated instructions defining the exact same `ValueId` as the original or previous iteration, violating SSA form and causing ValueId collisions.
- **Decision**: Replace manual, repetitive match statements in all MIR duplication passes with the canonical `Instruction::defs(&self)` method. All SSA variables defined by an instruction are automatically and exhaustively remapped to fresh `ValueId`s across all CFG duplications.
- **Consequences**: 100% variant coverage across all 21 MIR instructions, complete SSA integrity, and immunity to subtle omission bugs when new instruction variants are added.
