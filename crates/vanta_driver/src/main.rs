// The Venta Compiler CLI (`ventac`).

use std::env;
use std::path::PathBuf;
use std::process;
use vanta_driver::{
    run_fmt, CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, FmtOptions, FmtResult,
    LinkerKind, OptLevel, TargetFeatures, TargetTriple,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandMode {
    Compile,
    Check,
    Fmt,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn get_usage_text() -> String {
    let invoked = env::args().next().unwrap_or_default();
    let file_name = std::path::Path::new(&invoked)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let bin_name = if file_name == "vantac" {
        "vantac"
    } else {
        "ventac"
    };
    let lang_name = if file_name == "vantac" {
        "Vanta"
    } else {
        "Venta"
    };
    format!(
        r#"{lang_name} Compiler ({bin_name}) v{version}

USAGE:
    {bin_name} [COMMAND] [OPTIONS] <input.va...>

COMMANDS:
    compile               Compile the input file (default)
    build                 Compile and link input files incrementally
    check                 Quickly verify syntax, types, borrows, and MIR without linking
    fmt                   Format {lang_name} source files canonically
    archive <subcommand>  Manage static archives: create, list, inspect
    cache <subcommand>    Manage build cache: stats, clean, verify, inspect

OPTIONS:
    -o <file>             Write output to <file> (default: a.out)
    --incremental         Enable deterministic incremental compilation
    --no-incremental      Disable incremental compilation (clean rebuild)
    --cache-dir=<dir>     Path to incremental cache directory
    --build-stats         Print detailed incremental build statistics
    --explain-rebuild     Explain reasons for rebuilding affected units
    -g, --debug           Emit DWARF 5 source-level debug information
    -O0, --no-opt         Disable all optimizations
    -O1, -O, --opt        Enable standard optimizations (default)
    --no-inline           Disable function inlining
    --no-ipa-constprop    Disable interprocedural constant propagation
    --no-licm             Disable Loop Invariant Code Motion (LICM)
    --no-indvar           Disable Induction Variable Optimization (IVO)
    --no-strength-reduction Disable Loop Strength Reduction (LSR)
    --no-sroa             Disable Scalar Replacement of Aggregates (SROA)
    --no-loop-peel        Disable Loop Peeling
    --no-loop-unroll      Disable Loop Unrolling
    --no-vectorize        Disable Loop Auto-Vectorization (SIMD)
    --no-instruction-schedule Disable Instruction Scheduling
    --no-global-dce       Disable Global Dead Code Elimination
    --no-tail-calls       Disable Safe Tail-Call Optimization
    --no-block-layout     Disable Branch-Aware Machine-Code Layout
    --no-code-align       Disable loop code alignment
    --no-gvn              Disable Global Value Numbering & CSE
    --no-mem2reg          Disable Memory-to-Register (SSA) promotion
    --no-regalloc         Disable physical register allocation (spill all values)
    --emit=<target>       Emit intermediate target:
                            tokens   - Token stream
                            ast      - Abstract Syntax Tree
                            mir      - Vanta Mid-Level IR
                            obj      - Relocatable object file (.o / .obj)
                            exe      - Linked native executable (default)
    --target=<triple>     Target triple:
                            x86_64-unknown-linux-gnu (linux, default)
                            x86_64-pc-windows-msvc (windows, win64)
                            aarch64-apple-darwin (macos, apple-silicon)
    --linker=<kind>       Linker to use: internal (pure-Rust, default) or system
    --link=<archive>      Link static archive (.a / .lib)
    --map-file=<file>     Emit detailed link map file
    -L <path>             Add library search path
    -l <lib>              Link against specified library
    --json                Emit diagnostics formatted as JSON
    --diagnostics=<fmt>   Diagnostics format: human (default) or json
    --format=<fmt>        Alias for --diagnostics
    --std-path=<path>     Path to standard library root
    --runtime-path=<path> Path to precompiled libvanta_rt.a archive
    --target-feature=<f>  Target CPU features (e.g. +avx2,+fma, or -avx512f)
    --target-cpu=<cpu>    Target CPU architecture (baseline, x86-64-v3, x86-64-v4, native)
    --check               Check formatting without modifying files (fmt mode)
    --stdout              Write formatted file to stdout (fmt mode)
    -v, --verbose         Enable verbose diagnostic output
    -V, --version         Print version information
    -h, --help            Print help information
"#,
        version = VERSION
    )
}

fn print_help() {
    print!("{}", get_usage_text());
}

fn print_usage_stderr() {
    eprint!("{}", get_usage_text());
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage_stderr();
        process::exit(2);
    }

    let mut mode = CommandMode::Compile;
    let mut input_file: Option<PathBuf> = None;
    let mut fmt_files: Vec<PathBuf> = Vec::new();
    let mut output_file: Option<PathBuf> = None;
    let mut emit_target = EmitTarget::Exe;
    let mut opt_level = OptLevel::O1;
    let mut verbose = false;
    let mut diagnostic_format = DiagnosticFormat::Human;
    let mut fmt_check = false;
    let mut fmt_stdout = false;
    let mut std_path: Option<PathBuf> = None;
    let mut runtime_path: Option<PathBuf> = None;
    let mut enable_reg_alloc = true;
    let mut enable_mem2reg = true;
    let mut enable_gvn = true;
    let mut enable_inline = true;
    let mut enable_ipa_constprop = true;
    let mut enable_licm = true;
    let mut enable_indvar = true;
    let mut enable_strength_reduction = true;
    let mut enable_sroa = true;
    let mut enable_loop_peel = true;
    let mut enable_loop_unroll = true;
    let mut enable_vectorize = true;
    let mut enable_instruction_schedule = true;
    let mut enable_global_dce = true;
    let mut enable_tail_calls = true;
    let mut enable_block_layout = true;
    let mut enable_code_align = true;
    let mut target_features = TargetFeatures::baseline();
    let mut target = TargetTriple::default();
    let mut debug_info = false;
    let mut linker = LinkerKind::Internal;
    let mut link_archives: Vec<PathBuf> = Vec::new();
    let mut additional_inputs: Vec<PathBuf> = Vec::new();
    let mut map_file: Option<PathBuf> = None;
    let mut lib_search_paths: Vec<PathBuf> = Vec::new();
    let mut libs: Vec<String> = Vec::new();
    let mut incremental = false;
    let mut cache_dir: Option<PathBuf> = None;
    let mut build_stats = false;
    let mut explain_rebuild = false;

    let mut idx = 1;

    // Check for explicit subcommand as first argument
    if idx < args.len() {
        match args[idx].as_str() {
            "check" => {
                mode = CommandMode::Check;
                idx += 1;
            }
            "compile" => {
                mode = CommandMode::Compile;
                idx += 1;
            }
            "build" => {
                mode = CommandMode::Compile;
                incremental = true;
                idx += 1;
            }
            "fmt" => {
                mode = CommandMode::Fmt;
                idx += 1;
            }
            "archive" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!(
                        "Error: 'archive' requires a subcommand: 'create', 'list', or 'inspect'"
                    );
                    process::exit(2);
                }
                match args[idx].as_str() {
                    "create" => {
                        idx += 1;
                        if idx >= args.len() {
                            eprintln!("Error: 'archive create' requires output archive path");
                            process::exit(2);
                        }
                        let out_ar = PathBuf::from(&args[idx]);
                        idx += 1;
                        let member_paths: Vec<PathBuf> =
                            args[idx..].iter().map(PathBuf::from).collect();
                        if member_paths.is_empty() {
                            eprintln!(
                                "Error: 'archive create' requires at least one object member"
                            );
                            process::exit(2);
                        }
                        match vanta_linker::create_archive(&out_ar, &member_paths) {
                            Ok(bytes) => {
                                println!(
                                    "Created static archive '{}' ({} bytes, {} members)",
                                    out_ar.display(),
                                    bytes,
                                    member_paths.len()
                                );
                                process::exit(0);
                            }
                            Err(e) => {
                                eprintln!("Error creating archive: {}", e);
                                process::exit(1);
                            }
                        }
                    }
                    "list" => {
                        idx += 1;
                        if idx >= args.len() {
                            eprintln!("Error: 'archive list' requires archive path");
                            process::exit(2);
                        }
                        let ar_path = PathBuf::from(&args[idx]);
                        match vanta_linker::list_archive(&ar_path) {
                            Ok(members) => {
                                for m in members {
                                    println!("{}", m);
                                }
                                process::exit(0);
                            }
                            Err(e) => {
                                eprintln!("Error listing archive: {}", e);
                                process::exit(1);
                            }
                        }
                    }
                    "inspect" => {
                        idx += 1;
                        if idx >= args.len() {
                            eprintln!("Error: 'archive inspect' requires archive path");
                            process::exit(2);
                        }
                        let ar_path = PathBuf::from(&args[idx]);
                        match vanta_linker::inspect_archive(&ar_path) {
                            Ok(insp) => {
                                println!("Archive: {}", insp.path.display());
                                println!("Total size: {} bytes", insp.total_size);
                                println!("Members ({}):", insp.members_count);
                                for (name, sz) in &insp.members {
                                    println!("  - {:<30} ({} bytes)", name, sz);
                                }
                                println!("Symbols ({}):", insp.symbols_count);
                                for (sym, mem) in &insp.symbols {
                                    println!("  - {:<30} -> {}", sym, mem);
                                }
                                process::exit(0);
                            }
                            Err(e) => {
                                eprintln!("Error inspecting archive: {}", e);
                                process::exit(1);
                            }
                        }
                    }
                    other => {
                        eprintln!(
                            "Error: Unknown archive subcommand '{}'. Expected 'create', 'list', or 'inspect'",
                            other
                        );
                        process::exit(2);
                    }
                }
            }
            "cache" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!(
                        "Error: 'cache' requires a subcommand: 'stats', 'clean', 'verify', or 'inspect'"
                    );
                    process::exit(2);
                }
                let mut target_filter: Option<String> = None;
                let mut cache_root =
                    PathBuf::from(vanta_driver::CacheManager::DEFAULT_CACHE_REL_PATH);
                let subcmd = args[idx].clone();
                idx += 1;
                while idx < args.len() {
                    if (args[idx] == "--target" || args[idx] == "-t") && idx + 1 < args.len() {
                        target_filter = Some(args[idx + 1].clone());
                        idx += 2;
                    } else if args[idx].starts_with("--target=") {
                        target_filter = Some(args[idx]["--target=".len()..].to_string());
                        idx += 1;
                    } else if args[idx] == "--cache-dir" && idx + 1 < args.len() {
                        cache_root = PathBuf::from(&args[idx + 1]);
                        idx += 2;
                    } else if args[idx].starts_with("--cache-dir=") {
                        cache_root = PathBuf::from(&args[idx]["--cache-dir=".len()..]);
                        idx += 1;
                    } else {
                        idx += 1;
                    }
                }
                let default_triple = target.spec().triple.to_string();
                let target_triple = target_filter.as_deref().unwrap_or(&default_triple);
                let manager = match vanta_driver::CacheManager::new(cache_root, target_triple) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Error opening cache: {}", e);
                        process::exit(1);
                    }
                };
                match subcmd.as_str() {
                    "stats" => {
                        let stats = manager.stats();
                        println!("Venta Cache Stats (target: {}):", manager.target_triple());
                        println!("  Total cache size: {} bytes", stats.total_bytes);
                        println!("  Cached objects:   {}", stats.object_count);
                        println!("  Cached functions: {}", stats.function_count);
                        println!("  Cached MIR units: {}", stats.mir_count);
                        println!("  Cached interfaces:{}", stats.interface_count);
                        println!("  Cached links:     {}", stats.link_count);
                        println!("  Cached archives:  {}", stats.archive_count);
                        process::exit(0);
                    }
                    "clean" => {
                        let report = manager.clean_cache(target_filter.as_deref());
                        println!("Venta Cache Cleaned:");
                        println!("  Files removed: {}", report.files_removed);
                        println!("  Bytes freed:   {} bytes", report.bytes_freed);
                        process::exit(0);
                    }
                    "verify" | "inspect" => {
                        let report = manager.verify_cache();
                        println!(
                            "Venta Cache Verification (target: {}):",
                            manager.target_triple()
                        );
                        println!("  Total entries:       {}", report.total_entries);
                        println!("  Valid entries:       {}", report.valid_entries);
                        println!("  Corrupted entries:   {}", report.corrupted_entries);
                        println!("  Schema mismatches:   {}", report.schema_mismatches);
                        println!("  Compiler mismatches: {}", report.compiler_mismatches);
                        println!("  Missing data files:  {}", report.missing_data_files);
                        process::exit(0);
                    }
                    other => {
                        eprintln!(
                            "Error: Unknown cache subcommand '{}'. Expected 'stats', 'clean', 'verify', or 'inspect'",
                            other
                        );
                        process::exit(2);
                    }
                }
            }
            "-V" | "--version" => {
                let invoked = env::args().next().unwrap_or_default();
                let file_name = std::path::Path::new(&invoked)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                let bin_name = if file_name == "vantac" {
                    "vantac"
                } else {
                    "ventac"
                };
                println!("{} {}", bin_name, VERSION);
                process::exit(0);
            }
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            _ => {}
        }
    }

    while idx < args.len() {
        let arg = &args[idx];
        match arg.as_str() {
            "-V" | "--version" => {
                let invoked = env::args().next().unwrap_or_default();
                let file_name = std::path::Path::new(&invoked)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                let bin_name = if file_name == "vantac" {
                    "vantac"
                } else {
                    "ventac"
                };
                println!("{} {}", bin_name, VERSION);
                process::exit(0);
            }
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "--incremental" => {
                incremental = true;
            }
            "--no-incremental" => {
                incremental = false;
            }
            "--build-stats" => {
                build_stats = true;
            }
            "--explain-rebuild" => {
                explain_rebuild = true;
            }
            s if s.starts_with("--cache-dir=") => {
                let p = s.split_once('=').unwrap().1;
                cache_dir = Some(PathBuf::from(p));
            }
            "--cache-dir" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --cache-dir requires a path");
                    process::exit(2);
                }
                cache_dir = Some(PathBuf::from(&args[idx]));
            }
            "--check" => {
                fmt_check = true;
            }
            "--stdout" => {
                fmt_stdout = true;
            }
            "-O0" | "--no-opt" => {
                opt_level = OptLevel::None;
            }
            "-O1" | "-O" | "--opt" => {
                opt_level = OptLevel::O1;
            }
            "-g" | "--debug" => {
                debug_info = true;
            }
            "--no-regalloc" => {
                enable_reg_alloc = false;
            }
            "--no-mem2reg" => {
                enable_mem2reg = false;
            }
            "--no-gvn" => {
                enable_gvn = false;
            }
            "--no-inline" => {
                enable_inline = false;
            }
            "--no-ipa-constprop" => {
                enable_ipa_constprop = false;
            }
            "--no-licm" => {
                enable_licm = false;
            }
            "--no-indvar" => {
                enable_indvar = false;
            }
            "--no-strength-reduction" => {
                enable_strength_reduction = false;
            }
            "--no-sroa" => {
                enable_sroa = false;
            }
            "--no-loop-peel" | "--no-peel" => {
                enable_loop_peel = false;
            }
            "--no-loop-unroll" | "--no-unroll" => {
                enable_loop_unroll = false;
            }
            "--no-vectorize" | "--no-vec" => {
                enable_vectorize = false;
            }
            "--no-instruction-schedule" | "--no-sched" => {
                enable_instruction_schedule = false;
            }
            "--no-global-dce" => {
                enable_global_dce = false;
            }
            "--no-tail-calls" | "--no-tco" => {
                enable_tail_calls = false;
            }
            "--no-block-layout" | "--no-layout" => {
                enable_block_layout = false;
            }
            "--no-code-align" | "--no-align" => {
                enable_code_align = false;
            }
            "-o" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: -o requires an argument");
                    process::exit(2);
                }
                output_file = Some(PathBuf::from(&args[idx]));
            }
            "--json" => {
                diagnostic_format = DiagnosticFormat::Json;
            }
            s if s.starts_with("--diagnostics=") || s.starts_with("--format=") => {
                let fmt = s
                    .strip_prefix("--diagnostics=")
                    .or_else(|| s.strip_prefix("--format="))
                    .unwrap_or_default();
                diagnostic_format = match fmt {
                    "json" => DiagnosticFormat::Json,
                    "human" => DiagnosticFormat::Human,
                    _ => {
                        eprintln!(
                            "Error: Unknown diagnostics format '{}', expected 'human' or 'json'",
                            fmt
                        );
                        process::exit(2);
                    }
                };
            }
            s if s.starts_with("--emit=") => {
                let target = &s["--emit=".len()..];
                emit_target = match target {
                    "tokens" => EmitTarget::Tokens,
                    "ast" => EmitTarget::Ast,
                    "mir" => EmitTarget::Mir,
                    "obj" => EmitTarget::Obj,
                    "exe" => EmitTarget::Exe,
                    _ => {
                        eprintln!("Error: Unknown emit target '{}'", target);
                        process::exit(2);
                    }
                };
            }
            s if s.starts_with("--std-path=") => {
                let p = s.split_once('=').unwrap().1;
                std_path = Some(PathBuf::from(p));
            }
            "--std-path" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --std-path requires an argument");
                    process::exit(2);
                }
                std_path = Some(PathBuf::from(&args[idx]));
            }
            s if s.starts_with("--runtime-path=") => {
                let p = s.split_once('=').unwrap().1;
                runtime_path = Some(PathBuf::from(p));
            }
            "--runtime-path" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --runtime-path requires an argument");
                    process::exit(2);
                }
                runtime_path = Some(PathBuf::from(&args[idx]));
            }
            s if s.starts_with("--target-feature=") => {
                let feat_str = &s["--target-feature=".len()..];
                if let Err(e) = target_features.parse_feature_list(feat_str) {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
            "--target-feature" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --target-feature requires an argument");
                    process::exit(2);
                }
                if let Err(e) = target_features.parse_feature_list(&args[idx]) {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
            s if s.starts_with("--target-cpu=") => {
                let cpu_str = &s["--target-cpu=".len()..];
                match TargetFeatures::from_target_cpu(cpu_str) {
                    Ok(feats) => target_features = feats,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        process::exit(2);
                    }
                }
            }
            "--target-cpu" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --target-cpu requires an argument");
                    process::exit(2);
                }
                match TargetFeatures::from_target_cpu(&args[idx]) {
                    Ok(feats) => target_features = feats,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        process::exit(2);
                    }
                }
            }
            s if s.starts_with("--target=") => {
                let t_str = &s["--target=".len()..];
                match t_str.parse::<TargetTriple>() {
                    Ok(t) => target = t,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        process::exit(2);
                    }
                }
            }
            "--target" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --target requires an argument");
                    process::exit(2);
                }
                match args[idx].parse::<TargetTriple>() {
                    Ok(t) => target = t,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        process::exit(2);
                    }
                }
            }
            s if s.starts_with("-C") => {
                let c_arg = if s == "-C" {
                    idx += 1;
                    if idx >= args.len() {
                        eprintln!("Error: -C requires an argument");
                        process::exit(2);
                    }
                    args[idx].as_str()
                } else {
                    &s[2..]
                };
                if let Some(feat) = c_arg.strip_prefix("target-feature=") {
                    if let Err(e) = target_features.parse_feature_list(feat) {
                        eprintln!("Error: {}", e);
                        process::exit(2);
                    }
                } else if let Some(cpu) = c_arg.strip_prefix("target-cpu=") {
                    match TargetFeatures::from_target_cpu(cpu) {
                        Ok(feats) => target_features = feats,
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            process::exit(2);
                        }
                    }
                } else if let Some(t_str) = c_arg.strip_prefix("target=") {
                    match t_str.parse::<TargetTriple>() {
                        Ok(t) => target = t,
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            process::exit(2);
                        }
                    }
                } else {
                    eprintln!("Error: Unknown codegen option '-C {}'", c_arg);
                    process::exit(2);
                }
            }
            s if s.starts_with("--linker=") => {
                let l_str = &s["--linker=".len()..];
                linker = match l_str {
                    "internal" => LinkerKind::Internal,
                    "system" => LinkerKind::System,
                    _ => {
                        eprintln!(
                            "Error: Unknown linker '{}', expected 'internal' or 'system'",
                            l_str
                        );
                        process::exit(2);
                    }
                };
            }
            "--linker" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --linker requires an argument");
                    process::exit(2);
                }
                linker = match args[idx].as_str() {
                    "internal" => LinkerKind::Internal,
                    "system" => LinkerKind::System,
                    _ => {
                        eprintln!(
                            "Error: Unknown linker '{}', expected 'internal' or 'system'",
                            args[idx]
                        );
                        process::exit(2);
                    }
                };
            }
            s if s.starts_with("--link=") => {
                let p = s.split_once('=').unwrap().1;
                link_archives.push(PathBuf::from(p));
            }
            "--link" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --link requires an archive path");
                    process::exit(2);
                }
                link_archives.push(PathBuf::from(&args[idx]));
            }
            s if s.starts_with("--map-file=") || s.starts_with("--map=") => {
                let p = s.split_once('=').unwrap().1;
                map_file = Some(PathBuf::from(p));
            }
            "--map-file" | "--map" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: --map-file requires a path");
                    process::exit(2);
                }
                map_file = Some(PathBuf::from(&args[idx]));
            }
            "-L" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: -L requires a path");
                    process::exit(2);
                }
                lib_search_paths.push(PathBuf::from(&args[idx]));
            }
            "-l" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: -l requires a library name");
                    process::exit(2);
                }
                libs.push(args[idx].clone());
            }
            s if s.starts_with('-') => {
                eprintln!("Error: Unknown option '{}'", s);
                process::exit(2);
            }
            _ => {
                if mode == CommandMode::Fmt {
                    fmt_files.push(PathBuf::from(arg));
                } else if input_file.is_none() {
                    input_file = Some(PathBuf::from(arg));
                } else {
                    additional_inputs.push(PathBuf::from(arg));
                }
            }
        }
        idx += 1;
    }

    if mode == CommandMode::Fmt {
        let fmt_options = FmtOptions {
            files: fmt_files,
            check: fmt_check,
            stdout: fmt_stdout,
            verbose,
            diagnostic_format,
        };

        let (res, stdout_str, stderr_str) = run_fmt(&fmt_options);
        if !stdout_str.is_empty() {
            print!("{}", stdout_str);
        }
        if !stderr_str.is_empty() {
            eprint!("{}", stderr_str);
        }

        let code = match res {
            FmtResult::Success => 0,
            FmtResult::NeedsFormatting => 1,
            FmtResult::SyntaxError => {
                if fmt_check {
                    2
                } else {
                    1
                }
            }
            FmtResult::IoError => 2,
        };
        process::exit(code);
    }

    let input_path = match input_file {
        Some(p) => p,
        None => {
            eprintln!("Error: No input file specified");
            process::exit(2);
        }
    };

    let out_path = output_file.unwrap_or_else(|| match emit_target {
        EmitTarget::Obj => PathBuf::from(format!("a.{}", target.spec().obj_extension)),
        EmitTarget::Exe => {
            if target.spec().os == vanta_driver::Os::Windows {
                PathBuf::from("a.exe")
            } else {
                PathBuf::from("a.out")
            }
        }
        _ => PathBuf::from("out.txt"),
    });

    let options = CompilerOptions {
        input_file: input_path,
        output_file: out_path,
        emit_target,
        target,
        opt_level,
        verbose,
        diagnostic_format,
        std_path,
        runtime_path,
        enable_reg_alloc,
        enable_mem2reg,
        enable_gvn,
        enable_inline,
        enable_ipa_constprop,
        enable_licm,
        enable_indvar,
        enable_strength_reduction,
        enable_sroa,
        enable_loop_peel,
        enable_loop_unroll,
        enable_vectorize,
        enable_instruction_schedule,
        enable_global_dce,
        enable_tail_calls,
        enable_block_layout,
        enable_code_align,
        target_features,
        debug_info,
        linker,
        link_archives,
        additional_inputs,
        map_file,
        lib_search_paths,
        libs,
        incremental,
        cache_dir,
        build_stats,
        explain_rebuild,
    };

    let mut session = CompilerSession::new();

    match mode {
        CommandMode::Fmt => unreachable!(),
        CommandMode::Check => match session.check(&options) {
            Ok(()) => {
                if diagnostic_format == DiagnosticFormat::Json {
                    println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
                } else if verbose {
                    println!("Check successful: 0 errors");
                }
                process::exit(0);
            }
            Err(_) => {
                if diagnostic_format == DiagnosticFormat::Json {
                    println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
                } else {
                    eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
                }
                process::exit(1);
            }
        },
        CommandMode::Compile => match session.compile(&options) {
            Ok(Some(bytes)) => {
                if diagnostic_format == DiagnosticFormat::Json {
                    println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
                }
                if matches!(
                    emit_target,
                    EmitTarget::Tokens | EmitTarget::Ast | EmitTarget::Mir
                ) {
                    println!("{}", String::from_utf8_lossy(&bytes));
                }
                if build_stats {
                    if let Some(stats) = &session.last_build_stats {
                        println!("Venta incremental build:");
                        println!("  Source files:");
                        println!("    Total:                 {}", stats.source_files_total);
                        println!(
                            "    Unchanged:             {}",
                            stats.source_files_unchanged
                        );
                        println!("    Changed:                {}", stats.source_files_changed);
                        println!("  Objects:");
                        println!("    Reused:                 {}", stats.objects_reused);
                        println!("    Regenerated:            {}", stats.objects_regenerated);
                        println!("  Linking:");
                        println!(
                            "    Status:                 {}",
                            if stats.linking_performed {
                                "Performed"
                            } else {
                                "Skipped (no-op)"
                            }
                        );
                        println!(
                            "    Reason:                 {}",
                            stats
                                .linking_reason
                                .as_deref()
                                .unwrap_or("No link inputs changed")
                        );
                        println!("  Cache:");
                        println!("    Hits:                  {}", stats.cache_hits);
                        println!("    Misses:                 {}", stats.cache_misses);
                        println!("    Corrupted recovered:    {}", stats.cache_corruptions);
                    }
                }
                if explain_rebuild {
                    if session.rebuild_explanations.is_empty() {
                        println!("All artifacts up to date; zero rebuilds.");
                    } else {
                        println!("Incremental rebuild explanations:");
                        for (unit, reason) in &session.rebuild_explanations {
                            println!("  - {}: {}", unit, reason);
                        }
                    }
                }
                process::exit(0);
            }
            Ok(None) => {
                if diagnostic_format == DiagnosticFormat::Json {
                    println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
                }
                if build_stats {
                    if let Some(stats) = &session.last_build_stats {
                        println!("Venta incremental build:");
                        println!("  Source files:");
                        println!("    Total:                 {}", stats.source_files_total);
                        println!(
                            "    Unchanged:             {}",
                            stats.source_files_unchanged
                        );
                        println!("    Changed:                {}", stats.source_files_changed);
                        println!("  Objects:");
                        println!("    Reused:                 {}", stats.objects_reused);
                        println!("    Regenerated:            {}", stats.objects_regenerated);
                        println!("  Linking:");
                        println!(
                            "    Status:                 {}",
                            if stats.linking_performed {
                                "Performed"
                            } else {
                                "Skipped (no-op)"
                            }
                        );
                        println!(
                            "    Reason:                 {}",
                            stats
                                .linking_reason
                                .as_deref()
                                .unwrap_or("No link inputs changed")
                        );
                        println!("  Cache:");
                        println!("    Hits:                  {}", stats.cache_hits);
                        println!("    Misses:                 {}", stats.cache_misses);
                        println!("    Corrupted recovered:    {}", stats.cache_corruptions);
                    }
                }
                process::exit(0);
            }
            Err(err) => {
                if diagnostic_format == DiagnosticFormat::Json {
                    println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
                } else {
                    let rendered = session.render_diagnostics(DiagnosticFormat::Human);
                    if !rendered.trim().is_empty() {
                        eprintln!("{}", rendered);
                    } else {
                        eprintln!("vantac error: {}", err);
                    }
                }
                process::exit(1);
            }
        },
    }
}
