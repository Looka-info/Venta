//! Source management and text position tracking for the Vanta compiler.

use std::sync::Arc;

/// A unique identifier for a registered source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// A source span representing a byte range in a specific source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file_id: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(file_id: FileId, start: u32, end: u32) -> Self {
        Self { file_id, start, end }
    }

    pub const fn dummy() -> Self {
        Self {
            file_id: FileId(0),
            start: 0,
            end: 0,
        }
    }

    pub fn is_dummy(&self) -> bool {
        self.file_id.0 == 0 && self.start == 0 && self.end == 0
    }

    pub fn merge(&self, other: Span) -> Span {
        if self.is_dummy() {
            return other;
        }
        if other.is_dummy() {
            return *self;
        }
        assert_eq!(
            self.file_id, other.file_id,
            "Cannot merge spans from different files"
        );
        Span {
            file_id: self.file_id,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A registered source file with indexed line starts.
#[derive(Debug)]
pub struct SourceFile {
    pub id: FileId,
    pub path: String,
    pub src: Arc<str>,
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(id: FileId, path: String, src: String) -> Self {
        let mut line_starts = vec![0];
        for (idx, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((idx + 1) as u32);
            }
        }
        Self {
            id,
            path,
            src: Arc::from(src),
            line_starts,
        }
    }

    /// Look up 1-based (line, column) for a byte offset.
    pub fn line_col(&self, offset: u32) -> (usize, usize) {
        let offset = offset.min(self.src.len() as u32);
        match self.line_starts.binary_search(&offset) {
            Ok(line_idx) => (line_idx + 1, 1),
            Err(next_line_idx) => {
                let line_idx = next_line_idx - 1;
                let line_start = self.line_starts[line_idx];
                let col = (offset - line_start) as usize + 1;
                (line_idx + 1, col)
            }
        }
    }

    /// Retrieve the text of a 1-based line index.
    pub fn get_line(&self, line: usize) -> Option<&str> {
        if line == 0 || line > self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[line - 1] as usize;
        let end = if line < self.line_starts.len() {
            let next_start = self.line_starts[line] as usize;
            if next_start > 0 && self.src.as_bytes().get(next_start - 1) == Some(&b'\n') {
                if next_start > 1 && self.src.as_bytes().get(next_start - 2) == Some(&b'\r') {
                    next_start - 2
                } else {
                    next_start - 1
                }
            } else {
                next_start
            }
        } else {
            self.src.len()
        };
        Some(&self.src[start..end])
    }
}

/// Collection of source files in a compilation session.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<Arc<SourceFile>>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, path: String, src: String) -> FileId {
        let id = FileId(self.files.len() as u32 + 1);
        let file = Arc::new(SourceFile::new(id, path, src));
        self.files.push(file);
        id
    }

    pub fn get_file(&self, id: FileId) -> Option<Arc<SourceFile>> {
        if id.0 == 0 || id.0 as usize > self.files.len() {
            None
        } else {
            Some(self.files[(id.0 - 1) as usize].clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_col_lookup() {
        let mut sm = SourceMap::new();
        let id = sm.add_file("test.va".into(), "hello\nworld\r\nthird".into());
        let file = sm.get_file(id).unwrap();

        // "hello" line 1
        assert_eq!(file.line_col(0), (1, 1));
        assert_eq!(file.line_col(4), (1, 5));
        // "world" line 2
        assert_eq!(file.line_col(6), (2, 1));
        // "third" line 3
        assert_eq!(file.line_col(13), (3, 1));

        assert_eq!(file.get_line(1), Some("hello"));
        assert_eq!(file.get_line(2), Some("world"));
        assert_eq!(file.get_line(3), Some("third"));
    }
}
