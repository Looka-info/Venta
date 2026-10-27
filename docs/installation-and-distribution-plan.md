# Vanta: Installation, Packaging, and Distribution Plan
**Date:** October 27, 2026  
**Status:** Approved Engineering Directive  
**Objective:** Transforming Vanta from a Developer-Workspace Prototype into a Standalone Global Package

---

## 1. Audit of the Current Installation Experience

Today, installing and running Vanta requires an intimate knowledge of Rust, Cargo, and Linux development tooling:

```bash
# What an external developer must currently do:
git clone https://github.com/vanta-lang/vanta
cd Venta
cargo build --release
sudo cp target/release/vantac /usr/local/bin/
```

### Critical Friction Points Identified:
1. **Requires Rust & Cargo:** Users cannot install Vanta as a standalone programming language. They must already have a working Rust 1.80+ toolchain installed.
2. **Requires Host `gcc`:** When `vantac` compiles an executable (`EmitTarget::Exe`), it writes a temporary `.rt.c` source file and invokes the host system's `gcc` compiler driver. A user on a minimal container without `gcc` cannot produce executables.
3. **Repeated Runtime Recompilation:** On **every single compilation of an executable**, `vantac` invokes `gcc` to recompile `vanta_rt.c` from C source text. This adds 150ms–200ms of unnecessary latency to every build.
4. **No Prebuilt Binaries:** There are no GitHub Release artifacts, no `.tar.gz` archives, and no precompiled standalone binaries.
5. **No Package Manager Integration:** No Homebrew (`brew install vanta`), no Debian APT package, no Arch PKGBUILD.
6. **Platform Limitation:** Strictly limited to Linux x86-64 (or Windows via WSL2). Native Windows (PE/COFF) and macOS (Mach-O) cannot be targeted.

---

## 2. Proposed Standalone Distribution Architecture

To eliminate the Rust and GCC build dependencies for end users, Vanta must be distributed as a self-contained, prebuilt binary package:

```
vanta-0.1.0-x86_64-unknown-linux-gnu/
├── bin/
│   ├── vantac                  # Prebuilt Vanta compiler binary
│   └── vanta                   # Unified developer CLI driver
├── lib/
│   └── vanta/
│       ├── libvanta_rt.a       # Precompiled C runtime static archive (NO gcc compilation needed!)
│       └── std/                # Standard library source modules
│           ├── mod.va
│           ├── option.va
│           ├── result.va
│           ├── iter.va
│           └── string.va
└── share/
    ├── man/man1/vantac.1       # Manual page
    └── vscode/vanta-syntax/    # TextMate syntax highlighting for VS Code
```

### Architectural Key: Precompiled `libvanta_rt.a`
Instead of embedding `vanta_rt.c` as a raw string and invoking `gcc` to compile it every time:
1. Precompile `vanta_rt.c` once into `libvanta_rt.a` during release packaging:
   ```bash
   gcc -O3 -fno-stack-protector -c runtime/vanta_rt.c -o runtime/vanta_rt.o
   ar rcs lib/vanta/libvanta_rt.a runtime/vanta_rt.o
   ```
2. When linking executables, invoke the system linker directly:
   ```bash
   ld -no-pie <input.o> -L/usr/local/lib/vanta -lvanta_rt -lc -o <output>
   ```
   (or invoke `gcc` without recompiling C source files).

---

## 3. Single-Command Standalone Installer

Provide a secure, standard POSIX installer script:

```bash
curl -sSf https://vanta-lang.org/install.sh | sh
```

### The `install.sh` Workflow:
1. Detect host architecture and operating system (`uname -m`, `uname -s`).
   - If not Linux x86-64, explain that Linux x86-64 is the primary supported target and recommend WSL2 on Windows.
2. Download verified release tarball from GitHub Releases:
   `https://github.com/vanta-lang/vanta/releases/download/v0.1.0/vanta-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`
3. Verify SHA-256 checksum against signed checksum manifest.
4. Unpack into `$HOME/.vanta` (or `/usr/local` if run with sudo).
5. Append `export PATH="$HOME/.vanta/bin:$PATH"` to the user's shell profile (`.bashrc`, `.zshrc`).
6. Verify installation:
   ```bash
   $ vanta --version
   vanta 0.1.0 (x86_64-unknown-linux-gnu)
   ```

---

## 4. The Unified `vanta` Developer CLI

Currently, developers invoke `vantac` directly. To provide an intuitive first-use experience, Vanta should offer a single unified CLI (`vanta`):

```bash
# 1. Check version
vanta --version

# 2. Scaffold a new project
vanta new hello_world
cd hello_world

# 3. Project layout created by 'vanta new':
#    hello_world/
#    ├── vanta.toml           # Project manifest (name, version, author)
#    ├── src/
#    │   └── main.va          # Entrypoint source
#    └── tests/
#        └── basic_test.va    # Starter test

# 4. Fast semantic check
vanta check

# 5. Format code canonically
vanta fmt

# 6. Execute native binary
vanta run
```

---

## 5. Platform Support Sequence & Roadmap

| Tier | Target Platform | Object Format | Linker | Target Status | Planned Milestone |
|---|---|---|---|:---:|:---:|
| **Tier 1** | **Linux x86-64** | ELF64 | System `ld` / `gcc` | **Supported (Current)** | Milestone 1–14 |
| **Tier 2** | **Windows x86-64 (Native)** | PE/COFF | MSVC `link.exe` or `lld-link` | Planned | Milestone 19 |
| **Tier 2** | **macOS ARM64 (Apple Silicon)** | Mach-O 64 | Apple `ld64` / `lld` | Planned | Milestone 20 |
| **Tier 3** | **Linux aarch64 (ARM64)** | ELF64 | System `ld` | Planned | Milestone 21 |
| **Tier 3** | **WebAssembly (wasm32)** | Wasm Object | `wasm-ld` | Exploratory | Milestone 22+ |

### Execution Strategy for Windows Native:
1. Implement a pure-Rust PE/COFF relocatable object writer (`crates/vanta_codegen_pe_coff`).
2. Implement Microsoft x64 Calling Convention (`%rcx`, `%rdx`, `%r8`, `%r9` for first 4 arguments; 32-byte shadow stack space).
3. Port C runtime primitives to Windows APIs (`WriteFile` instead of POSIX `write`).

---

## 6. Release Checklist for Public Alpha (v0.1.0-alpha)

- [ ] GitHub Actions CI workflow building release tarballs on `ubuntu-24.04`.
- [ ] SHA-256 checksums published for all release artifacts.
- [ ] Precompiled `libvanta_rt.a` bundled in archive.
- [ ] `vantac --version` flag implemented and reporting clean version string.
- [ ] POSIX `install.sh` script hosted and tested on clean Ubuntu, Debian, Fedora, and Arch installations.
- [ ] `vanta new` project template generator implemented.
- [ ] MIT and Apache-2.0 license headers verified on all crates and distribution files.
