import os
import subprocess
import time
import statistics
import json
import sys
import platform

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
BIN_DIR = "target/bench_bins"

def run_cmd(cmd):
    t0 = time.perf_counter()
    if os.path.exists("/proc/version"):
        res = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True)
    else:
        res = subprocess.run(["wsl", "bash", "-c", cmd], capture_output=True, text=True)
    t1 = time.perf_counter()
    return res.returncode, res.stdout, res.stderr, (t1 - t0)

def get_env_metadata():
    _, kernel, _, _ = run_cmd("uname -srm")
    _, cpu_info, _, _ = run_cmd("lscpu | grep 'Model name' | head -n 1")
    _, rust_ver, _, _ = run_cmd("rustc --version")
    _, gcc_ver, _, _ = run_cmd("gcc --version | head -n 1")
    _, vanta_ver, _, _ = run_cmd("./target/release/vantac --version")
    
    cpu_clean = cpu_info.split(":")[-1].strip() if ":" in cpu_info else "x86_64"
    return {
        "kernel": kernel.strip(),
        "cpu": cpu_clean,
        "rustc": rust_ver.strip(),
        "gcc": gcc_ver.strip(),
        "vantac": vanta_ver.strip(),
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }

def main():
    run_cmd(f"mkdir -p {BIN_DIR}")
    run_cmd("mkdir -p benchmarks/results")
    
    metadata = get_env_metadata()
    results = {
        "metadata": metadata,
        "benchmarks": {}
    }

    for bench in BENCHMARKS:
        print(f"=== Compiling and running benchmark: {bench} ===")
        results["benchmarks"][bench] = {}
        
        # 1. Compile Vanta -O0
        cmd_vanta_o0 = f"cd /mnt/c/Users/mumer/OneDrive/Documents/Venta && ./target/release/vantac -O0 benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_o0"
        ret, out, err, compile_time_vanta_o0 = run_cmd(cmd_vanta_o0)
        assert ret == 0, f"Vanta -O0 failed ({bench}): {err}\n{out}"
        
        # 2. Compile Vanta -O1 (No TCO)
        cmd_vanta_no_tco = f"cd /mnt/c/Users/mumer/OneDrive/Documents/Venta && ./target/release/vantac -O1 --no-tail-calls benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_no_tco"
        ret, out, err, compile_time_vanta_no_tco = run_cmd(cmd_vanta_no_tco)
        assert ret == 0, f"Vanta -O1 No-TCO failed ({bench}): {err}\n{out}"
        
        # 3. Compile Vanta -O1 (No Global DCE)
        cmd_vanta_no_dce = f"cd /mnt/c/Users/mumer/OneDrive/Documents/Venta && ./target/release/vantac -O1 --no-global-dce benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_no_dce"
        ret, out, err, compile_time_vanta_no_dce = run_cmd(cmd_vanta_no_dce)
        assert ret == 0, f"Vanta -O1 No-DCE failed ({bench}): {err}\n{out}"
        
        # 4. Compile Vanta -O1 (No Block Layout)
        cmd_vanta_no_layout = f"cd /mnt/c/Users/mumer/OneDrive/Documents/Venta && ./target/release/vantac -O1 --no-block-layout benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_no_layout"
        ret, out, err, compile_time_vanta_no_layout = run_cmd(cmd_vanta_no_layout)
        assert ret == 0, f"Vanta -O1 No-Layout failed ({bench}): {err}\n{out}"
        
        # 5. Compile Vanta -O1 (Full Opt)
        cmd_vanta_o1 = f"cd /mnt/c/Users/mumer/OneDrive/Documents/Venta && ./target/release/vantac -O1 benchmarks/{bench}/{bench}.va -o {BIN_DIR}/{bench}_vanta_o1"
        ret, out, err, compile_time_vanta_o1 = run_cmd(cmd_vanta_o1)
        assert ret == 0, f"Vanta -O1 failed ({bench}): {err}\n{out}"
        
        # 6. Compile Rust -O3
        cmd_rust = f"cd /mnt/c/Users/mumer/OneDrive/Documents/Venta && rustc -O benchmarks/{bench}/{bench}.rs -o {BIN_DIR}/{bench}_rust"
        ret, out, err, compile_time_rust = run_cmd(cmd_rust)
        assert ret == 0, f"Rust failed ({bench}): {err}\n{out}"
        
        # 7. Compile C -O3
        cmd_c = f"cd /mnt/c/Users/mumer/OneDrive/Documents/Venta && gcc -O3 benchmarks/{bench}/{bench}.c -o {BIN_DIR}/{bench}_c"
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
        
        for name, bin_path, c_time in targets:
            # Measure binary size
            _, size_str, _, _ = run_cmd(f"stat -c %s {bin_path}")
            bin_size = int(size_str.strip())
            
            # Warmup run
            ret, out, err, _ = run_cmd(f"{bin_path}")
            assert ret == 0, f"Run failed for {name} ({bench}): {err}"
            out_trimmed = out.strip()
            
            if expected_output is None:
                expected_output = out_trimmed
            else:
                assert out_trimmed == expected_output, f"Output mismatch for {name} ({bench}): got '{out_trimmed}', expected '{expected_output}'"
            
            # Measured runs (5 trials)
            times = []
            for _ in range(5):
                _, _, _, run_time = run_cmd(f"{bin_path}")
                times.append(run_time)
            
            med_time = statistics.median(times)
            mad_time = statistics.median([abs(x - med_time) for x in times])
            min_time = min(times)
            max_time = max(times)
            
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
    print("| Benchmark | Metric | Vanta -O0 | Vanta -O1 (No TCO) | Vanta -O1 (No DCE) | Vanta -O1 (No Layout) | Vanta -O1 (Full Opt) | Rust -O3 | GCC -O3 | Full vs -O0 | Vanta/Rust Ratio |")
    print("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for bench in BENCHMARKS:
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

    out_json = sys.argv[1] if len(sys.argv) > 1 else "benchmarks/results/milestone-25-release-audit.json"
    with open(out_json, "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2)
    print(f"\nSaved benchmark results to {out_json}")

if __name__ == "__main__":
    main()
