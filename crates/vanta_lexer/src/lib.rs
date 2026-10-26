//! Lexer for Vanta source code.

pub mod token;

pub use token::{Token, TokenKind};
use vanta_diagnostics::{Diagnostic, DiagnosticBag};
use vanta_source::{SourceFile, Span};

pub struct Lexer<'a> {
    file: &'a SourceFile,
    chars: Vec<(usize, char)>,
    cursor: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(file: &'a SourceFile) -> Self {
        let chars: Vec<(usize, char)> = file.src.char_indices().collect();
        Self {
            file,
            chars,
            cursor: 0,
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.cursor).map(|&(_, c)| c)
    }

    fn peek_char2(&self) -> Option<char> {
        self.chars.get(self.cursor + 1).map(|&(_, c)| c)
    }

    fn bump(&mut self) -> Option<char> {
        if self.cursor < self.chars.len() {
            let c = self.chars[self.cursor].1;
            self.cursor += 1;
            Some(c)
        } else {
            None
        }
    }

    fn current_offset(&self) -> usize {
        if self.cursor < self.chars.len() {
            self.chars[self.cursor].0
        } else {
            self.file.src.len()
        }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file.id, start as u32, end as u32)
    }

    fn skip_whitespace_and_comments(&mut self, diags: &mut DiagnosticBag) {
        loop {
            match self.peek_char() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') => {
                    if self.peek_char2() == Some('/') {
                        // Single-line comment
                        self.bump();
                        self.bump();
                        while let Some(c) = self.bump() {
                            if c == '\n' {
                                break;
                            }
                        }
                    } else if self.peek_char2() == Some('*') {
                        // Multi-line comment (supporting nesting)
                        let start = self.current_offset();
                        self.bump();
                        self.bump();
                        let mut depth = 1;
                        while depth > 0 && self.cursor < self.chars.len() {
                            if self.peek_char() == Some('*') && self.peek_char2() == Some('/') {
                                self.bump();
                                self.bump();
                                depth -= 1;
                            } else if self.peek_char() == Some('/')
                                && self.peek_char2() == Some('*')
                            {
                                self.bump();
                                self.bump();
                                depth += 1;
                            } else {
                                self.bump();
                            }
                        }
                        if depth > 0 {
                            diags.emit(
                                Diagnostic::error("Unterminated block comment")
                                    .with_span(self.span(start, self.current_offset())),
                            );
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    pub fn next_token(&mut self, diags: &mut DiagnosticBag) -> Token {
        self.skip_whitespace_and_comments(diags);

        let start_pos = self.current_offset();
        let Some(c) = self.bump() else {
            return Token::new(TokenKind::Eof, self.span(start_pos, start_pos));
        };

        // Identifiers and Keywords
        if c.is_alphabetic() || c == '_' {
            let mut s = String::new();
            s.push(c);
            while let Some(next) = self.peek_char() {
                if next.is_alphanumeric() || next == '_' {
                    s.push(next);
                    self.bump();
                } else {
                    break;
                }
            }
            let end_pos = self.current_offset();
            let span = self.span(start_pos, end_pos);

            let kind = match s.as_str() {
                "fn" => TokenKind::Fn,
                "let" => TokenKind::Let,
                "mut" => TokenKind::Mut,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "while" => TokenKind::While,
                "loop" => TokenKind::Loop,
                "return" => TokenKind::Return,
                "break" => TokenKind::Break,
                "continue" => TokenKind::Continue,
                "struct" => TokenKind::Struct,
                "pub" => TokenKind::Pub,
                "as" => TokenKind::As,
                "const" => TokenKind::Const,
                "unsafe" => TokenKind::Unsafe,
                "true" => TokenKind::BoolLiteral(true),
                "false" => TokenKind::BoolLiteral(false),
                "i8" => TokenKind::I8,
                "i16" => TokenKind::I16,
                "i32" => TokenKind::I32,
                "i64" => TokenKind::I64,
                "u8" => TokenKind::U8,
                "u16" => TokenKind::U16,
                "u32" => TokenKind::U32,
                "u64" => TokenKind::U64,
                "f32" => TokenKind::F32,
                "f64" => TokenKind::F64,
                "bool" => TokenKind::BoolType,
                _ => TokenKind::Ident(s),
            };

            return Token::new(kind, span);
        }

        // Numbers (Integer and Float)
        if c.is_ascii_digit() {
            let mut num_str = String::new();
            num_str.push(c);

            // Hex, Binary, Octal
            if c == '0' {
                match self.peek_char() {
                    Some('x') | Some('X') => {
                        self.bump();
                        let mut hex_str = String::new();
                        while let Some(h) = self.peek_char() {
                            if h.is_ascii_hexdigit() || h == '_' {
                                if h != '_' {
                                    hex_str.push(h);
                                }
                                self.bump();
                            } else {
                                break;
                            }
                        }
                        let end_pos = self.current_offset();
                        let span = self.span(start_pos, end_pos);
                        return match i64::from_str_radix(&hex_str, 16) {
                            Ok(val) => Token::new(TokenKind::IntLiteral(val), span),
                            Err(e) => {
                                diags.emit(
                                    Diagnostic::error(format!("Invalid hex literal: {}", e))
                                        .with_span(span),
                                );
                                Token::new(TokenKind::Error(hex_str), span)
                            }
                        };
                    }
                    Some('b') | Some('B') => {
                        self.bump();
                        let mut bin_str = String::new();
                        while let Some(b) = self.peek_char() {
                            if b == '0' || b == '1' || b == '_' {
                                if b != '_' {
                                    bin_str.push(b);
                                }
                                self.bump();
                            } else {
                                break;
                            }
                        }
                        let end_pos = self.current_offset();
                        let span = self.span(start_pos, end_pos);
                        return match i64::from_str_radix(&bin_str, 2) {
                            Ok(val) => Token::new(TokenKind::IntLiteral(val), span),
                            Err(e) => {
                                diags.emit(
                                    Diagnostic::error(format!("Invalid binary literal: {}", e))
                                        .with_span(span),
                                );
                                Token::new(TokenKind::Error(bin_str), span)
                            }
                        };
                    }
                    _ => {}
                }
            }

            // Decimal digits
            while let Some(d) = self.peek_char() {
                if d.is_ascii_digit() || d == '_' {
                    if d != '_' {
                        num_str.push(d);
                    }
                    self.bump();
                } else {
                    break;
                }
            }

            // Type suffix (e.g. 10i64, 42u32)
            if let Some(ch) = self.peek_char() {
                if ch == 'i' || ch == 'u' {
                    let mut suffix = String::new();
                    while let Some(s) = self.peek_char() {
                        if s.is_ascii_alphanumeric() {
                            suffix.push(s);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    // Validate suffix
                }
            }

            let end_pos = self.current_offset();
            let span = self.span(start_pos, end_pos);

            return match num_str.parse::<i64>() {
                Ok(val) => Token::new(TokenKind::IntLiteral(val), span),
                Err(e) => {
                    diags.emit(
                        Diagnostic::error(format!("Integer literal out of range: {}", e))
                            .with_span(span),
                    );
                    Token::new(TokenKind::Error(num_str), span)
                }
            };
        }

        // String literals
        if c == '"' {
            let mut s = String::new();
            let mut closed = false;
            while let Some(ch) = self.bump() {
                if ch == '"' {
                    closed = true;
                    break;
                } else if ch == '\\' {
                    match self.bump() {
                        Some('n') => s.push('\n'),
                        Some('r') => s.push('\r'),
                        Some('t') => s.push('\t'),
                        Some('\\') => s.push('\\'),
                        Some('"') => s.push('"'),
                        Some('0') => s.push('\0'),
                        Some(other) => {
                            diags.emit(
                                Diagnostic::error(format!("Unknown escape sequence \\{}", other))
                                    .with_span(self.span(start_pos, self.current_offset())),
                            );
                            s.push(other);
                        }
                        None => break,
                    }
                } else {
                    s.push(ch);
                }
            }
            let end_pos = self.current_offset();
            let span = self.span(start_pos, end_pos);
            if !closed {
                diags.emit(Diagnostic::error("Unterminated string literal").with_span(span));
            }
            return Token::new(TokenKind::StringLiteral(s), span);
        }

        // Multi-char and single-char operators & punctuation
        let end_pos = self.current_offset();
        let kind = match c {
            '+' => {
                if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::PlusEq
                } else {
                    TokenKind::Plus
                }
            }
            '-' => {
                if self.peek_char() == Some('>') {
                    self.bump();
                    TokenKind::Arrow
                } else if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::MinusEq
                } else {
                    TokenKind::Minus
                }
            }
            '*' => {
                if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::StarEq
                } else {
                    TokenKind::Star
                }
            }
            '/' => {
                if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::SlashEq
                } else {
                    TokenKind::Slash
                }
            }
            '%' => {
                if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::PercentEq
                } else {
                    TokenKind::Percent
                }
            }
            '=' => {
                if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::EqEq
                } else {
                    TokenKind::Eq
                }
            }
            '!' => {
                if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::BangEq
                } else {
                    TokenKind::Bang
                }
            }
            '<' => {
                if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::LtEq
                } else if self.peek_char() == Some('<') {
                    self.bump();
                    TokenKind::Shl
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.peek_char() == Some('=') {
                    self.bump();
                    TokenKind::GtEq
                } else if self.peek_char() == Some('>') {
                    self.bump();
                    TokenKind::Shr
                } else {
                    TokenKind::Gt
                }
            }
            '&' => {
                if self.peek_char() == Some('&') {
                    self.bump();
                    TokenKind::AmpAmp
                } else {
                    TokenKind::Ampersand
                }
            }
            '|' => {
                if self.peek_char() == Some('|') {
                    self.bump();
                    TokenKind::PipePipe
                } else {
                    TokenKind::Pipe
                }
            }
            '^' => TokenKind::Caret,
            '~' => TokenKind::Tilde,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ';' => TokenKind::Semicolon,
            ':' => {
                if self.peek_char() == Some(':') {
                    self.bump();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            ',' => TokenKind::Comma,
            '.' => TokenKind::Dot,
            _ => {
                diags.emit(
                    Diagnostic::error(format!("Unexpected character: '{}'", c))
                        .with_span(self.span(start_pos, end_pos)),
                );
                TokenKind::Error(c.to_string())
            }
        };

        Token::new(kind, self.span(start_pos, self.current_offset()))
    }

    /// Tokenize the entire source file.
    pub fn tokenize(&mut self, diags: &mut DiagnosticBag) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token(diags);
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_source::SourceMap;

    #[test]
    fn test_lex_basic_tokens() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "test.va".into(),
            "fn main() -> i64 { let mut x = 42; return x + 10; }".into(),
        );
        let file = sm.get_file(id).unwrap();
        let mut lexer = Lexer::new(&file);
        let mut diags = DiagnosticBag::new();
        let tokens = lexer.tokenize(&mut diags);

        assert!(!diags.has_errors());
        assert_eq!(tokens[0].kind, TokenKind::Fn);
        assert_eq!(tokens[1].kind, TokenKind::Ident("main".into()));
        assert_eq!(tokens[2].kind, TokenKind::LParen);
        assert_eq!(tokens[3].kind, TokenKind::RParen);
        assert_eq!(tokens[4].kind, TokenKind::Arrow);
        assert_eq!(tokens[5].kind, TokenKind::I64);
        assert_eq!(tokens[6].kind, TokenKind::LBrace);
        assert_eq!(tokens[7].kind, TokenKind::Let);
        assert_eq!(tokens[8].kind, TokenKind::Mut);
        assert_eq!(tokens[9].kind, TokenKind::Ident("x".into()));
        assert_eq!(tokens[10].kind, TokenKind::Eq);
        assert_eq!(tokens[11].kind, TokenKind::IntLiteral(42));
    }

    #[test]
    fn test_nested_comments() {
        let mut sm = SourceMap::new();
        let id = sm.add_file("comments.va".into(), "/* outer /* inner */ */ 123".into());
        let file = sm.get_file(id).unwrap();
        let mut lexer = Lexer::new(&file);
        let mut diags = DiagnosticBag::new();
        let tokens = lexer.tokenize(&mut diags);

        assert!(!diags.has_errors());
        assert_eq!(tokens[0].kind, TokenKind::IntLiteral(123));
    }
}
