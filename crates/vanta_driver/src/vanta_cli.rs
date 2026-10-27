//! Unified developer CLI for Vanta (`vanta`).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};
use vanta_driver::{
    run_fmt, CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, FmtOptions, FmtResult,
    OptLevel,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn get_usage_text() -> String {
    format!(
        r#"Vanta Toolchain Driver (vanta) v{version}

USAGE:
    vanta <COMMAND> [OPTIONS]

COMMANDS:
    run <file.va> [-- <args>...]   Compile and run a Vanta program immediately
    compile <file.va> [-o <out>]   Compile a Vanta program to a native binary
    check <file.va>                Verify syntax, types, borrows, and MIR without compiling
    fmt [files...] [--check]       Format Vanta source files canonically

OPTIONS:
    -V, --version                  Print version information
    -h, --help                     Print help information
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

    match args[1].as_str() {
        "-V" | "--version" => {
            println!("vanta {}", VERSION);
            process::exit(0);
        }
        "-h" | "--help" => {
            print_help();
            process::exit(0);
        }
        "run" => handle_run(&args[2..]),
        "compile" => handle_compile(&args[2..]),
        "check" => handle_check(&args[2..]),
        "fmt" => handle_fmt(&args[2..]),
        unknown => {
            eprintln!("Error: Unknown command '{}'", unknown);
            print_usage_stderr();
            process::exit(2);
        }
    }
}

fn handle_run(args: &[String]) {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        process::exit(0);
    }

    if args.is_empty() {
        eprintln!("Error: 'vanta run' requires a source file");
        process::exit(2);
    }

    let mut input_file: Option<PathBuf> = None;
    let mut program_args: Vec<String> = Vec::new();
    let mut std_path: Option<PathBuf> = None;
    let mut runtime_path: Option<PathBuf> = None;
    let mut opt_level = OptLevel::O1;
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
    let mut enable_instruction_schedule = true;
    let mut enable_global_dce = true;
    let mut enable_tail_calls = true;
    let mut enable_block_layout = true;
    let mut enable_code_align = true;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            program_args.extend_from_slice(&args[i + 1..]);
            break;
        } else if arg == "-O0" || arg == "--no-opt" {
            opt_level = OptLevel::None;
        } else if arg == "-O1" || arg == "-O" || arg == "--opt" {
            opt_level = OptLevel::O1;
        } else if arg == "--no-regalloc" {
            enable_reg_alloc = false;
        } else if arg == "--no-mem2reg" {
            enable_mem2reg = false;
        } else if arg == "--no-gvn" {
            enable_gvn = false;
        } else if arg == "--no-inline" {
            enable_inline = false;
        } else if arg == "--no-ipa-constprop" {
            enable_ipa_constprop = false;
        } else if arg == "--no-licm" {
            enable_licm = false;
        } else if arg == "--no-indvar" {
            enable_indvar = false;
        } else if arg == "--no-strength-reduction" {
            enable_strength_reduction = false;
        } else if arg == "--no-sroa" {
            enable_sroa = false;
        } else if arg == "--no-loop-peel" || arg == "--no-peel" {
            enable_loop_peel = false;
        } else if arg == "--no-loop-unroll" || arg == "--no-unroll" {
            enable_loop_unroll = false;
        } else if arg == "--no-instruction-schedule" || arg == "--no-sched" {
            enable_instruction_schedule = false;
        } else if arg == "--no-global-dce" {
            enable_global_dce = false;
        } else if arg == "--no-tail-calls" || arg == "--no-tco" {
            enable_tail_calls = false;
        } else if arg == "--no-block-layout" || arg == "--no-layout" {
            enable_block_layout = false;
        } else if arg == "--no-code-align" || arg == "--no-align" {
            enable_code_align = false;
        } else if let Some(p) = arg.strip_prefix("--std-path=") {
            std_path = Some(PathBuf::from(p));
        } else if arg == "--std-path" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --std-path requires an argument");
                process::exit(2);
            }
            std_path = Some(PathBuf::from(&args[i]));
        } else if let Some(p) = arg.strip_prefix("--runtime-path=") {
            runtime_path = Some(PathBuf::from(p));
        } else if arg == "--runtime-path" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --runtime-path requires an argument");
                process::exit(2);
            }
            runtime_path = Some(PathBuf::from(&args[i]));
        } else if arg.starts_with('-') {
            eprintln!("Error: Unknown flag '{}' for 'vanta run'", arg);
            process::exit(2);
        } else if input_file.is_none() {
            input_file = Some(PathBuf::from(arg));
        } else {
            program_args.push(arg.clone());
        }
        i += 1;
    }

    let input_path = match input_file {
        Some(p) => p,
        None => {
            eprintln!("Error: No input file specified for 'vanta run'");
            process::exit(2);
        }
    };

    let temp_dir = std::env::temp_dir().join(format!("vanta_run_{}", process::id()));
    let _ = fs::create_dir_all(&temp_dir);
    let temp_exe = temp_dir.join("vanta_bin");

    let options = CompilerOptions {
        input_file: input_path,
        output_file: temp_exe.clone(),
        emit_target: EmitTarget::Exe,
        opt_level,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
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
        enable_instruction_schedule,
        enable_global_dce,
        enable_tail_calls,
        enable_block_layout,
        enable_code_align,
    };

    let mut session = CompilerSession::new();
    if let Err(_e) = session.compile(&options) {
        eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
        let _ = fs::remove_dir_all(&temp_dir);
        process::exit(1);
    }

    let mut child = Command::new(&temp_exe);
    child.args(&program_args);
    let status = child.status();

    let _ = fs::remove_dir_all(&temp_dir);

    match status {
        Ok(s) => {
            let code = s.code().unwrap_or(1);
            process::exit(code);
        }
        Err(e) => {
            eprintln!("Error executing binary: {}", e);
            process::exit(1);
        }
    }
}

fn handle_compile(args: &[String]) {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        process::exit(0);
    }

    if args.is_empty() {
        eprintln!("Error: 'vanta compile' requires a source file");
        process::exit(2);
    }

    let mut input_file: Option<PathBuf> = None;
    let mut output_file: Option<PathBuf> = None;
    let mut std_path: Option<PathBuf> = None;
    let mut runtime_path: Option<PathBuf> = None;
    let mut opt_level = OptLevel::O1;
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
    let mut enable_instruction_schedule = true;
    let mut enable_global_dce = true;
    let mut enable_tail_calls = true;
    let mut enable_block_layout = true;
    let mut enable_code_align = true;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-O0" || arg == "--no-opt" {
            opt_level = OptLevel::None;
        } else if arg == "-O1" || arg == "-O" || arg == "--opt" {
            opt_level = OptLevel::O1;
        } else if arg == "--no-regalloc" {
            enable_reg_alloc = false;
        } else if arg == "--no-mem2reg" {
            enable_mem2reg = false;
        } else if arg == "--no-gvn" {
            enable_gvn = false;
        } else if arg == "--no-inline" {
            enable_inline = false;
        } else if arg == "--no-ipa-constprop" {
            enable_ipa_constprop = false;
        } else if arg == "--no-licm" {
            enable_licm = false;
        } else if arg == "--no-indvar" {
            enable_indvar = false;
        } else if arg == "--no-strength-reduction" {
            enable_strength_reduction = false;
        } else if arg == "--no-sroa" {
            enable_sroa = false;
        } else if arg == "--no-loop-peel" || arg == "--no-peel" {
            enable_loop_peel = false;
        } else if arg == "--no-loop-unroll" || arg == "--no-unroll" {
            enable_loop_unroll = false;
        } else if arg == "--no-instruction-schedule" || arg == "--no-sched" {
            enable_instruction_schedule = false;
        } else if arg == "--no-global-dce" {
            enable_global_dce = false;
        } else if arg == "--no-tail-calls" || arg == "--no-tco" {
            enable_tail_calls = false;
        } else if arg == "--no-block-layout" || arg == "--no-layout" {
            enable_block_layout = false;
        } else if arg == "--no-code-align" || arg == "--no-align" {
            enable_code_align = false;
        } else if arg == "-o" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: -o requires an argument");
                process::exit(2);
            }
            output_file = Some(PathBuf::from(&args[i]));
        } else if let Some(p) = arg.strip_prefix("--std-path=") {
            std_path = Some(PathBuf::from(p));
        } else if arg == "--std-path" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --std-path requires an argument");
                process::exit(2);
            }
            std_path = Some(PathBuf::from(&args[i]));
        } else if let Some(p) = arg.strip_prefix("--runtime-path=") {
            runtime_path = Some(PathBuf::from(p));
        } else if arg == "--runtime-path" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --runtime-path requires an argument");
                process::exit(2);
            }
            runtime_path = Some(PathBuf::from(&args[i]));
        } else if arg.starts_with('-') {
            eprintln!("Error: Unknown option '{}' for 'vanta compile'", arg);
            process::exit(2);
        } else if input_file.is_none() {
            input_file = Some(PathBuf::from(arg));
        } else {
            eprintln!("Error: Multiple input files not supported");
            process::exit(2);
        }
        i += 1;
    }

    let input_path = match input_file {
        Some(p) => p,
        None => {
            eprintln!("Error: No input file specified for 'vanta compile'");
            process::exit(2);
        }
    };

    let out_path = output_file.unwrap_or_else(|| {
        let stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("a");
        PathBuf::from(stem)
    });

    let options = CompilerOptions {
        input_file: input_path,
        output_file: out_path,
        emit_target: EmitTarget::Exe,
        opt_level,
        verbose: false,
        diagnostic_format: DiagnosticFormat::Human,
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
        enable_instruction_schedule,
        enable_global_dce,
        enable_tail_calls,
        enable_block_layout,
        enable_code_align,
    };

    let mut session = CompilerSession::new();
    if let Err(_e) = session.compile(&options) {
        eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
        process::exit(1);
    }
}

fn handle_check(args: &[String]) {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        process::exit(0);
    }

    if args.is_empty() {
        eprintln!("Error: 'vanta check' requires a source file");
        process::exit(2);
    }

    let mut input_file: Option<PathBuf> = None;
    let mut std_path: Option<PathBuf> = None;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if let Some(p) = arg.strip_prefix("--std-path=") {
            std_path = Some(PathBuf::from(p));
        } else if arg == "--std-path" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --std-path requires an argument");
                process::exit(2);
            }
            std_path = Some(PathBuf::from(&args[i]));
        } else if arg.starts_with('-') {
            eprintln!("Error: Unknown option '{}' for 'vanta check'", arg);
            process::exit(2);
        } else if input_file.is_none() {
            input_file = Some(PathBuf::from(arg));
        } else {
            eprintln!("Error: Multiple input files not supported");
            process::exit(2);
        }
        i += 1;
    }

    let input_path = match input_file {
        Some(p) => p,
        None => {
            eprintln!("Error: No input file specified for 'vanta check'");
            process::exit(2);
        }
    };

    let options = CompilerOptions {
        input_file: input_path,
        output_file: PathBuf::from("dummy.out"),
        emit_target: EmitTarget::Exe,
        opt_level: OptLevel::None,
        verbose,
        diagnostic_format: DiagnosticFormat::Human,
        std_path,
        runtime_path: None,
        enable_reg_alloc: false,
        enable_mem2reg: false,
        enable_gvn: false,
        enable_inline: false,
        enable_ipa_constprop: false,
        enable_licm: false,
        enable_indvar: false,
        enable_strength_reduction: false,
        enable_sroa: false,
        enable_loop_peel: false,
        enable_loop_unroll: false,
        enable_instruction_schedule: false,
        enable_global_dce: false,
        enable_tail_calls: false,
        enable_block_layout: false,
        enable_code_align: false,
    };

    let mut session = CompilerSession::new();
    match session.check(&options) {
        Ok(()) => {
            if verbose {
                println!("Check successful: 0 errors");
            }
            process::exit(0);
        }
        Err(_) => {
            eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
            process::exit(1);
        }
    }
}

fn handle_fmt(args: &[String]) {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        process::exit(0);
    }

    let mut files = Vec::new();
    let mut check = false;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--check" {
            check = true;
        } else if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg.starts_with('-') {
            eprintln!("Error: Unknown option '{}' for 'vanta fmt'", arg);
            process::exit(2);
        } else {
            files.push(PathBuf::from(arg));
        }
        i += 1;
    }

    if files.is_empty() {
        eprintln!("Error: No files specified for 'vanta fmt'");
        process::exit(2);
    }

    let fmt_opts = FmtOptions {
        files,
        check,
        stdout: false,
        verbose,
        diagnostic_format: DiagnosticFormat::Human,
    };

    let (result, out_msg, err_msg) = run_fmt(&fmt_opts);
    if !out_msg.is_empty() {
        print!("{}", out_msg);
    }
    if !err_msg.is_empty() {
        eprint!("{}", err_msg);
    }

    let code = match result {
        FmtResult::Success => 0,
        FmtResult::NeedsFormatting => {
            if check {
                1
            } else {
                0
            }
        }
        FmtResult::SyntaxError => 1,
        FmtResult::IoError => 2,
    };
    process::exit(code);
}
