# Vanta ABI and Platform Support Specification

---

## 1. Supported Platform Matrix

| Platform | Target Triple | Object Format | Calling Convention | Status |
| :--- | :--- | :--- | :--- | :--- |
| **Linux x86-64** | `x86_64-unknown-linux-gnu` | ELF64 | System V AMD64 ABI | **Primary (v1.0+)** |
| **Windows x86-64** | `x86_64-pc-windows-msvc` | PE/COFF | Microsoft x64 ABI | **Supported (v1.4+)** |
| **macOS Apple Silicon** | `aarch64-apple-darwin` | Mach-O 64 | Apple AAPCS64 ABI | **Supported (v1.4+)** |

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
| `%r11` | Caller-saved | Scratch / Indirect call callee address |
| `%rbx` | Callee-saved | Preserved |
| `%rsp` | Callee-saved | Stack pointer (16-byte aligned before `call`) |
| `%rbp` | Callee-saved | Frame pointer / Preserved |
| `%r12` | Callee-saved | Preserved |
| `%r13` | Callee-saved | Preserved |
| `%r14` | Callee-saved | Preserved |
| `%r15` | Callee-saved | Preserved |

---

## 3. Microsoft x64 ABI Specification (Windows x86-64)

### 3.1 Register Classification and Positional Slots
The Microsoft x64 ABI uses a strict 4-slot positional argument convention:
| Slot | Integer Register | Floating-Point / Vector |
| :--- | :--- | :--- |
| Slot 1 | `%rcx` | `%xmm0` |
| Slot 2 | `%rdx` | `%xmm1` |
| Slot 3 | `%r8`  | `%xmm2` |
| Slot 4 | `%r9`  | `%xmm3` |

Arguments beyond the 4th slot are placed on the stack starting at `[rsp + 32]`.

### 3.2 Caller Shadow Space (Home Space)
Every caller is required by the Windows x64 ABI to allocate at least 32 bytes of shadow space (`4 * 8` bytes) immediately above the return address prior to executing the `call` instruction. The callee may use this space to spill the incoming register arguments.

### 3.3 Callee-Saved Registers
Unlike System V AMD64, the Microsoft x64 ABI designates both `%rsi` and `%rdi` as **callee-saved** (non-volatile). Vanta's linear scan register allocator respects this distinction, preserving `%rsi` and `%rdi` across function calls when targeting Windows.

### 3.4 Hidden Return Buffer (`sret`)
For aggregates and values requiring memory returns, a hidden pointer is passed as the first parameter in `%rcx`. All user-defined parameters shift right by one positional slot.

### 3.5 Exception Handling & Unwind Metadata
Vanta emits 64-bit PE/COFF `.obj` files with:
- `.pdata`: `IMAGE_RUNTIME_FUNCTION_ENTRY` records (BeginAddress, EndAddress, UnwindInfoAddress).
- `.xdata`: `UNWIND_INFO` records describing stack allocation, frame register usage, and saved non-volatile registers.

---

## 4. Apple Silicon Darwin AAPCS64 ABI (macOS ARM64)

### 4.1 Register Usage
| Register | Classification | Purpose |
| :--- | :--- | :--- |
| `x0`–`x7` | Caller-saved | Arguments 1–8, Return values (`x0`/`x1`) |
| `x8` | Caller-saved | Indirect result location (`sret`) |
| `x9`–`x15` | Caller-saved | Scratch / Temporary |
| `x16`–`x17` | Caller-saved | Intra-procedure call scratch / trampolines |
| `x18` | **Reserved** | Platform register (reserved by Apple Darwin kernel/OS, never allocated) |
| `x19`–`x28` | Callee-saved | Preserved across calls |
| `x29` (`fp`) | Callee-saved | Frame pointer |
| `x30` (`lr`) | Caller-saved | Link register (return address) |
| `sp` | Stack pointer | Always 16-byte aligned |
| `xzr` | Read-only zero | Discards writes, reads as 0 |

### 4.2 Stack Frames and Linkage
Stack frames follow Apple ARM64 conventions:
```aarch64
stp x29, x30, [sp, -frame_size]!
mov x29, sp
...
ldp x29, x30, [sp], frame_size
ret
```

### 4.3 Mach-O 64-bit Object File Conventions
Relocatable objects conform to Mach-O 64-bit format (`MH_MAGIC_64 = 0xFEEDFACF`, `CPU_TYPE_ARM64 = 0x0100000C`):
- All C and runtime global symbols carry a leading underscore prefix (`_main`, `_vanta_alloc`).
- Sections are placed in segment `__TEXT` (`__text`, `__const`).
- Standard relocation types (`ARM64_RELOC_BRANCH26`, `ARM64_RELOC_PAGE21`, `ARM64_RELOC_PAGEOFF12`, `ARM64_RELOC_UNSIGNED`) are emitted by the pure-Rust Mach-O writer.

---

## 5. CLI Target Selection and Aliases

The compiler supports explicit cross-compilation targeting via `--target`:

```bash
# Windows x86-64 COFF object
ventac --target=x86_64-pc-windows-msvc --emit=obj main.va -o main.obj

# Apple Silicon macOS Mach-O object
ventac --target=aarch64-apple-darwin --emit=obj main.va -o main.o

# Target aliases
ventac --target=windows --emit=obj main.va
ventac --target=macos --emit=obj main.va
ventac --target=apple-silicon --emit=obj main.va
ventac --target=linux --emit=obj main.va
```

---

## 6. Self-Contained Native Linker & Executable Emission (v2.0+)

Starting with Venta v2.0 (Milestone 31), the compiler includes an integrated pure-Rust native linker (`--linker internal`, default).

### 6.1 Executable Formats by Target
- **Linux x86-64 (`x86_64-unknown-linux-gnu`)**:
  - Emits loadable static ELF64 executables (`ET_EXEC`).
  - No `PT_INTERP` or dynamic loader requirements.
  - Pure-Rust synthetic runtime provides process startup (`_start`), termination (`sys_exit`), I/O, and page-backed heap management (`sys_mmap`).
- **Windows x86-64 (`x86_64-pc-windows-msvc`)**:
  - Emits PE32+ console executables (`IMAGE_FILE_MACHINE_AMD64 = 0x8664`).
  - Contains `.idata` for `KERNEL32.dll` dynamic imports (`ExitProcess`, `GetStdHandle`, `WriteFile`).
  - Contains `.pdata` and `.xdata` exception unwind directories and `.reloc` base relocations.
- **Apple Silicon macOS (`aarch64-apple-darwin`)**:
  - Emits dyld-compatible `MH_EXECUTE` binaries (`CPU_TYPE_ARM64 = 0x0100000C`).
  - Uses `LC_LOAD_DYLINKER` (`/usr/lib/dyld`) and `LC_LOAD_DYLIB` (`/usr/lib/libSystem.B.dylib`). Note: standard macOS applications cannot be fully static due to Apple kernel dyld requirements.

### 6.2 Linking Commands
```bash
# Compile and link using pure-Rust internal linker
ventac main.va -o program

# Multi-object linking with static archives
ventac main.va --link libmath.a -o program

# Emit link map
ventac main.va --map-file=program.map -o program
```

