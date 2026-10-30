// Venta Language Server Protocol (`vanta-lsp`) Daemon.

use lsp_server::{Connection, Message};
use lsp_types::notification::{Exit, Notification as _};
use std::env;
use std::process;
use vanta_lsp::handlers::{handle_notification, handle_request, ServerState};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    eprintln!(
        r#"Venta Language Server Protocol Daemon (vanta-lsp) v{version}

USAGE:
    vanta-lsp [OPTIONS]

OPTIONS:
    -v, --version         Print version information
    -h, --help            Print help information

DESCRIPTION:
    Communicates via JSON-RPC over standard input/output (stdio) with LSP-compatible
    editors (VS Code, Neovim, Zed, Helix, Emacs) to provide live diagnostics,
    hover information, go-to-definition, code completion, document symbols,
    and canonical formatting.
"#,
        version = VERSION
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "-v" | "--version" => {
                println!("vanta-lsp v{}", VERSION);
                process::exit(0);
            }
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            unknown => {
                eprintln!(
                    "Unknown argument '{}'. Run 'vanta-lsp --help' for usage.",
                    unknown
                );
                process::exit(2);
            }
        }
    }

    eprintln!(
        "[vanta-lsp] Starting Venta Language Server v{} on stdio...",
        VERSION
    );

    let (connection, io_threads) = Connection::stdio();
    let mut state = ServerState::new();
    let mut exit_code = 0;

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                handle_request(req, &mut state, &connection);
            }
            Message::Response(_resp) => {
                // Client responses to server-initiated requests
            }
            Message::Notification(notif) => {
                let is_exit = notif.method == Exit::METHOD;
                handle_notification(notif, &mut state, &connection);
                if is_exit {
                    exit_code = if state.is_shutdown { 0 } else { 1 };
                    break;
                }
            }
        }
    }

    drop(connection);
    let _ = io_threads.join();
    process::exit(exit_code);
}
