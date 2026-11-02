# ⚡ vanta-lang

**Official Node.js distribution of the Vanta Systems Programming Language.**

Vanta is a performance-first, memory-safe systems programming language featuring a custom SSA optimizer, multi-platform native backends (Linux x86-64, Windows x86-64, macOS Apple Silicon ARM64), and a self-contained pure-Rust linker (no LLVM / Cranelift).

---

## 🧭 Supported Platforms

| Platform | Target Architecture | Executable Format | Toolchain Backend |
| :--- | :--- | :--- | :--- |
| **Linux** | `x86_64-unknown-linux-gnu` | Static ELF64 | `vanta_codegen_x86_64` + `vanta_linker::elf` |
| **Windows** | `x86_64-pc-windows-msvc` | Standalone PE32+ (`.exe`) | `vanta_codegen_x86_64` + `vanta_linker::pe` |
| **macOS** | `aarch64-apple-darwin` | Standalone Mach-O 64 | `vanta_codegen_aarch64` + `vanta_linker::macho` |

---

## 🚀 Quick Install

Install globally via `npm`:

```bash
npm install -g vanta-lang
```

Or run directly without installation via `npx`:

```bash
npx vanta-lang --version
```

---

## 🛠 Usage

### Run a Vanta program immediately:
```bash
vanta run main.va
```

### Compile to a standalone native executable (Zero External Linker):
```bash
vantac compile main.va -o my_app --linker=internal
./my_app
```

### Cross-compile for other targets:
```bash
# Target Linux
vantac compile main.va -o app_linux --target=linux

# Target Windows
vantac compile main.va -o app_win.exe --target=windows

# Target macOS Apple Silicon
vantac compile main.va -o app_mac --target=macos
```

---

## 📖 Links

* **Official Repository:** [https://github.com/Looka-info/Venta](https://github.com/Looka-info/Venta)
* **Documentation:** [https://github.com/Looka-info/Venta/tree/main/docs](https://github.com/Looka-info/Venta/tree/main/docs)
* **Installation Guide:** [https://github.com/Looka-info/Venta/blob/main/docs/installation.md](https://github.com/Looka-info/Venta/blob/main/docs/installation.md)
* **Platform & ABI Specification:** [https://github.com/Looka-info/Venta/blob/main/docs/abi-and-platforms.md](https://github.com/Looka-info/Venta/blob/main/docs/abi-and-platforms.md)
* **License:** MIT OR Apache-2.0
