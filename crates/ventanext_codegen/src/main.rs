// ============================================================================
// ⚡ VentaNext Framework — CLI Route Generator (`ventanext-gen`)
// Scans app/ directory, validates routes, and generates route manifest and dispatchers
// ============================================================================

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use ventanext_codegen::{
    export_static_site, generate_manifest_json, generate_venta_code, scan_app_directory,
};

fn print_usage() {
    eprintln!(
        r#"VentaNext Route Generator & Static Site Exporter v1.0.0-rc.2
USAGE:
    ventanext-gen <APP_DIR> [OPTIONS]

OPTIONS:
    -o <FILE>                Output path for generated pure Venta code (default: stdout)
    --manifest-json <FILE>   Output path for JSON manifest
    --export <OUT_DIR>       Prerender and export static site (SSG) to target directory
    -h, --help               Print help information
"#
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    if args[1] == "-h" || args[1] == "--help" {
        print_usage();
        process::exit(0);
    }

    let app_dir = PathBuf::from(&args[1]);
    let mut out_va_path: Option<PathBuf> = None;
    let mut out_json_path: Option<PathBuf> = None;
    let mut export_dir: Option<PathBuf> = None;

    let mut i = 2;
    while i < args.len() {
        if args[i] == "-o" && i + 1 < args.len() {
            out_va_path = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if args[i] == "--manifest-json" && i + 1 < args.len() {
            out_json_path = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if args[i] == "--export" && i + 1 < args.len() {
            export_dir = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else {
            eprintln!("Unknown argument: {}", args[i]);
            print_usage();
            process::exit(1);
        }
    }

    println!("[ventanext] Scanning app directory: {:?}", app_dir);
    match scan_app_directory(&app_dir) {
        Ok(discovered) => {
            println!(
                "[ventanext] Discovered {} route(s) and special boundaries.",
                discovered.routes.len()
            );

            let venta_code = generate_venta_code(&discovered);
            if let Some(ref path) = out_va_path {
                if let Err(e) = fs::write(path, &venta_code) {
                    eprintln!(
                        "[ventanext] Failed to write Venta output to {:?}: {}",
                        path, e
                    );
                    process::exit(1);
                }
                println!(
                    "[ventanext] Wrote generated Venta dispatch code to: {:?}",
                    path
                );
            } else {
                println!("{}", venta_code);
            }

            if let Some(ref json_path) = out_json_path {
                let json_data = generate_manifest_json(&discovered, "ventanext-app");
                if let Err(e) = fs::write(json_path, &json_data) {
                    eprintln!(
                        "[ventanext] Failed to write JSON manifest to {:?}: {}",
                        json_path, e
                    );
                    process::exit(1);
                }
                println!("[ventanext] Wrote JSON route manifest to: {:?}", json_path);
            }

            if let Some(ref out_dir) = export_dir {
                println!(
                    "[ventanext] Starting static site export (SSG) to: {:?}",
                    out_dir
                );
                match export_static_site(&app_dir, out_dir, &discovered) {
                    Ok(exp) => {
                        println!(
                            "[ventanext] SSG Export succeeded: {} files generated in {:?}",
                            exp.files.len(),
                            out_dir
                        );
                    }
                    Err(e) => {
                        eprintln!("[ventanext ERROR] Failed during static export: {}", e);
                        process::exit(1);
                    }
                }
            }
        }
        Err(err) => {
            eprintln!("[ventanext-gen ERROR] {}", err);
            process::exit(1);
        }
    }
}
