#!/usr/bin/env python3
"""
Venta Milestone 26.3 Loop Auto-Vectorization Benchmark.

Measures computational throughput of scalar loops (--no-vectorize) vs
auto-vectorized loops (--target-feature=+avx2) for array vector addition
across Short (8 elements), Medium (64 elements), and Long (1024 elements).
"""

import os
import subprocess
import time
import statistics
import json

VENTAC = "./target/release/ventac"
TEMP_DIR = "/tmp/venta_vec_bench"

BENCH_CONFIGS = [
    {
        "name": "Short (8 lanes)",
        "len": 8,
        "reps": 10000000,
    },
    {
        "name": "Medium (64 lanes)",
        "len": 64,
        "reps": 2000000,
    },
    {
        "name": "Long (1024 lanes)",
        "len": 1024,
        "reps": 150000,
    },
]

def generate_source(length, reps):
    init_a = ", ".join(str(i % 100 + 1) for i in range(length))
    init_b = ", ".join(str((i * 3) % 100 + 2) for i in range(length))
    init_out = ", ".join("0" for _ in range(length))

    return f"""
fn run_kernel(reps: i64, a: &[{'i64'}; {length}], b: &[{'i64'}; {length}], out: &mut [{'i64'}; {length}]) {{
    let mut rep = 0;
    while rep < reps {{
        let mut i = 0;
        while i < {length} {{
            out[i] = a[i] + b[i];
            i = i + 1;
        }}
        rep = rep + 1;
    }}
}}

fn main() -> i64 {{
    let a: [i64; {length}] = [{init_a}];
    let b: [i64; {length}] = [{init_b}];
    let mut out: [i64; {length}] = [{init_out}];

    run_kernel({reps}, &a, &b, &mut out);

    // Return element to verify correctness and prevent elimination
    out[0]
}}
"""

def compile_bin(src_path, exe_path, extra_flags):
    cmd = [VENTAC, "compile", "-O1", "--no-inline", "-o", exe_path, src_path] + extra_flags
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0:
        print(f"Compilation error for {exe_path}:")
        print(res.stderr)
        raise RuntimeError(f"Failed to compile {src_path}")

def time_bin(exe_path, runs=5):
    timings = []
    for _ in range(runs):
        t0 = time.perf_counter()
        res = subprocess.run([exe_path], capture_output=True)
        t1 = time.perf_counter()
        timings.append((t1 - t0) * 1000.0) # ms
    return statistics.mean(timings), statistics.stdev(timings) if len(timings) > 1 else 0.0

def main():
    os.makedirs(TEMP_DIR, exist_ok=True)
    results = []

    print("=" * 80)
    print("Venta v1.1 SIMD Loop Auto-Vectorization Benchmark (Kernel Throughput)")
    print("Target Architecture: x86_64-unknown-linux-gnu (AVX2 / 256-bit YMM)")
    print("=" * 80)
    print(f"{'Workload':<24} | {'Scalar Time (ms)':<18} | {'SIMD Time (ms)':<18} | {'Speedup':<10}")
    print("-" * 80)

    for cfg in BENCH_CONFIGS:
        name = cfg["name"]
        length = cfg["len"]
        reps = cfg["reps"]

        src_file = os.path.join(TEMP_DIR, f"kernel_{length}.va")
        scalar_exe = os.path.join(TEMP_DIR, f"scalar_k_{length}")
        simd_exe = os.path.join(TEMP_DIR, f"simd_k_{length}")

        with open(src_file, "w") as f:
            f.write(generate_source(length, reps))

        compile_bin(src_file, scalar_exe, ["--no-vectorize"])
        compile_bin(src_file, simd_exe, ["--target-feature=+avx2"])

        scalar_mean, scalar_std = time_bin(scalar_exe, runs=5)
        simd_mean, simd_std = time_bin(simd_exe, runs=5)
        speedup = scalar_mean / simd_mean if simd_mean > 0 else 1.0

        results.append({
            "workload": name,
            "length": length,
            "reps": reps,
            "scalar_ms": scalar_mean,
            "scalar_std": scalar_std,
            "simd_ms": simd_mean,
            "simd_std": simd_std,
            "speedup": speedup
        })

        scalar_str = f"{scalar_mean:.1f} ± {scalar_std:.1f} ms"
        simd_str = f"{simd_mean:.1f} ± {simd_std:.1f} ms"
        print(f"{name:<24} | {scalar_str:<18} | {simd_str:<18} | {speedup:.2f}x")

    print("=" * 80)

    results_path = "benchmarks/results/vectorize_benchmark.json"
    os.makedirs(os.path.dirname(results_path), exist_ok=True)
    with open(results_path, "w") as f:
        json.dump(results, f, indent=2)
    print(f"Benchmark results recorded in {results_path}")

if __name__ == "__main__":
    main()
