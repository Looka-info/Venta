# Venta v2.0 Self-Contained Native Linker Architecture

## 1. Executive Summary

Milestone 31 introduces a pure-Rust, self-contained native linker (`vanta_linker`) directly into the Venta compiler toolchain. The linker operates independently of system toolchains, eliminating dependencies on `gcc`, `clang`, `link.exe`, `lld-link`, `ld`, and `ar`.

### Core Capabilities:
- **Multi-Object Linking**: Reads and combines relocatable object files across multiple modules and translation units.
- **Static Archives**: Fully parses and emits deterministic `!<arch>\n` archives (`.a` on Linux/macOS, `.lib` on Windows) with GNU `/`, COFF `/`, and BSD `__.SYMDEF` symbol indexes.
- **Lazy Archive Extraction**: Iteratively pulls in only required archive members to satisfy unresolved external references, resolving transitive dependencies until a fixed point is reached.
- **Strict Symbol Resolution**: Implements strong versus weak symbol precedence, detects and rejects duplicate strong global symbols, and isolates local symbols with identical names across distinct object files.
- **Cross-Platform Target Support**:
  - `x86_64-unknown-linux-gnu`: Emits standalone static ELF64 executables (`ET_EXEC`, `PT_LOAD`, `PT_GNU_STACK`) with a self-contained pure-Rust Linux syscall runtime (`sys_exit`, `sys_write`, `sys_mmap`).
  - `x86_64-pc-windows-msvc`: Emits valid 64-bit PE32+ executables (`IMAGE_FILE_MACHINE_AMD64 = 0x8664`, `PE32_PLUS_MAGIC = 0x020B`, Console subsystem) with synthesized `.idata` import directories for `KERNEL32.dll` (`ExitProcess`, `GetStdHandle`, `WriteFile`), `.pdata`/`.xdata` exception unwind directories, and `.reloc` base relocation page blocks.
  - `aarch64-apple-darwin`: Emits dyld-compatible `MH_EXECUTE` binaries (`CPU_TYPE_ARM64 = 0x0100000C`, `MH_MAGIC_64 = 0xFEEDFACF`) with standard Apple Silicon load commands (`LC_SEGMENT_64` for `__PAGEZERO`, `__TEXT`, `__DATA`, `__LINKEDIT`, `LC_LOAD_DYLINKER`, `LC_MAIN`, `LC_LOAD_DYLIB`, `LC_SYMTAB`).
- **Reproducibility**: Byte-for-byte deterministic binary output without timestamps or random hash-map iteration ordering.
- **Link Maps**: Formatted visual inspection maps detailing input objects, extracted archive members, segment layouts, virtual addresses, symbol addresses, and entry points.

---

## 2. Linker Subsystem Decomposition (`crates/vanta_linker/`)

```
crates/vanta_linker/
├── Cargo.toml
└── src/
    ├── lib.rs              # Public linker API, dispatch, and archive tools
    ├── config.rs           # LinkerConfig, LinkMode, OutputFormat
    ├── error.rs            # LinkerError and LinkResult diagnostics
    ├── input.rs            # Input format detection (Elf64, CoffAmd64, MachO64, Archive)
    ├── object.rs           # Intermediate object representation (ObjectFile)
    ├── section.rs          # Section representation, alignment, and MergedSection
    ├── symbol.rs           # Symbol table, kinds (Function, Data), and visibility
    ├── relocation.rs       # RelocationRecord model
    ├── resolve.rs          # Global symbol resolver, weak/strong precedence, collision checks
    ├── layout.rs           # SegmentLayout: page alignments, virtual addresses, file offsets
    ├── map_file.rs         # Formatted link map generator
    ├── archive.rs          # Pure-Rust ar / .lib parser, symbol index, and deterministic writer
    ├── elf/
    │   ├── mod.rs          # ELF module definitions
    │   ├── reader.rs       # ELF64 relocatable object (.o) parser
    │   ├── reloc.rs        # x86-64 relocation engine (R_X86_64_64, PC32, PLT32, 32, 32S)
    │   ├── runtime.rs      # Synthetic Linux static runtime (_start, exit, alloc, print)
    │   ├── writer.rs       # Loadable ELF64 static executable (ET_EXEC) writer
    │   └── linker.rs       # ELF linking coordinator
    ├── coff/
    │   ├── mod.rs          # COFF module definitions
    │   ├── reader.rs       # AMD64 COFF object (.obj) parser
    │   ├── reloc.rs        # AMD64 COFF relocation engine (ADDR64, ADDR32NB, REL32)
    │   ├── pe_writer.rs    # PE32+ (DOS header, PE header, sections, .idata, .reloc) writer
    │   └── linker.rs       # Windows COFF to PE32+ linking coordinator
    └── macho/
        ├── mod.rs          # Mach-O module definitions
        ├── reader.rs       # ARM64 Mach-O 64-bit object (.o) parser
        ├── reloc.rs        # AAPCS64 relocation engine (UNSIGNED, BRANCH26, PAGE21, PAGEOFF12)
        ├── writer.rs       # Apple Silicon dyld-compatible MH_EXECUTE writer
        └── linker.rs       # macOS Mach-O linking coordinator
```

---

## 3. Implemented Relocation Types by Target

| Target Triple | Relocation Type | Numeric Code | Semantics & Fixup Formula |
|:---|:---|:---:|:---|
| **x86_64 Linux ELF64** | `R_X86_64_64` | 1 | 64-bit absolute: `S + A` |
| | `R_X86_64_PC32` | 2 | 32-bit signed PC-relative: `S + A - P` |
| | `R_X86_64_PLT32` | 4 | 32-bit signed PLT-relative: `S + A - P` |
| | `R_X86_64_32` | 10 | 32-bit unsigned absolute: `S + A` (checked range `0..2^32-1`) |
| | `R_X86_64_32S` | 11 | 32-bit signed absolute: `S + A` (checked range `-2^31..2^31-1`) |
| **x86_64 Windows COFF** | `IMAGE_REL_AMD64_ADDR64` | 1 | 64-bit absolute VA: `S + existing` (emits `.reloc` base relocation entry) |
| | `IMAGE_REL_AMD64_ADDR32` | 2 | 32-bit absolute VA: `S + existing` |
| | `IMAGE_REL_AMD64_ADDR32NB` | 3 | 32-bit RVA: `(S + existing) - ImageBase` (used in `.pdata` and `.xdata`) |
| | `IMAGE_REL_AMD64_REL32` | 4 | 32-bit PC-relative: `S + existing - P - 4` (branch/call displacement) |
| | `IMAGE_REL_AMD64_REL32_1..5`| 5..9 | 32-bit PC-relative with instruction tail offsets: `S + existing - P - 4 - k` |
| **ARM64 Apple Darwin** | `ARM64_RELOC_UNSIGNED` | 0 | 64-bit pointer address: `S + existing` |
| | `ARM64_RELOC_BRANCH26` | 2 | 26-bit branch displacement: `(S - P) >> 2` (encoded into `B` / `BL` instruction) |
| | `ARM64_RELOC_PAGE21` | 3 | 21-bit page displacement: `(S_page - P_page) >> 12` (encoded into `ADRP` `immlo` + `immhi`) |
| | `ARM64_RELOC_PAGEOFF12` | 4 | 12-bit intra-page offset: `S & 0xFFF` (encoded into `ADD` or scaled `LDR`/`STR` immediate) |

---

## 4. Symbol Resolution Rules

1. **Definition Registration**: Each input relocatable object registers its defined symbols.
   - Symbols marked `SymbolVisibility::Local` are kept private to the defining object and never collide with local symbols in other objects.
   - Symbols marked `SymbolVisibility::Global` are registered in the global resolver as strong symbols. If a duplicate strong symbol is encountered from a second file, linking halts with `LinkerError::DuplicateSymbol`.
   - Symbols marked `SymbolVisibility::Weak` are registered only if no strong definition exists. A strong symbol overrides a weak symbol unconditionally.
2. **Lazy Archive Resolution**: Unresolved external symbol references from user objects are matched against static archive symbol indexes (`/` or `__.SYMDEF`).
   - Only the specific archive member containing the defining symbol is extracted.
   - If that member introduces additional unresolved dependencies, extraction continues iteratively until a fixed point is reached.
   - Archive members defining unused functions are excluded from the output binary.
3. **Address Binding**: After section merging, every section contribution is assigned a final virtual address. Relocation entries look up the target symbol's resolved virtual address and calculate fixups.

---

## 5. Memory Layout and Segments

### 5.1 Linux x86-64 ELF64 Static Executable
- Base Virtual Address: `0x400000`
- Segment 1 (`PT_LOAD`, Permissions `PF_R | PF_X`): Contains ELF header (64 bytes), program headers, and merged `.text` section.
- Segment 2 (`PT_LOAD`, Permissions `PF_R`): Merged `.rodata` on a 4KB page boundary.
- Segment 3 (`PT_LOAD`, Permissions `PF_R | PF_W`): Merged `.data` and `.bss` (uninitialized memory sized via `p_memsz`).
- Segment 4 (`PT_GNU_STACK`, Permissions `PF_R | PF_W`): Enforces non-executable stack security.

### 5.2 Windows x86-64 PE32+ Executable
- Preferred ImageBase: `0x140000000`
- `SectionAlignment`: `0x1000` (4096 bytes)
- `FileAlignment`: `0x200` (512 bytes)
- Sections:
  - `.text`: Code (`IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_READ`)
  - `.rdata`: Read-only data (`IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ`)
  - `.data`: Writable globals (`IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE`)
  - `.pdata`: Function runtime exception table entries (`RUNTIME_FUNCTION`)
  - `.xdata`: Exception unwind info records
  - `.idata`: Synthesized import directory, lookup table (ILT), and address table (IAT) for `KERNEL32.dll` (`ExitProcess`, `GetStdHandle`, `WriteFile`)
  - `.reloc`: Base relocation table partitioned into 4KB page blocks with `IMAGE_REL_BASED_DIR64` fixup records.

### 5.3 Apple Silicon ARM64 Mach-O Executable
- Base Virtual Address: `0x100000000`
- Page Size: `0x4000` (16KB)
- Segments:
  - `__PAGEZERO`: Sized at 4GB (`0x100000000`), zero permissions (`VM_PROT_NONE`) to catch NULL pointer dereferences.
  - `__TEXT`: Contains Mach header, load commands, section `__text`, and section `__const`. Permissions: `VM_PROT_READ | VM_PROT_EXECUTE`.
  - `__DATA`: Contains section `__data` and zero-fill section `__bss`. Permissions: `VM_PROT_READ | VM_PROT_WRITE`.
  - `__LINKEDIT`: Contains dyld symbol tables (`nlist_64`) and string tables. Permissions: `VM_PROT_READ`.
  - Load commands: `LC_LOAD_DYLINKER` (`/usr/lib/dyld`), `LC_MAIN` (entry point offset), and `LC_LOAD_DYLIB` (`/usr/lib/libSystem.B.dylib`).
