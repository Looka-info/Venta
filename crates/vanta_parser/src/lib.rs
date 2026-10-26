//! Recursive descent and precedence climbing parser for Vanta.

pub mod ast;

use ast::*;
use vanta_diagnostics::{codes, Diagnostic, DiagnosticBag};
use vanta_lexer::{Token, TokenKind};
use vanta_source::Span;

pub struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, cursor: 0 }
    }

    fn current(&self) -> &Token {
        if self.cursor < self.tokens.len() {
            &self.tokens[self.cursor]
        } else {
            self.tokens.last().expect("Tokens should not be empty")
        }
    }

    fn peek(&self) -> &TokenKind {
        &self.current().kind
    }

    fn current_span(&self) -> Span {
        self.current().span
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn bump(&mut self) -> Token {
        let tok = self.current().clone();
        if self.cursor < self.tokens.len() {
            self.cursor += 1;
        }
        tok
    }

    fn expect(&mut self, expected: TokenKind, diags: &mut DiagnosticBag) -> Option<Token> {
        if self.check(&expected) {
            Some(self.bump())
        } else {
            let code = if self.peek() == &TokenKind::Eof {
                codes::E0005_UNEXPECTED_EOF
            } else {
                codes::E0004_SYNTAX_ERROR
            };
            diags.emit(
                Diagnostic::error(format!(
                    "Expected '{:?}', found '{:?}'",
                    expected,
                    self.peek()
                ))
                .with_code(code)
                .with_span(self.current_span()),
            );
            None
        }
    }

    // --- High-level Parsing ---

    pub fn parse_program(&mut self, diags: &mut DiagnosticBag) -> Program {
        let mut items = Vec::new();
        while !self.check(&TokenKind::Eof) {
            if let Some(item) = self.parse_item(diags) {
                items.push(item);
            } else {
                // Recover to next synchronization point
                self.synchronize();
            }
        }
        Program { items }
    }

    fn synchronize(&mut self) {
        self.bump();
        while !self.check(&TokenKind::Eof) {
            if matches!(self.peek(), TokenKind::Fn | TokenKind::Struct) {
                break;
            }
            self.bump();
        }
    }

    fn parse_item(&mut self, diags: &mut DiagnosticBag) -> Option<Item> {
        let is_pub = if self.check(&TokenKind::Pub) {
            self.bump();
            true
        } else {
            false
        };

        match self.peek() {
            TokenKind::Fn => self.parse_fn(is_pub, diags).map(Item::Function),
            TokenKind::Struct => self.parse_struct(is_pub, diags).map(Item::Struct),
            TokenKind::Impl => self.parse_impl(diags).map(Item::Impl),
            other => {
                diags.emit(
                    Diagnostic::error(format!(
                        "Expected item (fn, struct, or impl), found {:?}",
                        other
                    ))
                    .with_code(codes::E0004_SYNTAX_ERROR)
                    .with_span(self.current_span()),
                );
                None
            }
        }
    }

    fn parse_impl(&mut self, diags: &mut DiagnosticBag) -> Option<ImplBlock> {
        let start_span = self.current_span();
        self.expect(TokenKind::Impl, diags)?;
        let target = self.parse_ident(diags)?;
        self.expect(TokenKind::LBrace, diags)?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let is_pub = if self.check(&TokenKind::Pub) {
                self.bump();
                true
            } else {
                false
            };
            if let Some(m) = self.parse_fn(is_pub, diags) {
                methods.push(m);
            } else {
                break;
            }
        }
        let end_tok = self.expect(TokenKind::RBrace, diags)?;

        Some(ImplBlock {
            target,
            methods,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_fn(&mut self, is_pub: bool, diags: &mut DiagnosticBag) -> Option<FnDecl> {
        let start_span = self.current_span();
        self.expect(TokenKind::Fn, diags)?;

        let name = self.parse_ident(diags)?;
        self.expect(TokenKind::LParen, diags)?;

        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
            let is_mut = if self.check(&TokenKind::Mut) {
                self.bump();
                true
            } else {
                false
            };
            let param_name = self.parse_ident(diags)?;
            self.expect(TokenKind::Colon, diags)?;
            let ty = self.parse_type(diags)?;

            let param_span = param_name.span.merge(ty.span);
            params.push(Param {
                is_mut,
                name: param_name,
                ty,
                span: param_span,
            });

            if self.check(&TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokenKind::RParen, diags)?;

        let ret_type = if self.check(&TokenKind::Arrow) {
            self.bump();
            Some(self.parse_type(diags)?)
        } else {
            None
        };

        let body = self.parse_block(diags)?;
        let end_span = body.span;

        Some(FnDecl {
            is_pub,
            name,
            params,
            ret_type,
            body,
            span: start_span.merge(end_span),
        })
    }

    fn parse_struct(&mut self, is_pub: bool, diags: &mut DiagnosticBag) -> Option<StructDecl> {
        let start_span = self.current_span();
        self.expect(TokenKind::Struct, diags)?;
        let name = self.parse_ident(diags)?;
        self.expect(TokenKind::LBrace, diags)?;

        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let f_pub = if self.check(&TokenKind::Pub) {
                self.bump();
                true
            } else {
                false
            };
            let f_name = self.parse_ident(diags)?;
            self.expect(TokenKind::Colon, diags)?;
            let f_type = self.parse_type(diags)?;
            let f_span = f_name.span.merge(f_type.span);

            fields.push(FieldDecl {
                is_pub: f_pub,
                name: f_name,
                ty: f_type,
                span: f_span,
            });

            if self.check(&TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        let end_tok = self.expect(TokenKind::RBrace, diags)?;

        Some(StructDecl {
            is_pub,
            name,
            fields,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_ident(&mut self, diags: &mut DiagnosticBag) -> Option<Ident> {
        match self.peek() {
            TokenKind::Ident(name) => {
                let name = name.clone();
                let span = self.current_span();
                self.bump();
                Some(Ident::new(name, span))
            }
            _ => {
                diags.emit(
                    Diagnostic::error(format!("Expected identifier, found {:?}", self.peek()))
                        .with_code(codes::E0004_SYNTAX_ERROR)
                        .with_span(self.current_span()),
                );
                None
            }
        }
    }

    // --- Type Parsing ---

    fn parse_type(&mut self, diags: &mut DiagnosticBag) -> Option<TypeExpr> {
        let start_span = self.current_span();
        match self.peek() {
            TokenKind::I8 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::I8),
                    span: start_span,
                })
            }
            TokenKind::I16 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::I16),
                    span: start_span,
                })
            }
            TokenKind::I32 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::I32),
                    span: start_span,
                })
            }
            TokenKind::I64 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::I64),
                    span: start_span,
                })
            }
            TokenKind::U8 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::U8),
                    span: start_span,
                })
            }
            TokenKind::U16 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::U16),
                    span: start_span,
                })
            }
            TokenKind::U32 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::U32),
                    span: start_span,
                })
            }
            TokenKind::U64 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::U64),
                    span: start_span,
                })
            }
            TokenKind::F32 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::F32),
                    span: start_span,
                })
            }
            TokenKind::F64 => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::F64),
                    span: start_span,
                })
            }
            TokenKind::BoolType => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::Bool),
                    span: start_span,
                })
            }
            TokenKind::Str => {
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Primitive(PrimType::Str),
                    span: start_span,
                })
            }
            TokenKind::Ampersand => {
                self.bump();
                let is_mut = if self.check(&TokenKind::Mut) {
                    self.bump();
                    true
                } else {
                    false
                };
                let inner = self.parse_type(diags)?;
                let span = start_span.merge(inner.span);
                Some(TypeExpr {
                    kind: TypeExprKind::Reference {
                        is_mut,
                        inner: Box::new(inner),
                    },
                    span,
                })
            }
            TokenKind::LBracket => {
                self.bump();
                let inner = self.parse_type(diags)?;
                if self.check(&TokenKind::RBracket) {
                    let end_tok = self.bump();
                    Some(TypeExpr {
                        kind: TypeExprKind::Slice {
                            inner: Box::new(inner),
                        },
                        span: start_span.merge(end_tok.span),
                    })
                } else {
                    self.expect(TokenKind::Semicolon, diags)?;
                    let len = match self.peek() {
                        TokenKind::IntLiteral(n) if *n >= 0 => {
                            let len = *n as usize;
                            self.bump();
                            len
                        }
                        _ => {
                            diags.emit(
                                Diagnostic::error(
                                    "Array length must be a non-negative integer literal",
                                )
                                .with_code(codes::E0004_SYNTAX_ERROR)
                                .with_span(self.current_span()),
                            );
                            0
                        }
                    };
                    let end_tok = self.expect(TokenKind::RBracket, diags)?;
                    Some(TypeExpr {
                        kind: TypeExprKind::Array {
                            inner: Box::new(inner),
                            len,
                        },
                        span: start_span.merge(end_tok.span),
                    })
                }
            }
            TokenKind::LParen => {
                self.bump();
                let end_tok = self.expect(TokenKind::RParen, diags)?;
                Some(TypeExpr {
                    kind: TypeExprKind::Unit,
                    span: start_span.merge(end_tok.span),
                })
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.bump();
                if name == "str" {
                    Some(TypeExpr {
                        kind: TypeExprKind::Primitive(PrimType::Str),
                        span: start_span,
                    })
                } else {
                    Some(TypeExpr {
                        kind: TypeExprKind::Named(name),
                        span: start_span,
                    })
                }
            }
            _ => {
                diags.emit(
                    Diagnostic::error(format!("Expected type, found {:?}", self.peek()))
                        .with_code(codes::E0004_SYNTAX_ERROR)
                        .with_span(self.current_span()),
                );
                None
            }
        }
    }

    // --- Statement & Block Parsing ---

    pub fn parse_block(&mut self, diags: &mut DiagnosticBag) -> Option<Block> {
        let start_span = self.current_span();
        self.expect(TokenKind::LBrace, diags)?;

        let mut stmts = Vec::new();
        let mut tail_expr = None;

        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            // Check for statement vs tail expression
            if self.check(&TokenKind::Let)
                || self.check(&TokenKind::While)
                || self.check(&TokenKind::Loop)
                || self.check(&TokenKind::Return)
                || self.check(&TokenKind::Break)
                || self.check(&TokenKind::Continue)
            {
                if let Some(stmt) = self.parse_stmt(diags) {
                    stmts.push(stmt);
                }
            } else {
                // Could be assignment, expression statement, or tail expression
                let expr = self.parse_expr(diags)?;

                // Check for assignment
                let assign_op = match self.peek() {
                    TokenKind::Eq => Some(AssignOp::Eq),
                    TokenKind::PlusEq => Some(AssignOp::PlusEq),
                    TokenKind::MinusEq => Some(AssignOp::MinusEq),
                    TokenKind::StarEq => Some(AssignOp::StarEq),
                    TokenKind::SlashEq => Some(AssignOp::SlashEq),
                    TokenKind::PercentEq => Some(AssignOp::PercentEq),
                    _ => None,
                };

                if let Some(op) = assign_op {
                    self.bump();
                    let val = self.parse_expr(diags)?;
                    let end_tok = self.expect(TokenKind::Semicolon, diags)?;
                    stmts.push(Stmt::Assign {
                        target: expr.clone(),
                        op,
                        value: val,
                        span: expr.span.merge(end_tok.span),
                    });
                } else if self.check(&TokenKind::Semicolon) {
                    self.bump();
                    stmts.push(Stmt::Expr(expr));
                } else if matches!(expr.kind, ExprKind::If { .. } | ExprKind::Block { .. }) {
                    if self.check(&TokenKind::Semicolon) {
                        self.bump();
                        stmts.push(Stmt::Expr(expr));
                    } else if self.check(&TokenKind::RBrace) {
                        tail_expr = Some(Box::new(expr));
                        break;
                    } else {
                        stmts.push(Stmt::Expr(expr));
                    }
                } else if self.check(&TokenKind::RBrace) {
                    // Tail expression of the block
                    tail_expr = Some(Box::new(expr));
                    break;
                } else {
                    diags.emit(
                        Diagnostic::error("Expected ';' after expression")
                            .with_code(codes::E0004_SYNTAX_ERROR)
                            .with_suggestion(
                                "add ';' to terminate statement",
                                Some(self.current_span()),
                                ";",
                            )
                            .with_span(self.current_span()),
                    );
                    break;
                }
            }
        }

        let end_tok = self.expect(TokenKind::RBrace, diags)?;

        Some(Block {
            stmts,
            expr: tail_expr,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_stmt(&mut self, diags: &mut DiagnosticBag) -> Option<Stmt> {
        match self.peek() {
            TokenKind::Let => {
                let start_span = self.current_span();
                self.bump();
                let is_mut = if self.check(&TokenKind::Mut) {
                    self.bump();
                    true
                } else {
                    false
                };
                let name = self.parse_ident(diags)?;
                let ty = if self.check(&TokenKind::Colon) {
                    self.bump();
                    Some(self.parse_type(diags)?)
                } else {
                    None
                };
                let init = if self.check(&TokenKind::Eq) {
                    self.bump();
                    Some(self.parse_expr(diags)?)
                } else {
                    None
                };
                let semi = self.expect(TokenKind::Semicolon, diags)?;
                Some(Stmt::Let {
                    is_mut,
                    name,
                    ty,
                    init,
                    span: start_span.merge(semi.span),
                })
            }
            TokenKind::While => {
                let start_span = self.current_span();
                self.bump();
                let cond = self.parse_expr(diags)?;
                let body = self.parse_block(diags)?;
                let span = start_span.merge(body.span);
                Some(Stmt::While { cond, body, span })
            }
            TokenKind::Loop => {
                let start_span = self.current_span();
                self.bump();
                let body = self.parse_block(diags)?;
                let span = start_span.merge(body.span);
                Some(Stmt::Loop { body, span })
            }
            TokenKind::Return => {
                let start_span = self.current_span();
                self.bump();
                let value = if !self.check(&TokenKind::Semicolon) {
                    Some(self.parse_expr(diags)?)
                } else {
                    None
                };
                let semi = self.expect(TokenKind::Semicolon, diags)?;
                Some(Stmt::Return {
                    value,
                    span: start_span.merge(semi.span),
                })
            }
            TokenKind::Break => {
                let start_span = self.current_span();
                self.bump();
                let semi = self.expect(TokenKind::Semicolon, diags)?;
                Some(Stmt::Break {
                    span: start_span.merge(semi.span),
                })
            }
            TokenKind::Continue => {
                let start_span = self.current_span();
                self.bump();
                let semi = self.expect(TokenKind::Semicolon, diags)?;
                Some(Stmt::Continue {
                    span: start_span.merge(semi.span),
                })
            }
            _ => None,
        }
    }

    // --- Expression Parsing (Precedence Climbing) ---

    pub fn parse_expr(&mut self, diags: &mut DiagnosticBag) -> Option<Expr> {
        self.parse_expr_precedence(0, diags)
    }

    fn get_infix_binding_power(op: &TokenKind) -> Option<(u8, u8, BinaryOp)> {
        match op {
            TokenKind::PipePipe => Some((1, 2, BinaryOp::Or)),
            TokenKind::AmpAmp => Some((3, 4, BinaryOp::And)),
            TokenKind::Pipe => Some((5, 6, BinaryOp::BitOr)),
            TokenKind::Caret => Some((7, 8, BinaryOp::BitXor)),
            TokenKind::Ampersand => Some((9, 10, BinaryOp::BitAnd)),
            TokenKind::EqEq => Some((11, 12, BinaryOp::Eq)),
            TokenKind::BangEq => Some((11, 12, BinaryOp::NotEq)),
            TokenKind::Lt => Some((13, 14, BinaryOp::Lt)),
            TokenKind::LtEq => Some((13, 14, BinaryOp::LtEq)),
            TokenKind::Gt => Some((13, 14, BinaryOp::Gt)),
            TokenKind::GtEq => Some((13, 14, BinaryOp::GtEq)),
            TokenKind::Shl => Some((15, 16, BinaryOp::Shl)),
            TokenKind::Shr => Some((15, 16, BinaryOp::Shr)),
            TokenKind::Plus => Some((17, 18, BinaryOp::Add)),
            TokenKind::Minus => Some((17, 18, BinaryOp::Sub)),
            TokenKind::Star => Some((19, 20, BinaryOp::Mul)),
            TokenKind::Slash => Some((19, 20, BinaryOp::Div)),
            TokenKind::Percent => Some((19, 20, BinaryOp::Rem)),
            _ => None,
        }
    }

    fn parse_expr_precedence(&mut self, min_bp: u8, diags: &mut DiagnosticBag) -> Option<Expr> {
        let mut lhs = self.parse_prefix_expr(diags)?;

        while let Some((l_bp, r_bp, bin_op)) = Self::get_infix_binding_power(self.peek()) {
            if l_bp < min_bp {
                break;
            }

            self.bump();
            let rhs = self.parse_expr_precedence(r_bp, diags)?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr::new(
                ExprKind::Binary {
                    op: bin_op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Some(lhs)
    }

    fn parse_prefix_expr(&mut self, diags: &mut DiagnosticBag) -> Option<Expr> {
        let start_span = self.current_span();
        match self.peek() {
            TokenKind::Minus => {
                self.bump();
                let inner = self.parse_prefix_expr(diags)?;
                let span = start_span.merge(inner.span);
                Some(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::Neg,
                        expr: Box::new(inner),
                    },
                    span,
                ))
            }
            TokenKind::Bang => {
                self.bump();
                let inner = self.parse_prefix_expr(diags)?;
                let span = start_span.merge(inner.span);
                Some(Expr::new(
                    ExprKind::Unary {
                        op: UnaryOp::Not,
                        expr: Box::new(inner),
                    },
                    span,
                ))
            }
            TokenKind::Star => {
                self.bump();
                let inner = self.parse_prefix_expr(diags)?;
                let span = start_span.merge(inner.span);
                Some(Expr::new(ExprKind::Deref(Box::new(inner)), span))
            }
            TokenKind::Ampersand => {
                self.bump();
                let is_mut = if self.check(&TokenKind::Mut) {
                    self.bump();
                    true
                } else {
                    false
                };
                let inner = self.parse_prefix_expr(diags)?;
                let span = start_span.merge(inner.span);
                Some(Expr::new(
                    ExprKind::Ref {
                        is_mut,
                        expr: Box::new(inner),
                    },
                    span,
                ))
            }
            _ => self.parse_postfix_expr(diags),
        }
    }

    fn parse_postfix_expr(&mut self, diags: &mut DiagnosticBag) -> Option<Expr> {
        let mut expr = self.parse_primary_expr(diags)?;

        loop {
            match self.peek() {
                TokenKind::LParen => {
                    // Function Call
                    self.bump();
                    let mut args = Vec::new();
                    while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                        args.push(self.parse_expr(diags)?);
                        if self.check(&TokenKind::Comma) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let end_tok = self.expect(TokenKind::RParen, diags)?;
                    let span = expr.span.merge(end_tok.span);
                    let callee_name = match expr.kind {
                        ExprKind::Ident(name) => name,
                        _ => {
                            diags.emit(
                                Diagnostic::error("Callee must be a function name")
                                    .with_code(codes::E0004_SYNTAX_ERROR)
                                    .with_span(expr.span),
                            );
                            "<error>".into()
                        }
                    };
                    expr = Expr::new(
                        ExprKind::Call {
                            callee: callee_name,
                            args,
                        },
                        span,
                    );
                }
                TokenKind::Dot => {
                    self.bump();
                    let field = self.parse_ident(diags)?;
                    if self.check(&TokenKind::LParen) {
                        self.bump();
                        let mut args = Vec::new();
                        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                            args.push(self.parse_expr(diags)?);
                            if self.check(&TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                        let end_tok = self.expect(TokenKind::RParen, diags)?;
                        let span = expr.span.merge(end_tok.span);
                        expr = Expr::new(
                            ExprKind::MethodCall {
                                target: Box::new(expr),
                                method: field,
                                args,
                            },
                            span,
                        );
                    } else {
                        let span = expr.span.merge(field.span);
                        expr = Expr::new(
                            ExprKind::FieldAccess {
                                target: Box::new(expr),
                                field,
                            },
                            span,
                        );
                    }
                }
                TokenKind::LBracket => {
                    // Index Access
                    self.bump();
                    let index = self.parse_expr(diags)?;
                    let end_tok = self.expect(TokenKind::RBracket, diags)?;
                    let span = expr.span.merge(end_tok.span);
                    expr = Expr::new(
                        ExprKind::Index {
                            array: Box::new(expr),
                            index: Box::new(index),
                        },
                        span,
                    );
                }
                _ => break,
            }
        }

        Some(expr)
    }

    fn parse_primary_expr(&mut self, diags: &mut DiagnosticBag) -> Option<Expr> {
        let start_span = self.current_span();
        match self.peek() {
            TokenKind::IntLiteral(val) => {
                let val = *val;
                self.bump();
                Some(Expr::new(ExprKind::Int(val), start_span))
            }
            TokenKind::BoolLiteral(b) => {
                let b = *b;
                self.bump();
                Some(Expr::new(ExprKind::Bool(b), start_span))
            }
            TokenKind::StringLiteral(s) => {
                let s = s.clone();
                self.bump();
                Some(Expr::new(ExprKind::String(s), start_span))
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.bump();

                // Check if this is a struct literal: Name { field: expr, ... }
                let is_capitalized = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                let looks_like_struct_init = if is_capitalized && self.check(&TokenKind::LBrace) {
                    let next1 = self.tokens.get(self.cursor + 1).map(|t| &t.kind);
                    let next2 = self.tokens.get(self.cursor + 2).map(|t| &t.kind);
                    matches!(next1, Some(TokenKind::RBrace))
                        || (matches!(next1, Some(TokenKind::Ident(_)))
                            && matches!(next2, Some(TokenKind::Colon)))
                } else {
                    false
                };

                if looks_like_struct_init {
                    self.bump(); // consume '{'
                    let mut fields = Vec::new();
                    let mut is_struct = true;
                    while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                        let f_name = match self.parse_ident(diags) {
                            Some(id) => id,
                            None => {
                                is_struct = false;
                                break;
                            }
                        };
                        if self.expect(TokenKind::Colon, diags).is_none() {
                            is_struct = false;
                            break;
                        }
                        let f_val = match self.parse_expr(diags) {
                            Some(e) => e,
                            None => {
                                is_struct = false;
                                break;
                            }
                        };
                        fields.push((f_name, f_val));
                        if self.check(&TokenKind::Comma) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    if is_struct {
                        let end_tok = self.expect(TokenKind::RBrace, diags)?;
                        return Some(Expr::new(
                            ExprKind::StructLiteral {
                                name: Ident::new(name, start_span),
                                fields,
                            },
                            start_span.merge(end_tok.span),
                        ));
                    }
                }
                Some(Expr::new(ExprKind::Ident(name), start_span))
            }
            TokenKind::If => {
                self.bump();
                let cond = self.parse_expr(diags)?;
                let then_branch = self.parse_block(diags)?;
                let (else_branch, end_span) = if self.check(&TokenKind::Else) {
                    self.bump();
                    if self.check(&TokenKind::If) {
                        let if_expr = self.parse_expr(diags)?;
                        let span = if_expr.span;
                        (Some(ElseBranch::If(Box::new(if_expr))), span)
                    } else {
                        let else_block = self.parse_block(diags)?;
                        let span = else_block.span;
                        (Some(ElseBranch::Block(else_block)), span)
                    }
                } else {
                    let span = then_branch.span;
                    (None, span)
                };
                let span = start_span.merge(end_span);
                Some(Expr::new(
                    ExprKind::If {
                        cond: Box::new(cond),
                        then_branch,
                        else_branch,
                    },
                    span,
                ))
            }
            TokenKind::LBrace => {
                let block = self.parse_block(diags)?;
                let span = block.span;
                Some(Expr::new(ExprKind::Block(block), span))
            }
            TokenKind::LBracket => {
                self.bump();
                let mut elements = Vec::new();
                while !self.check(&TokenKind::RBracket) && !self.check(&TokenKind::Eof) {
                    elements.push(self.parse_expr(diags)?);
                    if self.check(&TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                let end_tok = self.expect(TokenKind::RBracket, diags)?;
                Some(Expr::new(
                    ExprKind::ArrayLiteral(elements),
                    start_span.merge(end_tok.span),
                ))
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.parse_expr(diags)?;
                let end_tok = self.expect(TokenKind::RParen, diags)?;
                Some(Expr::new(inner.kind, start_span.merge(end_tok.span)))
            }
            _ => {
                diags.emit(
                    Diagnostic::error(format!("Expected expression, found {:?}", self.peek()))
                        .with_code(codes::E0004_SYNTAX_ERROR)
                        .with_span(start_span),
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_lexer::Lexer;
    use vanta_source::SourceMap;

    #[test]
    fn test_parse_fn_declaration() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "add.va".into(),
            "pub fn add(a: i64, b: i64) -> i64 { return a + b; }".into(),
        );
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        assert!(!diags.has_errors());

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        assert!(!diags.has_errors());
        assert_eq!(program.items.len(), 1);

        match &program.items[0] {
            Item::Function(f) => {
                assert!(f.is_pub);
                assert_eq!(f.name.name, "add");
                assert_eq!(f.params.len(), 2);
                assert_eq!(f.params[0].name.name, "a");
                assert_eq!(f.params[1].name.name, "b");
            }
            _ => panic!("Expected function"),
        }
    }
}
