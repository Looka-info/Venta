#!/usr/bin/env bash
set -euo pipefail

# Determine repository root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

VERSION="${1:-1.0-RC2}"
VERSION="${VERSION#v}"

TARGET="${2:-}"
if [ -z "${TARGET}" ]; then
    UNAME_S="$(uname -s)"
    UNAME_M="$(uname -m)"
    if [ "${UNAME_S}" = "Linux" ]; then
        if [ "${UNAME_M}" = "aarch64" ]; then
            TARGET="aarch64-unknown-linux-gnu"
        else
            TARGET="x86_64-unknown-linux-gnu"
        fi
    elif [ "${UNAME_S}" = "Darwin" ]; then
        if [ "${UNAME_M}" = "arm64" ] || [ "${UNAME_M}" = "aarch64" ]; then
            TARGET="aarch64-apple-darwin"
        else
            TARGET="x86_64-apple-darwin"
        fi
    else
        TARGET="x86_64-unknown-linux-gnu"
    fi
fi

PKG_NAME="vanta-v${VERSION}-${TARGET}"
DIST_DIR="${REPO_ROOT}/dist"
STAGE_DIR="${DIST_DIR}/${PKG_NAME}"

echo "=== Packaging Vanta v${VERSION} for ${TARGET} ==="

# 1. Compile release binaries with Cargo
echo "--> Building release binaries (vantac, vanta)..."
cargo build --release --bin vantac --bin vanta

# 2. Compile static runtime archive libvanta_rt.a
echo "--> Building static runtime archive (libvanta_rt.a)..."
TMP_O="/tmp/vanta_rt_pkg_$$.o"
CC_BIN="gcc"
if ! command -v gcc >/dev/null 2>&1; then
    CC_BIN="clang"
fi
"${CC_BIN}" -c -O2 -fPIC "${REPO_ROOT}/runtime/vanta_rt.c" -o "${TMP_O}"
ar rcs "${REPO_ROOT}/runtime/libvanta_rt.a" "${TMP_O}"
rm -f "${TMP_O}"

# 3. Create staging layout
echo "--> Preparing package directory structure..."
mkdir -p "${DIST_DIR}"
rm -rf "${STAGE_DIR}"
mkdir -p "${STAGE_DIR}/bin"
mkdir -p "${STAGE_DIR}/lib/vanta/std"

# Copy binaries
cp -p "${REPO_ROOT}/target/release/vantac" "${STAGE_DIR}/bin/vantac"
cp -p "${REPO_ROOT}/target/release/vanta" "${STAGE_DIR}/bin/vanta"
chmod 0755 "${STAGE_DIR}/bin/vantac" "${STAGE_DIR}/bin/vanta"

# Codesign on macOS if applicable
if [ "$(uname -s)" = "Darwin" ]; then
    codesign -s - -f "${STAGE_DIR}/bin/vantac" 2>/dev/null || true
    codesign -s - -f "${STAGE_DIR}/bin/vanta" 2>/dev/null || true
fi

# Copy runtime archive
cp -p "${REPO_ROOT}/runtime/libvanta_rt.a" "${STAGE_DIR}/lib/vanta/libvanta_rt.a"
chmod 0644 "${STAGE_DIR}/lib/vanta/libvanta_rt.a"

# Copy standard library
cp -r -p "${REPO_ROOT}/library/std/"* "${STAGE_DIR}/lib/vanta/std/"
find "${STAGE_DIR}/lib/vanta/std" -type f -exec chmod 0644 {} +
find "${STAGE_DIR}/lib/vanta/std" -type d -exec chmod 0755 {} +

# Copy installer and uninstaller scripts
cp -p "${REPO_ROOT}/scripts/install.sh" "${STAGE_DIR}/install.sh"
cp -p "${REPO_ROOT}/scripts/uninstall.sh" "${STAGE_DIR}/uninstall.sh"
chmod 0755 "${STAGE_DIR}/install.sh" "${STAGE_DIR}/uninstall.sh"

# Copy docs and license
if [ -f "${REPO_ROOT}/README.md" ]; then
    cp -p "${REPO_ROOT}/README.md" "${STAGE_DIR}/README.md"
fi
if [ -f "${REPO_ROOT}/LICENSE" ]; then
    cp -p "${REPO_ROOT}/LICENSE" "${STAGE_DIR}/LICENSE"
fi

# Copy examples
if [ -d "${REPO_ROOT}/examples" ]; then
    echo "--> Packaging example programs..."
    mkdir -p "${STAGE_DIR}/examples"
    cp -rp "${REPO_ROOT}/examples/"* "${STAGE_DIR}/examples/"
fi

# 4. Create compressed distribution tarball
echo "--> Compressing archive (${PKG_NAME}.tar.gz)..."
TARBALL="${DIST_DIR}/${PKG_NAME}.tar.gz"

if tar --version 2>&1 | grep -q "GNU tar"; then
    tar --sort=name --mtime='2026-10-27 00:00:00Z' --owner=0 --group=0 --numeric-owner -cf - -C "${DIST_DIR}" "${PKG_NAME}" | gzip -n > "${TARBALL}"
else
    tar -czf "${TARBALL}" -C "${DIST_DIR}" "${PKG_NAME}"
fi

# 5. Compute SHA-256
echo "--> Generating SHA-256 checksum..."
if command -v sha256sum >/dev/null 2>&1; then
    (cd "${DIST_DIR}" && sha256sum "${PKG_NAME}.tar.gz" > "${PKG_NAME}.tar.gz.sha256")
else
    (cd "${DIST_DIR}" && shasum -a 256 "${PKG_NAME}.tar.gz" > "${PKG_NAME}.tar.gz.sha256")
fi

# 6. Copy standalone binaries for direct download
if [ "${TARGET}" = "x86_64-unknown-linux-gnu" ]; then
    cp -p "${REPO_ROOT}/target/release/vanta" "${DIST_DIR}/vanta-linux-x86_64"
    cp -p "${REPO_ROOT}/target/release/vantac" "${DIST_DIR}/vantac-linux-x86_64"
    chmod 0755 "${DIST_DIR}/vanta-linux-x86_64" "${DIST_DIR}/vantac-linux-x86_64"
elif [ "${TARGET}" = "aarch64-apple-darwin" ]; then
    cp -p "${REPO_ROOT}/target/release/vanta" "${DIST_DIR}/vanta-macos-arm64"
    cp -p "${REPO_ROOT}/target/release/vantac" "${DIST_DIR}/vantac-macos-arm64"
    chmod 0755 "${DIST_DIR}/vanta-macos-arm64" "${DIST_DIR}/vantac-macos-arm64"
    if [ "$(uname -s)" = "Darwin" ]; then
        codesign -s - -f "${DIST_DIR}/vanta-macos-arm64" 2>/dev/null || true
        codesign -s - -f "${DIST_DIR}/vantac-macos-arm64" 2>/dev/null || true
    fi
fi

echo "=== Release package successfully created ==="
echo "Tarball:  ${TARBALL}"
echo "Checksum: ${DIST_DIR}/${PKG_NAME}.tar.gz.sha256"
cat "${DIST_DIR}/${PKG_NAME}.tar.gz.sha256"
