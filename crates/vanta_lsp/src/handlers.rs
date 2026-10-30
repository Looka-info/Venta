//! LSP request and notification dispatch.

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
        Initialized, Notification as _, PublishDiagnostics,
    },
    request::{
        Completion, DocumentSymbolRequest, Formatting, GotoDefinition, HoverRequest, Initialize,
        Request as _, Shutdown,
    },
    ClientCapabilities, CompletionOptions, CompletionParams, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentFormattingParams, DocumentSymbolParams, GotoDefinitionParams, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, OneOf, PublishDiagnosticsParams,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};
use serde_json::Value;
use std::sync::Arc;

use crate::analysis::AnalysisSnapshot;
use crate::document::DocumentManager;

/// Persistent state for the language server session.
pub struct ServerState {
    pub document_manager: DocumentManager,
    pub client_capabilities: Option<ClientCapabilities>,
    pub is_shutdown: bool,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            document_manager: DocumentManager::new(),
            client_capabilities: None,
            is_shutdown: false,
        }
    }

    /// Analyzes the document if not already cached, and caches the snapshot.
    pub fn get_or_analyze(&mut self, uri: &Uri) -> Option<Arc<AnalysisSnapshot>> {
        let doc = self.document_manager.get_mut(uri)?;
        if let Some(ref snapshot) = doc.last_analysis {
            return Some(snapshot.clone());
        }

        let snapshot = Arc::new(AnalysisSnapshot::new(doc));
        doc.last_analysis = Some(snapshot.clone());
        Some(snapshot)
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Dispatches an incoming LSP request and sends the response.
pub fn handle_request(req: Request, state: &mut ServerState, connection: &Connection) {
    if state.is_shutdown && req.method != Shutdown::METHOD {
        let resp = Response::new_err(
            req.id,
            lsp_server::ErrorCode::InvalidRequest as i32,
            "Server is shut down".to_string(),
        );
        let _ = connection.sender.send(Message::Response(resp));
        return;
    }

    match req.method.as_str() {
        Initialize::METHOD => {
            let res = serde_json::from_value::<InitializeParams>(req.params)
                .map_err(|e| e.to_string())
                .and_then(|params| handle_initialize(params, state));
            send_response(req.id, res, connection);
        }
        Shutdown::METHOD => {
            state.is_shutdown = true;
            let resp = Response::new_ok(req.id, Value::Null);
            let _ = connection.sender.send(Message::Response(resp));
        }
        HoverRequest::METHOD => {
            let res = serde_json::from_value::<HoverParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|params| handle_hover(params, state));
            send_response(req.id, res, connection);
        }
        GotoDefinition::METHOD => {
            let res = serde_json::from_value::<GotoDefinitionParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|params| handle_definition(params, state));
            send_response(req.id, res, connection);
        }
        Completion::METHOD => {
            let res = serde_json::from_value::<CompletionParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|params| handle_completion(params, state));
            send_response(req.id, res, connection);
        }
        DocumentSymbolRequest::METHOD => {
            let res = serde_json::from_value::<DocumentSymbolParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|params| handle_document_symbols(params, state));
            send_response(req.id, res, connection);
        }
        Formatting::METHOD => {
            let res = serde_json::from_value::<DocumentFormattingParams>(req.params)
                .map_err(|e| e.to_string())
                .map(|params| handle_formatting(params, state));
            send_response(req.id, res, connection);
        }
        unknown => {
            eprintln!("[vanta-lsp] Unhandled request method: {}", unknown);
            let resp = Response::new_err(
                req.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                format!("Method not found: {}", unknown),
            );
            let _ = connection.sender.send(Message::Response(resp));
        }
    }
}

/// Dispatches an incoming LSP notification.
pub fn handle_notification(notif: Notification, state: &mut ServerState, connection: &Connection) {
    match notif.method.as_str() {
        Initialized::METHOD => {
            eprintln!("[vanta-lsp] Client initialized");
        }
        DidOpenTextDocument::METHOD => {
            if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(notif.params) {
                let uri = params.text_document.uri;
                let version = params.text_document.version;
                let doc = state.document_manager.open(
                    uri.clone(),
                    version,
                    params.text_document.language_id,
                    params.text_document.text,
                );

                let snapshot = Arc::new(AnalysisSnapshot::new(doc));
                let diags = snapshot.diagnostics.clone();
                doc.last_analysis = Some(snapshot);

                publish_diagnostics(uri, Some(version), diags, connection);
            }
        }
        DidChangeTextDocument::METHOD => {
            if let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(notif.params)
            {
                let uri = params.text_document.uri;
                let version = params.text_document.version;

                if let Ok(doc) =
                    state
                        .document_manager
                        .apply_changes(&uri, version, params.content_changes)
                {
                    let snapshot = Arc::new(AnalysisSnapshot::new(doc));
                    let diags = snapshot.diagnostics.clone();
                    doc.last_analysis = Some(snapshot);

                    publish_diagnostics(uri, Some(version), diags, connection);
                }
            }
        }
        DidCloseTextDocument::METHOD => {
            if let Ok(params) = serde_json::from_value::<DidCloseTextDocumentParams>(notif.params) {
                let uri = params.text_document.uri;
                state.document_manager.close(&uri);
                // Clear published diagnostics for closed document
                publish_diagnostics(uri, None, Vec::new(), connection);
            }
        }
        DidSaveTextDocument::METHOD => {
            if let Ok(params) = serde_json::from_value::<DidSaveTextDocumentParams>(notif.params) {
                let uri = params.text_document.uri;
                if let Some(doc) = state.document_manager.get_mut(&uri) {
                    let snapshot = Arc::new(AnalysisSnapshot::new(doc));
                    let diags = snapshot.diagnostics.clone();
                    let version = doc.version;
                    doc.last_analysis = Some(snapshot);
                    publish_diagnostics(uri, Some(version), diags, connection);
                }
            }
        }
        _ => {
            // Notifications do not receive error responses per LSP spec
        }
    }
}

fn send_response<T: serde::Serialize>(
    id: RequestId,
    res: Result<T, String>,
    connection: &Connection,
) {
    let resp = match res {
        Ok(val) => match serde_json::to_value(val) {
            Ok(v) => Response::new_ok(id, v),
            Err(e) => Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                format!("Serialization error: {}", e),
            ),
        },
        Err(err_msg) => Response::new_err(id, lsp_server::ErrorCode::InvalidParams as i32, err_msg),
    };
    let _ = connection.sender.send(Message::Response(resp));
}

fn publish_diagnostics(
    uri: Uri,
    version: Option<i32>,
    diagnostics: Vec<lsp_types::Diagnostic>,
    connection: &Connection,
) {
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics,
        version,
    };

    let notif = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
    let _ = connection.sender.send(Message::Notification(notif));
}

fn handle_initialize(
    params: InitializeParams,
    state: &mut ServerState,
) -> Result<InitializeResult, String> {
    state.client_capabilities = Some(params.capabilities);

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
            all_commit_characters: None,
            work_done_progress_options: Default::default(),
            completion_item: None,
        }),
        document_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..Default::default()
    };

    Ok(InitializeResult {
        capabilities,
        server_info: Some(ServerInfo {
            name: "vanta-lsp".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    })
}

fn handle_hover(params: HoverParams, state: &mut ServerState) -> Option<lsp_types::Hover> {
    let uri = params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;
    let snapshot = state.get_or_analyze(&uri)?;
    snapshot.get_hover(pos)
}

fn handle_definition(
    params: GotoDefinitionParams,
    state: &mut ServerState,
) -> Option<lsp_types::GotoDefinitionResponse> {
    let uri = params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;
    let snapshot = state.get_or_analyze(&uri)?;
    snapshot.get_definition(&uri, pos)
}

fn handle_completion(
    params: CompletionParams,
    state: &mut ServerState,
) -> lsp_types::CompletionList {
    let uri = params.text_document_position.text_document.uri;
    let pos = params.text_document_position.position;
    if let Some(snapshot) = state.get_or_analyze(&uri) {
        snapshot.get_completion(pos)
    } else {
        lsp_types::CompletionList {
            is_incomplete: false,
            items: Vec::new(),
        }
    }
}

fn handle_document_symbols(
    params: DocumentSymbolParams,
    state: &mut ServerState,
) -> Vec<lsp_types::DocumentSymbol> {
    let uri = params.text_document.uri;
    if let Some(snapshot) = state.get_or_analyze(&uri) {
        snapshot.get_document_symbols()
    } else {
        Vec::new()
    }
}

fn handle_formatting(
    params: DocumentFormattingParams,
    state: &mut ServerState,
) -> Option<Vec<lsp_types::TextEdit>> {
    let uri = params.text_document.uri;
    let snapshot = state.get_or_analyze(&uri)?;
    snapshot.format_document()
}
