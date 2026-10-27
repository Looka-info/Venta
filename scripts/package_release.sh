#!/usr/bin/env bash
set -euo pipefail

# Determine repository root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

VERSION="1.0-RC1"
TARGET="x86_64-unknown-linux-gnu"
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
gcc -c -O2 -fPIC "${REPO_ROOT}/runtime/vanta_rt.c" -o "${TMP_O}"
ar rcs "${REPO_ROOT}/runtime/libvanta_rt.a" "${TMP_O}"
rm -f "${TMP_O}"

# 3. Create staging layout
echo "--> Preparing package directory structure..."
rm -rf "${STAGE_DIR}"
mkdir -p "${STAGE_DIR}/bin"
mkdir -p "${STAGE_DIR}/lib/vanta/std"

# Copy binaries
cp -p "${REPO_ROOT}/target/release/vantac" "${STAGE_DIR}/bin/vantac"
cp -p "${REPO_ROOT}/target/release/vanta" "${STAGE_DIR}/bin/vanta"
chmod 0755 "${STAGE_DIR}/bin/vantac" "${STAGE_DIR}/bin/vanta"

# Copy runtime archive
cp -p "${REPO_ROOT}/runtime/libvanta_rt.a" "${STAGE_DIR}/lib/vanta/libvanta_rt.a"
chmod 0644 "${STAGE_DIR}/lib/vanta/libvanta_rt.a"

# Copy standard library
cp -p "${REPO_ROOT}/library/std/"*.va "${STAGE_DIR}/lib/vanta/std/"
chmod 0644 "${STAGE_DIR}/lib/vanta/std/"*.va

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

# 4. Create deterministic, reproducible tarball
echo "--> Compressing archive with deterministic metadata (${PKG_NAME}.tar.gz)..."
TARBALL="${DIST_DIR}/${PKG_NAME}.tar.gz"
tar --sort=name --mtime='2026-10-27 00:00:00Z' --owner=0 --group=0 --numeric-owner -cf - -C "${DIST_DIR}" "${PKG_NAME}" | gzip -n > "${TARBALL}"

# 5. Compute SHA-256
echo "--> Generating SHA-256 checksum..."
(cd "${DIST_DIR}" && sha256sum "${PKG_NAME}.tar.gz" > "${PKG_NAME}.tar.gz.sha256")

echo "=== Release package successfully created ==="
echo "Tarball:  ${TARBALL}"
echo "Checksum: ${DIST_DIR}/${PKG_NAME}.tar.gz.sha256"
cat "${DIST_DIR}/${PKG_NAME}.tar.gz.sha256"
