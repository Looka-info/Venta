# ⚡ Official Vanta Installation Guide

This guide details all official methods to install the **Vanta Systems Programming Language** and its native multi-platform compiler toolchain.

Vanta natively supports **Linux x86-64**, **Windows x86-64**, and **macOS Apple Silicon (ARM64)** with zero external linker or compiler runtime dependencies.

---

## 🧭 Supported Platforms Matrix

| Operating System | Target Architecture | Binary Format | Default ABI | Toolchain Support |
| :--- | :--- | :--- | :--- | :--- |
| **Linux** (Ubuntu, Debian, Fedora, Arch, WSL2) | `x86_64-unknown-linux-gnu` | Static ELF64 | System V AMD64 | **Tier 1 (Native + Pure-Rust Linker)** |
| **Windows** (10, 11, Server 2022) | `x86_64-pc-windows-msvc` | PE32+ (`.exe`) | Microsoft x64 | **Tier 1 (Native + Pure-Rust Linker)** |
| **macOS** (macOS 14 Sonoma, 15 Sequoia) | `aarch64-apple-darwin` | Mach-O 64-bit | Apple AAPCS64 | **Tier 1 (Native + Pure-Rust Linker)** |

---

## 📦 Method 1: Install via Node / NPM (Recommended for Quick Cross-Platform Setup)

You can install Vanta globally on any system with Node.js installed using `npm`:

```bash
npm install -g vanta-lang
```

### Run directly without global installation via `npx`:
```bash
# Verify version
npx vanta-lang --version

# Run any Vanta file immediately
npx vanta-lang run main.va

# Or compile to native executable
npx vanta-lang compile main.va -o my_app
```

### Install from cloned repository:
```bash
git clone https://github.com/Looka-info/Venta.git
cd Venta
npm link
```

---

## 🚀 Method 2: Universal 1-Line Installers

The fastest native system installation method (similar to `rustup` or `bun`):

### Linux & macOS (Apple Silicon):
```bash
# Install to default directory (~/.local/bin)
curl -fsSL https://raw.githubusercontent.com/Looka-info/Venta/main/scripts/install.sh | bash

# Or specify a custom prefix:
curl -fsSL https://raw.githubusercontent.com/Looka-info/Venta/main/scripts/install.sh | bash -s -- --prefix /usr/local
```

The shell installer automatically:
1. Detects your operating system and hardware architecture:
   - Linux x86-64 (`x86_64-unknown-linux-gnu`)
   - macOS Apple Silicon (`aarch64-apple-darwin`)
2. Provisions the matching native release binaries (`vanta` and `vantac`).
3. Installs the native runtime library (`libvanta_rt.a`) and standard library (`std`).
4. Configures execution permissions and environment profile (`.bashrc` / `.zshrc`).

### Windows (PowerShell):
```powershell
# Run in Windows PowerShell / Terminal:
irm https://raw.githubusercontent.com/Looka-info/Venta/main/scripts/install.ps1 | iex
```

The Windows PowerShell installer automatically:
1. Detects AMD64 / x86-64 Windows environment.
2. Deploys native Windows executables (`ventac.exe`, `vantac.exe`, `venta.exe`, `vanta.exe`).
3. Installs the standard library to `$HOME\.vanta\lib\vanta\std`.
4. Automatically adds `$HOME\.vanta\bin` to your User `PATH` environment variable.

---

## 📥 Method 3: Pre-compiled Standalone Release Archives (GitHub Releases)

Download pre-compiled release packages directly from [GitHub Releases](https://github.com/Looka-info/Venta/releases):

### Linux x86-64:
```bash
# 1. Download release archive
wget https://github.com/Looka-info/Venta/releases/download/v1.0-RC2/vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz

# 2. Extract tarball
tar -xzf vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz
cd vanta-v1.0-RC2-x86_64-unknown-linux-gnu

# 3. Run installer
./install.sh --prefix ~/.local
```

### Windows x86-64:
```powershell
# 1. Download official Windows release archive
Invoke-WebRequest -Uri "https://github.com/Looka-info/Venta/releases/download/v1.0-RC2/vanta-v1.0-RC2-x86_64-pc-windows-msvc.zip" -OutFile "vanta-v1.0-RC2-x86_64-pc-windows-msvc.zip"

# 2. Extract archive
Expand-Archive -Path "vanta-v1.0-RC2-x86_64-pc-windows-msvc.zip" -DestinationPath "$env:TEMP\vanta-extracted"

# 3. Run the installer (supports -Prefix and -NoPathUpdate)
& "$env:TEMP\vanta-extracted\install.ps1" -Prefix "$HOME\.vanta"

# 4. Clean up temporary extraction folder
Remove-Item -Recurse -Force "$env:TEMP\vanta-extracted"

# 5. Verify in PowerShell
vantac --version
vanta --version
```

To safely uninstall Vanta from Windows at any time:
```powershell
# Run the official uninstaller
& "$HOME\.vanta\uninstall.ps1"
```

### macOS Apple Silicon (ARM64):
```bash
# 1. Download macOS Darwin release archive
curl -fsSL -O https://github.com/Looka-info/Venta/releases/download/v1.0-RC2/vanta-v1.0-RC2-aarch64-apple-darwin.tar.gz

# 2. Extract tarball
tar -xzf vanta-v1.0-RC2-aarch64-apple-darwin.tar.gz
cd vanta-v1.0-RC2-aarch64-apple-darwin

# 3. Run installer
./install.sh --prefix ~/.local
```

---

## 🔨 Method 4: Build from Source

You can build the full Vanta multi-platform compiler toolchain on any supported OS with Rust 1.80+ installed:

```bash
# Clone the repository
git clone https://github.com/Looka-info/Venta.git
cd Venta

# Build the reference compiler and unified CLI driver
cargo build --release --bin vantac --bin vanta
```

### Run installation after building:
- **On Linux / macOS**:
  ```bash
  ./scripts/install.sh
  ```
- **On Windows (PowerShell)**:
  ```powershell
  .\scripts\install.ps1
  ```

---

## 🔍 Verifying Your Installation

Confirm that both compilers are available on your system `PATH`:

```bash
vanta --version
vantac --version
```

### Expected Output by Platform:
- **Linux x86-64**:
  ```text
  vanta 1.0.0-rc.2 (x86_64-unknown-linux-gnu)
  vantac 1.0.0-rc.2 (x86_64-unknown-linux-gnu)
  ```
- **Windows x86-64**:
  ```text
  vanta 1.0.0-rc.2 (x86_64-pc-windows-msvc)
  vantac 1.0.0-rc.2 (x86_64-pc-windows-msvc)
  ```
- **macOS Apple Silicon**:
  ```text
  vanta 1.0.0-rc.2 (aarch64-apple-darwin)
  vantac 1.0.0-rc.2 (aarch64-apple-darwin)
  ```

---

## 🎯 Cross-Platform Compilation

Vanta's built-in native linker allows compiling for any supported target without external cross-compilers:

```bash
# Target Linux x86-64 (Static ELF64 executable)
ventac compile main.va -o app_linux --target=linux --linker=internal

# Target Windows x86-64 (Standalone PE32+ .exe)
ventac compile main.va -o app_win.exe --target=windows --linker=internal

# Target macOS Apple Silicon (Mach-O ARM64 executable)
ventac compile main.va -o app_mac --target=macos --linker=internal
```

---

## 🎨 VS Code & Antigravity IDE Setup

Install official syntax highlighting, brackets, and 3D gold lightning file icons:

### Linux / macOS:
```bash
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/vanta-lang
```

### Windows (PowerShell):
```powershell
New-Item -ItemType Junction -Path "$HOME\.vscode\extensions\vanta-lang" -Target "$PWD\editors\vscode"
```

Restart VS Code or reload the window, and all `.va` and `.vanta` files will display full syntax highlighting and official file icons!
