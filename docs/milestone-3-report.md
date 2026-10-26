# Vanta — Milestone 3 Implementation Report
## Standard Output Intrinsics and UTF-8 String Slices

**Date:** 2026-10-26  
**Target:** Linux x86-64, ELF64, System V AMD64 ABI  
**Compiler Backend:** Custom pure-Rust x86-64 code generator & ELF64 writer (no LLVM/Cranelift)

---

### 1. Architectural Overview & Implemented Files

Milestone 3 adds first-class immutable string slices (`&str`), standard output intrinsics (`print_i64` and `print_str`), `.rodata` emission with RIP-relative string referencing, and differential stdout test verification to Vanta.

#### Files Changed / Created:
- [`crates/vanta_lexer/src/token.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_lexer/src/token.rs): Added `TokenKind::Str` keyword token.
- [`crates/vanta_lexer/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_lexer/src/lib.rs): Added keyword recognition for `"str"`. Preserved escape sequence decoding (`\n`, `\r`, `\t`, `\\`, `\"`, `\0`).
- [`crates/vanta_parser/src/ast.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_parser/src/ast.rs): Added `PrimType::Str` to AST.
- [`crates/vanta_parser/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_parser/src/lib.rs): Added parsing for `str` primitive and `&str` references.
- [`crates/vanta_sema/src/types.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_sema/src/types.rs): Added `Type::Str`. Sized `&str` references as 16 bytes (two 64-bit words, 8-byte aligned). `&str` implements `Copy`. `str` is an unsized dynamically sized type. Implemented `fmt::Display` for diagnostic formatting.
- [`crates/vanta_sema/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_sema/src/lib.rs):
  - Pre-registered built-in intrinsic signatures: `print_i64(i64) -> ()` and `print_str(&str) -> ()`.
  - Disallowed redefinition of intrinsics by user programs with explicit diagnostics.
  - Disallowed bare `str` variables and parameters.
  - Disallowed mutable `&mut str` slices.
  - Issued compile-time diagnostic for user functions attempting to return `&str`.
  - Typed string literals as `&str`.
- [`crates/vanta_mir/src/ir.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/ir.rs):
  - Added `StringLiteral { symbol, bytes }` and `pub string_literals: Vec<StringLiteral>` on `MirProgram`.
  - Added instructions: `Instruction::StrSlice`, `Instruction::StoreSlice`, `Instruction::LoadSlice`.
- [`crates/vanta_mir/src/lower.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/lower.rs):
  - String interning into `.rodata` symbols (`.Lstr_0`, etc.) with embedded NUL preservation.
  - Lowering for string literals and local string slice variables.
  - Lowering calls with two-register decomposition for `&str` parameters (`data_pointer` and `byte_length`).
- [`crates/vanta_mir/src/verify.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/verify.rs): Added invariant checks for `StrSlice`, `StoreSlice`, and `LoadSlice`.
- [`crates/vanta_mir/src/opt/const_prop.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/opt/const_prop.rs) & [`crates/vanta_mir/src/opt/dce.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_mir/src/opt/dce.rs): Safe constant propagation tracking for slice lengths and dead instruction elimination preserving memory writes.
- [`crates/vanta_codegen_x86_64/src/machine_ir.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/machine_ir.rs): Added `MachineInst::LeaRip { dest, symbol }`.
- [`crates/vanta_codegen_x86_64/src/encoding.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/encoding.rs): Added encoder for RIP-relative `lea` with `R_X86_64_PC32` relocation. Added relocation type field (`r_type`) on `Relocation`.
- [`crates/vanta_codegen_x86_64/src/frame.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/frame.rs): Allocated 16-byte slots for `&str`. Handled 2-register unpacking for `&str` parameters in function prologues.
- [`crates/vanta_codegen_x86_64/src/instruction_select.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/instruction_select.rs): Lowered `StrSlice`, `StoreSlice`, and `LoadSlice` to machine instructions.
- [`crates/vanta_codegen_x86_64/src/elf_writer.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_codegen_x86_64/src/elf_writer.rs): Emitted `.rodata` section (`SHT_PROGBITS`, `SHF_ALLOC`), `.rodata` section symbol, local symbols for string literals, and correct `sh_info` on `.symtab`.
- [`runtime/vanta_rt.c`](file:///c:/Users/mumer/OneDrive/Documents/Venta/runtime/vanta_rt.c): Standard runtime helper implementing `print_i64` and `print_str` using Linux `write` syscalls with safe `EINTR` retry loops, partial write advancement, and `i64::MIN` signed-overflow immunity.
- [`crates/vanta_driver/src/lib.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_driver/src/lib.rs): Embedded `runtime/vanta_rt.c` into compiler driver, passing it to `gcc -no-pie` during executable linking.
- [`crates/vanta_driver/tests/e2e_tests.rs`](file:///c:/Users/mumer/OneDrive/Documents/Venta/crates/vanta_driver/tests/e2e_tests.rs): Added differential stdout test harness (`run_differential_stdout_test`) and 18 new integration and negative compile-fail test cases.
- [`examples/06_strings_and_stdout.va`](file:///c:/Users/mumer/OneDrive/Documents/Venta/examples/06_strings_and_stdout.va): Runnable showcase example.

---

### 2. String Slice Representation (`&str`)

In Vanta, a string slice is a 16-byte, 8-byte aligned aggregate:
- **Word 0 (`data_pointer: *const u8`)**: 64-bit pointer addressing UTF-8 bytes stored in the `.rodata` ELF section or heap/stack memory.
- **Word 1 (`byte_length: u64`)**: 64-bit count of UTF-8 bytes in the slice (excluding any null terminator).

#### Properties & Invariants:
1. **Immutability**: String literals and slices are immutable. `&mut str` is rejected at compile time.
2. **Unsized `str`**: Bare `str` cannot be used directly as a sized value (e.g. `let x: str;` produces a compile-time error).
3. **Copy Semantics**: `&str` implements `Copy`, allowing slice descriptors to be duplicated across variables without invalidating the source.
4. **Embedded NUL Safety**: String lengths are determined by `byte_length`, not null-termination. Literals containing embedded NUL (`\0`) preserve all bytes.

---

### 3. Calling Convention & Standard Output Intrinsics

Both intrinsics adhere strictly to the System V AMD64 ABI:

#### 3.1 `print_i64(value: i64)`
- **Register Assignment**: `value` passed in `%rdi`.
- **Implementation**: Converts `int64_t` to ASCII digits in a 32-byte stack buffer without dynamic memory allocation. Handles `i64::MIN` (`-9223372036854775808`) by operating on unsigned magnitude arithmetic to prevent signed overflow UB. Emits bytes to `stdout` (`fd = 1`).

#### 3.2 `print_str(value: &str)`
- **Register Assignment**: The 16-byte slice descriptor is decomposed into two integer registers:
  - `%rdi`: `data_pointer` (`const char *`)
  - `%rsi`: `byte_length` (`uint64_t`)
- **Implementation**: Issues `write(1, ptr, len)`. If `len == 0`, immediately returns without syscall overhead. Retries upon interrupted writes (`EINTR`) and loops until all bytes are written.

---

### 4. ELF64 Output & Relocation Layout

The custom pure-Rust ELF writer emits standard 64-bit relocatable object files:

```
Section Headers:
  [Nr] Name              Type             Address           Offset    Size   Flags  Link  Info  Align
  [ 0]                   NULL             0000000000000000  00000000  00000000          0     0     0
  [ 1] .text             PROGBITS         0000000000000000  00000040  ...        AX     0     0     16
  [ 2] .rodata           PROGBITS         0000000000000000  000001f0  ...         A     0     0     16
  [ 3] .rela.text        RELA             0000000000000000  00000280  ...         I     4     1     8
  [ 4] .symtab           SYMTAB           0000000000000000  00000400  ...               5    10     8
  [ 5] .strtab           STRTAB           0000000000000000  00000550  ...               0     0     1
  [ 6] .shstrtab         STRTAB           0000000000000000  000005a8  ...               0     0     1
```

- **Addressing String Data**: Code accesses literal data via `lea dest, [rip + disp32]` using relocation `R_X86_64_PC32` (relocation type 2) with addend `-4`.
- **Function Calls**: Code invokes `print_i64` and `print_str` via `call disp32` using relocation `R_X86_64_PLT32` (relocation type 4) with addend `-4`.

---

### 5. Verification & Test Evidence

#### Test Commands:
```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets
cargo build --release --bin vantac
```

#### Test Suite Breakdown (46 passed, 0 failed):
- `crates/vanta_codegen_x86_64`: 1 passed (ELF generation)
- `crates/vanta_lexer`: 2 passed (lexing, nested comments)
- `crates/vanta_parser`: 1 passed (fn declaration parsing)
- `crates/vanta_sema`: 2 passed (borrow checking, valid typecheck)
- `crates/vanta_source`: 1 passed (source line/col lookup)
- `crates/vanta_mir`: 1 passed (lowering and verification)
- `crates/vanta_mir/opt_tests`: 12 passed (constant propagation, dead code elimination, loop safety, trap preservation)
- `crates/vanta_driver/e2e_tests`: 26 passed:
  - `test_e2e_print_i64_zero`: verified output `b"0"`
  - `test_e2e_print_i64_positive_and_negative`: verified output `b"42-171-999"`
  - `test_e2e_print_i64_min_and_max`: verified exact output `b"9223372036854775807-9223372036854775808"`
  - `test_e2e_print_str_literals`: verified exact output `b"Hello, Vanta!"`
  - `test_e2e_print_str_empty`: verified zero output bytes
  - `test_e2e_print_str_escapes`: verified escape rendering `b"Line 1\nLine 2\t\"Quoted\" \\ Backslash\n"`
  - `test_e2e_print_str_utf8`: verified multibyte UTF-8 and emojis (`"Привет, мир! 🚀 🦀\n"`)
  - `test_e2e_print_str_embedded_null`: verified exact binary bytes `b"Hello\0World"`
  - `test_e2e_print_str_variable_and_reassignment`: verified local variable updates
  - `test_e2e_print_str_helper_function`: verified user function accepting `&str`
  - `test_e2e_print_loops_and_branches`: verified interleaved output in loops
  - 8 negative compile-fail tests: disallow redefinition of `print_i64`/`print_str`, type mismatches, bare `str`, `&mut str`, and returning `&str`.

---

### 6. Representative Example Program

```vanta
// examples/06_strings_and_stdout.va
fn greet(name: &str, count: i64) {
    print_str("Hello, ");
    print_str(name);
    print_str("! Visit count: ");
    print_i64(count);
    print_str("\n");
}

pub fn main() -> i64 {
    let mut msg: &str = "Welcome to Vanta Systems Language!\n";
    print_str(msg);

    msg = "UTF-8 support: Привет, мир! 🚀 🦀\n";
    print_str(msg);

    greet("Explorer", 42);

    print_str("All tests finished.\n");
    return 0;
}
```

**Emitted Native Execution Output:**
```
Welcome to Vanta Systems Language!
UTF-8 support: Привет, мир! 🚀 🦀
Hello, Explorer! Visit count: 42
All tests finished.
```

---

### 7. Scope Boundaries & Remaining Limitations

- **Returning `&str` from user functions**: Returning a 16-byte slice descriptor across registers (`%rax` and `%rdx`) is not yet enabled for user-defined functions and emits a clear compile-time error.
- **Dynamic Strings**: Heap-allocated, mutable strings (`String`), string slicing syntax (`s[a..b]`), and string concatenation (`+`) are outside the scope of Milestone 3.
- **Array Bounds Checks**: Runtime panicking on out-of-bounds array indexing remains a documented limitation from Milestone 1.

---

### 8. Recommended Next Milestone

**Milestone 4: Heap Allocation, Box<T>, and Deterministic Destructors (Drop)**
- Introduce safe heap allocation (`Box<T>`) backed by standard `malloc` and `free` runtime wrappers.
- Implement automatic deterministic resource destruction when owner variables exit scope.
- Extend borrow-checking to prevent use-after-free and double-free in safe Vanta code.
