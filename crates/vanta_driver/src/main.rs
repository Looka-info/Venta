// The Venta Compiler CLI (`ventac`).

use std::env;
use std::path::PathBuf;
use std::process;
use vanta_driver::{
    run_fmt, CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, FmtOptions, FmtResult,
    OptLevel, TargetFeatures,
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
    check                 Quickly verify syntax, types, borrows, and MIR without linking
    fmt                   Format {lang_name} source files canonically

OPTIONS:
    -o <file>             Write output to <file> (default: a.out)
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
                            obj      - Relocatable ELF64 object file (.o)
                            exe      - Linked native executable (default)
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
    let mut debug_info = false;

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
            "fmt" => {
                mode = CommandMode::Fmt;
                idx += 1;
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
                } else {
                    eprintln!("Error: Unknown codegen option '-C {}'", c_arg);
                    process::exit(2);
                }
            }
            s if s.starts_with('-') => {
                eprintln!("Error: Unknown option '{}'", s);
                process::exit(2);
            }
            _ => {
                if mode == CommandMode::Fmt {
                    fmt_files.push(PathBuf::from(arg));
                } else {
                    if input_file.is_some() {
                        eprintln!("Error: Multiple input files not supported");
                        process::exit(2);
                    }
                    input_file = Some(PathBuf::from(arg));
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
        EmitTarget::Obj => PathBuf::from("a.o"),
        EmitTarget::Exe => PathBuf::from("a.out"),
        _ => PathBuf::from("out.txt"),
    });

    let options = CompilerOptions {
        input_file: input_path,
        output_file: out_path,
        emit_target,
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
                process::exit(0);
            }
            Ok(None) => {
                if diagnostic_format == DiagnosticFormat::Json {
                    println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
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
