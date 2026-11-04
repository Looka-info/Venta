#!/usr/bin/env bash
# ============================================================================
# ⚡ Vanta Runtime Build Script (Linux / macOS)
# Compiles runtime/vanta_rt.c into target-specific ELF/Mach-O archive
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
RUNTIME_DIR="${REPO_ROOT}/runtime"

UNAME_S="$(uname -s)"
if [ "${UNAME_S}" = "Linux" ]; then
    TARGET_DIR="${RUNTIME_DIR}/x86_64-unknown-linux-gnu"
elif [ "${UNAME_S}" = "Darwin" ]; then
    ARCH="$(uname -m)"
    if [ "${ARCH}" = "arm64" ]; then
        TARGET_DIR="${RUNTIME_DIR}/aarch64-apple-darwin"
    else
        TARGET_DIR="${RUNTIME_DIR}/x86_64-apple-darwin"
    fi
else
    TARGET_DIR="${RUNTIME_DIR}/unknown"
fi

mkdir -p "${TARGET_DIR}"

CC_BIN="${CC:-gcc}"
AR_BIN="${AR:-ar}"

echo "--> Compiling ${RUNTIME_DIR}/vanta_rt.c using ${CC_BIN}..."
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

rm -f "${TARGET_DIR}/libvanta_rt.a" "${RUNTIME_DIR}/libvanta_rt.a"
"${CC_BIN}" -c -O2 -fPIC -fno-stack-protector -U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=0 "${RUNTIME_DIR}/vanta_rt.c" -o "${TMP_DIR}/vanta_rt.o"
"${AR_BIN}" rcs "${TARGET_DIR}/libvanta_rt.a" "${TMP_DIR}/vanta_rt.o"
cp -f "${TARGET_DIR}/libvanta_rt.a" "${RUNTIME_DIR}/libvanta_rt.a"

echo "--> Target runtime built successfully: ${TARGET_DIR}/libvanta_rt.a"
