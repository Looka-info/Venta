# Contributing to Vanta

Thank you for contributing to Vanta.

## Guiding Principles

1. **Correctness First**: A fast compiler producing incorrect machine code is worthless. Every feature must include positive tests, negative compile-fail tests, and regression tests.
2. **Custom Backend Integrity**: Vanta's native backend must remain self-contained. LLVM and Cranelift must not generate the final machine code.
3. **Reproducible Benchmarks**: Never claim performance advantages over Rust or C++ without reproducible, open benchmarks running under identical constraints.
4. **Verified Code Only**: Do not commit TODOs or unimplemented stubs while reporting features complete.
