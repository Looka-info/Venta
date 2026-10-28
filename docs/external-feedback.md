# Vanta External Evaluator Feedback & Triage Workflow

## 1. Overview

The Vanta language project relies on rigorous, evidence-based feedback from external systems programmers, compiler engineers, and language researchers to determine readiness for General Availability (GA).

This document establishes the official workflow for submitting, categorizing, prioritizing, and resolving feedback from the external evaluation cycle of **Vanta v1.0 Release Candidates** on Linux x86-64.

---

## 2. Feedback Submission Channels

External evaluators are encouraged to submit observations through the following channels:

1. **GitHub Issues**:
   - Utilize the preconfigured issue template: [`.github/ISSUE_TEMPLATE/evaluator-feedback.md`](file:///.github/ISSUE_TEMPLATE/evaluator-feedback.md).
   - Tag submissions with the label `evaluator-feedback`.
2. **Evaluation Scenario Matrix**:
   - Verify scenarios defined in the [External Evaluation Guide](file:///docs/external-evaluation.md).
   - Report pass/fail statuses alongside machine details (`uname -srm`, `lscpu`).
3. **Structured Machine Diagnostics**:
   - For tooling or automated pipeline integrations, submit output produced with `--json` diagnostics.

---

## 3. Required Report Information

To ensure complete reproducibility without speculative back-and-forth, evaluators must include:

| Field | Description | Command / Example |
| :--- | :--- | :--- |
| **Host Kernel** | Linux kernel version and architecture | `uname -srm` (e.g., `Linux 5.15.0-89-generic x86_64`) |
| **CPU Model** | Specific CPU microarchitecture | `lscpu \| grep "Model name"` |
| **Toolchain Version** | Exact Vanta compiler version | `vanta --version` / `vantac --version` |
| **Package Checksum** | Release tarball SHA-256 integrity hash | `sha256sum dist/vanta-*.tar.gz` |
| **Linker** | System ELF linker version | `gcc --version` or `clang --version` |
| **Reproduction Snippet** | Minimal, self-contained Vanta source file | Single `.va` file (e.g., `repro.va`) |
| **Compiler Flags** | Full command-line options tested | e.g., `vanta compile repro.va -O1 --no-gvn` |
| **Observed vs Expected** | Concrete output, exit code, or error | Console logs, exit status `$?`, or core dump |

---

## 4. Feedback Severity Classification & SLA

Incoming feedback is triaged according to the following strict severity framework:

### Priority 0 (P0) — General Availability Blocker
- **Criteria**:
  - Compiler crashes (Internal Compiler Error / panic).
  - Silent bad-code generation (miscompilation leading to incorrect runtime results).
  - Memory safety violations in runtime or compiler (heap corruption, double free, use-after-free).
  - Privilege escalation or security boundary violation.
- **SLA**: Immediate triage within 4 hours; patch and release candidate rebuild within 24 hours. **Blocks GA release.**

### Priority 1 (P1) — Release Candidate Blocker
- **Criteria**:
  - Toolchain CLI incompatibilities or rejected flags documented in user guides.
  - Standard installation/uninstallation script failure on clean, standard Linux environments.
  - False positive borrow checker / type checker rejections of valid conforming code.
  - Diagnostic formatting failures (e.g., malformed `--json` output) breaking external tooling.
- **SLA**: Triage within 12 hours; patch incorporated into next release candidate (`RC2`, `RC3`).

### Priority 2 (P2) — Non-Blocking Defect (Point Release)
- **Criteria**:
  - Minor diagnostic cosmetic inconsistencies (e.g., column offset off by 1).
  - Missing warning diagnostics for non-critical edge cases.
  - Non-standard Linux distribution quirks with clean workarounds available.
  - Isolated performance anomalies without correctness impact.
- **SLA**: Scheduled for resolution in v1.0.1 maintenance release.

### Priority 3 (P3) — Enhancement / Post-v1.0 Scope
- **Criteria**:
  - Feature requests beyond the frozen v1.0 specification.
  - New target platforms (Windows native, macOS ARM64).
  - SIMD / AVX2 / AVX-512 auto-vectorization.
  - User-defined traits and generic specialization.
- **SLA**: Logged to [`docs/post-v1.0-roadmap.md`](file:///docs/post-v1.0-roadmap.md) for v1.1+.

---

## 5. Candidate Verification Protocol

Every reported defect is triaged through the following verification sequence:

1. **Isolation**: Extract the minimal `.va` source code and test against both unoptimized (`-O0`) and optimized (`-O1`) builds.
2. **Differential Validation**: Run against existing compiler test harnesses (`crates/vanta_driver/tests/`).
3. **Root Cause Analysis**: Identify whether the issue originates in the Lexer, Parser, Semantic Analyzer, Borrow Checker, MIR Optimization Pipeline, Register Allocator, or x86-64 Code Generator.
4. **Automated Regression Test**: Commit an isolated regression test reproducing the failure before landing the fix.
5. **Release Candidate Cut**: If compiler or toolchain behavior changes, cut a new candidate tag (e.g., `v1.0-RC2`) and produce updated distribution tarballs.
