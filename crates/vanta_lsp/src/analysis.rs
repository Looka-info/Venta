//! Semantic and syntactic analysis over in-memory Venta documents.

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, DocumentSymbol, GotoDefinitionResponse,
    Hover, HoverContents, Location, MarkupContent, MarkupKind, NumberOrString, Position, Range,
    SymbolKind, TextEdit, Uri,
};
use std::collections::HashSet;
use vanta_diagnostics::Severity;
use vanta_lexer::Lexer;
use vanta_parser::ast::*;
use vanta_parser::Parser;
use vanta_sema::{SemanticAnalyzer, SemanticContext};
use vanta_source::{SourceMap, Span};

use crate::document::{Document, LineIndex};

/// An immutable snapshot of compiler analysis results for a specific document version.
pub struct AnalysisSnapshot {
    pub version: i32,
    pub text: String,
    pub file_name: String,
    pub line_index: LineIndex,
    pub program: Option<Program>,
    pub sema_ctx: Option<SemanticContext>,
    pub diagnostics: Vec<lsp_types::Diagnostic>,
}

impl std::fmt::Debug for AnalysisSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalysisSnapshot")
            .field("version", &self.version)
            .field("file_name", &self.file_name)
            .field("diagnostics_count", &self.diagnostics.len())
            .finish()
    }
}

impl AnalysisSnapshot {
    /// Runs lexer, parser, and semantic analysis on the document.
    pub fn new(doc: &Document) -> Self {
        let text = doc.text.clone();
        let file_name = doc.file_name();
        let line_index = doc.line_index.clone();
        let version = doc.version;

        let mut source_map = SourceMap::new();
        let file_id = source_map.add_file(file_name.clone(), text.clone());
        let source_file = source_map.get_file(file_id).unwrap();

        let mut diag_bag = vanta_diagnostics::DiagnosticBag::new();

        // 1. Lexing
        let mut lexer = Lexer::new(&source_file);
        let tokens = lexer.tokenize(&mut diag_bag);

        // 2. Parsing (attempt parsing even if lexing had non-fatal diagnostics)
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diag_bag);

        // 3. Semantic Analysis
        let sema = SemanticAnalyzer::new();
        let sema_ctx = sema.analyze(&program, &mut diag_bag);

        // 4. Convert compiler diagnostics to LSP diagnostics
        let mut lsp_diagnostics = Vec::new();
        for d in diag_bag.diagnostics() {
            let severity = match d.severity {
                Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
                Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
                Severity::Note => lsp_types::DiagnosticSeverity::INFORMATION,
            };

            let range = if let Some(sp) = d.primary_span {
                let sp: Span = sp;
                if !sp.is_dummy() {
                    line_index.span_to_range(&text, sp)
                } else {
                    line_index.full_range(&text)
                }
            } else if let Some(first_label) = d.labels.first() {
                if !first_label.span.is_dummy() {
                    line_index.span_to_range(&text, first_label.span)
                } else {
                    line_index.full_range(&text)
                }
            } else {
                Range::new(Position::new(0, 0), Position::new(0, 0))
            };

            let mut message = d.message.clone();
            for label in &d.labels {
                if !label.message.is_empty() && label.message != d.message {
                    message.push_str(&format!("\n• {}", label.message));
                }
            }
            for note in &d.notes {
                message.push_str(&format!("\nNote: {}", note));
            }

            let code = d.code.map(|c| NumberOrString::String(c.to_string()));

            lsp_diagnostics.push(lsp_types::Diagnostic {
                range,
                severity: Some(severity),
                code,
                code_description: None,
                source: Some("venta".to_string()),
                message,
                related_information: None,
                tags: None,
                data: None,
            });
        }

        Self {
            version,
            text,
            file_name,
            line_index,
            program: Some(program),
            sema_ctx: Some(sema_ctx),
            diagnostics: lsp_diagnostics,
        }
    }

    /// Finds the word/identifier at the given offset.
    fn identifier_at_offset(&self, offset: usize) -> Option<(&str, Range)> {
        if self.text.is_empty() {
            return None;
        }
        let offset = offset.min(self.text.len());

        let bytes = self.text.as_bytes();
        let check_idx = if offset < bytes.len()
            && (bytes[offset].is_ascii_alphanumeric() || bytes[offset] == b'_')
        {
            offset
        } else if offset > 0
            && (bytes[offset - 1].is_ascii_alphanumeric() || bytes[offset - 1] == b'_')
        {
            offset - 1
        } else {
            return None;
        };

        let mut start = check_idx;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }

        let mut end = check_idx;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }

        if start >= end {
            return None;
        }

        let word = &self.text[start..end];
        let range = Range {
            start: self.line_index.offset_to_position(&self.text, start),
            end: self.line_index.offset_to_position(&self.text, end),
        };

        Some((word, range))
    }

    /// Handles hover request at the given position.
    pub fn get_hover(&self, pos: Position) -> Option<Hover> {
        let offset = self.line_index.position_to_offset(&self.text, pos);
        let (word, range) = self.identifier_at_offset(offset)?;

        let sema = self.sema_ctx.as_ref()?;

        // 1. Check local variables / parameters
        for sym in sema.symbols.values() {
            if sym.name == word {
                let hover_text = format!(
                    "let {}{}: {}",
                    if sym.is_mut { "mut " } else { "" },
                    sym.name,
                    sym.ty
                );
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("```venta\n{}\n```", hover_text),
                    }),
                    range: Some(range),
                });
            }
        }

        // 2. Check functions
        if let Some(fn_sig) = sema.functions.get(word) {
            let mut params_str = Vec::new();
            for (p_name, p_ty, is_mut) in &fn_sig.params {
                params_str.push(format!(
                    "{}{}: {}",
                    if *is_mut { "mut " } else { "" },
                    p_name,
                    p_ty
                ));
            }
            let sig_str = format!(
                "fn {}({}) -> {}",
                fn_sig.name,
                params_str.join(", "),
                fn_sig.ret_type
            );
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```venta\n{}\n```", sig_str),
                }),
                range: Some(range),
            });
        }

        // 3. Check structs
        if let Some(st) = sema.structs.get(word) {
            let mut fields_str = Vec::new();
            for f in &st.fields {
                fields_str.push(format!("    {}: {},", f.name, f.ty));
            }
            let val = if fields_str.is_empty() {
                format!("struct {} {{}}", st.name)
            } else {
                format!("struct {} {{\n{}\n}}", st.name, fields_str.join("\n"))
            };
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```venta\n{}\n```", val),
                }),
                range: Some(range),
            });
        }

        // 4. Check enums
        if let Some(en) = sema.enums.get(word) {
            let mut variants_str = Vec::new();
            for v in &en.variants {
                if v.payload.is_empty() {
                    variants_str.push(format!("    {},", v.name));
                } else {
                    let types: Vec<String> = v.payload.iter().map(|t| format!("{}", t)).collect();
                    variants_str.push(format!("    {}({}),", v.name, types.join(", ")));
                }
            }
            let val = format!("enum {} {{\n{}\n}}", en.name, variants_str.join("\n"));
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```venta\n{}\n```", val),
                }),
                range: Some(range),
            });
        }

        // 5. Check traits
        if let Some(tr) = sema.traits.get(word) {
            let val = format!("trait {}", tr.name);
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```venta\n{}\n```", val),
                }),
                range: Some(range),
            });
        }

        None
    }

    /// Handles go to definition request at the given position.
    pub fn get_definition(&self, uri: &Uri, pos: Position) -> Option<GotoDefinitionResponse> {
        let offset = self.line_index.position_to_offset(&self.text, pos);
        let (word, _range) = self.identifier_at_offset(offset)?;

        let sema = self.sema_ctx.as_ref()?;

        // 1. Local variable or parameter declaration
        for sym in sema.symbols.values() {
            if sym.name == word && !sym.span.is_dummy() {
                let range = self.line_index.span_to_range(&self.text, sym.span);
                return Some(GotoDefinitionResponse::Scalar(Location::new(
                    uri.clone(),
                    range,
                )));
            }
        }

        // 2. Function declaration
        if let Some(fn_sig) = sema.functions.get(word) {
            if !fn_sig.span.is_dummy() {
                let range = self.line_index.span_to_range(&self.text, fn_sig.span);
                return Some(GotoDefinitionResponse::Scalar(Location::new(
                    uri.clone(),
                    range,
                )));
            }
        }

        // 3. Struct declaration
        if let Some(st) = sema.structs.get(word) {
            if !st.span.is_dummy() {
                let range = self.line_index.span_to_range(&self.text, st.span);
                return Some(GotoDefinitionResponse::Scalar(Location::new(
                    uri.clone(),
                    range,
                )));
            }
        }

        // 4. Enum declaration
        if let Some(en) = sema.enums.get(word) {
            if !en.span.is_dummy() {
                let range = self.line_index.span_to_range(&self.text, en.span);
                return Some(GotoDefinitionResponse::Scalar(Location::new(
                    uri.clone(),
                    range,
                )));
            }
        }

        // 5. Trait declaration
        if let Some(tr) = sema.traits.get(word) {
            if !tr.span.is_dummy() {
                let range = self.line_index.span_to_range(&self.text, tr.span);
                return Some(GotoDefinitionResponse::Scalar(Location::new(
                    uri.clone(),
                    range,
                )));
            }
        }

        None
    }

    /// Handles completion request at the given position.
    pub fn get_completion(&self, pos: Position) -> CompletionList {
        let mut items = Vec::new();
        let mut seen = HashSet::new();

        // 1. Language keywords
        let keywords = [
            "fn", "let", "mut", "pub", "return", "if", "else", "while", "for", "in", "struct",
            "enum", "match", "trait", "impl", "import", "mod", "break", "continue", "true",
            "false", "i64", "i32", "i16", "i8", "u64", "u32", "u16", "u8", "f64", "f32", "bool",
            "str", "self",
        ];

        for kw in keywords {
            if seen.insert(kw.to_string()) {
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("keyword".to_string()),
                    ..Default::default()
                });
            }
        }

        let offset = self.line_index.position_to_offset(&self.text, pos);

        // 2. Visible symbols from semantic analysis
        if let Some(ref sema) = self.sema_ctx {
            // Local variables
            for sym in sema.symbols.values() {
                if (sym.span.start as usize) <= offset {
                    if seen.insert(sym.name.clone()) {
                        items.push(CompletionItem {
                            label: sym.name.clone(),
                            kind: Some(CompletionItemKind::VARIABLE),
                            detail: Some(format!("{}", sym.ty)),
                            ..Default::default()
                        });
                    }
                }
            }

            // Visible functions
            for fn_sig in sema.functions.values() {
                if seen.insert(fn_sig.name.clone()) {
                    let mut params = Vec::new();
                    for (p, ty, is_mut) in &fn_sig.params {
                        params.push(format!(
                            "{}{}: {}",
                            if *is_mut { "mut " } else { "" },
                            p,
                            ty
                        ));
                    }
                    items.push(CompletionItem {
                        label: fn_sig.name.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(format!("fn({}) -> {}", params.join(", "), fn_sig.ret_type)),
                        ..Default::default()
                    });
                }
            }

            // Structs
            for st in sema.structs.values() {
                if seen.insert(st.name.clone()) {
                    items.push(CompletionItem {
                        label: st.name.clone(),
                        kind: Some(CompletionItemKind::STRUCT),
                        detail: Some(format!("struct {}", st.name)),
                        ..Default::default()
                    });
                }
            }

            // Enums
            for en in sema.enums.values() {
                if seen.insert(en.name.clone()) {
                    items.push(CompletionItem {
                        label: en.name.clone(),
                        kind: Some(CompletionItemKind::ENUM),
                        detail: Some(format!("enum {}", en.name)),
                        ..Default::default()
                    });
                }
            }

            // Traits
            for tr in sema.traits.values() {
                if seen.insert(tr.name.clone()) {
                    items.push(CompletionItem {
                        label: tr.name.clone(),
                        kind: Some(CompletionItemKind::INTERFACE),
                        detail: Some(format!("trait {}", tr.name)),
                        ..Default::default()
                    });
                }
            }
        }

        CompletionList {
            is_incomplete: false,
            items,
        }
    }

    /// Handles document symbol hierarchy extraction.
    pub fn get_document_symbols(&self) -> Vec<DocumentSymbol> {
        let mut symbols = Vec::new();
        let program = match &self.program {
            Some(p) => p,
            None => return symbols,
        };

        #[allow(deprecated)]
        for item in &program.items {
            match item {
                Item::Function(f) => {
                    let range = self.line_index.span_to_range(&self.text, f.span);
                    let sel_range = self.line_index.span_to_range(&self.text, f.name.span);
                    let mut params_str = Vec::new();
                    for p in &f.params {
                        params_str.push(format!(
                            "{}: {}",
                            p.name.name,
                            match &p.ty.kind {
                                TypeExprKind::Primitive(pt) => format!("{:?}", pt).to_lowercase(),
                                TypeExprKind::Named(n) => n.clone(),
                                _ => "any".to_string(),
                            }
                        ));
                    }
                    symbols.push(DocumentSymbol {
                        name: f.name.name.clone(),
                        detail: Some(format!("fn({})", params_str.join(", "))),
                        kind: SymbolKind::FUNCTION,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: sel_range,
                        children: None,
                    });
                }
                Item::Struct(s) => {
                    let range = self.line_index.span_to_range(&self.text, s.span);
                    let sel_range = self.line_index.span_to_range(&self.text, s.name.span);
                    let mut children = Vec::new();
                    for field in &s.fields {
                        let f_range = self.line_index.span_to_range(&self.text, field.span);
                        let f_sel = self.line_index.span_to_range(&self.text, field.name.span);
                        children.push(DocumentSymbol {
                            name: field.name.name.clone(),
                            detail: None,
                            kind: SymbolKind::FIELD,
                            tags: None,
                            deprecated: None,
                            range: f_range,
                            selection_range: f_sel,
                            children: None,
                        });
                    }
                    symbols.push(DocumentSymbol {
                        name: s.name.name.clone(),
                        detail: Some("struct".to_string()),
                        kind: SymbolKind::STRUCT,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: sel_range,
                        children: if children.is_empty() {
                            None
                        } else {
                            Some(children)
                        },
                    });
                }
                Item::Enum(e) => {
                    let range = self.line_index.span_to_range(&self.text, e.span);
                    let sel_range = self.line_index.span_to_range(&self.text, e.name.span);
                    let mut children = Vec::new();
                    for v in &e.variants {
                        let v_range = self.line_index.span_to_range(&self.text, v.span);
                        let v_sel = self.line_index.span_to_range(&self.text, v.name.span);
                        children.push(DocumentSymbol {
                            name: v.name.name.clone(),
                            detail: None,
                            kind: SymbolKind::ENUM_MEMBER,
                            tags: None,
                            deprecated: None,
                            range: v_range,
                            selection_range: v_sel,
                            children: None,
                        });
                    }
                    symbols.push(DocumentSymbol {
                        name: e.name.name.clone(),
                        detail: Some("enum".to_string()),
                        kind: SymbolKind::ENUM,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: sel_range,
                        children: if children.is_empty() {
                            None
                        } else {
                            Some(children)
                        },
                    });
                }
                Item::Trait(t) => {
                    let range = self.line_index.span_to_range(&self.text, t.span);
                    let sel_range = self.line_index.span_to_range(&self.text, t.name.span);
                    let mut children = Vec::new();
                    for m in &t.methods {
                        let m_range = self.line_index.span_to_range(&self.text, m.span);
                        let m_sel = self.line_index.span_to_range(&self.text, m.name.span);
                        children.push(DocumentSymbol {
                            name: m.name.name.clone(),
                            detail: None,
                            kind: SymbolKind::METHOD,
                            tags: None,
                            deprecated: None,
                            range: m_range,
                            selection_range: m_sel,
                            children: None,
                        });
                    }
                    symbols.push(DocumentSymbol {
                        name: t.name.name.clone(),
                        detail: Some("trait".to_string()),
                        kind: SymbolKind::INTERFACE,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: sel_range,
                        children: if children.is_empty() {
                            None
                        } else {
                            Some(children)
                        },
                    });
                }
                Item::Impl(imp) => {
                    let range = self.line_index.span_to_range(&self.text, imp.span);
                    let sel_range = self.line_index.span_to_range(&self.text, imp.target.span);
                    let mut children = Vec::new();
                    for m in &imp.methods {
                        let m_range = self.line_index.span_to_range(&self.text, m.span);
                        let m_sel = self.line_index.span_to_range(&self.text, m.name.span);
                        children.push(DocumentSymbol {
                            name: m.name.name.clone(),
                            detail: None,
                            kind: SymbolKind::METHOD,
                            tags: None,
                            deprecated: None,
                            range: m_range,
                            selection_range: m_sel,
                            children: None,
                        });
                    }
                    symbols.push(DocumentSymbol {
                        name: format!("impl {}", imp.target.name),
                        detail: None,
                        kind: SymbolKind::NAMESPACE,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: sel_range,
                        children: if children.is_empty() {
                            None
                        } else {
                            Some(children)
                        },
                    });
                }
                Item::Mod(m) => {
                    let range = self.line_index.span_to_range(&self.text, m.span);
                    let sel_range = self.line_index.span_to_range(&self.text, m.name.span);
                    symbols.push(DocumentSymbol {
                        name: m.name.name.clone(),
                        detail: Some("mod".to_string()),
                        kind: SymbolKind::MODULE,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: sel_range,
                        children: None,
                    });
                }
                Item::Import(imp) => {
                    let range = self.line_index.span_to_range(&self.text, imp.span);
                    let path_str: Vec<String> = imp.path.iter().map(|i| i.name.clone()).collect();
                    symbols.push(DocumentSymbol {
                        name: format!("import {}", path_str.join(".")),
                        detail: None,
                        kind: SymbolKind::MODULE,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: range,
                        children: None,
                    });
                }
            }
        }

        symbols
    }

    /// Formats the document using the canonical Vanta formatter (`vanta_fmt`).
    pub fn format_document(&self) -> Option<Vec<TextEdit>> {
        match vanta_fmt::format_source(&self.text, &self.file_name) {
            Ok(formatted) => {
                if formatted == self.text {
                    Some(Vec::new()) // already canonical
                } else {
                    Some(vec![TextEdit {
                        range: self.line_index.full_range(&self.text),
                        new_text: formatted,
                    }])
                }
            }
            Err(_) => None, // syntax error prevented formatting
        }
    }
}
