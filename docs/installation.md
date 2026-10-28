# ⚡ Official Vanta Installation Guide

This guide details all official methods to install the **Vanta Systems Programming Language** and its native compiler toolchain.

---

## 📦 Method 1: Install via Node / NPM (Recommended for Quick Setup)

You can install Vanta globally on any system with Node.js installed using `npm`:

```bash
npm install -g vanta-lang
```

### Run directly without installation via `npx`:
```bash
npx vanta-lang --version
npx vanta-lang main.va -o main.s
gcc -no-pie main.s -o main
./main
```

### Install from cloned repository:
```bash
git clone https://github.com/Looka-info/Venta.git
cd Venta
npm link
```

---

## 🚀 Method 2: Universal 1-Line Shell Installer (`curl | bash`)

The standard systems installation method (similar to `rustup` or `bun`):

```bash
curl -fsSL https://raw.githubusercontent.com/Looka-info/Venta/main/scripts/install.sh | bash
```

### Custom Installation Prefix:
```bash
curl -fsSL https://raw.githubusercontent.com/Looka-info/Venta/main/scripts/install.sh | bash -s -- --prefix ~/.local
```

The installer automatically:
1. Detects your system architecture (Linux x86-64 / WSL).
2. Provisions the latest stable release binaries (`vanta` and `vantac`).
3. Installs the native runtime library (`libvanta_rt.a`) and standard library (`std`).
4. Verifies installation permissions.

---

## 📥 Method 3: Pre-compiled Release Tarball (GitHub Releases)

Download pre-compiled release archives directly from GitHub:

```bash
# 1. Download release archive
wget https://github.com/Looka-info/Venta/releases/download/v1.0-RC2/vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz

# 2. Extract tarball
tar -xzf vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz
cd vanta-v1.0-RC2-x86_64-unknown-linux-gnu

# 3. Run installer
./install.sh --prefix ~/.local
```

---

## 🔨 Method 4: Build from Source

Build both the pure Vanta self-hosted compiler and the reference toolchain from source:

```bash
git clone https://github.com/Looka-info/Venta.git
cd Venta

# Build the pure Vanta compiler (bin/vanta)
./scripts/build_self_hosted_compiler.sh

# Or build the Rust bootstrap reference compiler (target/release/vantac)
cargo build --release
```

---

## 🔍 Verifying Your Installation

Confirm that both compilers are available on your `PATH`:

```bash
vanta --version
vantac --version
```

Output:
```text
vanta 1.0.0 (x86_64-unknown-linux-gnu, pure vanta)
vantac 1.0.0-rc.2
```

---

## 🎨 VS Code Editor Setup

Install official syntax highlighting and language configuration:

### Linux / macOS:
```bash
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/vanta-lang
```

### Windows (PowerShell):
```powershell
New-Item -ItemType SymbolicLink -Path "$HOME\.vscode\extensions\vanta-lang" -Target "$PWD\editors\vscode"
```
Restart VS Code, and `.va` / `.vanta` files will have full syntax highlighting and bracket matching!
