// ============================================================================
// ⚡ VentaNext Framework — CLI Route Generator & Template Engine (`ventanext-gen`)
// Scans app/ directory, validates routes, generates dispatchers, and manages
// the official VentaNext Template Gallery and project scaffolding.
// ============================================================================

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use ventanext_codegen::{
    create_project_from_template, export_static_site, find_templates_root, generate_manifest_json,
    generate_venta_code, scan_app_directory, write_generated_routes, RegistryCatalog,
    TemplateManifest,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_usage() {
    eprintln!(
        r#"VentaNext Route Generator, Template Engine & Scaffolder v{version}
USAGE:
    ventanext-gen <COMMAND> [OPTIONS]
    ventanext-gen <APP_DIR> [OPTIONS]

COMMANDS:
    dev [APP_DIR] [OPTIONS]             Start development server with live preview & hot reload
    templates [OPTIONS]                 List all templates in the official registry
    new <TEMPLATE_ID> [NAME] [OPTIONS]  Scaffold a new complete project from a template
    verify-template <TEMPLATE_DIR>      Verify manifest and cryptographic checksums

OPTIONS (for route generation on <APP_DIR>):
    -o <FILE>                Output path for generated pure Venta code (default: stdout)
    --manifest-json <FILE>   Output path for JSON manifest
    --export <OUT_DIR>       Prerender and export static site (SSG) to target directory
    -h, --help               Print help information

OPTIONS (for templates and new):
    --registry <DIR>         Path to templates directory (default: ./templates)
    --dest <DIR>             Destination directory for new project (default: ./<NAME>)
    --category <CAT>         Filter templates by category
    --search <QUERY>         Search templates by keyword
"#,
        version = VERSION
    );
}

fn handle_templates(args: &[String]) {
    let mut custom_reg: Option<PathBuf> = None;
    let mut filter_cat: Option<String> = None;
    let mut search_query: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        if args[i] == "--registry" && i + 1 < args.len() {
            custom_reg = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if args[i] == "--category" && i + 1 < args.len() {
            filter_cat = Some(args[i + 1].clone());
            i += 2;
        } else if args[i] == "--search" && i + 1 < args.len() {
            search_query = Some(args[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }

    let root = find_templates_root(custom_reg.as_deref());
    let reg_file = root.join("registry.json");
    if !reg_file.exists() {
        eprintln!(
            "[ventanext ERROR] Template registry not found at {:?}. Use --registry <DIR>.",
            reg_file
        );
        process::exit(1);
    }

    match RegistryCatalog::load_from_file(&reg_file) {
        Ok(cat) => {
            let list = if let Some(ref q) = search_query {
                cat.search(q)
            } else if let Some(ref c) = filter_cat {
                cat.filter_by_category(c)
            } else {
                cat.templates.iter().collect()
            };

            println!(
                "================================================================================"
            );
            println!(
                "   ⚡ VentaNext Official Template Gallery (Total: {} templates)",
                list.len()
            );
            println!(
                "================================================================================"
            );
            for t in list {
                println!("• ID:           {}", t.id);
                println!("  Name:         {}", t.name);
                println!("  Category:     {}", t.category);
                println!(
                    "  Version:      v{} ({})",
                    t.version, t.framework_compatibility
                );
                println!("  Description:  {}", t.description);
                println!(
                    "  Pages:        {} routes | Files: {} verified files",
                    t.page_count, t.file_count
                );
                println!("  Features:     {}", t.features.join(", "));
                println!("--------------------------------------------------------------------------------");
            }
            println!("To scaffold a project, run: ventanext-gen new <TEMPLATE_ID> <PROJECT_NAME>");
        }
        Err(e) => {
            eprintln!("[ventanext ERROR] Failed to load template registry: {}", e);
            process::exit(1);
        }
    }
}

fn handle_new(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: 'ventanext-gen new' requires a template ID.");
        eprintln!("Run 'ventanext-gen templates' to view available templates.");
        process::exit(1);
    }

    let template_id = &args[0];
    let project_name = if args.len() > 1 && !args[1].starts_with('-') {
        &args[1]
    } else {
        template_id
    };

    let mut custom_dest: Option<PathBuf> = None;
    let mut custom_reg: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--dest" && i + 1 < args.len() {
            custom_dest = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if args[i] == "--registry" && i + 1 < args.len() {
            custom_reg = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else {
            i += 1;
        }
    }

    let dest_dir = custom_dest.unwrap_or_else(|| PathBuf::from(project_name));
    let templates_root = find_templates_root(custom_reg.as_deref());
    let cache_dir = PathBuf::from(".ventanext/cache/templates");

    println!(
        "[ventanext] Preparing project '{}' from template '{}'...",
        project_name, template_id
    );
    println!("[ventanext] Destination: {:?}", dest_dir);

    match create_project_from_template(
        template_id,
        &dest_dir,
        project_name,
        &templates_root,
        Some(&cache_dir),
        VERSION,
    ) {
        Ok(rep) => {
            println!(
                "================================================================================"
            );
            println!("   ✨ Project Scaffolding Succeeded!");
            println!(
                "================================================================================"
            );
            println!("• Project Name:     {}", rep.project_name);
            println!("• Template:         {}", rep.template_id);
            println!("• Destination:      {:?}", rep.destination);
            println!(
                "• Files Created:    {} files ({} bytes)",
                rep.files_created, rep.total_bytes
            );
            println!(
                "• Offline Cache:    {}",
                if rep.from_offline_cache {
                    "Active (from cache)"
                } else {
                    "Populated"
                }
            );
            println!("• Entry Point:      {:?}", rep.entry_point);
            println!("\nNext Steps:");
            println!("  1. cd {:?}", dest_dir);
            println!("  2. vanta run main.va --port 3000");
            println!(
                "================================================================================"
            );
        }
        Err(e) => {
            eprintln!("\n[ventanext ERROR] Scaffolding aborted: {}", e);
            process::exit(1);
        }
    }
}

fn handle_verify_template(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: 'ventanext-gen verify-template' requires a template directory.");
        process::exit(1);
    }
    let t_dir = PathBuf::from(&args[0]);
    let manifest_path = t_dir.join("template.json");
    if !manifest_path.exists() {
        eprintln!("Error: template.json not found in {:?}", t_dir);
        process::exit(1);
    }

    match TemplateManifest::load_from_file(&manifest_path) {
        Ok(manifest) => {
            let project_dir = t_dir.join("project");
            println!(
                "[ventanext] Verifying manifest for '{}' (v{})...",
                manifest.id, manifest.version
            );
            for entry in &manifest.file_inventory {
                let file_path = project_dir.join(&entry.path);
                if !file_path.exists() {
                    eprintln!("[FAIL] Missing file: {:?}", file_path);
                    process::exit(1);
                }
                let data = fs::read(&file_path).unwrap();
                let hash = ventanext_codegen::compute_sha256(&data);
                if hash != entry.sha256 {
                    eprintln!("[FAIL] Hash mismatch for {:?}", entry.path);
                    process::exit(1);
                }
            }
            println!(
                "[OK] All {} file(s) cryptographically verified against SHA-256 signatures.",
                manifest.file_inventory.len()
            );
        }
        Err(e) => {
            eprintln!("[ventanext ERROR] Invalid manifest: {}", e);
            process::exit(1);
        }
    }
}

fn handle_dev(args: &[String]) {
    let mut config = ventanext_codegen::DevServerConfig::default();
    let mut project_dir = PathBuf::from(".");
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "-p" || args[i] == "--port") && i + 1 < args.len() {
            if let Ok(p) = args[i + 1].parse::<u16>() {
                config.port = p;
            }
            i += 2;
        } else if args[i] == "--host" && i + 1 < args.len() {
            config.host = args[i + 1].clone();
            i += 2;
        } else if args[i] == "--debounce" && i + 1 < args.len() {
            if let Ok(d) = args[i + 1].parse::<u64>() {
                config.debounce_ms = d;
            }
            i += 2;
        } else if !args[i].starts_with('-') {
            project_dir = PathBuf::from(&args[i]);
            i += 1;
        } else {
            i += 1;
        }
    }
    config.project_dir = project_dir;
    if let Err(e) = ventanext_codegen::run_development_server(config, false) {
        eprintln!("[ventanext ERROR] Dev server error: {}", e);
        process::exit(1);
    }
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

    if args[1] == "-V" || args[1] == "--version" {
        println!("ventanext-gen {}", VERSION);
        process::exit(0);
    }

    match args[1].as_str() {
        "dev" => {
            handle_dev(&args[2..]);
            return;
        }
        "templates" => {
            handle_templates(&args[2..]);
            return;
        }
        "new" => {
            handle_new(&args[2..]);
            return;
        }
        "verify-template" => {
            handle_verify_template(&args[2..]);
            return;
        }
        _ => {}
    }

    // Default route scanning behavior for backwards compatibility
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

            if let Some(ref path) = out_va_path {
                if let Err(e) = write_generated_routes(&discovered, path) {
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
                let venta_code = generate_venta_code(&discovered);
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
