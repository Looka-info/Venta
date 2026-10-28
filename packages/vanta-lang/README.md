# ⚡ vanta-lang

**Official Node.js distribution of the Vanta Systems Programming Language.**

Vanta is a performance-first, memory-safe systems programming language featuring a custom SSA optimizer and native x86-64 backend (no LLVM / Cranelift).

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

### Compile a Vanta program:
```bash
vanta main.va -o output.s
gcc -no-pie output.s -o my_app
./my_app
```

### Full Compiler Toolchain (`vantac`):
```bash
vantac main.va -o my_app
./my_app
```

---

## 📖 Links

* **Official Repository:** [https://github.com/Looka-info/Venta](https://github.com/Looka-info/Venta)
* **Documentation:** [https://github.com/Looka-info/Venta/tree/main/docs](https://github.com/Looka-info/Venta/tree/main/docs)
* **License:** MIT OR Apache-2.0
