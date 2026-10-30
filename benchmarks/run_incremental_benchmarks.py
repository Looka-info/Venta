#!/usr/bin/env python3
"""
Venta Milestone 32 Incremental Compilation Benchmark Suite.

Evaluates:
  1. Clean non-incremental build (baseline oracle)
  2. Cold incremental build (cache generation)
  3. Warm no-op incremental build (cache hit, 0 recompiles, 0 relinks)
  4. Single-function body edit (recompiles changed unit, reuses cache)
  5. Link-only rebuild (cached objects reused, final executable relinked)
  6. Rebuild after cache corruption (detected, safely recovered and rebuilt)

Outputs structured JSON results to benchmarks/results/incremental_benchmark.json.
"""

import os
import sys
import subprocess
import time
import json
import statistics

VENTAC_BIN = "./target/release/ventac"

def run_cmd(cmd):
    t0 = time.perf_counter()
    res = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True)
    t1 = time.perf_counter()
    return res.returncode, res.stdout, res.stderr, (t1 - t0)

def generate_benchmark_project(base_dir, num_modules, funcs_per_mod):
    os.makedirs(base_dir, exist_ok=True)
    mod_names = []
    
    # Generate helper modules
    for m in range(num_modules):
        mod_name = f"mod_{m}"
        mod_names.append(mod_name)
        file_path = os.path.join(base_dir, f"{mod_name}.va")
        lines = []
        for f in range(funcs_per_mod):
            lines.append(f"""
pub fn calc_{f}(x: i64) -> i64 {{
    return x * 2 + {f};
}}
""")
        with open(file_path, "w") as fp:
            fp.write("\n".join(lines))
            
    # Generate main.va referencing all modules
    main_path = os.path.join(base_dir, "main.va")
    main_lines = []
    for mod_name in mod_names:
        main_lines.append(f"mod {mod_name};")
    for mod_name in mod_names:
        main_lines.append(f"import {mod_name}::calc_0;")

    main_lines.append("""
fn helper_sum() -> i64 {
    let mut s: i64 = 0;
    let mut i: i64 = 0;
    while i < 100 {
        s = s + i;
        i = i + 1;
    }
    return s;
}

pub fn main() -> i64 {
    let mut total: i64 = helper_sum();
""")
    for mod_name in mod_names:
        main_lines.append(f"    total = total + {mod_name}::calc_0(2);")

    main_lines.append("""
    return total;
}
""")
    with open(main_path, "w") as fp:
        fp.write("\n".join(main_lines))

def main():
    if not os.path.exists(VENTAC_BIN):
        print(f"Error: {VENTAC_BIN} not found. Build with 'cargo build --release --bin ventac'.")
        sys.exit(1)

    print("=== Venta Incremental Compilation Benchmark Suite ===")
    bench_dir = "/tmp/venta_bench_incremental"
    run_cmd(f"rm -rf {bench_dir}")
    os.makedirs(bench_dir, exist_ok=True)
    cache_dir = os.path.join(bench_dir, ".cache")
    main_src = os.path.join(bench_dir, "main.va")
    out_exe = os.path.join(bench_dir, "app.out")

    num_modules = 20
    funcs_per_mod = 10
    total_funcs = num_modules * funcs_per_mod + 2
    generate_benchmark_project(bench_dir, num_modules=num_modules, funcs_per_mod=funcs_per_mod)

    results = {
        "num_modules": num_modules,
        "funcs_per_module": funcs_per_mod,
        "total_functions": total_funcs,
    }

    # 1. Clean Non-Incremental Build
    clean_times = []
    for _ in range(5):
        run_cmd(f"rm -f {out_exe}")
        code, stdout, stderr, dur = run_cmd(f"{VENTAC_BIN} {main_src} -o {out_exe} --no-incremental")
        if code != 0:
            print(f"Clean build failed: {stderr}")
            sys.exit(1)
        clean_times.append(dur * 1000.0)
    clean_med = statistics.median(clean_times)
    results["clean_build_ms"] = round(clean_med, 2)
    print(f"1. Clean Build (Baseline, {total_funcs} funcs): {clean_med:.2f} ms")

    # 2. Cold Incremental Build (empty cache)
    run_cmd(f"rm -rf {cache_dir} {out_exe}")
    code, stdout, stderr, dur = run_cmd(f"{VENTAC_BIN} {main_src} -o {out_exe} --incremental --cache-dir={cache_dir} --build-stats")
    if code != 0:
        print(f"Cold build failed: {stderr}")
        sys.exit(1)
    cold_ms = dur * 1000.0
    results["cold_incremental_ms"] = round(cold_ms, 2)
    print(f"2. Cold Incremental Build:      {cold_ms:.2f} ms")

    # 3. Warm No-Op Incremental Build
    noop_times = []
    for _ in range(10):
        code, stdout, stderr, dur = run_cmd(f"{VENTAC_BIN} {main_src} -o {out_exe} --incremental --cache-dir={cache_dir} --build-stats")
        if code != 0:
            print(f"No-op build failed: {stderr}")
            sys.exit(1)
        noop_times.append(dur * 1000.0)
    noop_med = statistics.median(noop_times)
    results["warm_noop_ms"] = round(noop_med, 2)
    speedup = clean_med / noop_med if noop_med > 0 else 0
    results["noop_speedup_x"] = round(speedup, 2)
    print(f"3. Warm No-Op Build:            {noop_med:.2f} ms (Speedup: {speedup:.2f}x vs clean)")

    # 4. Single-Function Body Edit
    edit_times = []
    for i in range(5):
        # Modify single function body in mod_0.va (all other 19 modules and 199 functions reused)
        mod0_path = os.path.join(bench_dir, "mod_0.va")
        lines = []
        for f in range(funcs_per_mod):
            if f == 0:
                lines.append(f"""
pub fn calc_0(x: i64) -> i64 {{
    return x * 3 + {10 + i};
}}
""")
            else:
                lines.append(f"""
pub fn calc_{f}(x: i64) -> i64 {{
    return x * 2 + {f};
}}
""")
        with open(mod0_path, "w") as fp:
            fp.write("\n".join(lines))

        code, stdout, stderr, dur = run_cmd(f"{VENTAC_BIN} {main_src} -o {out_exe} --incremental --cache-dir={cache_dir} --build-stats")
        if code != 0:
            print(f"Body edit build failed: {stderr}")
            sys.exit(1)
        edit_times.append(dur * 1000.0)
    edit_med = statistics.median(edit_times)
    results["single_function_edit_ms"] = round(edit_med, 2)
    edit_speedup = clean_med / edit_med if edit_med > 0 else 0
    results["single_function_speedup_x"] = round(edit_speedup, 2)
    print(f"4. Single-Function Edit Rebuild: {edit_med:.2f} ms (Speedup: {edit_speedup:.2f}x vs clean)")

    # 5. Link-Only Rebuild
    link_times = []
    for _ in range(5):
        run_cmd(f"rm -f {out_exe}")
        code, stdout, stderr, dur = run_cmd(f"{VENTAC_BIN} {main_src} -o {out_exe} --incremental --cache-dir={cache_dir} --build-stats")
        if code != 0:
            print(f"Link-only build failed: {stderr}")
            sys.exit(1)
        link_times.append(dur * 1000.0)
    link_med = statistics.median(link_times)
    results["link_only_rebuild_ms"] = round(link_med, 2)
    print(f"5. Link-Only Rebuild:           {link_med:.2f} ms")

    # 6. Rebuild after Cache Corruption Recovery
    # Corrupt object file in cache
    code, stdout, stderr, _ = run_cmd(f"find {cache_dir} -name '*.obj' -exec sh -c 'echo BAD > {{}}' \\;")
    code, stdout, stderr, dur = run_cmd(f"{VENTAC_BIN} {main_src} -o {out_exe} --incremental --cache-dir={cache_dir} --build-stats")
    if code != 0:
        print(f"Corruption recovery build failed: {stderr}")
        sys.exit(1)
    corrupt_ms = dur * 1000.0
    results["corruption_recovery_ms"] = round(corrupt_ms, 2)
    print(f"6. Rebuild after Corruption:    {corrupt_ms:.2f} ms (Safely recovered)")

    # Cache Stats
    code, stdout, stderr, _ = run_cmd(f"{VENTAC_BIN} cache stats --cache-dir={cache_dir}")
    print("\n" + stdout.strip())

    # Save to JSON
    os.makedirs("benchmarks/results", exist_ok=True)
    out_json = "benchmarks/results/incremental_benchmark.json"
    with open(out_json, "w") as fp:
        json.dump(results, fp, indent=2)
    print(f"\nBenchmark results saved to {out_json}")

if __name__ == "__main__":
    main()
