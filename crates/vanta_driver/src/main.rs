//! The Vanta Compiler CLI (`vantac`).

use std::env;
use std::path::PathBuf;
use std::process;
use vanta_driver::{CompilerOptions, CompilerSession, EmitTarget, OptLevel};

fn print_usage() {
    eprintln!(
        r#"Vanta Compiler (vantac) v0.1.0

USAGE:
    vantac [OPTIONS] <input.va>

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
    -v, --verbose         Enable verbose diagnostic output
    -h, --help            Print help information
"#
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let mut input_file: Option<PathBuf> = None;
    let mut output_file: Option<PathBuf> = None;
    let mut emit_target = EmitTarget::Exe;
    let mut opt_level = OptLevel::O1;
    let mut verbose = false;

    let mut idx = 1;
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
                    process::exit(1);
                }
                output_file = Some(PathBuf::from(&args[idx]));
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
                        process::exit(1);
                    }
                };
            }
            s if s.starts_with('-') => {
                eprintln!("Error: Unknown option '{}'", s);
                process::exit(1);
            }
            _ => {
                if input_file.is_some() {
                    eprintln!("Error: Multiple input files not supported");
                    process::exit(1);
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
            process::exit(1);
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
    };

    let mut session = CompilerSession::new();
    match session.compile(&options) {
        Ok(Some(bytes)) => {
            if matches!(
                emit_target,
                EmitTarget::Tokens | EmitTarget::Ast | EmitTarget::Mir
            ) {
                println!("{}", String::from_utf8_lossy(&bytes));
            }
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("vantac error: {}", err);
            process::exit(1);
        }
    }
}
