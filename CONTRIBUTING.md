# Contributing to Venta

Thank you for your interest in contributing to **Venta**, a performance-focused systems programming language.

Our priorities are correctness, memory safety, native performance, reproducibility, and transparent engineering.

## Guiding Principles

1. **Correctness First**

   A fast compiler that generates incorrect results is not acceptable. New language features should include positive tests, negative compile-fail tests, and regression tests where applicable. Code generation must preserve the documented language semantics.

2. **Project-Owned Native Backend**

   Venta's native backend must remain implemented and maintained within the project. LLVM and Cranelift must not generate the final machine code. Any external assembler, linker, or other toolchain dependencies must be documented accurately. Do not claim fully standalone compilation unless verified.

3. **Reproducible Benchmarks**

   Do not claim performance advantages over Rust, C, or C++ without reproducible benchmarks under comparable conditions. Document compiler versions, build settings, workloads, hardware, methodology, and results wherever practical.

4. **Verified Features Only**

   Do not describe a feature as complete unless its implementation and relevant tests support that claim. Clearly identify TODOs, known limitations, and incomplete implementations. Never substitute stubs or simulated results for working functionality.

5. **Regression Prevention**

   Add regression tests for bug fixes whenever practical. Changes to the parser, type checker, optimizer, memory model, or backend must be tested against existing behavior.

6. **Clear Diagnostics**

   Invalid programs should produce useful, deterministic diagnostics wherever feasible. Do not silently accept invalid code or hide compilation failures.

7. **Documentation and Transparency**

   Update the language specification, compiler documentation, tests, and examples when behavior changes. Clearly distinguish implemented functionality from planned work.

## Before Submitting a Contribution

- Inspect existing code and conventions before introducing new abstractions.
- Keep changes focused and avoid unrelated rewrites.
- Run formatting, linting, and relevant tests.
- Report the commands used and their actual results.
- Document known limitations and tests that could not be run.
- Never claim a test, benchmark, or build passed unless it was actually executed successfully.

Thank you for helping make Venta reliable, fast, and maintainable.
