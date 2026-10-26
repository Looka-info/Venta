# Vanta ABI and Platform Support Specification

---

## 1. Supported Platform Matrix

| Platform | Target Triple | Object Format | Calling Convention | Status |
| :--- | :--- | :--- | :--- | :--- |
| **Linux x86-64** | `x86_64-unknown-linux-gnu` | ELF64 | System V AMD64 ABI | **Primary (Milestone 1)** |
| Windows x86-64 | `x86_64-pc-windows-msvc` | COFF | Microsoft x64 ABI | Planned (Post-M1) |
| Linux AArch64 | `aarch64-unknown-linux-gnu`| ELF64 | AAPCS64 | Planned (Post-M1) |

---

## 2. System V AMD64 ABI Specification (Linux x86-64)

### 2.1 Register Usage
| Register | Classification | Purpose |
| :--- | :--- | :--- |
| `%rax` | Caller-saved | Return value (1st), temporary |
| `%rdx` | Caller-saved | Return value (2nd), 3rd function argument |
| `%rcx` | Caller-saved | 4th function argument |
| `%rsi` | Caller-saved | 2nd function argument |
| `%rdi` | Caller-saved | 1st function argument |
| `%r8`  | Caller-saved | 5th function argument |
| `%r9`  | Caller-saved | 6th function argument |
| `%r10` | Caller-saved | Static chain / scratch |
| `%r11` | Caller-saved | Scratch |
| `%rbx` | Callee-saved | Preserved |
| `%rsp` | Callee-saved | Stack pointer (16-byte aligned before `call`) |
| `%rbp` | Callee-saved | Frame pointer / Preserved |
| `%r12` | Callee-saved | Preserved |
| `%r13` | Callee-saved | Preserved |
| `%r14` | Callee-saved | Preserved |
| `%r15` | Callee-saved | Preserved |

---

## 3. C ABI Interoperability and Aggregate Argument Passing

Vanta functions marked with standard export symbols follow the C ABI:
- Integer and single-word pointer arguments up to 64 bits are passed in the designated argument registers (`%rdi`, `%rsi`, `%rdx`, `%rcx`, `%r8`, `%r9`).
- String slice descriptors (`&str`, 16 bytes: `(data_pointer: *const u8, byte_length: u64)`) are classified as `INTEGER` class according to System V AMD64 ABI Section 3.2.3 and passed across two consecutive registers (e.g. `%rdi` for data pointer and `%rsi` for byte length).
- Return values of primitive integers and pointers are returned in `%rax`.
- Standard C runtime functions (e.g. `write`, `exit`, `print_i64`, `print_str`) can be linked and invoked directly.
