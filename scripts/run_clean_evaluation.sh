#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

EVAL_DIR="/tmp/vanta_clean_eval_test"
echo "=== Setting up clean evaluation directory: ${EVAL_DIR} ==="
rm -rf "${EVAL_DIR}"
mkdir -p "${EVAL_DIR}/archive" "${EVAL_DIR}/prefix" "${EVAL_DIR}/workspace"

echo "=== Extracting release archive ==="
tar -xzf "${REPO_ROOT}/dist/vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz" -C "${EVAL_DIR}/archive"

echo "=== Installing to isolated prefix ==="
cd "${EVAL_DIR}/archive/vanta-v1.0-RC2-x86_64-unknown-linux-gnu"
./install.sh --prefix="${EVAL_DIR}/prefix"

echo "=== Checking installed version banners ==="
"${EVAL_DIR}/prefix/bin/vantac" --version
"${EVAL_DIR}/prefix/bin/vanta" --version

export PATH="${EVAL_DIR}/prefix/bin:${PATH}"
cd "${EVAL_DIR}/workspace"

echo "=== Evaluation Scenario 1: Exit code reporting ==="
cat << 'EOF' > test1.va
fn main() -> i64 {
    return 42;
}
EOF
vanta compile test1.va -o test1_bin
set +e
./test1_bin
CODE=$?
set -e
echo "Test 1 Exit Code: ${CODE}"
if [ "${CODE}" -ne 42 ]; then
    echo "ERROR: Test 1 failed (expected 42, got ${CODE})"
    exit 1
fi

echo "=== Evaluation Scenario 2: Deep recursion ==="
cat << 'EOF' > test2.va
fn fib(n: i64) -> i64 {
    if n <= 1 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> i64 {
    let res = fib(10);
    print_i64(res);
    print_str("\n");
    return 0;
}
EOF
vanta compile test2.va -o test2_bin
OUT2=$(./test2_bin)
echo "Test 2 Output: ${OUT2}"
if [ "${OUT2}" != "55" ]; then
    echo "ERROR: Test 2 failed"
    exit 2
fi

echo "=== Evaluation Scenario 3: Struct layout & aggregate return ==="
cat << 'EOF' > test3.va
struct Transform {
    x: i64,
    y: i64,
    z: i64,
}

impl Transform {
    fn scale(self: &Self, factor: i64) -> Transform {
        return Transform {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        };
    }
}

fn main() -> i64 {
    let t = Transform { x: 10, y: 20, z: 30 };
    let scaled = t.scale(2);
    print_i64(scaled.x);
    print_str(" ");
    print_i64(scaled.y);
    print_str(" ");
    print_i64(scaled.z);
    print_str("\n");
    return 0;
}
EOF
vanta compile test3.va -o test3_bin
OUT3=$(./test3_bin)
echo "Test 3 Output: ${OUT3}"
if [ "${OUT3}" != "20 40 60" ]; then
    echo "ERROR: Test 3 failed"
    exit 3
fi

echo "=== Evaluation Scenario 4: Loops & Control flow with break ==="
cat << 'EOF' > test4.va
fn main() -> i64 {
    let mut sum = 0;
    let mut i = 1;
    while i <= 100 {
        if i == 51 {
            break;
        }
        sum = sum + i;
        i = i + 1;
    }
    print_i64(sum);
    print_str("\n");
    return 0;
}
EOF
vanta compile test4.va -o test4_bin
OUT4=$(./test4_bin)
echo "Test 4 Output: ${OUT4}"
if [ "${OUT4}" != "1275" ]; then
    echo "ERROR: Test 4 failed"
    exit 4
fi

echo "=== Evaluation Scenario 5: Standard-library File I/O ==="
cat << 'EOF' > test5.va
import std::fs;
import std::io;
import std::result::Result;

fn main() -> i64 {
    let write_res = fs::write("msg.txt", "Verified GA candidate\n");
    match write_res {
        Result::Ok(_) => {}
        Result::Err(_) => {
            return 1;
        }
    }

    let read_res = fs::read_to_string("msg.txt");
    match read_res {
        Result::Ok(content) => {
            io::write_stdout(content.as_str());
        }
        Result::Err(_) => {
            return 2;
        }
    }

    return 0;
}
EOF
vanta compile test5.va -o test5_bin
OUT5=$(./test5_bin)
echo "Test 5 Output: ${OUT5}"
if [ "${OUT5}" != "Verified GA candidate" ]; then
    echo "ERROR: Test 5 failed"
    exit 5
fi

echo "=== Evaluation Scenario 6: Diagnostic error reporting (--json) ==="
cat << 'EOF' > test6.va
fn main() -> i64 {
    let x: i64 = "invalid_string_literal";
    return x;
}
EOF
set +e
CHECK_OUT=$(vanta check test6.va --json 2>&1)
CHECK_STATUS=$?
set -e
echo "Test 6 Exit Code: ${CHECK_STATUS}"
echo "Test 6 JSON Output: ${CHECK_OUT}"
if [ "${CHECK_STATUS}" -eq 0 ]; then
    echo "ERROR: Test 6 should fail check"
    exit 6
fi
if [[ ! "${CHECK_OUT}" =~ "E0201" ]]; then
    echo "ERROR: Test 6 missing E0201 error code"
    exit 6
fi
if [[ ! "${CHECK_OUT}" =~ '"success":false' ]]; then
    echo "ERROR: Test 6 JSON missing success:false"
    exit 6
fi

# Also test vanta check on valid file
VALID_OUT=$(vanta check test1.va --json)
if [[ ! "${VALID_OUT}" =~ '"success":true' ]]; then
    echo "ERROR: Valid file check failed JSON success"
    exit 6
fi

echo "=== Evaluation Scenario 7: Optimization Equivalence (-O0 vs -O1) ==="
vanta compile test4.va -o test4_o0 --no-gvn --no-mem2reg
vanta compile test4.va -o test4_o1
OUT4_O0=$(./test4_o0)
OUT4_O1=$(./test4_o1)
echo "O0 Output: ${OUT4_O0}"
echo "O1 Output: ${OUT4_O1}"
if [ "${OUT4_O0}" != "${OUT4_O1}" ]; then
    echo "ERROR: Optimization equivalence mismatch!"
    exit 7
fi

echo "=== Testing Uninstallation ==="
cd "${EVAL_DIR}/archive/vanta-v1.0-RC2-x86_64-unknown-linux-gnu"
./uninstall.sh --prefix="${EVAL_DIR}/prefix"
if [ -f "${EVAL_DIR}/prefix/bin/vanta" ]; then
    echo "ERROR: Uninstall failed to remove binary"
    exit 8
fi

echo "=== ALL 7 EXTERNAL EVALUATION SCENARIOS PASSED WITH ZERO ERRORS ==="
