#!/usr/bin/env bash
set -euo pipefail

PREFIX="${HOME}/.local"

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

echo "=== Uninstalling Vanta from ${PREFIX} ==="

LIB_DEST="${PREFIX}/lib/vanta"
MANIFEST="${LIB_DEST}/.install_manifest.txt"

if [ -f "${MANIFEST}" ]; then
    echo "--> Removing files listed in manifest..."
    while IFS= read -r file || [ -n "$file" ]; do
        if [ -f "$file" ]; then
            rm -f "$file"
            echo "Removed $file"
        fi
    done < "${MANIFEST}"
else
    echo "--> Removing standard Vanta installation files..."
    rm -f "${PREFIX}/bin/vantac"
    rm -f "${PREFIX}/bin/vanta"
    rm -rf "${LIB_DEST}"
fi

# Clean up empty directories
rm -rf "${LIB_DEST}"
rmdir "${PREFIX}/bin" 2>/dev/null || true
rmdir "${PREFIX}/lib" 2>/dev/null || true

echo "=== Vanta successfully uninstalled ==="
