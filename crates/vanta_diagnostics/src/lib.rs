//! Rich diagnostic reporting with labeled source spans.

use vanta_source::{SourceMap, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: Option<&'static str>,
    pub message: String,
    pub primary_span: Option<Span>,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: None,
            message: message.into(),
            primary_span: None,
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: None,
            message: message.into(),
            primary_span: None,
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.primary_span = Some(span);
        self
    }

    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Render formatted diagnostic message with source context.
    pub fn render(&self, sm: &SourceMap, use_color: bool) -> String {
        let mut out = String::new();
        let (red, yellow, blue, bold, reset) = if use_color {
            ("\x1b[31m", "\x1b[33m", "\x1b[36m", "\x1b[1m", "\x1b[0m")
        } else {
            ("", "", "", "", "")
        };

        let (color, sev_str) = match self.severity {
            Severity::Error => (red, "error"),
            Severity::Warning => (yellow, "warning"),
            Severity::Note => (blue, "note"),
        };

        let code_str = match self.code {
            Some(c) => format!("[{}]", c),
            None => String::new(),
        };

        if let Some(span) = self.primary_span {
            if let Some(file) = sm.get_file(span.file_id) {
                let (line, col) = file.line_col(span.start);
                out.push_str(&format!(
                    "{}{}{}{}: {}{}: {}{}\n",
                    bold,
                    file.path,
                    format!(":{}:{}", line, col),
                    reset,
                    color,
                    format!("{}{}", sev_str, code_str),
                    self.message,
                    reset
                ));

                // Print snippet
                if let Some(line_text) = file.get_line(line) {
                    let line_num_str = format!("{:4} | ", line);
                    out.push_str(&format!("{}{}{}{}\n", bold, blue, line_num_str, reset));
                    out.push_str(&format!(
                        "{}{}{}{} {}\n",
                        bold, blue, line_num_str, reset, line_text
                    ));

                    let indent = " ".repeat(line_num_str.len() + col.saturating_sub(1));
                    let span_len = (span.end.saturating_sub(span.start) as usize).max(1);
                    let underline = "^".repeat(span_len);
                    out.push_str(&format!(
                        "{}{}{}{}{}\n",
                        indent, bold, color, underline, reset
                    ));
                }
            } else {
                out.push_str(&format!("{}: {}\n", sev_str, self.message));
            }
        } else {
            out.push_str(&format!(
                "{}{}{}: {}{}\n",
                color, sev_str, code_str, self.message, reset
            ));
        }

        for note in &self.notes {
            out.push_str(&format!("  {}note{}: {}\n", blue, reset, note));
        }

        out
    }
}

/// Collection and emitter of compilation diagnostics.
#[derive(Debug, Default)]
pub struct DiagnosticBag {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn emit(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn render_all(&self, sm: &SourceMap, use_color: bool) -> String {
        let mut res = String::new();
        for diag in &self.diagnostics {
            res.push_str(&diag.render(sm, use_color));
            res.push('\n');
        }
        res
    }
}
