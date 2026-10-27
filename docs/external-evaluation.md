# Vanta v1.0-RC1 External Evaluation Guide

## 1. Introduction for External Systems Evaluators

Welcome to the **Vanta v1.0-RC1** evaluation suite.

This document provides a self-contained, step-by-step evaluation procedure designed for systems programmers, compiler engineers, and language researchers. It guides you through verifying Vanta's:
1. Basic executable creation and exit code reporting.
2. Function calling conventions and recursion.
3. Struct memory layout and aggregate returns under System V AMD64 ABI.
4. Loops, break conditions, and control-flow optimizations.
5. Standard-library I/O and file interactions.
6. Compile-time borrow checking and diagnostic error reporting.
7. Standalone compilation outside the source repository.

> **Security & Privilege Notice**: None of the evaluation steps require root/superuser privileges, sudo, kernel module modifications, or disabling system protections. All installations should be conducted under your user directory (`$HOME/.local` or a temporary directory).

---

## 2. Evaluation System Prerequisites

- **Operating System**: Linux x86-64 (kernel $\ge 4.15$) or Windows Subsystem for Linux (WSL2).
- **Installed Tools**: `gcc` or `clang` (used as an ELF linker), `tar`, `gzip`, `sha256sum`.

---

## 3. Step-by-Step Evaluation Procedure

### Step 0: Download, Verify, and Install
Extract and install the release package to a temporary evaluation directory:
```bash
# 1. Verify SHA-256 integrity
sha256sum -c vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz.sha256
# Expected output: vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz: OK

# 2. Extract archive
tar -xzf vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz
cd vanta-v1.0-RC1-x86_64-unknown-linux-gnu

# 3. Install to isolated prefix
mkdir -p "$HOME/vanta_eval"
./install.sh --prefix="$HOME/vanta_eval"

# 4. Add to PATH for this shell session
export PATH="$HOME/vanta_eval/bin:$PATH"

# 5. Confirm version
vanta --version
# Expected: vanta 1.0.0-rc.1
vantac --version
# Expected: vantac 1.0.0-rc.1
```

Now, navigate to an empty directory outside the release package:
```bash
mkdir -p "$HOME/vanta_test_workspace"
cd "$HOME/vanta_test_workspace"
```

---

### Test 1: Basic Executable & Exit Status
Create `test1_exit.va`:
```vanta
fn main() -> i64 {
    return 42;
}
```
**Execution Command**:
```bash
vanta run test1_exit.va
echo "Exit code: $?"
```
**Expected Output**:
```
Exit code: 42
```

---

### Test 2: Functions & Deep Recursion
Create `test2_fib.va`:
```vanta
fn fib(n: i64) -> i64 {
    if n <= 1 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> i64 {
    let res = fib(10);
    print_i64(res);
    print_str("\n");
    return 0;
}
```
**Execution Command**:
```bash
vanta run test2_fib.va
```
**Expected Output**:
```
55
```

---

### Test 3: Struct Layout & Aggregate Returns (System V AMD64 ABI)
Exercises 24-byte struct returning by value via hidden `sret` pointer:
Create `test3_struct.va`:
```vanta
struct Transform {
    x: i64,
    y: i64,
    z: i64,
}

impl Transform {
    fn scale(self: &Self, factor: i64) -> Transform {
        return Transform {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        };
    }
}

fn main() -> i64 {
    let t = Transform { x: 10, y: 20, z: 30 };
    let scaled = t.scale(2);
    print_i64(scaled.x);
    print_str(" ");
    print_i64(scaled.y);
    print_str(" ");
    print_i64(scaled.z);
    print_str("\n");
    return 0;
}
```
**Execution Command**:
```bash
vanta run test3_struct.va
```
**Expected Output**:
```
20 40 60
```

---

### Test 4: Loops & Control Flow with Break
Create `test4_loop.va`:
```vanta
fn main() -> i64 {
    let mut sum = 0;
    let mut i = 1;
    while i <= 100 {
        if i == 51 {
            break;
        }
        sum = sum + i;
        i = i + 1;
    }
    print_i64(sum);
    print_str("\n");
    return 0;
}
```
**Execution Command**:
```bash
vanta run test4_loop.va
```
**Expected Output**:
```
1275
```

---

### Test 5: Standard-Library File I/O
Exercises `std::fs` and `std::result::Result`:
Create `test5_io.va`:
```vanta
import std::fs;
import std::io;
import std::result::Result;

fn main() -> i64 {
    let write_res = fs::write("evaluation_message.txt", "Vanta v1.0-RC1 verified successfully!\n");
    match write_res {
        Result::Ok(_) => {}
        Result::Err(_) => {
            return 1;
        }
    }

    let read_res = fs::read_to_string("evaluation_message.txt");
    match read_res {
        Result::Ok(content) => {
            io::write_stdout(content.as_str());
        }
        Result::Err(_) => {
            return 2;
        }
    }

    return 0;
}
```
**Execution Command**:
```bash
vanta run test5_io.va
```
**Expected Output**:
```
Vanta v1.0-RC1 verified successfully!
```

---

### Test 6: Compile-Time Error Reporting & Borrow Check
Verify that Vanta catches borrow checker violations at compile time with machine-readable diagnostics.
Create `test6_invalid.va`:
```vanta
fn main() -> i64 {
    let mut arr = [1, 2, 3];
    let slice = &arr;
    arr[0] = 99; // Error: Cannot mutate array while borrowed
    return 0;
}
```
**Execution Command**:
```bash
vanta check test6_invalid.va --json
```
**Expected Output (Exit code 1)**:
Outputs a structured JSON array containing `E0202` (borrow conflict error) with exact file path, line, and column numbers:
```json
[
  {
    "code": "E0202",
    "severity": "error",
    "message": "Cannot mutate 'arr' because it is borrowed",
    "location": {
      "line": 4,
      "column": 5
    }
  }
]
```

---

### Test 7: Standalone Compilation to Native Executable
Compile directly to a standalone native ELF64 executable and inspect it with system tools:
```bash
vanta compile test3_struct.va -o standalone_prog
./standalone_prog
# Output: 20 40 60

# Inspect file type:
file standalone_prog
# Expected: standalone_prog: ELF 64-bit LSB executable, x86-64 ...

# Inspect executable size:
ls -lh standalone_prog
# Expected: ~22 KB
```

---

## 4. Evaluator Feedback Submission

We invite all evaluators to submit their observations to the Vanta engineering team. Please include:

1. **Host Environment**: Output of `uname -srm` and Linux distribution version.
2. **CPU Model**: Output of `lscpu | grep 'Model name'`.
3. **Artifact Integrity**: Output of `sha256sum vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz`.
4. **Compiler Version**: Output of `vanta --version` and `vantac --version`.
5. **Observed vs. Expected Results**: Confirmation of test passes or exact error output if any step diverged.
