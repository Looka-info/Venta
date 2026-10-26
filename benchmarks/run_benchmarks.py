#!/usr/bin/env python3
"""
Vanta Benchmark Harness for Milestone 2.
Compares -O0 (unoptimized) vs -O1 (optimized) generated executables.
Measures execution time (median of N runs), text section size, and total binary size.
"""

import subprocess
import time
import os
import sys

BENCHMARKS = [
    ("constant_arithmetic", "benchmarks/micro/constant_arithmetic.va"),
    ("dead_code", "benchmarks/micro/dead_code.va"),
    ("branch_folding", "benchmarks/micro/branch_folding.va"),
    ("loop_computation", "benchmarks/micro/loop_computation.va"),
]

VANTAC = "/home/mumer/vanta-target/release/vantac"
NUM_TRIALS = 7

def get_text_size(binary_path):
    try:
        out = subprocess.check_output(["size", binary_path]).decode()
        lines = out.strip().split("\n")
        if len(lines) >= 2:
            return int(lines[1].split()[0])
    except Exception:
        pass
    return os.path.getsize(binary_path)

def run_benchmark(name, va_path):
    print(f"\n--- Benchmark: {name} ---")
    bin_o0 = f"/tmp/{name}_o0"
    bin_o1 = f"/tmp/{name}_o1"

    # Compile O0
    t_start = time.perf_counter()
    subprocess.check_call([VANTAC, "-O0", va_path, "-o", bin_o0])
    compile_time_o0 = (time.perf_counter() - t_start) * 1000

    # Compile O1
    t_start = time.perf_counter()
    subprocess.check_call([VANTAC, "-O1", va_path, "-o", bin_o1])
    compile_time_o1 = (time.perf_counter() - t_start) * 1000

    size_o0 = get_text_size(bin_o0)
    size_o1 = get_text_size(bin_o1)

    # Benchmark runtime O0
    times_o0 = []
    for _ in range(NUM_TRIALS):
        t0 = time.perf_counter()
        subprocess.check_call([bin_o0])
        t1 = time.perf_counter()
        times_o0.append((t1 - t0) * 1000)

    # Benchmark runtime O1
    times_o1 = []
    for _ in range(NUM_TRIALS):
        t0 = time.perf_counter()
        subprocess.check_call([bin_o1])
        t1 = time.perf_counter()
        times_o1.append((t1 - t0) * 1000)

    med_o0 = sorted(times_o0)[len(times_o0) // 2]
    med_o1 = sorted(times_o1)[len(times_o1) // 2]
    speedup = med_o0 / med_o1 if med_o1 > 0 else 1.0

    print(f"  .text section size: O0 = {size_o0} B, O1 = {size_o1} B (diff: {size_o1 - size_o0} B)")
    print(f"  Compile time:       O0 = {compile_time_o0:.1f} ms, O1 = {compile_time_o1:.1f} ms")
    print(f"  Runtime (median):   O0 = {med_o0:.2f} ms, O1 = {med_o1:.2f} ms")
    print(f"  Speedup (-O1 vs -O0): {speedup:.2f}x")

def main():
    print("Vanta Benchmark Suite — Milestone 2")
    for name, path in BENCHMARKS:
        run_benchmark(name, path)

if __name__ == "__main__":
    main()
