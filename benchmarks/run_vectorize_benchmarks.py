#!/usr/bin/env python3
"""
Venta Milestone 26.5 Loop Auto-Vectorization Benchmark.

Measures computational throughput across:
1. Scalar execution (--no-vectorize)
2. 256-bit AVX2 vectorization (--target-feature=+avx2)
3. 512-bit AVX-512 8-lane vectorization (--target-feature=+avx512f)

Truthfully reports host CPU model, hardware feature support, and only executes
512-bit binaries if native AVX-512F hardware is detected.
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

def detect_cpu_info():
    cpu_model = "Unknown x86-64"
    has_avx2 = False
    has_avx512f = False
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    cpu_model = line.split(":", 1)[1].strip()
                elif line.startswith("flags"):
                    flags = line.split(":", 1)[1].split()
                    has_avx2 = "avx2" in flags
                    has_avx512f = "avx512f" in flags
                    break
    except Exception:
        pass
    return cpu_model, has_avx2, has_avx512f

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

    // Return 0 on success, verify element to prevent dead-code elimination
    if out[0] == 0 {{
        return 1;
    }}
    0
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
        if res.returncode != 0:
            raise RuntimeError(f"Execution failed for {exe_path}")
        timings.append((t1 - t0) * 1000.0) # ms
    return statistics.mean(timings), statistics.stdev(timings) if len(timings) > 1 else 0.0

def main():
    os.makedirs(TEMP_DIR, exist_ok=True)
    results = []

    cpu_model, has_avx2, has_avx512f = detect_cpu_info()

    print("=" * 96)
    print("Venta v1.1 SIMD Loop Auto-Vectorization Benchmark (Milestone 26.5)")
    print(f"Host CPU: {cpu_model}")
    print(f"Hardware Feature Support: AVX2={has_avx2}, AVX-512F={has_avx512f}")
    print("=" * 96)
    print(f"{'Workload':<20} | {'Scalar (ms)':<16} | {'AVX2 256-bit':<16} | {'AVX-512 512-bit':<20} | {'AVX2 Speedup':<12}")
    print("-" * 96)

    for cfg in BENCH_CONFIGS:
        name = cfg["name"]
        length = cfg["len"]
        reps = cfg["reps"]

        src_file = os.path.join(TEMP_DIR, f"kernel_{length}.va")
        scalar_exe = os.path.join(TEMP_DIR, f"scalar_k_{length}")
        avx2_exe = os.path.join(TEMP_DIR, f"avx2_k_{length}")
        avx512_exe = os.path.join(TEMP_DIR, f"avx512_k_{length}")

        with open(src_file, "w") as f:
            f.write(generate_source(length, reps))

        # 1. Compile all targets
        compile_bin(src_file, scalar_exe, ["--no-vectorize"])
        compile_bin(src_file, avx2_exe, ["--target-feature=+avx2"])
        compile_bin(src_file, avx512_exe, ["--target-feature=+avx512f"])

        # 2. Benchmark Scalar & AVX2
        scalar_mean, scalar_std = time_bin(scalar_exe, runs=5)
        avx2_mean, avx2_std = time_bin(avx2_exe, runs=5)
        avx2_speedup = scalar_mean / avx2_mean if avx2_mean > 0 else 1.0

        # 3. Benchmark AVX-512 if native hardware available
        if has_avx512f:
            avx512_mean, avx512_std = time_bin(avx512_exe, runs=5)
            avx512_speedup = scalar_mean / avx512_mean if avx512_mean > 0 else 1.0
            avx512_str = f"{avx512_mean:.1f} ± {avx512_std:.1f} ms"
        else:
            avx512_mean, avx512_std = None, None
            avx512_speedup = None
            avx512_str = "N/A (no AVX-512F CPU)"

        results.append({
            "workload": name,
            "length": length,
            "reps": reps,
            "cpu_model": cpu_model,
            "scalar_ms": scalar_mean,
            "scalar_std": scalar_std,
            "avx2_ms": avx2_mean,
            "avx2_std": avx2_std,
            "avx2_speedup": avx2_speedup,
            "avx512_ms": avx512_mean,
            "avx512_std": avx512_std,
            "avx512_speedup": avx512_speedup,
            "avx512_hardware_present": has_avx512f
        })

        scalar_str = f"{scalar_mean:.1f} ± {scalar_std:.1f} ms"
        avx2_str = f"{avx2_mean:.1f} ± {avx2_std:.1f} ms"
        print(f"{name:<20} | {scalar_str:<16} | {avx2_str:<16} | {avx512_str:<20} | {avx2_speedup:.2f}x")

    print("=" * 96)

    results_path = "benchmarks/results/vectorize_benchmark.json"
    os.makedirs(os.path.dirname(results_path), exist_ok=True)
    with open(results_path, "w") as f:
        json.dump(results, f, indent=2)
    print(f"Benchmark results recorded in {results_path}")

if __name__ == "__main__":
    main()
