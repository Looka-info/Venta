# Vanta Programming Language

> **Performance-First, Memory-Safe Systems Programming Language**

Vanta is a statically typed, ahead-of-time compiled systems programming language designed for maximum practical native execution performance, predictable resource management, and memory safety by default without a mandatory tracing garbage collector.

---

## Key Features (Milestone 1)

- **Memory Safety by Default**: Statically verified ownership and borrow checking rules (aliasing XOR mutability, definite initialization, move vs copy semantics).
- **Custom Native x86-64 Backend**: Machine code instruction encoder and pure-Rust ELF64 object file writer. LLVM and Cranelift are NOT used for final machine code generation.
- **System V AMD64 ABI Compliance**: Direct ABI compatibility with C libraries on Linux x86-64.
- **Typed Intermediate Representation (MIR)**: CFG-based intermediate representation with basic blocks and explicit control flow.
- **Zero-Dependency Native Object Emission**: Emits valid Linux `.o` relocatable object files directly from Rust compiler code.

---

## Workspace Architecture

```
vanta/
├── Cargo.toml               # Virtual workspace root
├── rust-toolchain.toml      # Rust toolchain pinning (stable)
├── docs/                    # Formal specifications
│   ├── language-spec.md     # Language syntax, types, and semantics
│   ├── grammar.ebnf         # EBNF grammar specification
│   ├── memory-model.md      # Ownership, borrowing, and lifetimes
│   ├── ir-spec.md           # Vanta Mid-Level IR (MIR) specification
│   ├── backend-design.md    # Custom x86-64 backend architecture
│   └── abi-and-platforms.md # Platform support & System V ABI
├── crates/
│   ├── vanta_source         # Source files, spans, file management
│   ├── vanta_diagnostics    # Rich diagnostic reporting with source snippets
│   ├── vanta_lexer          # Tokenizer, nested comments, numeric literals
│   ├── vanta_parser         # AST & precedence climbing parser
│   ├── vanta_sema           # Symbol resolution, type checking, borrow checking
│   ├── vanta_mir            # CFG, basic blocks, typed MIR, verifier
│   ├── vanta_codegen_x86_64 # Custom x86-64 encoder, frame layout, ELF64 writer
│   └── vanta_driver         # Compiler CLI (`vantac`), linker driver
├── examples/                # Working Vanta programs
└── tests/                   # End-to-end and unit test suites
```

---

## Building and Running

### Prerequisites
- Linux x86-64 or WSL2 (Ubuntu 22.04 / 24.04 / 26.04)
- Rust 1.80+ and Cargo
- GCC (system linker for runtime startup)

### Running Tests
```bash
cargo test
```

### Compiling and Running a Vanta Program
```bash
# Build compiler in release mode
cargo build --release --bin vantac

# Compile a Vanta source program to native Linux executable
./target/release/vantac examples/03_while_loop.va -o while_loop

# Run the generated native binary
./while_loop
echo "Exit code: $?"
```
