# Vanta v1.0-RC1 Release Identity & Versioning Strategy

## 1. Executive Summary & Purpose

This document establishes the official release identity, version numbering conventions, artifact naming schemes, and component mappings for **Vanta v1.0-RC1 (Release Candidate 1)**.

Before Milestone 26, the compiler repository used `0.1.0` as an internal placeholder across `Cargo.toml` and `--version` outputs while milestone documentation and packaging referred to `v1.0-RC1`. This document resolves that inconsistency by establishing a unified, canonical release identity across the entire repository and distribution toolchain.

---

## 2. Release Identity Specification

The canonical release identity across all distribution channels is defined as follows:

| Context | Exact Identifier | Standard / Convention |
|:---|:---|:---|
| **Git Release Tag** | `v1.0-RC1` | Standard Git release tag convention (`v<MAJOR>.<MINOR>-<PRERELEASE>`) |
| **Cargo Package Version** | `1.0.0-rc.1` | Strictly compliant with **SemVer 2.0.0** required by Cargo and crates.io |
| **Compiler CLI Version (`vantac -V`)** | `vantac 1.0.0-rc.1` | Emitted by `vantac --version` and `vantac -V` |
| **Driver CLI Version (`vanta -V`)** | `vanta 1.0.0-rc.1` | Emitted by `vanta --version` and `vanta -V` |
| **Target Architecture** | `x86_64-unknown-linux-gnu` | Canonical LLVM/Rust target triple for Linux x86-64 |
| **Release Archive (Tarball)** | `vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz` | Distribution archive containing binaries, runtime, std, and scripts |
| **Cryptographic Checksum** | `vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz.sha256` | SHA-256 verification hash |
| **Default Install Prefix** | `~/.local` | Configurable via `--prefix=<dir>` in `install.sh` |
| **C Runtime Library** | `libvanta_rt.a` | Static archive compiled from `runtime/vanta_rt.c` with `-O2 -fPIC` |

---

## 3. Version Mapping & Semantic Versioning Justification

### 3.1 Why `1.0.0-rc.1` in `Cargo.toml`?
Rust's `cargo` enforces strict adherence to Semantic Versioning 2.0.0. Under SemVer 2.0:
- Version strings must follow `MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]`.
- Letters and hyphens are permitted only in the pre-release section following a hyphen.
- Therefore, `v1.0-RC1` is not a valid Cargo version string (because it lacks the patch level `0` and contains a leading `v`).
- The canonical SemVer 2.0.0 representation of Release Candidate 1 for version 1.0 is `1.0.0-rc.1`.

### 3.2 Why `v1.0-RC1` for Tags and Distribution Archives?
By industry and project convention, human-facing Git release tags and distribution tarball filenames use:
- A leading `v` for tag clarity: `v1.0-RC1`.
- Concise version notation omitting the zero-patch for initial major releases: `v1.0-RC1`.
- Full target triple qualification: `vanta-v1.0-RC1-x86_64-unknown-linux-gnu.tar.gz`.

### 3.3 Elimination of the `0.1.0` Inconsistency
Leaving `0.1.0` in `Cargo.toml` caused `vantac --version` to print `vantac 0.1.0`. For external systems developers evaluating the release candidate, this created immediate ambiguity regarding whether they were running an early alpha build or the audited candidate. Setting the workspace version to `1.0.0-rc.1` eliminates this confusion.

---

## 4. Component Version Inventory

Every crate and artifact in the workspace is verified to reflect this unified release identity:

1. **Root Manifest ([`Cargo.toml`](file:///Cargo.toml))**:
   ```toml
   [workspace.package]
   version = "1.0.0-rc.1"
   edition = "2021"
   ```
2. **Workspace Crates**:
   All 9 member crates inherit `version.workspace = true`:
   - `crates/vanta_source/Cargo.toml`
   - `crates/vanta_diagnostics/Cargo.toml`
   - `crates/vanta_lexer/Cargo.toml`
   - `crates/vanta_parser/Cargo.toml`
   - `crates/vanta_sema/Cargo.toml`
   - `crates/vanta_mir/Cargo.toml`
   - `crates/vanta_codegen_x86_64/Cargo.toml`
   - `crates/vanta_driver/Cargo.toml`
   - `crates/vanta_fmt/Cargo.toml`
3. **Compiler Drivers**:
   - `vantac` uses `env!("CARGO_PKG_VERSION")`, yielding `vantac 1.0.0-rc.1`.
   - `vanta` uses `env!("CARGO_PKG_VERSION")`, yielding `vanta 1.0.0-rc.1`.
4. **Packaging Script ([`scripts/package_release.sh`](file:///scripts/package_release.sh))**:
   ```bash
   VERSION="1.0-RC1"
   TARGET="x86_64-unknown-linux-gnu"
   PKG_NAME="vanta-v${VERSION}-${TARGET}"
   ```
5. **Install Script ([`scripts/install.sh`](file:///scripts/install.sh))**:
   Queries the installed compiler dynamically to report:
   ```
   === Installation Complete ===
   Installed: vantac 1.0.0-rc.1
   Installed: vanta 1.0.0-rc.1
   ```
