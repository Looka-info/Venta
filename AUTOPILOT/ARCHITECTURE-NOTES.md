# Architecture Notes: Vanta & VentaNext

## System Overview

The Vanta language system is an industrial, production-grade systems programming language with an integrated compiler, linker, package manager, and full-stack web framework (`VentaNext`).

### 1. Compiler Toolchain (`crates/`)
- **`vanta_source`**: Source code abstraction, file IDs, `Span` tracking, line/column index computation.
- **`vanta_diagnostics`**: Rich diagnostic reporting with JSON/Human formats, error codes (`E0100`..`E0900`), and source annotations.
- **`vanta_lexer`**: High-performance zero-allocation tokenizer.
- **`vanta_parser`**: Recursive-descent parser producing typed AST nodes.
- **`vanta_sema`**: Semantic analysis, symbol table management, type inference, trait resolution, borrow checking, generics monomorphization, and module loading.
- **`vanta_mir`**: Mid-level Intermediate Representation (SSA form, basic blocks, phi nodes, optimization passes including mem2reg, SROA, GVN, LICM, inlining, loop unrolling/peeling, IPA, dead-code elimination).
- **`vanta_target`**: Target triple abstraction (`x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`).
- **`vanta_codegen_x86_64`**: Native x86-64 machine code generation, register allocation (linear scan), ELF/COFF object writers, DWARF 5 debug info emission.
- **`vanta_codegen_aarch64`**: Native ARM64/AArch64 code generation, Mach-O/ELF object writing.
- **`vanta_linker`**: Self-contained internal linker capable of ELF64, PE32+ (COFF), and Mach-O binary emission without external system linker dependencies. Contains built-in static runtime synthesis (`elf/runtime.rs`).
- **`vanta_build`**: Build graph management, cryptographic source fingerprinting, fine-grained incremental compilation cache.
- **`vanta_driver`**: Compiler driver CLI (`vantac`, `vanta`), session management, multi-file compilation orchestration.
- **`vanta_fmt`**: Canonical AST-based code formatter.
- **`vanta_lsp`**: Language Server Protocol implementation.
- **`ventanext_codegen`**: VentaNext framework compiler: filesystem route discovery, adapter generation, SSR dispatcher, built-in development server with WebSockets/SSE live reload, template cryptographic verification.

### 2. Runtime System (`runtime/`)
- `vanta_rt.c`: C runtime implementation for foreign targets and standard library bridging (`vanta_alloc`, `vanta_free`, `vanta_str_*`, `vanta_vec_*`, POSIX/Win32 I/O, SQLite3, libcurl, graphics).
- Built static libraries: `libvanta_rt.a` (Linux/macOS), `vanta_rt.lib` (Windows MSVC).

### 3. Standard Library (`library/std/`)
- Written in pure Vanta:
  - `collections`: `HashMap`, `RingBuffer`, modular data structures.
  - `curl`: High-level HTTP client bindings with streaming and POST support.
  - `sqlite`: Database engine bindings with prepared statements and typed column extraction.
  - `graphics`: Windowing and rendering abstractions.
  - `fs`, `io`, `net`, `env`, `string`, `iter`, `option`, `result`, `simd`, `ffi`.

### 4. VentaNext Full-Stack Framework (`ventanext/`, `templates/`)
- Zero-external-dependency web framework written in Vanta.
- File-system router with support for static pages, dynamic segments, catch-all routes, and route groups.
- Live dev server with automatic hot-reloading.
- Official template gallery (Admin Dashboard, SaaS, E-commerce, Portfolio, Docs, Agency).
