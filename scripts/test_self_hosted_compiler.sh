#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

echo "=========================================================="
echo "    Running Vanta Self-Hosted Compiler Test Suite        "
echo "=========================================================="

echo "Compiling self-hosted compiler (tools/self_host/vanta_c.va)..."
cargo run -p vanta_driver --bin vantac -- compile tools/self_host/vanta_c.va -o tools/self_host/vanta_c_bin

echo ""
echo "--- Running Pure Vanta Backend Unit Tests (backend_test.va) ---"
cargo run -p vanta_driver --bin vantac -- compile tools/self_host/backend_test.va -o tools/self_host/backend_test_bin
set +e
./tools/self_host/backend_test_bin
BACKEND_RET=$?
set -e
if [ "$BACKEND_RET" -eq 100 ]; then
    echo "PASS: Pure Vanta native backend passed all verification tests (exit 100)"
else
    echo "FAIL: Backend tests failed with exit code $BACKEND_RET"
    exit 1
fi

COMPILER="./tools/self_host/vanta_c_bin"

run_test() {
    local name="$1"
    local src="tools/self_host/tests/${name}.va"
    local asm="tools/self_host/tests/${name}.s"
    local bin="tools/self_host/tests/${name}_bin"
    local expected="$2"

    echo ""
    echo "--- Testing $name ---"
    "$COMPILER" "$src" -o "$asm"
    gcc -no-pie "$asm" -o "$bin"
    
    set +e
    "$bin"
    local actual=$?
    set -e

    if [ "$actual" -eq "$expected" ]; then
        echo "PASS: $name returned $actual (expected $expected)"
    else
        echo "FAIL: $name returned $actual, but expected $expected"
        exit 1
    fi
}

run_test "01_return_42" 42
run_test "02_arithmetic" 40
run_test "03_while_loop" 55
run_test "04_if_else" 10
run_test "05_fibonacci" 55
run_test "06_call_6_args" 21
run_test "07_comparisons" 63
run_test "08_nested_loops" 60
run_test "09_operator_precedence" 28
run_test "10_stack_alignment_and_calls" 44

echo ""
echo "=========================================================="
echo "  ALL SELF-HOSTED COMPILER TESTS PASSED SUCCESSFULLY!    "
echo "=========================================================="
