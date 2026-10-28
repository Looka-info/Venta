#!/usr/bin/env bash
# ============================================================================
# Official Vanta Toolchain Universal Installer
#
# Supports:
#   1. Local installation from extracted release tarball or repository
#   2. Universal 1-line remote installation via curl / wget:
#      curl -fsSL https://raw.githubusercontent.com/Looka-info/Venta/main/scripts/install.sh | bash
# ============================================================================
set -euo pipefail

VERSION="v1.0-RC2"
PREFIX="${HOME}/.local"
CLEANUP_TMP=""

cleanup() {
    if [ -n "${CLEANUP_TMP}" ] && [ -d "${CLEANUP_TMP}" ]; then
        rm -rf "${CLEANUP_TMP}"
    fi
}
trap cleanup EXIT

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            shift 2
            ;;
        --prefix=*)
            PREFIX="${1#*=}"
            shift 1
            ;;
        --version)
            VERSION="$2"
            shift 2
            ;;
        -h|--help)
            echo "Vanta Universal Installer"
            echo "Usage: $0 [--prefix <install_dir>] [--version <tag>]"
            echo "Defaults to prefix ~/.local"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 2
            ;;
    esac
done

SCRIPT_DIR=""
if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "${BASH_SOURCE[0]}" ]; then
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

SRC_BIN=""
SRC_LIB=""

# 1. Check local directory candidates
if [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/bin/vantac" ]; then
    SRC_BIN="${SCRIPT_DIR}/bin"
    SRC_LIB="${SCRIPT_DIR}/lib/vanta"
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../dist/vanta-${VERSION}-x86_64-unknown-linux-gnu/bin/vantac" ]; then
    SRC_BIN="${SCRIPT_DIR}/../dist/vanta-${VERSION}-x86_64-unknown-linux-gnu/bin"
    SRC_LIB="${SCRIPT_DIR}/../dist/vanta-${VERSION}-x86_64-unknown-linux-gnu/lib/vanta"
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../target/release/vantac" ]; then
    SRC_BIN="${SCRIPT_DIR}/../target/release"
    SRC_LIB="${SCRIPT_DIR}/../lib/vanta"
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../target/debug/vantac" ]; then
    SRC_BIN="${SCRIPT_DIR}/../target/debug"
    SRC_LIB="${SCRIPT_DIR}/../lib/vanta"
fi

# 2. Remote fallback: download official release tarball if piped from curl or standalone
if [ -z "${SRC_BIN}" ]; then
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    if [ "${OS}" != "Linux" ] || [ "${ARCH}" != "x86_64" ]; then
        echo "Error: Pre-compiled native Vanta binaries are built for Linux x86-64 (or Windows via WSL)."
        echo "Detected system: ${OS} (${ARCH})."
        echo "Please build from source using: cargo build --release"
        exit 1
    fi

    echo "=========================================================="
    echo "  ⚡ Downloading Vanta Systems Language (${VERSION})...   "
    echo "=========================================================="

    CLEANUP_TMP="$(mktemp -d)"
    TARBALL="vanta-${VERSION}-x86_64-unknown-linux-gnu.tar.gz"
    RELEASE_URL="https://github.com/Looka-info/Venta/releases/download/${VERSION}/${TARBALL}"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "${RELEASE_URL}" -o "${CLEANUP_TMP}/${TARBALL}"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "${RELEASE_URL}" -O "${CLEANUP_TMP}/${TARBALL}"
    else
        echo "Error: curl or wget is required to download the Vanta release package."
        exit 1
    fi

    echo "--> Extracting release archive..."
    tar -xzf "${CLEANUP_TMP}/${TARBALL}" -C "${CLEANUP_TMP}" --strip-components=1
    SRC_BIN="${CLEANUP_TMP}/bin"
    SRC_LIB="${CLEANUP_TMP}/lib/vanta"
fi

echo "=== Installing Vanta to ${PREFIX} ==="

BIN_DEST="${PREFIX}/bin"
LIB_DEST="${PREFIX}/lib/vanta"
STD_DEST="${LIB_DEST}/std"

mkdir -p "${BIN_DEST}"
mkdir -p "${STD_DEST}"

MANIFEST="${LIB_DEST}/.install_manifest.txt"
rm -f "${MANIFEST}"

install_file() {
    local src="$1"
    local dst="$2"
    local mode="$3"
    cp -p "${src}" "${dst}"
    chmod "${mode}" "${dst}"
    echo "${dst}" >> "${MANIFEST}"
}

echo "--> Installing compiler binaries to ${BIN_DEST}..."
if [ -f "${SRC_BIN}/vantac" ]; then
    install_file "${SRC_BIN}/vantac" "${BIN_DEST}/vantac" 0755
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../target/release/vantac" ]; then
    install_file "${SCRIPT_DIR}/../target/release/vantac" "${BIN_DEST}/vantac" 0755
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../target/debug/vantac" ]; then
    install_file "${SCRIPT_DIR}/../target/debug/vantac" "${BIN_DEST}/vantac" 0755
else
    echo "Error: vantac binary not found in ${SRC_BIN}"
    exit 1
fi

if [ -f "${SRC_BIN}/vanta" ]; then
    install_file "${SRC_BIN}/vanta" "${BIN_DEST}/vanta" 0755
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../bin/vanta" ]; then
    install_file "${SCRIPT_DIR}/../bin/vanta" "${BIN_DEST}/vanta" 0755
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../target/release/vanta" ]; then
    install_file "${SCRIPT_DIR}/../target/release/vanta" "${BIN_DEST}/vanta" 0755
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../target/debug/vanta" ]; then
    install_file "${SCRIPT_DIR}/../target/debug/vanta" "${BIN_DEST}/vanta" 0755
fi

echo "--> Installing native runtime archive to ${LIB_DEST}..."
if [ -f "${SRC_LIB}/libvanta_rt.a" ]; then
    install_file "${SRC_LIB}/libvanta_rt.a" "${LIB_DEST}/libvanta_rt.a" 0644
elif [ -n "${SCRIPT_DIR}" ] && [ -f "${SCRIPT_DIR}/../runtime/libvanta_rt.a" ]; then
    install_file "${SCRIPT_DIR}/../runtime/libvanta_rt.a" "${LIB_DEST}/libvanta_rt.a" 0644
else
    echo "Error: libvanta_rt.a not found"
    exit 1
fi

echo "--> Installing standard library to ${STD_DEST}..."
if [ -d "${SRC_LIB}/std" ]; then
    for f in "${SRC_LIB}/std/"*.va; do
        if [ -f "$f" ]; then
            install_file "$f" "${STD_DEST}/$(basename "$f")" 0644
        fi
    done
elif [ -n "${SCRIPT_DIR}" ] && [ -d "${SCRIPT_DIR}/../library/std" ]; then
    for f in "${SCRIPT_DIR}/../library/std/"*.va; do
        if [ -f "$f" ]; then
            install_file "$f" "${STD_DEST}/$(basename "$f")" 0644
        fi
    done
fi

echo "${MANIFEST}" >> "${MANIFEST}"

echo ""
echo "=========================================================="
echo "  ⚡ Vanta Toolchain Installed Successfully!               "
echo "=========================================================="
if [ -x "${BIN_DEST}/vantac" ]; then
    echo "Compiler:   $(${BIN_DEST}/vantac --version 2>/dev/null || echo 'vantac installed')"
fi
if [ -x "${BIN_DEST}/vanta" ]; then
    echo "Driver:     $(${BIN_DEST}/vanta --version 2>/dev/null || echo 'vanta installed')"
fi
echo "Location:   ${BIN_DEST}"

# Check PATH
case ":$PATH:" in
    *":${BIN_DEST}:"*)
        ;;
    *)
        echo ""
        echo "NOTE: ${BIN_DEST} is not currently in your PATH."
        echo "To add it permanently, run:"
        echo ""
        echo "    echo 'export PATH=\"${BIN_DEST}:\$PATH\"' >> ~/.bashrc"
        echo "    source ~/.bashrc"
        echo ""
        ;;
esac
