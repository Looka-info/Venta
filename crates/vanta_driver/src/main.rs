//! The Vanta Compiler CLI (`vantac`).

use std::env;
use std::path::PathBuf;
use std::process;
use vanta_driver::{CompilerOptions, CompilerSession, DiagnosticFormat, EmitTarget, OptLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandMode {
    Compile,
    Check,
}

fn print_usage() {
    eprintln!(
        r#"Vanta Compiler (vantac) v0.1.0

USAGE:
    vantac [COMMAND] [OPTIONS] <input.va>

COMMANDS:
    compile               Compile the input file (default)
    check                 Quickly verify syntax, types, borrows, and MIR without linking

OPTIONS:
    -o <file>             Write output to <file> (default: a.out)
    -O0, --no-opt         Disable all optimizations
    -O1, -O, --opt        Enable standard optimizations (default)
    --emit=<target>       Emit intermediate target:
                            tokens   - Token stream
                            ast      - Abstract Syntax Tree
                            mir      - Vanta Mid-Level IR
                            obj      - Relocatable ELF64 object file (.o)
                            exe      - Linked native executable (default)
    --diagnostics=<fmt>   Diagnostics format: human (default) or json
    --format=<fmt>        Alias for --diagnostics
    -v, --verbose         Enable verbose diagnostic output
    -h, --help            Print help information
"#
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(2);
    }

    let mut mode = CommandMode::Compile;
    let mut input_file: Option<PathBuf> = None;
    let mut output_file: Option<PathBuf> = None;
    let mut emit_target = EmitTarget::Exe;
    let mut opt_level = OptLevel::O1;
    let mut verbose = false;
    let mut diagnostic_format = DiagnosticFormat::Human;

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
            _ => {}
        }
    }

    while idx < args.len() {
        let arg = &args[idx];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            "-O0" | "--no-opt" => {
                opt_level = OptLevel::None;
            }
            "-O1" | "-O" | "--opt" => {
                opt_level = OptLevel::O1;
            }
            "-o" => {
                idx += 1;
                if idx >= args.len() {
                    eprintln!("Error: -o requires an argument");
                    process::exit(2);
                }
                output_file = Some(PathBuf::from(&args[idx]));
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
            s if s.starts_with('-') => {
                eprintln!("Error: Unknown option '{}'", s);
                process::exit(2);
            }
            _ => {
                if input_file.is_some() {
                    eprintln!("Error: Multiple input files not supported");
                    process::exit(2);
                }
                input_file = Some(PathBuf::from(arg));
            }
        }
        idx += 1;
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
    };

    let mut session = CompilerSession::new();

    match mode {
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
