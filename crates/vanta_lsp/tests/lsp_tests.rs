//! Comprehensive test suite for Venta Language Server Protocol (`vanta-lsp`).

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::{
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Initialized,
        Notification as _, PublishDiagnostics,
    },
    request::{
        Completion, DocumentSymbolRequest, Formatting, GotoDefinition, HoverRequest, Initialize,
        Shutdown,
    },
    ClientCapabilities, CompletionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, DocumentSymbolParams, FormattingOptions,
    GotoDefinitionParams, HoverParams, InitializeParams, InitializeResult, Position,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier,
};
use std::str::FromStr;
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::Duration;
use vanta_lsp::handlers::{handle_notification, handle_request, ServerState};

static REQ_ID: AtomicI32 = AtomicI32::new(1);

fn next_id() -> RequestId {
    RequestId::from(REQ_ID.fetch_add(1, Ordering::SeqCst))
}

struct TestClient {
    client_conn: Connection,
    server_thread: Option<thread::JoinHandle<()>>,
}

impl TestClient {
    fn new() -> Self {
        let (server_conn, client_conn) = Connection::memory();

        let server_thread = thread::spawn(move || {
            let mut state = ServerState::new();
            for msg in &server_conn.receiver {
                match msg {
                    Message::Request(req) => {
                        handle_request(req, &mut state, &server_conn);
                    }
                    Message::Notification(notif) => {
                        let is_exit = notif.method == Exit::METHOD;
                        handle_notification(notif, &mut state, &server_conn);
                        if is_exit {
                            break;
                        }
                    }
                    Message::Response(_) => {}
                }
            }
        });

        Self {
            client_conn,
            server_thread: Some(server_thread),
        }
    }

    fn send_request<R: lsp_types::request::Request>(
        &self,
        params: R::Params,
    ) -> (RequestId, R::Result)
    where
        R::Params: serde::Serialize,
        R::Result: serde::de::DeserializeOwned,
    {
        let id = next_id();
        let req = Request::new(id.clone(), R::METHOD.to_string(), params);
        self.client_conn
            .sender
            .send(Message::Request(req))
            .expect("Send request");

        loop {
            let msg = self
                .client_conn
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("Receive response");
            match msg {
                Message::Response(resp) => {
                    assert_eq!(resp.id, id, "Response ID mismatch");
                    match resp.response_result {
                        Ok(val) => {
                            let res = serde_json::from_value(val).expect("Deserialize result");
                            return (id, res);
                        }
                        Err(err) => panic!("Request failed: {:?}", err),
                    }
                }
                Message::Notification(_) => {
                    // Ignore background notifications (e.g. diagnostics)
                }
                _ => {}
            }
        }
    }

    fn send_notification<N: lsp_types::notification::Notification>(&self, params: N::Params)
    where
        N::Params: serde::Serialize,
    {
        let notif = Notification::new(N::METHOD.to_string(), params);
        self.client_conn
            .sender
            .send(Message::Notification(notif))
            .expect("Send notification");
    }

    fn recv_notification<N: lsp_types::notification::Notification>(&self) -> N::Params
    where
        N::Params: serde::de::DeserializeOwned,
    {
        loop {
            let msg = self
                .client_conn
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("Receive notification");
            match msg {
                Message::Notification(notif) => {
                    if notif.method == N::METHOD {
                        return serde_json::from_value(notif.params).expect("Deserialize params");
                    }
                }
                _ => {}
            }
        }
    }

    fn initialize(&self) -> InitializeResult {
        #[allow(deprecated)]
        let params = InitializeParams {
            process_id: None,
            root_path: None,
            root_uri: None,
            initialization_options: None,
            capabilities: ClientCapabilities::default(),
            trace: None,
            workspace_folders: None,
            client_info: None,
            locale: None,
            work_done_progress_params: Default::default(),
        };

        let (_, res) = self.send_request::<Initialize>(params);
        self.send_notification::<Initialized>(lsp_types::InitializedParams {});
        res
    }

    fn shutdown_and_exit(mut self) {
        let (_, _) = self.send_request::<Shutdown>(());
        self.send_notification::<Exit>(());
        if let Some(handle) = self.server_thread.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn test_lsp_lifecycle_and_capabilities() {
    let client = TestClient::new();
    let init_result = client.initialize();

    // Verify advertised server info
    let server_info = init_result.server_info.expect("Server info present");
    assert_eq!(server_info.name, "vanta-lsp");

    // Verify genuinely implemented capabilities
    let caps = init_result.capabilities;
    assert!(caps.hover_provider.is_some());
    assert!(caps.definition_provider.is_some());
    assert!(caps.completion_provider.is_some());
    assert!(caps.document_symbol_provider.is_some());
    assert!(caps.document_formatting_provider.is_some());

    client.shutdown_and_exit();
}

#[test]
fn test_lsp_diagnostics_syntax_and_semantic_errors() {
    let client = TestClient::new();
    client.initialize();

    let uri = Uri::from_str("file:///diagnostics_test.va").unwrap();

    // 1. Open file with a syntax error (missing semicolon and broken expr)
    let bad_source = r#"
pub fn add(a: i64, b: i64) -> i64 {
    let x = a + ;
    return x;
}
"#;
    client.send_notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "venta".into(),
            version: 1,
            text: bad_source.into(),
        },
    });

    let notif = client.recv_notification::<PublishDiagnostics>();
    assert_eq!(notif.uri, uri);
    assert!(
        !notif.diagnostics.is_empty(),
        "Expected syntax error diagnostics"
    );
    assert_eq!(
        notif.diagnostics[0].severity,
        Some(lsp_types::DiagnosticSeverity::ERROR)
    );

    // 2. Edit file to fix syntax error, but introduce a type mismatch (semantic error)
    let type_error_source = r#"
pub fn add(a: i64, b: i64) -> i64 {
    let x: bool = a + b;
    return 0;
}
"#;
    client.send_notification::<DidChangeTextDocument>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: type_error_source.into(),
        }],
    });

    let notif = client.recv_notification::<PublishDiagnostics>();
    assert_eq!(notif.version, Some(2));
    assert!(
        !notif.diagnostics.is_empty(),
        "Expected type mismatch error diagnostic"
    );
    let has_type_mismatch = notif.diagnostics.iter().any(|d| {
        d.message.contains("Type mismatch")
            || d.code == Some(lsp_types::NumberOrString::String("E0201".into()))
    });
    assert!(
        has_type_mismatch,
        "Expected E0201 / type mismatch error: {:?}",
        notif.diagnostics
    );

    // 3. Edit file to valid source -> diagnostics list must be empty
    let valid_source = r#"
pub fn add(a: i64, b: i64) -> i64 {
    let x: i64 = a + b;
    return x;
}
"#;
    client.send_notification::<DidChangeTextDocument>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: 3,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: valid_source.into(),
        }],
    });

    let notif = client.recv_notification::<PublishDiagnostics>();
    assert_eq!(notif.version, Some(3));
    assert!(
        notif.diagnostics.is_empty(),
        "Expected zero diagnostics for valid code: {:?}",
        notif.diagnostics
    );

    // 4. Close document -> published diagnostics must be cleared
    client.send_notification::<DidCloseTextDocument>(DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    });

    let notif = client.recv_notification::<PublishDiagnostics>();
    assert_eq!(notif.uri, uri);
    assert!(
        notif.diagnostics.is_empty(),
        "Expected cleared diagnostics on close"
    );

    client.shutdown_and_exit();
}

#[test]
fn test_lsp_hover_and_definition() {
    let client = TestClient::new();
    client.initialize();

    let uri = Uri::from_str("file:///symbols_test.va").unwrap();
    let source = r#"
pub fn calculate(val: i64) -> i64 {
    let doubled: i64 = val * 2;
    return doubled;
}

pub fn main() -> i64 {
    let result: i64 = calculate(21);
    return result;
}
"#;
    client.send_notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "venta".into(),
            version: 1,
            text: source.into(),
        },
    });

    // Hover over `calculate` in `let result: i64 = calculate(21);` (line 7, character 24)
    let (_, hover_res) = client.send_request::<HoverRequest>(HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(7, 24),
        },
        work_done_progress_params: Default::default(),
    });

    let hover = hover_res.expect("Expected hover result for calculate");
    let hover_text = match hover.contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markdown markup content"),
    };
    assert!(
        hover_text.contains("fn calculate(val: i64) -> i64"),
        "Hover content mismatch: {}",
        hover_text
    );

    // Hover over local variable `doubled` (line 2, character 10)
    let (_, var_hover) = client.send_request::<HoverRequest>(HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(2, 10),
        },
        work_done_progress_params: Default::default(),
    });
    let v_hover = var_hover.expect("Expected hover for local variable doubled");
    let v_text = match v_hover.contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markdown markup content"),
    };
    assert!(
        v_text.contains("let doubled: i64"),
        "Hover variable mismatch: {}",
        v_text
    );

    // Go to definition of `calculate` from callsite at line 7, char 24
    let (_, def_res) = client.send_request::<GotoDefinition>(GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(7, 24),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });

    let def_loc = match def_res.expect("Expected definition response") {
        lsp_types::GotoDefinitionResponse::Scalar(loc) => loc,
        _ => panic!("Expected scalar location"),
    };
    assert_eq!(def_loc.uri, uri);
    // Line 1 is `pub fn calculate(val: i64) -> i64 {`
    assert_eq!(def_loc.range.start.line, 1);

    client.shutdown_and_exit();
}

#[test]
fn test_lsp_completion() {
    let client = TestClient::new();
    client.initialize();

    let uri = Uri::from_str("file:///completion_test.va").unwrap();
    let source = r#"
pub struct User {
    id: i64,
}

pub fn greet() {
    let my_counter: i64 = 5;
    
}
"#;
    client.send_notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "venta".into(),
            version: 1,
            text: source.into(),
        },
    });

    // Request completion at empty line inside `greet` (line 7, character 4)
    let (_, comp_res) = client.send_request::<Completion>(CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position::new(7, 4),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    });

    let list = match comp_res.expect("Expected completion list") {
        lsp_types::CompletionResponse::List(l) => l,
        lsp_types::CompletionResponse::Array(items) => lsp_types::CompletionList {
            is_incomplete: false,
            items,
        },
    };

    let labels: Vec<String> = list.items.into_iter().map(|it| it.label).collect();

    // Verify keywords exist
    assert!(labels.contains(&"let".to_string()));
    assert!(labels.contains(&"return".to_string()));
    assert!(labels.contains(&"struct".to_string()));

    // Verify visible symbols
    assert!(labels.contains(&"User".to_string()));
    assert!(labels.contains(&"greet".to_string()));
    assert!(labels.contains(&"my_counter".to_string()));

    client.shutdown_and_exit();
}

#[test]
fn test_lsp_document_symbols() {
    let client = TestClient::new();
    client.initialize();

    let uri = Uri::from_str("file:///doc_symbols_test.va").unwrap();
    let source = r#"
pub struct Point {
    x: i64,
    y: i64,
}

pub enum Status {
    Active,
    Inactive,
}

pub fn run_app() {
}
"#;
    client.send_notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "venta".into(),
            version: 1,
            text: source.into(),
        },
    });

    let (_, sym_res) = client.send_request::<DocumentSymbolRequest>(DocumentSymbolParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    });

    let symbols = match sym_res.expect("Expected document symbols") {
        lsp_types::DocumentSymbolResponse::Nested(s) => s,
        _ => panic!("Expected nested document symbols"),
    };

    let names: Vec<String> = symbols.iter().map(|s| s.name.clone()).collect();
    assert!(names.contains(&"Point".to_string()));
    assert!(names.contains(&"Status".to_string()));
    assert!(names.contains(&"run_app".to_string()));

    let point_sym = symbols.iter().find(|s| s.name == "Point").unwrap();
    assert_eq!(point_sym.kind, lsp_types::SymbolKind::STRUCT);
    let point_children = point_sym.children.as_ref().expect("Point fields");
    let field_names: Vec<String> = point_children.iter().map(|c| c.name.clone()).collect();
    assert!(field_names.contains(&"x".to_string()));
    assert!(field_names.contains(&"y".to_string()));

    client.shutdown_and_exit();
}

#[test]
fn test_lsp_document_formatting() {
    let client = TestClient::new();
    client.initialize();

    let uri = Uri::from_str("file:///formatting_test.va").unwrap();
    // Unformatted source with irregular whitespace
    let unformatted = "pub fn add( a : i64 , b : i64 ) -> i64 {\nreturn a+b;\n}\n";

    client.send_notification::<DidOpenTextDocument>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "venta".into(),
            version: 1,
            text: unformatted.into(),
        },
    });

    let (_, format_res) = client.send_request::<Formatting>(DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri },
        options: FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..Default::default()
        },
        work_done_progress_params: Default::default(),
    });

    let edits = format_res.expect("Expected formatting edits");
    assert!(!edits.is_empty(), "Expected at least one formatting edit");
    let formatted_text = &edits[0].new_text;
    assert_eq!(
        formatted_text,
        "pub fn add(a: i64, b: i64) -> i64 {\n    return a + b;\n}\n"
    );

    client.shutdown_and_exit();
}

#[test]
fn test_lsp_end_to_end_stdio_subprocess() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let bin_path = env!("CARGO_BIN_EXE_vanta-lsp");
    let mut child = Command::new(bin_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn vanta-lsp binary");

    let mut stdin = child.stdin.take().expect("Child stdin");
    let mut stdout_reader = BufReader::new(child.stdout.take().expect("Child stdout"));

    fn send_rpc(stdin: &mut impl Write, json_msg: &str) {
        let payload = format!("Content-Length: {}\r\n\r\n{}", json_msg.len(), json_msg);
        stdin
            .write_all(payload.as_bytes())
            .expect("Write RPC message");
        stdin.flush().expect("Flush RPC message");
    }

    fn read_rpc(reader: &mut impl BufRead) -> serde_json::Value {
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            let bytes_read = reader
                .read_line(&mut line)
                .expect("Read line from LSP stdout");
            if bytes_read == 0 {
                panic!("EOF reached while reading LSP headers!");
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some(rest) = line.strip_prefix("Content-Length:") {
                length = rest.trim().parse().expect("Parse Content-Length");
            }
        }
        assert!(length > 0, "Expected non-zero Content-Length");
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).expect("Read RPC payload");
        serde_json::from_slice(&body).expect("Parse JSON-RPC payload")
    }

    // 1. Initialize
    send_rpc(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#,
    );
    let init_resp = read_rpc(&mut stdout_reader);
    assert_eq!(init_resp["id"], 1);
    assert_eq!(init_resp["result"]["serverInfo"]["name"], "vanta-lsp");

    // 2. Initialized notification
    send_rpc(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
    );

    // 3. Open document with an error
    let open_json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///stdio_test.va",
                "languageId": "venta",
                "version": 1,
                "text": "pub fn test() {\n    let a: i64 = \"not an int\";\n}\n"
            }
        }
    });
    send_rpc(&mut stdin, &open_json.to_string());

    // Expect publishDiagnostics with type error
    let diag_notif = read_rpc(&mut stdout_reader);
    assert_eq!(diag_notif["method"], "textDocument/publishDiagnostics");
    let diags = diag_notif["params"]["diagnostics"]
        .as_array()
        .expect("Diagnostics array");
    assert!(!diags.is_empty(), "Expected type mismatch diagnostic");

    // 4. Update document to valid code
    let change_json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": "file:///stdio_test.va",
                "version": 2
            },
            "contentChanges": [
                {
                    "text": "pub fn test() -> i64 {\n    let a: i64 = 42;\n    return a;\n}\n"
                }
            ]
        }
    });
    send_rpc(&mut stdin, &change_json.to_string());

    // Expect publishDiagnostics with 0 errors
    let diag_fixed = read_rpc(&mut stdout_reader);
    assert_eq!(diag_fixed["method"], "textDocument/publishDiagnostics");
    let diags_empty = diag_fixed["params"]["diagnostics"]
        .as_array()
        .expect("Diagnostics array");
    assert!(
        diags_empty.is_empty(),
        "Expected 0 diagnostics after fix: {:?}",
        diags_empty
    );

    // 5. Hover request on `a` (line 1, character 8)
    let hover_json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": "file:///stdio_test.va" },
            "position": { "line": 1, "character": 8 }
        }
    });
    send_rpc(&mut stdin, &hover_json.to_string());
    let hover_resp = read_rpc(&mut stdout_reader);
    assert_eq!(hover_resp["id"], 2);
    let hover_val = hover_resp["result"]["contents"]["value"]
        .as_str()
        .expect("Hover markdown value");
    assert!(
        hover_val.contains("let a: i64"),
        "Expected hover to contain 'let a: i64': {}",
        hover_val
    );

    // 6. Formatting request
    let fmt_json = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/formatting",
        "params": {
            "textDocument": { "uri": "file:///stdio_test.va" },
            "options": { "tabSize": 4, "insertSpaces": true }
        }
    });
    send_rpc(&mut stdin, &fmt_json.to_string());
    let fmt_resp = read_rpc(&mut stdout_reader);
    assert_eq!(fmt_resp["id"], 3);
    assert!(fmt_resp["result"].is_array());

    // 7. Close document
    let close_json = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": {
            "textDocument": { "uri": "file:///stdio_test.va" }
        }
    });
    send_rpc(&mut stdin, &close_json.to_string());
    let close_diag = read_rpc(&mut stdout_reader);
    assert_eq!(close_diag["method"], "textDocument/publishDiagnostics");
    assert!(close_diag["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());

    // 8. Shutdown
    send_rpc(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":4,"method":"shutdown","params":null}"#,
    );
    let shut_resp = read_rpc(&mut stdout_reader);
    assert_eq!(shut_resp["id"], 4);

    // 9. Exit
    send_rpc(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"exit","params":null}"#,
    );

    // Drop stdin to close pipe
    drop(stdin);

    let status = child.wait().expect("Child process wait");
    assert_eq!(status.code(), Some(0), "Expected clean exit code 0");
}
