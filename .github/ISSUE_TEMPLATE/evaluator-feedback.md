---
name: External Evaluator Feedback / Bug Report
about: Submit test results, environment observations, or issue reports for Vanta Release Candidates
title: "[Evaluator]: "
labels: ["evaluator-feedback"]
assignees: []
---

## 1. Evaluator Host Environment
- **Operating System**: (e.g., Ubuntu 22.04 LTS, Debian 12, Arch Linux, WSL2)
- **Kernel Version (`uname -srm`)**: 
- **Linker & Toolchain**: (e.g., `gcc --version` or `clang --version`, `ld --version`)

## 2. Hardware Information
- **CPU Model (`lscpu | grep 'Model name'`)**: 
- **Instruction Set Extensions**: (e.g., AVX2, BMI2, SSE4.2)
- **RAM / Available Memory**: 

## 3. Vanta Artifact Verification
- **Evaluated Version**: (e.g., `v1.0-RC1` or `v1.0-RC2`)
- **`vanta --version`**: 
- **`vantac --version`**: 
- **SHA-256 Digest (`sha256sum vanta-*.tar.gz`)**: 

## 4. Evaluation Scenario / Workload
- [ ] Evaluation Guide Scenario 1: Exit code reporting
- [ ] Evaluation Guide Scenario 2: Deep recursion (Fibonacci)
- [ ] Evaluation Guide Scenario 3: Struct layout & aggregate return (System V AMD64 ABI)
- [ ] Evaluation Guide Scenario 4: Loops & break control-flow
- [ ] Evaluation Guide Scenario 5: Standard-library File I/O & `Result` matching
- [ ] Evaluation Guide Scenario 6: Borrow checker & JSON diagnostics reporting
- [ ] Evaluation Guide Scenario 7: Standalone compilation & optimization equivalence
- [ ] Custom / Real-World System Workload (describe below)

## 5. Minimal Reproducible Example (`.va`)
```vanta
// Paste minimal reproducing Vanta source code here
```

## 6. Exact Command Line Invocation
```bash
# Paste exact command used to compile or run
vanta run ...
# or
vantac compile ...
```

## 7. Observed Behavior vs Expected Behavior
- **Expected Behavior**: 
- **Observed Behavior**: 

## 8. Compiler Output & Diagnostics
```
Paste full console output, including stdout, stderr, or --json diagnostic payload
```

## 9. Crash Information (if applicable)
- **Exit Status / Signal**: (e.g., SIGSEGV, SIGABRT, code 139)
- **GDB Backtrace / Coredump summary**:

## 10. Severity Assessment (Preliminary)
- [ ] **P0**: Crash, ICE (Internal Compiler Error), silent bad-code generation, memory safety violation, privilege escalation. (GA Blocker)
- [ ] **P1**: CLI option failure, installation/discovery failure on standard Linux, false positive rejection blocking valid code. (RC Blocker)
- [ ] **P2**: Non-blocking diagnostic formatting anomaly, minor performance regression, non-standard OS quirk. (Point Release)
- [ ] **P3**: Feature request, non-x86-64 target support, vectorization / SIMD optimization. (Post-v1.0 Roadmap)
