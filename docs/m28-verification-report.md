# Milestone 28: Verified Bootstrap, Compiler Hardening & CI Release Gate Repair Report

## 1. Executive Summary

Milestone 28 resolved the root cause of the GitHub Actions CI release gate failure on the `Build, Test & Package (Linux x86_64)` workflow, implemented strict version consistency validation, audited and established empirical evidence for the self-hosted compiler bootstrap chain (`tools/self_host/vanta_c.va`), expanded backend and ABI regression coverage, and executed the release pipeline end-to-end.

---

## 2. Root Cause of CI Version Gate Failure

### 2.1 The Failure Mode
The GitHub Actions workflow `.github/workflows/release-validation.yml` failed at step 54:
```text
Run CARGO_VER=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
Cargo workspace version: 1.0.0-rc.2
Error: Unexpected workspace version: 1.0.0-rc.2
Error: Process completed with exit code 1.
```

### 2.2 Forensic Analysis
Two independent root causes caused the failure:
1. **Hardcoded String Comparison**: The workflow step contained:
   ```bash
   if [[ "${CARGO_VER}" != "1.0.0-rc.1" ]]; then
       echo "Error: Unexpected workspace version: ${CARGO_VER}"
       exit 1
   fi
   ```
   When Milestone 27 bumped the workspace version to `1.0.0-rc.2` (to resolve external evaluator feedback regarding `--json` options and CLI flags), the workflow was not updated, resulting in an immediate rejection of any valid subsequent release candidate or GA release.
2. **Brittle Metadata Query**: `.packages[0].version` is non-deterministic in a virtual Cargo workspace (`[workspace]` with 9 member crates). `.packages[0]` was `vanta_source`, rather than the root package or the primary driver crate `vanta_driver`. If crate order shifts or an individual member crate does not share the version, `.packages[0]` produces incorrect results.
3. **Downstream Hardcoding**: Steps 78-91 of the workflow also hardcoded `vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz.sha256` and the archive name, which would have failed immediately upon packaging RC2.

---

## 3. Files Changed and Technical Justification

| File | Change | Technical Justification |
|:---|:---|:---|
| [`.github/workflows/release-validation.yml`](file:///.github/workflows/release-validation.yml) | Replaced hardcoded `1.0.0-rc.1` check with `verify_version_consistency.py --github-env --github-output`; parameterized checksum and artifact upload steps | Enforces dynamic, strict version policy for both RCs and GA without hardcoded fragility |
| [`scripts/verify_version_consistency.py`](file:///scripts/verify_version_consistency.py) | Created dedicated version consistency verifier | Validates that all 9 workspace crates have identical versions matching release policy (`1.0.0-rc.N` or `1.0.0`), validates Git tag alignment, and exports archive names |
| [`scripts/test_version_validation.py`](file:///scripts/test_version_validation.py) | Created regression test suite for version gate | Exercises 8 distinct unit cases (valid RCs, valid GA, invalid SemVer, mismatched crates, tag mismatches) |
| [`crates/vanta_driver/tests/version_gate_tests.rs`](file:///crates/vanta_driver/tests/version_gate_tests.rs) | Added integration tests | Hooks the version validation suite into `cargo test` so regressions are caught in workspace test runs |
| [`scripts/test_self_hosted_compiler.sh`](file:///scripts/test_self_hosted_compiler.sh) | Expanded from 5 to 9 automated test programs | Added automated coverage for 6-argument calls, comparisons, nested loops, and arithmetic precedence |
| [`tools/self_host/tests/06_call_6_args.va`](file:///tools/self_host/tests/06_call_6_args.va) | Added test program | Tests full 6-register passing sequence (`rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`) |
| [`tools/self_host/tests/07_comparisons.va`](file:///tools/self_host/tests/07_comparisons.va) | Added test program | Tests all comparison operators (`==`, `!=`, `<`, `<=`, `>`, `>=`) and branch codegen |
| [`tools/self_host/tests/08_nested_loops.va`](file:///tools/self_host/tests/08_nested_loops.va) | Added test program | Tests nested while loops and stack variable updates |
| [`tools/self_host/tests/09_operator_precedence.va`](file:///tools/self_host/tests/09_operator_precedence.va) | Added test program | Tests complex operator precedence and subexpression grouping |
| [`docs/artifact-integrity.md`](file:///docs/artifact-integrity.md) | Updated SHA-256 for RC2 | Maintains cryptographic auditability |
| [`.gitignore`](file:///.gitignore) | Added `__pycache__/` and `*.pyc` | Prevents Python bytecode pollution |

---

## 4. Version Policy & Regression Test Matrix

Under Vanta's documented release policy:
- **Release Candidates**: Must match `^1\.0\.0-rc\.(\d+)$`. Canonical Git tag: `v1.0-RC<N>`. Tarball: `vanta-v1.0-RC<N>-x86_64-unknown-linux-gnu.tar.gz`.
- **General Availability (GA)**: Must match `^1\.0\.0$`. Canonical Git tag: `v1.0.0` or `v1.0`. Tarball: `vanta-v1.0.0-x86_64-unknown-linux-gnu.tar.gz`.

### Test Scenarios Executed by [`scripts/test_version_validation.py`](file:///scripts/test_version_validation.py):
1. `test_valid_rc2_version`: Version `1.0.0-rc.2`, tag `v1.0-RC2` -> **PASS**
2. `test_valid_rc1_version`: Version `1.0.0-rc.1`, tag `v1.0-RC1` -> **PASS**
3. `test_valid_stable_version`: Version `1.0.0`, tags `v1.0.0` and `v1.0` -> **PASS**
4. `test_reject_unsupported_versions`:
   - `0.1.0` -> **REJECTED**
   - `0.9.0` -> **REJECTED**
   - `1.0.0-alpha.1` -> **REJECTED**
   - `1.0.0-beta` -> **REJECTED**
   - `2.0.0` -> **REJECTED**
   - `v1.0.0` (malformed Cargo version) -> **REJECTED**
5. `test_reject_mismatched_package_versions`: `vanta_source` at `1.0.0-rc.1`, `vanta_driver` at `1.0.0-rc.2` -> **REJECTED**
6. `test_order_independence_when_first_package_is_not_driver`: Metadata lists auxiliary package first -> **PASS**
7. `test_tag_mismatch_rejection`: Version `1.0.0-rc.2` paired with tag `v1.0-RC1` or `v1.0.0` -> **REJECTED**
8. `test_current_workspace_validation`: Validates live workspace files -> **PASS**

---

## 5. Self-Hosting Bootstrap Audit & Evidence

### 5.1 Bootstrap Stage Flow

```
[ Stage 0: Reference Rust Compiler ]
       |  (vantac compile tools/self_host/vanta_c.va -o vanta_c_bin)
       v
[ Stage 1: Native Vanta Compiler Binary (vanta_c_bin) ]
       |
       +---> Compiles procedural Vanta subset:
       |     - 01_return_42.va       --> PASS (Exit 42)
       |     - 02_arithmetic.va      --> PASS (Exit 40)
       |     - 03_while_loop.va      --> PASS (Exit 55)
       |     - 04_if_else.va         --> PASS (Exit 10)
       |     - 05_fibonacci.va       --> PASS (Exit 55)
       |     - 06_call_6_args.va     --> PASS (Exit 21)
       |     - 07_comparisons.va     --> PASS (Exit 63)
       |     - 08_nested_loops.va    --> PASS (Exit 60)
       |     - 09_operator_precedence.va --> PASS (Exit 28)
       |
       +---> Self-Compilation Attempt (vanta_c_bin tools/self_host/vanta_c.va -o stage2.s):
             --> Produces 32-line truncated assembly
             --> Missing 'struct', 'impl', 'match', and methods in Stage 1 parser
             --> Linking fails: "undefined reference to 'main'"
```

### 5.2 Verdict: Partially Self-Hosting / Bootstrap-Dependent
- **Stage 0 (`vantac`)**: The reference Rust compiler compiles `tools/self_host/vanta_c.va` into a functioning native executable (`Stage 1`).
- **Stage 1 (`vanta_c_bin`)**: Successfully compiles pure procedural Vanta code into valid System V AMD64 assembly.
- **Stage 2 Attempt**: Fails to self-compile. `vanta_c.va` is authored using advanced language features (`struct`, `impl`, `match`, methods) that Stage 1's lightweight recursive descent parser does not yet support.
- **Conclusion**: The compiler is **Bootstrap Dependent (Partially Self-Hosting)**. It is written in Vanta and compiles Vanta code to native x86-64 assembly, but cannot yet compile itself without Stage 0.

---

## 6. Backend & ABI Test Results

All 9 native test programs compiled by Stage 1 were assembled with `gcc -no-pie` and executed on Linux x86-64:

| Test Program | Focus Area | Expected | Observed | Status |
|:---|:---|:---:|:---:|:---:|
| `01_return_42.va` | Function return in `rax` | Exit 42 | Exit 42 | **PASS** |
| `02_arithmetic.va` | Binary ops (`+`, `-`, `*`) | Exit 40 | Exit 40 | **PASS** |
| `03_while_loop.va` | Mutable local iteration & conditions | Exit 55 | Exit 55 | **PASS** |
| `04_if_else.va` | Conditional branches (`je`, `jmp`) | Exit 10 | Exit 10 | **PASS** |
| `05_fibonacci.va` | Stack frame preservation across recursion | Exit 55 | Exit 55 | **PASS** |
| `06_call_6_args.va` | System V AMD64 argument sequence (`rdi`..`r9`) | Exit 21 | Exit 21 | **PASS** |
| `07_comparisons.va` | Relational flags (`sete`, `setne`, `setl`, `setle`, `setg`, `setge`) | Exit 63 | Exit 63 | **PASS** |
| `08_nested_loops.va` | Nested loops & stack offset resolution | Exit 60 | Exit 60 | **PASS** |
| `09_operator_precedence.va` | Multiplicative before additive precedence | Exit 28 | Exit 28 | **PASS** |

### System V AMD64 ABI Compliance:
- **Stack Alignment**: Function prologues emit `push rbp; mov rbp, rsp; sub rsp, 512`. 512 is divisible by 16; stack alignment is preserved across all calls.
- **Argument Passing**: Supports up to 6 64-bit integer arguments passed via `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`.
- **Return Value**: Returned in `rax`.
- **Division**: Emits `cqo; idiv rbx`. Division by zero raises standard hardware SIGFPE.

---

## 7. Remaining Compiler Limitations & External Tool Dependencies

1. **Stage 1 Language Subset Limitations**:
   - No `struct` or `impl` declarations.
   - No `match` expressions or enum pattern destructuring.
   - No dynamic heap allocation in user code (strings and vectors handled via C runtime intrinsics).
   - Expressions currently limited to 64-bit integers (`i64`) and string literals in standard calls.
2. **External Toolchain Dependencies**:
   - `tools/self_host/vanta_c.va` emits Intel-syntax x86-64 assembly text (`.s`).
   - Requires external assembler (`gcc -c` or `as`) and linker (`gcc` or `ld`) to produce ELF executables.
   - Requires C runtime archive (`runtime/libvanta_rt.a`) for standard library operations (`fs`, `env`, `io`).

---

## 8. CI Release Pipeline Re-Run Verification

The CI steps were executed locally in the exact order prescribed by the release gate:

1. **Verify Version Consistency**:
   `python3 scripts/verify_version_consistency.py` -> **PASSED** (Workspace: `1.0.0-rc.2`, Mapped Tag: `1.0-RC2`).
2. **Full Workspace Tests**:
   `cargo test --workspace` -> **PASSED** (150+ tests passed across 9 crates, 0 failures).
3. **Specialized Tests**:
   - `cargo test --test abi_tests` -> **PASSED** (9/9 passed).
   - `cargo test --test differential_tests` -> **PASSED** (5/5 passed).
   - `cargo test --test fuzz_tests` -> **PASSED** (4/4 passed).
   - `cargo test --test version_gate_tests` -> **PASSED** (2/2 passed).
4. **Build Release Package Archive**:
   `bash scripts/package_release.sh 1.0-RC2` -> **PASSED** (`dist/vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz` created).
5. **Run Clean-Prefix Packaging Tests**:
   `cargo test --test packaging_tests` -> **PASSED** (1/1 passed).
6. **Verify Cryptographic Checksum**:
   `cd dist && sha256sum -c vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz.sha256` -> **PASSED** (`vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz: OK`).

---

## 9. Status of Milestone 28 Completion

- **Version Gate Fix**: Complete and verified. Brittle hardcoded checks replaced with strict, policy-compliant validation.
- **Version Regression Tests**: Complete (8 unit scenarios + 2 Rust integration tests passing).
- **CI Pipeline Verification**: Complete. All release steps pass sequentially without error.
- **Compiler Bootstrap Audit**: Complete. Demonstrated Stage 0 -> Stage 1 compilation and established that the compiler is partially self-hosting (bootstrap-dependent).
- **Backend & ABI Hardening**: Complete. 9 test programs verified for 6-arg calling convention, 16-byte stack alignment, comparisons, and arithmetic precedence.
- **Milestone 28 is fully complete.**
