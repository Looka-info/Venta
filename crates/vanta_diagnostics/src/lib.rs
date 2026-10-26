//! Rich diagnostic reporting with labeled source spans and machine-actionable JSON output.

pub mod codes {
    pub const E0001_UNEXPECTED_CHAR: &str = "E0001";
    pub const E0002_UNTERMINATED_LITERAL: &str = "E0002";
    pub const E0003_INVALID_NUMBER: &str = "E0003";
    pub const E0004_SYNTAX_ERROR: &str = "E0004";
    pub const E0005_UNEXPECTED_EOF: &str = "E0005";
    pub const E0101_DUPLICATE_SYMBOL: &str = "E0101";
    pub const E0102_UNDEFINED_SYMBOL: &str = "E0102";
    pub const E0103_REDEFINED_INTRINSIC: &str = "E0103";
    pub const E0104_UNKNOWN_STRUCT: &str = "E0104";
    pub const E0105_UNKNOWN_FIELD: &str = "E0105";
    pub const E0106_DUPLICATE_FIELD: &str = "E0106";
    pub const E0107_MISSING_FIELD: &str = "E0107";
    pub const E0108_UNKNOWN_METHOD: &str = "E0108";
    pub const E0109_INVALID_RECEIVER: &str = "E0109";
    pub const E0201_TYPE_MISMATCH: &str = "E0201";
    pub const E0202_UNSIZED_TYPE_VALUE: &str = "E0202";
    pub const E0203_MUTABLE_STR_SLICE: &str = "E0203";
    pub const E0204_INVALID_OPERAND_TYPE: &str = "E0204";
    pub const E0205_ARGUMENT_MISMATCH: &str = "E0205";
    pub const E0206_INVALID_RETURN_TYPE: &str = "E0206";
    pub const E0207_RETURN_TYPE_MISMATCH: &str = "E0207";
    pub const E0208_RECURSIVE_STRUCT: &str = "E0208";
    pub const E0301_UNINITIALIZED_READ: &str = "E0301";
    pub const E0302_USE_OF_MOVED_VALUE: &str = "E0302";
    pub const E0303_MUTATE_IMMUTABLE: &str = "E0303";
    pub const E0304_MUT_BORROW_CONFLICT_SHARED: &str = "E0304";
    pub const E0305_MUT_BORROW_CONFLICT_MUT: &str = "E0305";
    pub const E0306_MUTATE_WHILE_BORROWED: &str = "E0306";
    pub const E0401_MIR_VERIFICATION_FAILED: &str = "E0401";
    pub const E0402_BACKEND_FAILED: &str = "E0402";
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub message: String,
    pub replacement_span: Option<Span>,
    pub replacement_text: String,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: Option<&'static str>,
    pub message: String,
    pub primary_span: Option<Span>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub suggestion: Option<Suggestion>,
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn span_to_json(span: Span, sm: &SourceMap) -> (Option<String>, String) {
    if let Some(file) = sm.get_file(span.file_id) {
        let (s_line, s_col) = file.line_col(span.start);
        let (e_line, e_col) = file.line_col(span.end);
        let file_path = escape_json(&file.path);
        let json = format!(
            "{{\"start\":{},\"end\":{},\"start_line\":{},\"start_col\":{},\"end_line\":{},\"end_col\":{}}}",
            span.start, span.end, s_line, s_col, e_line, e_col
        );
        (Some(file_path), json)
    } else {
        (
            None,
            format!(
                "{{\"start\":{},\"end\":{},\"start_line\":1,\"start_col\":1,\"end_line\":1,\"end_col\":1}}",
                span.start, span.end
            ),
        )
    }
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: None,
            message: message.into(),
            primary_span: None,
            expected: None,
            actual: None,
            labels: Vec::new(),
            notes: Vec::new(),
            suggestion: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: None,
            message: message.into(),
            primary_span: None,
            expected: None,
            actual: None,
            labels: Vec::new(),
            notes: Vec::new(),
            suggestion: None,
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

    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
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

    pub fn with_suggestion(
        mut self,
        message: impl Into<String>,
        span: Option<Span>,
        replacement: impl Into<String>,
    ) -> Self {
        self.suggestion = Some(Suggestion {
            message: message.into(),
            replacement_span: span,
            replacement_text: replacement.into(),
        });
        self
    }

    /// Render formatted human-readable diagnostic message with source context.
    pub fn render(&self, sm: &SourceMap, use_color: bool) -> String {
        let mut out = String::new();
        let (red, yellow, cyan, bold, reset) = if use_color {
            ("\x1b[31m", "\x1b[33m", "\x1b[36m", "\x1b[1m", "\x1b[0m")
        } else {
            ("", "", "", "", "")
        };

        let (color, sev_str) = match self.severity {
            Severity::Error => (red, "error"),
            Severity::Warning => (yellow, "warning"),
            Severity::Note => (cyan, "note"),
        };

        let code_str = match self.code {
            Some(c) => format!("[{}]", c),
            None => String::new(),
        };

        if let Some(span) = self.primary_span {
            if let Some(file) = sm.get_file(span.file_id) {
                let (line, col) = file.line_col(span.start);
                out.push_str(&format!(
                    "{}{}:{}:{}{}: {}{}{}: {}{}\n",
                    bold,
                    file.path,
                    line,
                    col,
                    reset,
                    color,
                    sev_str,
                    code_str,
                    self.message,
                    reset
                ));

                // Print snippet
                if let Some(line_text) = file.get_line(line) {
                    let line_num_str = format!("{:4} | ", line);
                    out.push_str(&format!("{}{}{}{}\n", bold, cyan, line_num_str, reset));
                    out.push_str(&format!(
                        "{}{}{}{} {}\n",
                        bold, cyan, line_num_str, reset, line_text
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
            out.push_str(&format!("  {}note{}: {}\n", cyan, reset, note));
        }

        if let Some(sug) = &self.suggestion {
            out.push_str(&format!("  {}help{}: {}\n", cyan, reset, sug.message));
        }

        out
    }

    /// Render machine-readable JSON object representing this diagnostic.
    pub fn to_json(&self, sm: &SourceMap) -> String {
        let code_val = match self.code {
            Some(c) => format!("\"{}\"", escape_json(c)),
            None => "null".to_string(),
        };

        let (file_val, span_val) = match self.primary_span {
            Some(s) => {
                let (f, sp) = span_to_json(s, sm);
                let f_str = match f {
                    Some(fp) => format!("\"{}\"", fp),
                    None => "null".to_string(),
                };
                (f_str, sp)
            }
            None => ("null".to_string(), "null".to_string()),
        };

        let expected_val = match &self.expected {
            Some(e) => format!("\"{}\"", escape_json(e)),
            None => "null".to_string(),
        };

        let actual_val = match &self.actual {
            Some(a) => format!("\"{}\"", escape_json(a)),
            None => "null".to_string(),
        };

        let mut labels_json = String::from("[");
        for (i, label) in self.labels.iter().enumerate() {
            if i > 0 {
                labels_json.push(',');
            }
            let (lbl_file, lbl_span) = span_to_json(label.span, sm);
            let lbl_file_str = match lbl_file {
                Some(f) => format!("\"{}\"", f),
                None => "null".to_string(),
            };
            labels_json.push_str(&format!(
                "{{\"file\":{},\"span\":{},\"message\":\"{}\"}}",
                lbl_file_str,
                lbl_span,
                escape_json(&label.message)
            ));
        }
        labels_json.push(']');

        let mut notes_json = String::from("[");
        for (i, note) in self.notes.iter().enumerate() {
            if i > 0 {
                notes_json.push(',');
            }
            notes_json.push_str(&format!("\"{}\"", escape_json(note)));
        }
        notes_json.push(']');

        let suggestion_val = match &self.suggestion {
            Some(sug) => {
                let repl_span_val = match sug.replacement_span {
                    Some(sp) => span_to_json(sp, sm).1,
                    None => "null".to_string(),
                };
                format!(
                    "{{\"message\":\"{}\",\"replacement_span\":{},\"replacement_text\":\"{}\"}}",
                    escape_json(&sug.message),
                    repl_span_val,
                    escape_json(&sug.replacement_text)
                )
            }
            None => "null".to_string(),
        };

        format!(
            "{{\"code\":{},\"severity\":\"{}\",\"message\":\"{}\",\"file\":{},\"span\":{},\"expected\":{},\"actual\":{},\"labels\":{},\"notes\":{},\"suggestion\":{}}}",
            code_val,
            self.severity.as_str(),
            escape_json(&self.message),
            file_val,
            span_val,
            expected_val,
            actual_val,
            labels_json,
            notes_json,
            suggestion_val
        )
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

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
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

    /// Render entire bag as a versioned JSON document matching schema 1.0.0.
    pub fn to_json(&self, sm: &SourceMap) -> String {
        let has_err = self.has_errors();
        let err_count = self.error_count();
        let warn_count = self.warning_count();

        let mut diags_json = String::from("[");
        for (i, diag) in self.diagnostics.iter().enumerate() {
            if i > 0 {
                diags_json.push(',');
            }
            diags_json.push_str(&diag.to_json(sm));
        }
        diags_json.push(']');

        format!(
            "{{\"version\":\"1.0.0\",\"success\":{},\"error_count\":{},\"warning_count\":{},\"diagnostics\":{}}}",
            !has_err, err_count, warn_count, diags_json
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_source::{SourceMap, Span};

    #[test]
    fn test_diagnostic_json_serialization() {
        let mut sm = SourceMap::new();
        let file_id = sm.add_file(
            "test.va".into(),
            "fn main() {\n    print_i64(\"bad\");\n}\n".into(),
        );

        let span = Span::new(file_id, 26, 31);
        let diag = Diagnostic::error("Type mismatch: expected 'i64', found '&str'")
            .with_code(codes::E0201_TYPE_MISMATCH)
            .with_span(span)
            .with_expected("i64")
            .with_actual("&str")
            .with_note("intrinsic 'print_i64' expects 'i64'")
            .with_suggestion(
                "call 'print_str'",
                Some(Span::new(file_id, 16, 25)),
                "print_str",
            );

        let mut bag = DiagnosticBag::new();
        bag.emit(diag);

        let json = bag.to_json(&sm);
        assert!(json.contains("\"version\":\"1.0.0\""));
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("\"error_count\":1"));
        assert!(json.contains("\"code\":\"E0201\""));
        assert!(json.contains("\"expected\":\"i64\""));
        assert!(json.contains("\"actual\":\"&str\""));
        assert!(json.contains("\"replacement_text\":\"print_str\""));
    }
}
