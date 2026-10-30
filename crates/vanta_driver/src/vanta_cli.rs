// Unified developer CLI for Venta (`venta`).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};
use vanta_driver::{
    run_fmt, CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, FmtOptions, FmtResult,
    LinkerKind, OptLevel, TargetFeatures, TargetTriple,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn get_usage_text() -> String {
    let invoked = env::args().next().unwrap_or_default();
    let file_name = std::path::Path::new(&invoked)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let bin_name = if file_name == "vanta" {
        "vanta"
    } else {
        "venta"
    };
    let lang_name = if file_name == "vanta" {
        "Vanta"
    } else {
        "Venta"
    };
    format!(
        r#"{lang_name} Toolchain Driver ({bin_name}) v{version}

USAGE:
    {bin_name} <COMMAND> [OPTIONS]

COMMANDS:
    run <file.va> [-- <args>...]   Compile and run a {lang_name} program immediately
    compile <file.va> [-o <out>] [--json]   Compile a {lang_name} program to a native binary
    check <file.va> [--json]                Verify syntax, types, borrows, and MIR without compiling
    fmt [files...] [--check]       Format {lang_name} source files canonically

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
            let invoked = env::args().next().unwrap_or_default();
            let file_name = std::path::Path::new(&invoked)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            let bin_name = if file_name == "vanta" {
                "vanta"
            } else {
                "venta"
            };
            println!("{} {}", bin_name, VERSION);
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
    let mut diagnostic_format = DiagnosticFormat::Human;
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
        } else if arg == "-g" || arg == "--debug" {
            debug_info = true;
        } else if arg == "--json" {
            diagnostic_format = DiagnosticFormat::Json;
        } else if let Some(fmt) = arg.strip_prefix("--diagnostics=") {
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
        } else if let Some(fmt) = arg.strip_prefix("--format=") {
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
        } else if arg == "--no-vectorize" || arg == "--no-vec" {
            enable_vectorize = false;
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
        } else if let Some(f) = arg.strip_prefix("--target-feature=") {
            if let Err(e) = target_features.parse_feature_list(f) {
                eprintln!("Error: {}", e);
                process::exit(2);
            }
        } else if arg == "--target-feature" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --target-feature requires an argument");
                process::exit(2);
            }
            if let Err(e) = target_features.parse_feature_list(&args[i]) {
                eprintln!("Error: {}", e);
                process::exit(2);
            }
        } else if let Some(cpu) = arg.strip_prefix("--target-cpu=") {
            match TargetFeatures::from_target_cpu(cpu) {
                Ok(feats) => target_features = feats,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        } else if arg == "--target-cpu" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --target-cpu requires an argument");
                process::exit(2);
            }
            match TargetFeatures::from_target_cpu(&args[i]) {
                Ok(feats) => target_features = feats,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        } else if let Some(t_str) = arg.strip_prefix("--target=") {
            match t_str.parse::<TargetTriple>() {
                Ok(t) => target = t,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        } else if arg == "--target" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --target requires an argument");
                process::exit(2);
            }
            match args[i].parse::<TargetTriple>() {
                Ok(t) => target = t,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        } else if arg.starts_with("-C") {
            let c_arg = if arg == "-C" {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: -C requires an argument");
                    process::exit(2);
                }
                args[i].as_str()
            } else {
                &arg[2..]
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
        target,
        opt_level,
        verbose: false,
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
        linker: LinkerKind::Internal,
        linker_flags: Vec::new(),
        link_archives: Vec::new(),
        additional_inputs: Vec::new(),
        map_file: None,
        lib_search_paths: Vec::new(),
        libs: Vec::new(),
        incremental: false,
        cache_dir: None,
        build_stats: false,
        explain_rebuild: false,
    };

    let mut session = CompilerSession::new();
    if let Err(_e) = session.compile(&options) {
        if diagnostic_format == DiagnosticFormat::Json {
            println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
        } else {
            eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
        }
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
    let mut diagnostic_format = DiagnosticFormat::Human;
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

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-O0" || arg == "--no-opt" {
            opt_level = OptLevel::None;
        } else if arg == "-O1" || arg == "-O" || arg == "--opt" {
            opt_level = OptLevel::O1;
        } else if arg == "-g" || arg == "--debug" {
            debug_info = true;
        } else if arg == "--json" {
            diagnostic_format = DiagnosticFormat::Json;
        } else if let Some(fmt) = arg.strip_prefix("--diagnostics=") {
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
        } else if let Some(fmt) = arg.strip_prefix("--format=") {
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
        } else if arg == "--no-vectorize" || arg == "--no-vec" {
            enable_vectorize = false;
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
        } else if let Some(f) = arg.strip_prefix("--target-feature=") {
            if let Err(e) = target_features.parse_feature_list(f) {
                eprintln!("Error: {}", e);
                process::exit(2);
            }
        } else if arg == "--target-feature" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --target-feature requires an argument");
                process::exit(2);
            }
            if let Err(e) = target_features.parse_feature_list(&args[i]) {
                eprintln!("Error: {}", e);
                process::exit(2);
            }
        } else if let Some(cpu) = arg.strip_prefix("--target-cpu=") {
            match TargetFeatures::from_target_cpu(cpu) {
                Ok(feats) => target_features = feats,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        } else if arg == "--target-cpu" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --target-cpu requires an argument");
                process::exit(2);
            }
            match TargetFeatures::from_target_cpu(&args[i]) {
                Ok(feats) => target_features = feats,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        } else if let Some(t_str) = arg.strip_prefix("--target=") {
            match t_str.parse::<TargetTriple>() {
                Ok(t) => target = t,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        } else if arg == "--target" {
            i += 1;
            if i >= args.len() {
                eprintln!("Error: --target requires an argument");
                process::exit(2);
            }
            match args[i].parse::<TargetTriple>() {
                Ok(t) => target = t,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        } else if arg.starts_with("-C") {
            let c_arg = if arg == "-C" {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: -C requires an argument");
                    process::exit(2);
                }
                args[i].as_str()
            } else {
                &arg[2..]
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
        target,
        opt_level,
        verbose: false,
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
        linker: LinkerKind::Internal,
        linker_flags: Vec::new(),
        link_archives: Vec::new(),
        additional_inputs: Vec::new(),
        map_file: None,
        lib_search_paths: Vec::new(),
        libs: Vec::new(),
        incremental: false,
        cache_dir: None,
        build_stats: false,
        explain_rebuild: false,
    };

    let mut session = CompilerSession::new();
    match session.compile(&options) {
        Ok(_) => {
            if diagnostic_format == DiagnosticFormat::Json {
                println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
            }
        }
        Err(_e) => {
            if diagnostic_format == DiagnosticFormat::Json {
                println!("{}", session.render_diagnostics(DiagnosticFormat::Json));
            } else {
                eprintln!("{}", session.render_diagnostics(DiagnosticFormat::Human));
            }
            process::exit(1);
        }
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
    let mut diagnostic_format = DiagnosticFormat::Human;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg == "--json" {
            diagnostic_format = DiagnosticFormat::Json;
        } else if let Some(fmt) = arg.strip_prefix("--diagnostics=") {
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
        } else if let Some(fmt) = arg.strip_prefix("--format=") {
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
        target: TargetTriple::default(),
        opt_level: OptLevel::None,
        verbose,
        diagnostic_format,
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
        enable_vectorize: false,
        enable_instruction_schedule: false,
        enable_global_dce: false,
        enable_tail_calls: false,
        enable_block_layout: false,
        enable_code_align: false,
        target_features: TargetFeatures::baseline(),
        debug_info: false,
        linker: LinkerKind::Internal,
        linker_flags: Vec::new(),
        link_archives: Vec::new(),
        additional_inputs: Vec::new(),
        map_file: None,
        lib_search_paths: Vec::new(),
        libs: Vec::new(),
        incremental: false,
        cache_dir: None,
        build_stats: false,
        explain_rebuild: false,
    };

    let mut session = CompilerSession::new();
    match session.check(&options) {
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
