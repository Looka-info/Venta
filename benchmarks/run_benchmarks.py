import os
import subprocess
import time
import statistics
import json
import sys
import platform
import argparse
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.as_posix()
BIN_DIR = f"{REPO_ROOT}/target/bench_bins"

BENCHMARKS = [
    "fib",
    "collatz",
    "points",
    "vec_churn",
    "string_growth",
    "arith_tree",
    "loop_accum",
    "reg_pressure",
    "cse_workload",
    "inline_leaf",
    "inline_loop",
    "licm_workload",
    "lsr_workload",
    "sroa_workload",
    "unroll_peel_workload",
    "tco_workload",
    "dce_workload",
    "layout_workload",
]

def run_cmd(cmd):
    full_cmd = f"cd {REPO_ROOT} && export VANTA_LINKER=system && {cmd}"
    t0 = time.perf_counter()
    if os.path.exists("/proc/version"):
        res = subprocess.run(["bash", "-c", full_cmd], capture_output=True, text=True)
    else:
        res = subprocess.run(["wsl", "bash", "-c", full_cmd], capture_output=True, text=True)
    t1 = time.perf_counter()
    return res.returncode, res.stdout, res.stderr, (t1 - t0)

def get_env_metadata():
    _, kernel, _, _ = run_cmd("uname -srm")
    _, cpu_info, _, _ = run_cmd("lscpu | grep 'Model name' | head -n 1")
    _, rust_ver, _, _ = run_cmd("rustc --version")
    _, gcc_ver, _, _ = run_cmd("gcc --version | head -n 1")
    _, vanta_ver, _, _ = run_cmd("./target/release/vantac --version")
    _, git_sha, _, _ = run_cmd("git rev-parse HEAD")
    _, vantac_sha, _, _ = run_cmd("sha256sum ./target/release/vantac 2>/dev/null || echo 'not_built'")
    
    cpu_clean = cpu_info.split(":")[-1].strip() if ":" in cpu_info else "x86_64"
    vantac_hash = vantac_sha.split()[0] if vantac_sha else "unknown"
    return {
        "kernel": kernel.strip(),
        "cpu": cpu_clean,
        "rustc": rust_ver.strip(),
        "gcc": gcc_ver.strip(),
        "vantac": vanta_ver.strip(),
        "git_commit": git_sha.strip(),
        "vantac_sha256": vantac_hash,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }

def main():
    parser = argparse.ArgumentParser(description="Venta Benchmark Runner")
    parser.add_argument("--benchmark", "-b", type=str, help="Specific benchmark to run")
    parser.add_argument("--output", "-o", type=str, default="benchmarks/results/milestone-25-release-audit.json", help="Output JSON path")
    parser.add_argument("--trials", "-n", type=int, default=20, help="Number of benchmark trials (default: 20)")
    args = parser.parse_args()

    run_cmd(f"mkdir -p {BIN_DIR}")
    run_cmd("mkdir -p benchmarks/results")
    
    metadata = get_env_metadata()
    results = {
        "metadata": metadata,
        "benchmarks": {}
    }

    selected_benchmarks = [args.benchmark] if args.benchmark else BENCHMARKS

    for bench in selected_benchmarks:
        print(f"=== Compiling and running benchmark: {bench} ===")
        results["benchmarks"][bench] = {}
        
        # 1. Compile Vanta -O0
        cmd_vanta_o0 = f"./target/release/vantac -O0 benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_o0"
        ret, out, err, compile_time_vanta_o0 = run_cmd(cmd_vanta_o0)
        assert ret == 0, f"Vanta -O0 failed ({bench}): {err}\n{out}"
        
        # 2. Compile Vanta -O1 (No TCO)
        cmd_vanta_no_tco = f"./target/release/vantac -O1 --no-tail-calls benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_no_tco"
        ret, out, err, compile_time_vanta_no_tco = run_cmd(cmd_vanta_no_tco)
        assert ret == 0, f"Vanta -O1 No-TCO failed ({bench}): {err}\n{out}"
        
        # 3. Compile Vanta -O1 (No Global DCE)
        cmd_vanta_no_dce = f"./target/release/vantac -O1 --no-global-dce benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_no_dce"
        ret, out, err, compile_time_vanta_no_dce = run_cmd(cmd_vanta_no_dce)
        assert ret == 0, f"Vanta -O1 No-DCE failed ({bench}): {err}\n{out}"
        
        # 4. Compile Vanta -O1 (No Block Layout)
        cmd_vanta_no_layout = f"./target/release/vantac -O1 --no-block-layout benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_no_layout"
        ret, out, err, compile_time_vanta_no_layout = run_cmd(cmd_vanta_no_layout)
        assert ret == 0, f"Vanta -O1 No-Layout failed ({bench}): {err}\n{out}"
        
        # 5. Compile Vanta -O1 (Full Opt)
        cmd_vanta_o1 = f"./target/release/vantac -O1 benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_o1"
        ret, out, err, compile_time_vanta_o1 = run_cmd(cmd_vanta_o1)
        assert ret == 0, f"Vanta -O1 failed ({bench}): {err}\n{out}"
        
        # 6. Compile Rust -O3
        cmd_rust = f"rustc -O benchmarks/{bench}/{bench}.rs -o {BIN_DIR}/{bench}_rust"
        ret, out, err, compile_time_rust = run_cmd(cmd_rust)
        assert ret == 0, f"Rust failed ({bench}): {err}\n{out}"
        
        # 7. Compile C -O3
        cmd_c = f"gcc -O3 benchmarks/{bench}/{bench}.c -o {BIN_DIR}/{bench}_c"
        ret, out, err, compile_time_c = run_cmd(cmd_c)
        assert ret == 0, f"C failed ({bench}): {err}\n{out}"
        
        targets = [
            ("Vanta -O0", f"{BIN_DIR}/{bench}_vanta_o0", compile_time_vanta_o0),
            ("Vanta -O1 (No TCO)", f"{BIN_DIR}/{bench}_vanta_no_tco", compile_time_vanta_no_tco),
            ("Vanta -O1 (No DCE)", f"{BIN_DIR}/{bench}_vanta_no_dce", compile_time_vanta_no_dce),
            ("Vanta -O1 (No Layout)", f"{BIN_DIR}/{bench}_vanta_no_layout", compile_time_vanta_no_layout),
            ("Vanta -O1 (Full Opt)", f"{BIN_DIR}/{bench}_vanta_o1", compile_time_vanta_o1),
            ("Rust -O3", f"{BIN_DIR}/{bench}_rust", compile_time_rust),
            ("GCC -O3", f"{BIN_DIR}/{bench}_c", compile_time_c),
        ]
        
        expected_output = None
        target_sizes = {}
        target_times = {name: [] for name, _, _ in targets}
        
        # Warmup and establish validated expected output across all targets
        for name, bin_path, _ in targets:
            _, size_str, _, _ = run_cmd(f"stat -c %s {bin_path}")
            target_sizes[name] = int(size_str.strip())
            
            ret, out, err, _ = run_cmd(f"{bin_path}")
            assert ret == 0, f"Warmup failed for {name} ({bench}): {err}"
            out_trimmed = out.strip()
            
            if expected_output is None:
                expected_output = out_trimmed
            else:
                assert out_trimmed == expected_output, f"Output mismatch for {name} ({bench}): got '{out_trimmed}', expected '{expected_output}'"
        
        # Interleaved benchmark execution for thermal/frequency parity
        n_trials = args.trials
        for trial in range(n_trials):
            for name, bin_path, _ in targets:
                ret, out, err, run_time = run_cmd(f"{bin_path}")
                assert ret == 0, f"Trial {trial + 1} failed for {name} ({bench}): {err}"
                assert out.strip() == expected_output, f"Trial {trial + 1} output mismatch for {name} ({bench})"
                target_times[name].append(run_time)
        
        for name, bin_path, c_time in targets:
            times = target_times[name]
            med_time = statistics.median(times)
            mad_time = statistics.median([abs(x - med_time) for x in times])
            min_time = min(times)
            max_time = max(times)
            bin_size = target_sizes[name]
            
            results["benchmarks"][bench][name] = {
                "median": med_time,
                "mad": mad_time,
                "min": min_time,
                "max": max_time,
                "samples": times,
                "size": bin_size,
                "compile_time": c_time,
                "output": expected_output
            }
            print(f"  [{name}] Median: {med_time:.4f}s (MAD: {mad_time:.4f}s, Min: {min_time:.4f}s, Max: {max_time:.4f}s) | Size: {bin_size} B | Output: {expected_output}")

    # Print summary table
    print("\n\n=== MILESTONE 25 BENCHMARK RESULTS TABLE ===")
    print(f"System: {metadata['cpu']} | {metadata['kernel']}")
    print(f"Toolchains: Vanta {metadata['vantac']} | Rust {metadata['rustc']} | GCC {metadata['gcc']}")
    print(f"Git Commit: {metadata['git_commit']} | Vantac SHA256: {metadata['vantac_sha256']}")
    print("| Benchmark | Metric | Vanta -O0 | Vanta -O1 (No TCO) | Vanta -O1 (No DCE) | Vanta -O1 (No Layout) | Vanta -O1 (Full Opt) | Rust -O3 | GCC -O3 | Full vs -O0 | Vanta/Rust Ratio |")
    print("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for bench in selected_benchmarks:
        v0 = results["benchmarks"][bench]["Vanta -O0"]["median"]
        v_no_tco = results["benchmarks"][bench]["Vanta -O1 (No TCO)"]["median"]
        v_no_dce = results["benchmarks"][bench]["Vanta -O1 (No DCE)"]["median"]
        v_no_layout = results["benchmarks"][bench]["Vanta -O1 (No Layout)"]["median"]
        v1 = results["benchmarks"][bench]["Vanta -O1 (Full Opt)"]["median"]
        rs = results["benchmarks"][bench]["Rust -O3"]["median"]
        c = results["benchmarks"][bench]["GCC -O3"]["median"]
        speedup = v0 / v1 if v1 > 0 else 0
        ratio = v1 / rs if rs > 0 else 0
        print(f"| `{bench}` | Runtime (s) | {v0:.4f}s | {v_no_tco:.4f}s | {v_no_dce:.4f}s | {v_no_layout:.4f}s | {v1:.4f}s | {rs:.4f}s | {c:.4f}s | **{speedup:.2f}x** | **{ratio:.2f}x** |")
        
        sz_v0 = results["benchmarks"][bench]["Vanta -O0"]["size"]
        sz_no_tco = results["benchmarks"][bench]["Vanta -O1 (No TCO)"]["size"]
        sz_no_dce = results["benchmarks"][bench]["Vanta -O1 (No DCE)"]["size"]
        sz_no_layout = results["benchmarks"][bench]["Vanta -O1 (No Layout)"]["size"]
        sz_v1 = results["benchmarks"][bench]["Vanta -O1 (Full Opt)"]["size"]
        sz_rs = results["benchmarks"][bench]["Rust -O3"]["size"]
        sz_c = results["benchmarks"][bench]["GCC -O3"]["size"]
        print(f"| | Binary Size | {sz_v0:,} B | {sz_no_tco:,} B | {sz_no_dce:,} B | {sz_no_layout:,} B | {sz_v1:,} B | {sz_rs:,} B | {sz_c:,} B | - | - |")
        
        ct_v0 = results["benchmarks"][bench]["Vanta -O0"]["compile_time"]
        ct_no_tco = results["benchmarks"][bench]["Vanta -O1 (No TCO)"]["compile_time"]
        ct_no_dce = results["benchmarks"][bench]["Vanta -O1 (No DCE)"]["compile_time"]
        ct_no_layout = results["benchmarks"][bench]["Vanta -O1 (No Layout)"]["compile_time"]
        ct_v1 = results["benchmarks"][bench]["Vanta -O1 (Full Opt)"]["compile_time"]
        ct_rs = results["benchmarks"][bench]["Rust -O3"]["compile_time"]
        ct_c = results["benchmarks"][bench]["GCC -O3"]["compile_time"]
        print(f"| | Compile Time | {ct_v0:.2f}s | {ct_no_tco:.2f}s | {ct_no_dce:.2f}s | {ct_no_layout:.2f}s | {ct_v1:.2f}s | {ct_rs:.2f}s | {ct_c:.2f}s | - | - |")

    out_json = args.output
    with open(out_json, "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2)
    print(f"\nSaved benchmark results to {out_json}")

if __name__ == "__main__":
    main()

if __name__ == "__main__":
    main()
