#!/usr/bin/env bash
set -e

cd /mnt/c/Users/mumer/OneDrive/Documents/Venta

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
