# Vanta Custom x86-64 Backend Design
Target: Linux x86-64, ELF64, System V AMD64 ABI

---

## 1. Principles

1. **Self-Contained Native Generation**:
   No reliance on LLVM, Cranelift, GCC, or external assemblers for machine-code generation. The compiler must directly emit machine instruction bytes and valid ELF64 relocatable object files (`.o`).
2. **Deterministic Correctness**:
   Every instruction encoded adheres strictly to Intel/AMD x86-64 manual definitions.
3. **Strict Calling Convention Compliance**:
   Strict implementation of the System V AMD64 ABI (argument passing, callee-saved register preservation, 16-byte stack alignment).

---

## 2. Compiler Backend Pipeline

```
          [Typed AST / Typed HIR]
                     │
                     ▼
             [Vanta Mid IR (MIR)]
        (SSA / CFG of Basic Blocks)
                     │
          [MIR Verifier & Optimizer]
                     │
                     ▼
           [Instruction Selection]
   (Lower MIR instructions to Machine IR)
                     │
                     ▼
            [Machine IR (LIR)]
    (x86-64 opcodes with Virtual Registers)
                     │
                     ▼
           [Register Allocation]
  (Allocate physical registers RAX..R15,
   handle spilling to stack slots)
                     │
                     ▼
         [Stack Frame Construction]
   (Prologue/Epilogue, 16-byte alignment,
    callee-saved regs, local slots)
                     │
                     ▼
      [x86-64 Machine Code Encoder]
    (Pure Rust byte-level instruction encoder)
                     │
                     ▼
          [ELF64 Object File Writer]
   (Emit ELF Header, .text, .symtab, .strtab,
    .shstrtab, .rela.text relocations)
                     │
                     ▼
      [System Linker: ld / gcc driver]
           (Produce native binary)
```

---

## 3. x86-64 Machine Code Encoding

The custom instruction encoder translates abstract machine instructions into x86-64 binary byte sequences:

1. **Prefixes**:
   - `0x66`: Operand size override (16-bit).
   - REX Prefix (`0x40` | `W` | `R` | `X` | `B`):
     - `W`: 64-bit operand size.
     - `R`: Extension of ModR/M `reg` field (e.g. `%r8`..`%r15`).
     - `X`: Extension of SIB `index` field.
     - `B`: Extension of ModR/M `r/m` field or opcode `reg` field.
2. **Opcode**:
   - 1-byte, 2-byte (`0x0F`), or 3-byte opcodes.
3. **ModR/M**:
   - `[mod: 2 bits][reg/opcode: 3 bits][r/m: 3 bits]`
4. **SIB** (Scale-Index-Base):
   - `[scale: 2 bits][index: 3 bits][base: 3 bits]`
5. **Displacement**:
   - None, 8-bit sign-extended, or 32-bit sign-extended.
6. **Immediate**:
   - 8-bit, 16-bit, 32-bit sign-extended, or 64-bit.

---

## 4. System V AMD64 ABI Implementation

### 4.1 Register Usage
- **Argument Passing (Integer/Pointer)**:
  1st: `%rdi`, 2nd: `%rsi`, 3rd: `%rdx`, 4th: `%rcx`, 5th: `%r8`, 6th: `%r9`. Additional arguments passed on stack.
- **Return Values**:
  `%rax` for primary 64-bit value; `%rdx` for secondary.
- **Callee-Saved (Preserved across calls)**:
  `%rbx`, `%rsp`, `%rbp`, `%r12`, `%r13`, `%r14`, `%r15`.
- **Caller-Saved (Scratch)**:
  `%rax`, `%rcx`, `%rdx`, `%rsi`, `%rdi`, `%r8`, `%r9`, `%r10`, `%r11`.

### 4.2 Stack Alignment
The stack frame layout enforces that immediately before any `call` instruction, `(%rsp + 8)` is a multiple of 16 (i.e. `%rsp` is 16-byte aligned after the return address is pushed).

---

## 5. ELF64 Relocatable Object File Format

The custom ELF64 object writer constructs a valid ELF relocatable file (`ET_REL`):
1. **ELF Header**:
   - `EI_MAG`: `\x7fELF`
   - `EI_CLASS`: `ELFCLASS64` (2)
   - `EI_DATA`: `ELFDATA2LSB` (1, little-endian)
   - `e_machine`: `EM_X86_64` (62)
2. **Sections**:
   - `.text`: Machine code bytes.
   - `.data`: Initialized global data.
   - `.rodata`: Read-only data (constants, string literals).
   - `.symtab`: Symbol table (global functions, local symbols).
   - `.strtab`: String table for symbol names.
   - `.shstrtab`: Section header string table.
   - `.rela.text`: Relocation entries for function calls and global references (`R_X86_64_PLT32`, `R_X86_64_PC32`).
