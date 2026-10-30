//! In-memory document management and UTF-16 / UTF-8 position tracking.

use lsp_types::{Position, Range, TextDocumentContentChangeEvent, Uri};
use std::collections::HashMap;
use std::sync::Arc;
use vanta_source::Span;

use crate::analysis::AnalysisSnapshot;

fn percent_decode(input: &str) -> String {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(val);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Line index for accurate conversions between UTF-8 byte offsets and LSP UTF-16 Positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    /// Byte offsets where each line begins. Line 0 starts at offset 0.
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Converts a UTF-8 byte offset in `text` to an LSP 0-based UTF-16 Position.
    pub fn offset_to_position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(next_idx) => next_idx.saturating_sub(1),
        };

        let line_start = self.line_starts[line];
        let slice = &text[line_start..offset];
        let mut utf16_col = 0u32;
        for c in slice.chars() {
            utf16_col += c.len_utf16() as u32;
        }

        Position {
            line: line as u32,
            character: utf16_col,
        }
    }

    /// Converts an LSP 0-based UTF-16 Position to a UTF-8 byte offset in `text`.
    pub fn position_to_offset(&self, text: &str, pos: Position) -> usize {
        let line = pos.line as usize;
        if line >= self.line_starts.len() {
            return text.len();
        }

        let line_start = self.line_starts[line];
        let line_end = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1]
        } else {
            text.len()
        };

        let line_text = &text[line_start..line_end];
        let mut utf16_accum = 0u32;
        let mut byte_offset = line_start;

        for c in line_text.chars() {
            if c == '\n' || c == '\r' {
                break;
            }
            if utf16_accum >= pos.character {
                break;
            }
            utf16_accum += c.len_utf16() as u32;
            byte_offset += c.len_utf8();
        }

        byte_offset.min(text.len())
    }

    /// Converts a Venta source Span to an LSP Range.
    pub fn span_to_range(&self, text: &str, span: Span) -> Range {
        let start = self.offset_to_position(text, span.start as usize);
        let mut end = self.offset_to_position(text, span.end as usize);

        // If span is empty, ensure at least 1 character is selected when possible
        if start == end {
            let offset = span.start as usize;
            if offset < text.len() {
                if let Some(ch) = text[offset..].chars().next() {
                    end.character += ch.len_utf16() as u32;
                }
            }
        }

        Range { start, end }
    }

    /// Returns a range covering the entire document.
    pub fn full_range(&self, text: &str) -> Range {
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: self.offset_to_position(text, text.len()),
        }
    }
}

/// An open source document managed in memory.
#[derive(Debug)]
pub struct Document {
    pub uri: Uri,
    pub version: i32,
    pub language_id: String,
    pub text: String,
    pub line_index: LineIndex,
    pub last_analysis: Option<Arc<AnalysisSnapshot>>,
}

impl Document {
    pub fn new(uri: Uri, version: i32, language_id: String, text: String) -> Self {
        let line_index = LineIndex::new(&text);
        Self {
            uri,
            version,
            language_id,
            text,
            line_index,
            last_analysis: None,
        }
    }

    pub fn file_path(&self) -> Option<String> {
        let s = self.uri.as_str();
        if let Some(rest) = s.strip_prefix("file://") {
            let decoded = percent_decode(rest);
            if cfg!(windows) && decoded.starts_with('/') && decoded.chars().nth(2) == Some(':') {
                Some(decoded[1..].to_string())
            } else {
                Some(decoded)
            }
        } else {
            None
        }
    }

    pub fn file_name(&self) -> String {
        self.file_path()
            .and_then(|p| {
                std::path::Path::new(&p)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "main.va".to_string())
    }

    pub fn update_full(&mut self, version: i32, new_text: String) {
        self.version = version;
        self.text = new_text;
        self.line_index = LineIndex::new(&self.text);
        self.last_analysis = None;
    }

    pub fn update_incremental(
        &mut self,
        version: i32,
        range: Range,
        replacement: &str,
    ) -> Result<(), String> {
        let start_byte = self.line_index.position_to_offset(&self.text, range.start);
        let end_byte = self.line_index.position_to_offset(&self.text, range.end);

        if start_byte > end_byte || end_byte > self.text.len() {
            return Err(format!(
                "Invalid incremental change range: {:?} -> byte range {}..{} for doc len {}",
                range,
                start_byte,
                end_byte,
                self.text.len()
            ));
        }

        let mut updated =
            String::with_capacity(self.text.len() + replacement.len() - (end_byte - start_byte));
        updated.push_str(&self.text[..start_byte]);
        updated.push_str(replacement);
        updated.push_str(&self.text[end_byte..]);

        self.version = version;
        self.text = updated;
        self.line_index = LineIndex::new(&self.text);
        self.last_analysis = None;

        Ok(())
    }
}

/// Central document manager tracking open documents by canonical URL.
#[derive(Debug, Default)]
pub struct DocumentManager {
    documents: HashMap<Uri, Document>,
}

impl DocumentManager {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn open(
        &mut self,
        uri: Uri,
        version: i32,
        language_id: String,
        text: String,
    ) -> &mut Document {
        let doc = Document::new(uri.clone(), version, language_id, text);
        self.documents.insert(uri.clone(), doc);
        self.documents.get_mut(&uri).unwrap()
    }

    pub fn apply_changes(
        &mut self,
        uri: &Uri,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> Result<&mut Document, String> {
        let doc = self
            .documents
            .get_mut(uri)
            .ok_or_else(|| format!("Document not found: {}", uri.as_str()))?;

        for change in changes {
            if let Some(range) = change.range {
                doc.update_incremental(version, range, &change.text)?;
            } else {
                doc.update_full(version, change.text);
            }
        }

        Ok(self.documents.get_mut(uri).unwrap())
    }

    pub fn close(&mut self, uri: &Uri) -> Option<Document> {
        self.documents.remove(uri)
    }

    pub fn get(&self, uri: &Uri) -> Option<&Document> {
        self.documents.get(uri)
    }

    pub fn get_mut(&mut self, uri: &Uri) -> Option<&mut Document> {
        self.documents.get_mut(uri)
    }

    pub fn all_uris(&self) -> Vec<Uri> {
        self.documents.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_line_index_ascii() {
        let text = "pub fn main() {\n    let x = 10;\n    return x;\n}\n";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 5);

        // Line 0: "pub fn main() {"
        let p0 = index.offset_to_position(text, 0);
        assert_eq!(p0, Position::new(0, 0));
        assert_eq!(index.position_to_offset(text, p0), 0);

        let p_let = index.offset_to_position(text, 20); // 'l' in "let"
        assert_eq!(p_let, Position::new(1, 4));
        assert_eq!(index.position_to_offset(text, p_let), 20);
    }

    #[test]
    fn test_line_index_unicode_and_emojis() {
        let text = "let text = \"🦀\";\nlet y = 1;\n";
        let index = LineIndex::new(text);

        let quote_offset = text.find(';').unwrap();
        let pos = index.offset_to_position(text, quote_offset);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 15);

        let roundtrip_offset = index.position_to_offset(text, pos);
        assert_eq!(roundtrip_offset, quote_offset);
    }

    #[test]
    fn test_incremental_edit() {
        let uri = Uri::from_str("file:///test.va").unwrap();
        let mut mgr = DocumentManager::new();
        mgr.open(
            uri.clone(),
            1,
            "venta".into(),
            "pub fn add(a: i64) -> i64 {\n    return a;\n}\n".into(),
        );

        let range = Range {
            start: Position::new(1, 12),
            end: Position::new(1, 12),
        };
        let change = TextDocumentContentChangeEvent {
            range: Some(range),
            range_length: None,
            text: " + 1".into(),
        };

        mgr.apply_changes(&uri, 2, vec![change]).unwrap();
        let doc = mgr.get(&uri).unwrap();
        assert_eq!(
            doc.text,
            "pub fn add(a: i64) -> i64 {\n    return a + 1;\n}\n"
        );
        assert_eq!(doc.version, 2);
    }
}
