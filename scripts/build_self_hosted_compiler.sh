#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

echo "=========================================================="
echo "    Building Vanta Native Self-Hosted Compiler (vanta)   "
echo "=========================================================="

mkdir -p bin

echo "Stage 0: Bootstrapping compiler/main.va with vantac..."
cargo run -p vanta_driver --bin vantac -- compile compiler/main.va -o bin/vanta

echo ""
echo "Verifying compiled binary (bin/vanta)..."
./bin/vanta --version

echo ""
echo "=========================================================="
echo "  Build successful! Standalone compiler located at: bin/vanta"
echo "=========================================================="
