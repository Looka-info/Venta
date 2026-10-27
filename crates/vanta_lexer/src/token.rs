//! Token definitions for the Vanta programming language.

use vanta_source::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Fn,
    Let,
    Mut,
    If,
    Else,
    While,
    Loop,
    Return,
    Break,
    Continue,
    Struct,
    Impl,
    Pub,
    As,
    Const,
    Unsafe,
    Enum,
    Match,
    Mod,
    Import,
    Trait,
    For,
    In,

    // Primitive Type names
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    BoolType,
    Str,

    // Literals
    IntLiteral(i64),
    FloatLiteral(f64),
    BoolLiteral(bool),
    StringLiteral(String),
    Ident(String),
    Lifetime(String),

    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Percent,

    // Bitwise / Logic
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    AmpAmp,
    PipePipe,
    Bang,

    // Comparison
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,

    // Assignment
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,

    // Delimiters & Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semicolon,
    Colon,
    ColonColon,
    Comma,
    Dot,
    Arrow,
    FatArrow,
    Underscore,
    Question,

    // Special
    Eof,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    Line,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub kind: CommentKind,
    pub span: Span,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}
