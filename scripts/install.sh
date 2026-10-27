#!/usr/bin/env bash
set -euo pipefail

PREFIX="${HOME}/.local"

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
        -h|--help)
            echo "Usage: $0 [--prefix <install_dir>]"
            echo "Defaults to ~/.local"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 2
            ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Find files to install (either from extracted package or repository)
if [ -f "${SCRIPT_DIR}/bin/vantac" ]; then
    SRC_BIN="${SCRIPT_DIR}/bin"
    SRC_LIB="${SCRIPT_DIR}/lib/vanta"
elif [ -f "${SCRIPT_DIR}/../target/release/vantac" ]; then
    SRC_BIN="${SCRIPT_DIR}/../target/release"
    SRC_LIB="${SCRIPT_DIR}/../lib/vanta"
elif [ -f "${SCRIPT_DIR}/../target/debug/vantac" ]; then
    SRC_BIN="${SCRIPT_DIR}/../target/debug"
    SRC_LIB="${SCRIPT_DIR}/../lib/vanta"
else
    echo "Error: Could not locate Vanta binaries to install."
    exit 1
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

echo "--> Installing binaries to ${BIN_DEST}..."
install_file "${SRC_BIN}/vantac" "${BIN_DEST}/vantac" 0755
install_file "${SRC_BIN}/vanta" "${BIN_DEST}/vanta" 0755

echo "--> Installing runtime archive to ${LIB_DEST}..."
if [ -f "${SRC_LIB}/libvanta_rt.a" ]; then
    install_file "${SRC_LIB}/libvanta_rt.a" "${LIB_DEST}/libvanta_rt.a" 0644
elif [ -f "${SCRIPT_DIR}/../runtime/libvanta_rt.a" ]; then
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
elif [ -d "${SCRIPT_DIR}/../library/std" ]; then
    for f in "${SCRIPT_DIR}/../library/std/"*.va; do
        if [ -f "$f" ]; then
            install_file "$f" "${STD_DEST}/$(basename "$f")" 0644
        fi
    done
fi

echo "${MANIFEST}" >> "${MANIFEST}"

echo "=== Installation Complete ==="
echo "Installed: $(${BIN_DEST}/vantac --version)"
echo "Installed: $(${BIN_DEST}/vanta --version)"

# Check PATH
case ":$PATH:" in
    *":${BIN_DEST}:"*)
        ;;
    *)
        echo ""
        echo "NOTE: ${BIN_DEST} is not in your PATH."
        echo "Add it to your shell profile (~/.bashrc or ~/.profile):"
        echo "    export PATH=\"${BIN_DEST}:\$PATH\""
        ;;
esac
