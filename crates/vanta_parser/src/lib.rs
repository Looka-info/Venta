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
            TokenKind::Enum => self.parse_enum(is_pub, diags).map(Item::Enum),
            TokenKind::Trait => self.parse_trait(is_pub, diags).map(Item::Trait),
            TokenKind::Impl => self.parse_impl(diags).map(Item::Impl),
            TokenKind::Mod => self.parse_mod(is_pub, diags).map(Item::Mod),
            TokenKind::Import => self.parse_import(is_pub, diags).map(Item::Import),
            other => {
                diags.emit(
                    Diagnostic::error(format!(
                        "Expected item (fn, struct, enum, trait, impl, mod, or import), found {:?}",
                        other
                    ))
                    .with_code(codes::E0004_SYNTAX_ERROR)
                    .with_span(self.current_span()),
                );
                None
            }
        }
    }

    fn parse_path_ident(&mut self, diags: &mut DiagnosticBag) -> Option<Ident> {
        let mut id = self.parse_ident(diags)?;
        while self.check(&TokenKind::ColonColon) {
            self.bump();
            let next_id = self.parse_ident(diags)?;
            id = Ident::new(
                format!("{}::{}", id.name, next_id.name),
                id.span.merge(next_id.span),
            );
        }
        Some(id)
    }

    fn parse_trait(&mut self, is_pub: bool, diags: &mut DiagnosticBag) -> Option<TraitDecl> {
        let start_span = self.current_span();
        self.expect(TokenKind::Trait, diags)?;
        let name = self.parse_ident(diags)?;
        let (lifetime_params, type_params) = if self.check(&TokenKind::Lt) {
            self.parse_generic_params(diags)?
        } else {
            (Vec::new(), Vec::new())
        };
        self.expect(TokenKind::LBrace, diags)?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            if let Some(m) = self.parse_trait_method(diags) {
                methods.push(m);
            } else {
                break;
            }
        }
        let end_tok = self.expect(TokenKind::RBrace, diags)?;

        Some(TraitDecl {
            is_pub,
            name,
            lifetime_params,
            type_params,
            methods,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_trait_method(&mut self, diags: &mut DiagnosticBag) -> Option<TraitMethodDecl> {
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
        let end_tok = self.expect(TokenKind::Semicolon, diags)?;

        Some(TraitMethodDecl {
            name,
            params,
            ret_type,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_mod(&mut self, is_pub: bool, diags: &mut DiagnosticBag) -> Option<ModDecl> {
        let start_span = self.current_span();
        self.expect(TokenKind::Mod, diags)?;
        let name = self.parse_ident(diags)?;
        let end_tok = self.expect(TokenKind::Semicolon, diags)?;
        Some(ModDecl {
            is_pub,
            name,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_import(&mut self, is_pub: bool, diags: &mut DiagnosticBag) -> Option<ImportDecl> {
        let start_span = self.current_span();
        self.expect(TokenKind::Import, diags)?;

        let mut path = Vec::new();
        let mut items = Vec::new();

        let first = self.parse_ident(diags)?;
        if self.check(&TokenKind::ColonColon) {
            path.push(first);
            while self.check(&TokenKind::ColonColon) {
                self.bump(); // consume '::'
                if self.check(&TokenKind::LBrace) {
                    self.bump(); // consume '{'
                    while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
                        items.push(self.parse_ident(diags)?);
                        if self.check(&TokenKind::Comma) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    self.expect(TokenKind::RBrace, diags)?;
                    break;
                } else {
                    let next = self.parse_ident(diags)?;
                    if self.check(&TokenKind::ColonColon) {
                        path.push(next);
                    } else {
                        items.push(next);
                        break;
                    }
                }
            }
        } else {
            items.push(first);
        }

        let end_tok = self.expect(TokenKind::Semicolon, diags)?;
        Some(ImportDecl {
            is_pub,
            path,
            items,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_type_args(&mut self, diags: &mut DiagnosticBag) -> Option<Vec<TypeExpr>> {
        self.expect(TokenKind::Lt, diags)?;
        let mut type_args = Vec::new();
        while !self.check(&TokenKind::Gt) && !self.check(&TokenKind::Eof) {
            let arg = self.parse_type(diags)?;
            type_args.push(arg);
            if self.check(&TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokenKind::Gt, diags)?;
        Some(type_args)
    }

    fn parse_impl(&mut self, diags: &mut DiagnosticBag) -> Option<ImplBlock> {
        let start_span = self.current_span();
        self.expect(TokenKind::Impl, diags)?;
        let (lifetime_params, type_params) = if self.check(&TokenKind::Lt) {
            self.parse_generic_params(diags)?
        } else {
            (Vec::new(), Vec::new())
        };
        let first_id = self.parse_path_ident(diags)?;
        let first_args = if self.check(&TokenKind::Lt) {
            self.parse_type_args(diags)?
        } else {
            Vec::new()
        };

        let (trait_name, trait_args, target, target_args) = if self.check(&TokenKind::For) {
            self.bump(); // consume 'for'
            let target_id = self.parse_path_ident(diags)?;
            let target_args = if self.check(&TokenKind::Lt) {
                self.parse_type_args(diags)?
            } else {
                Vec::new()
            };
            (Some(first_id), first_args, target_id, target_args)
        } else {
            (None, Vec::new(), first_id, first_args)
        };

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
            lifetime_params,
            type_params,
            trait_name,
            trait_args,
            target,
            target_args,
            methods,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_generic_params(
        &mut self,
        diags: &mut DiagnosticBag,
    ) -> Option<(Vec<Ident>, Vec<TypeParam>)> {
        self.expect(TokenKind::Lt, diags)?;
        let mut lifetime_params = Vec::new();
        let mut type_params = Vec::new();
        while !self.check(&TokenKind::Gt) && !self.check(&TokenKind::Eof) {
            if let TokenKind::Lifetime(lt_name) = self.peek() {
                let lt_name = lt_name.clone();
                let tok = self.bump();
                lifetime_params.push(Ident::new(lt_name, tok.span));
            } else {
                let id = self.parse_ident(diags)?;
                let bound = if self.check(&TokenKind::Colon) {
                    self.bump();
                    Some(self.parse_path_ident(diags)?)
                } else {
                    None
                };
                let span = if let Some(ref b) = bound {
                    id.span.merge(b.span)
                } else {
                    id.span
                };
                type_params.push(TypeParam {
                    name: id,
                    bound,
                    span,
                });
            }
            if self.check(&TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokenKind::Gt, diags)?;
        Some((lifetime_params, type_params))
    }

    fn parse_fn(&mut self, is_pub: bool, diags: &mut DiagnosticBag) -> Option<FnDecl> {
        let start_span = self.current_span();
        self.expect(TokenKind::Fn, diags)?;

        let name = self.parse_ident(diags)?;
        let (lifetime_params, type_params) = if self.check(&TokenKind::Lt) {
            self.parse_generic_params(diags)?
        } else {
            (Vec::new(), Vec::new())
        };
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
            lifetime_params,
            type_params,
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
        let (lifetime_params, type_params) = if self.check(&TokenKind::Lt) {
            self.parse_generic_params(diags)?
        } else {
            (Vec::new(), Vec::new())
        };
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
            lifetime_params,
            type_params,
            fields,
            span: start_span.merge(end_tok.span),
        })
    }

    fn parse_enum(&mut self, is_pub: bool, diags: &mut DiagnosticBag) -> Option<EnumDecl> {
        let start_span = self.current_span();
        self.expect(TokenKind::Enum, diags)?;
        let name = self.parse_ident(diags)?;
        let (lifetime_params, type_params) = if self.check(&TokenKind::Lt) {
            self.parse_generic_params(diags)?
        } else {
            (Vec::new(), Vec::new())
        };
        self.expect(TokenKind::LBrace, diags)?;

        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let v_span = self.current_span();
            let v_name = self.parse_ident(diags)?;
            let mut payload = Vec::new();
            if self.check(&TokenKind::LParen) {
                self.bump();
                while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                    let ty = self.parse_type(diags)?;
                    payload.push(ty);
                    if self.check(&TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                let end_paren = self.expect(TokenKind::RParen, diags)?;
                variants.push(VariantDecl {
                    name: v_name,
                    payload,
                    span: v_span.merge(end_paren.span),
                });
            } else {
                variants.push(VariantDecl {
                    name: v_name,
                    payload,
                    span: v_span,
                });
            }

            if self.check(&TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        let end_tok = self.expect(TokenKind::RBrace, diags)?;

        Some(EnumDecl {
            is_pub,
            name,
            lifetime_params,
            type_params,
            variants,
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
            TokenKind::Lifetime(name) => {
                let name = name.clone();
                let span = self.current_span();
                self.bump();
                Some(TypeExpr {
                    kind: TypeExprKind::Lifetime(Ident::new(name, span)),
                    span,
                })
            }
            TokenKind::Ampersand => {
                self.bump();
                let lifetime = if let TokenKind::Lifetime(name) = self.peek() {
                    let name = name.clone();
                    let tok = self.bump();
                    Some(Ident::new(name, tok.span))
                } else {
                    None
                };
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
                        lifetime,
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
                            lifetime: None,
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
                let mut name = name.clone();
                self.bump();
                while self.check(&TokenKind::ColonColon) {
                    self.bump();
                    let next_id = self.parse_ident(diags)?;
                    name.push_str("::");
                    name.push_str(&next_id.name);
                }
                if name == "str" {
                    Some(TypeExpr {
                        kind: TypeExprKind::Primitive(PrimType::Str),
                        span: start_span,
                    })
                } else if self.check(&TokenKind::Lt) {
                    self.bump();
                    let mut type_args = Vec::new();
                    while !self.check(&TokenKind::Gt) && !self.check(&TokenKind::Eof) {
                        let arg = self.parse_type(diags)?;
                        type_args.push(arg);
                        if self.check(&TokenKind::Comma) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let end_tok = self.expect(TokenKind::Gt, diags)?;
                    Some(TypeExpr {
                        kind: TypeExprKind::Generic { name, type_args },
                        span: start_span.merge(end_tok.span),
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
                || self.check(&TokenKind::For)
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
                } else if matches!(
                    expr.kind,
                    ExprKind::If { .. } | ExprKind::Block { .. } | ExprKind::Match { .. }
                ) {
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
            TokenKind::For => {
                let start_span = self.current_span();
                self.bump();
                let var = self.parse_ident(diags)?;
                self.expect(TokenKind::In, diags)?;
                let iter = self.parse_expr(diags)?;
                let body = self.parse_block(diags)?;
                let span = start_span.merge(body.span);
                Some(Stmt::For {
                    var,
                    iter,
                    body,
                    span,
                })
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
                            type_args: Vec::new(),
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
                TokenKind::Question => {
                    let q_tok = self.bump();
                    let span = expr.span.merge(q_tok.span);
                    expr = Expr::new(ExprKind::Try(Box::new(expr)), span);
                }
                _ => break,
            }
        }

        Some(expr)
    }

    fn parse_pattern(&mut self, diags: &mut DiagnosticBag) -> Option<Pattern> {
        let start_span = self.current_span();
        match self.peek() {
            TokenKind::Underscore => {
                self.bump();
                Some(Pattern::new(PatternKind::Wildcard, start_span))
            }
            TokenKind::BoolLiteral(b) => {
                let b = *b;
                self.bump();
                Some(Pattern::new(PatternKind::Bool(b), start_span))
            }
            TokenKind::IntLiteral(n) => {
                let n = *n;
                self.bump();
                Some(Pattern::new(PatternKind::Int(n), start_span))
            }
            TokenKind::Minus => {
                self.bump();
                match self.peek() {
                    TokenKind::IntLiteral(n) => {
                        let n = *n;
                        let end_span = self.current_span();
                        self.bump();
                        Some(Pattern::new(
                            PatternKind::Int(-n),
                            start_span.merge(end_span),
                        ))
                    }
                    _ => {
                        diags.emit(
                            Diagnostic::error("Expected integer literal after '-' in pattern")
                                .with_code(codes::E0004_SYNTAX_ERROR)
                                .with_span(self.current_span()),
                        );
                        None
                    }
                }
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.parse_pattern(diags)?;
                let end_tok = self.expect(TokenKind::RParen, diags)?;
                Some(Pattern::new(inner.kind, start_span.merge(end_tok.span)))
            }
            TokenKind::Ident(_) => {
                let mut path = vec![self.parse_ident(diags)?];
                while self.check(&TokenKind::ColonColon) {
                    self.bump(); // consume '::'
                    path.push(self.parse_ident(diags)?);
                }
                if path.len() > 1 {
                    let id2 = path.pop().unwrap();
                    let prefix_span = path[0].span.merge(path.last().unwrap().span);
                    let prefix_name = path
                        .into_iter()
                        .map(|id| id.name)
                        .collect::<Vec<_>>()
                        .join("::");
                    let id1 = Ident::new(prefix_name, prefix_span);
                    let mut subpatterns = Vec::new();
                    if self.check(&TokenKind::LParen) {
                        self.bump();
                        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                            subpatterns.push(self.parse_pattern(diags)?);
                            if self.check(&TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                        let end_tok = self.expect(TokenKind::RParen, diags)?;
                        Some(Pattern::new(
                            PatternKind::Variant {
                                enum_name: Some(id1),
                                variant: id2,
                                subpatterns,
                            },
                            start_span.merge(end_tok.span),
                        ))
                    } else {
                        let span = id1.span.merge(id2.span);
                        Some(Pattern::new(
                            PatternKind::Variant {
                                enum_name: Some(id1),
                                variant: id2,
                                subpatterns,
                            },
                            span,
                        ))
                    }
                } else {
                    let id1 = path.pop().unwrap();
                    if self.check(&TokenKind::LParen) {
                        self.bump();
                        let mut subpatterns = Vec::new();
                        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Eof) {
                            subpatterns.push(self.parse_pattern(diags)?);
                            if self.check(&TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                        let end_tok = self.expect(TokenKind::RParen, diags)?;
                        Some(Pattern::new(
                            PatternKind::Variant {
                                enum_name: None,
                                variant: id1,
                                subpatterns,
                            },
                            start_span.merge(end_tok.span),
                        ))
                    } else {
                        Some(Pattern::new(PatternKind::Ident(id1.name), id1.span))
                    }
                }
            }
            other => {
                diags.emit(
                    Diagnostic::error(format!("Expected pattern, found {:?}", other))
                        .with_code(codes::E0004_SYNTAX_ERROR)
                        .with_span(start_span),
                );
                None
            }
        }
    }

    fn parse_match_expr(&mut self, diags: &mut DiagnosticBag) -> Option<Expr> {
        let start_span = self.current_span();
        self.expect(TokenKind::Match, diags)?;
        let scrutinee = self.parse_expr(diags)?;
        self.expect(TokenKind::LBrace, diags)?;

        let mut arms = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let arm_span = self.current_span();
            let pattern = self.parse_pattern(diags)?;
            self.expect(TokenKind::FatArrow, diags)?;
            let body = self.parse_expr(diags)?;
            let arm_end = body.span;
            arms.push(MatchArm {
                pattern,
                body: body.clone(),
                span: arm_span.merge(arm_end),
            });
            if self.check(&TokenKind::Comma) {
                self.bump();
            } else if matches!(body.kind, ExprKind::Block { .. }) {
                if self.check(&TokenKind::Comma) {
                    self.bump();
                }
            } else if !self.check(&TokenKind::RBrace) {
                diags.emit(
                    Diagnostic::error("Expected ',' or '}' after match arm")
                        .with_code(codes::E0004_SYNTAX_ERROR)
                        .with_span(self.current_span()),
                );
                break;
            }
        }
        let end_tok = self.expect(TokenKind::RBrace, diags)?;
        Some(Expr::new(
            ExprKind::Match {
                expr: Box::new(scrutinee),
                arms,
            },
            start_span.merge(end_tok.span),
        ))
    }

    fn parse_primary_expr(&mut self, diags: &mut DiagnosticBag) -> Option<Expr> {
        let start_span = self.current_span();
        match self.peek() {
            TokenKind::Match => self.parse_match_expr(diags),
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

                let mut type_args = Vec::new();
                if self.check(&TokenKind::Lt) {
                    let saved = self.cursor;
                    self.bump(); // consume '<'
                    let mut args = Vec::new();
                    let mut ok = true;
                    let mut scratch_diags = DiagnosticBag::new();
                    while !self.check(&TokenKind::Gt) && !self.check(&TokenKind::Eof) {
                        if let Some(arg) = self.parse_type(&mut scratch_diags) {
                            args.push(arg);
                            if self.check(&TokenKind::Comma) {
                                self.bump();
                            } else {
                                break;
                            }
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if ok && !scratch_diags.has_errors() && self.check(&TokenKind::Gt) {
                        self.bump(); // consume '>'
                        if self.check(&TokenKind::LBrace)
                            || self.check(&TokenKind::LParen)
                            || self.check(&TokenKind::ColonColon)
                        {
                            type_args = args;
                        } else {
                            self.cursor = saved;
                        }
                    } else {
                        self.cursor = saved;
                    }
                }

                // Check for associated call, qualified function call, or variant constructor
                if self.check(&TokenKind::ColonColon) {
                    let mut path_segments = vec![Ident::new(name.clone(), start_span)];
                    while self.check(&TokenKind::ColonColon) {
                        self.bump(); // consume '::'
                        path_segments.push(self.parse_ident(diags)?);
                    }
                    let last = path_segments.pop().unwrap();
                    let prefix_span = path_segments[0]
                        .span
                        .merge(path_segments.last().unwrap().span);
                    let prefix_name = path_segments
                        .into_iter()
                        .map(|id| id.name)
                        .collect::<Vec<_>>()
                        .join("::");
                    let prefix_ident = Ident::new(prefix_name.clone(), prefix_span);
                    let full_qualified = format!("{}::{}", prefix_name, last.name);

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
                        let span = start_span.merge(end_tok.span);
                        if last
                            .name
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_uppercase())
                        {
                            return Some(Expr::new(
                                ExprKind::VariantConstructor {
                                    enum_name: Some(prefix_ident),
                                    variant: last,
                                    type_args,
                                    args,
                                },
                                span,
                            ));
                        } else {
                            return Some(Expr::new(
                                ExprKind::Call {
                                    callee: full_qualified,
                                    type_args,
                                    args,
                                },
                                span,
                            ));
                        }
                    } else if (last
                        .name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase())
                        || last.name == "v256"
                        || last.name == "v512"
                        || last.name == "v128")
                        && self.check(&TokenKind::LBrace)
                    {
                        // Qualified struct literal: models::Point { x: 10, y: 20 }
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
                            if !self.check(&TokenKind::Colon) {
                                is_struct = false;
                                break;
                            }
                            self.bump(); // consume ':'
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
                        if is_struct && self.check(&TokenKind::RBrace) {
                            let end_tok = self.expect(TokenKind::RBrace, diags)?;
                            return Some(Expr::new(
                                ExprKind::StructLiteral {
                                    name: Ident::new(full_qualified, start_span.merge(last.span)),
                                    type_args,
                                    fields,
                                },
                                start_span.merge(end_tok.span),
                            ));
                        }
                        return None;
                    } else {
                        // Unit variant constructor or path
                        let span = start_span.merge(last.span);
                        return Some(Expr::new(
                            ExprKind::VariantConstructor {
                                enum_name: Some(prefix_ident),
                                variant: last,
                                type_args,
                                args: Vec::new(),
                            },
                            span,
                        ));
                    }
                }

                // Explicit generic call: e.g. identity<i64>(42)
                if self.check(&TokenKind::LParen) && !type_args.is_empty() {
                    self.bump(); // consume '('
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
                    let span = start_span.merge(end_tok.span);
                    return Some(Expr::new(
                        ExprKind::Call {
                            callee: name,
                            type_args,
                            args,
                        },
                        span,
                    ));
                }

                // Check if this is a struct literal: Name { field: expr, ... } or Name<T> { ... }
                let is_capitalized = name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    || name == "v256"
                    || name == "v512"
                    || name == "v128";
                let looks_like_struct_init = if (is_capitalized || !type_args.is_empty())
                    && self.check(&TokenKind::LBrace)
                {
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
                                type_args,
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

    #[test]
    fn test_parse_enum_declaration_and_match() {
        let mut sm = SourceMap::new();
        let src = r#"
        pub enum Message<T> {
            Quit,
            Move(i64, i64),
            Data(T),
        }

        pub fn handle(m: Message<i64>) -> i64 {
            return match m {
                Message::Quit => 0,
                Message::Move(x, y) => x + y,
                Message::Data(val) => val,
                _ => -1,
            };
        }
        "#;
        let id = sm.add_file("enum.va".into(), src.into());
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        assert!(!diags.has_errors());

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        assert!(!diags.has_errors());
        assert_eq!(program.items.len(), 2);

        match &program.items[0] {
            Item::Enum(e) => {
                assert!(e.is_pub);
                assert_eq!(e.name.name, "Message");
                assert_eq!(e.type_params.len(), 1);
                assert_eq!(e.variants.len(), 3);
                assert_eq!(e.variants[0].name.name, "Quit");
                assert!(e.variants[0].payload.is_empty());
                assert_eq!(e.variants[1].name.name, "Move");
                assert_eq!(e.variants[1].payload.len(), 2);
                assert_eq!(e.variants[2].name.name, "Data");
                assert_eq!(e.variants[2].payload.len(), 1);
            }
            _ => panic!("Expected enum item"),
        }
    }

    #[test]
    fn test_parse_mod_and_import() {
        let mut sm = SourceMap::new();
        let src = r#"
        mod math;
        pub mod models;

        import math::{add, multiply};
        import models::Point;
        pub import geometry::distance;

        pub fn main() -> i64 {
            let p: models::Point = models::Point { x: 10, y: 20 };
            return add(p.x, p.y);
        }
        "#;
        let id = sm.add_file("main.va".into(), src.into());
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        assert!(!diags.has_errors());

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        assert!(!diags.has_errors());
        assert_eq!(program.items.len(), 6);

        match &program.items[0] {
            Item::Mod(m) => {
                assert!(!m.is_pub);
                assert_eq!(m.name.name, "math");
            }
            _ => panic!("Expected mod item"),
        }

        match &program.items[1] {
            Item::Mod(m) => {
                assert!(m.is_pub);
                assert_eq!(m.name.name, "models");
            }
            _ => panic!("Expected pub mod item"),
        }

        match &program.items[2] {
            Item::Import(imp) => {
                assert!(!imp.is_pub);
                assert_eq!(imp.path.len(), 1);
                assert_eq!(imp.path[0].name, "math");
                assert_eq!(imp.items.len(), 2);
                assert_eq!(imp.items[0].name, "add");
                assert_eq!(imp.items[1].name, "multiply");
            }
            _ => panic!("Expected import item"),
        }

        match &program.items[3] {
            Item::Import(imp) => {
                assert!(!imp.is_pub);
                assert_eq!(imp.path.len(), 1);
                assert_eq!(imp.path[0].name, "models");
                assert_eq!(imp.items.len(), 1);
                assert_eq!(imp.items[0].name, "Point");
            }
            _ => panic!("Expected import item"),
        }

        match &program.items[4] {
            Item::Import(imp) => {
                assert!(imp.is_pub);
                assert_eq!(imp.path.len(), 1);
                assert_eq!(imp.path[0].name, "geometry");
                assert_eq!(imp.items.len(), 1);
                assert_eq!(imp.items[0].name, "distance");
            }
            _ => panic!("Expected pub import item"),
        }
    }

    #[test]
    fn test_parse_trait_and_impl() {
        let src = r#"
        pub trait Describe {
            fn describe(self: &Self) -> i64;
        }

        impl Describe for User {
            fn describe(self: &User) -> i64 {
                return 42;
            }
        }

        fn print_val<T: Describe>(val: &T) {
            val.describe();
        }
        "#;
        let mut sm = SourceMap::new();
        let file_id = sm.add_file("trait_test.va".into(), src.into());
        let file = sm.get_file(file_id).unwrap();

        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        assert!(!diags.has_errors());

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        assert!(!diags.has_errors());
        assert_eq!(program.items.len(), 3);

        match &program.items[0] {
            Item::Trait(t) => {
                assert!(t.is_pub);
                assert_eq!(t.name.name, "Describe");
                assert_eq!(t.methods.len(), 1);
                assert_eq!(t.methods[0].name.name, "describe");
            }
            _ => panic!("Expected Trait item"),
        }

        match &program.items[1] {
            Item::Impl(i) => {
                assert_eq!(
                    i.trait_name.as_ref().map(|id| id.name.as_str()),
                    Some("Describe")
                );
                assert_eq!(i.target.name, "User");
                assert_eq!(i.methods.len(), 1);
                assert_eq!(i.methods[0].name.name, "describe");
            }
            _ => panic!("Expected Impl item"),
        }

        match &program.items[2] {
            Item::Function(f) => {
                assert_eq!(f.name.name, "print_val");
                assert_eq!(f.type_params.len(), 1);
                assert_eq!(f.type_params[0].name.name, "T");
                assert_eq!(
                    f.type_params[0].bound.as_ref().map(|id| id.name.as_str()),
                    Some("Describe")
                );
            }
            _ => panic!("Expected Function item"),
        }
    }

    #[test]
    fn test_parse_for_loop_and_generic_trait() {
        let src = r#"
        pub trait Iterator<T> {
            fn next(self: &mut Self) -> Option<T>;
        }

        impl Iterator<i64> for RangeI64 {
            fn next(self: &mut RangeI64) -> Option<i64> {
                return Option::None;
            }
        }

        fn run() {
            for x in range_i64(0, 10) {
                print_i64(x);
            }
        }
        "#;
        let mut sm = SourceMap::new();
        let file_id = sm.add_file("iter_test.va".into(), src.into());
        let file = sm.get_file(file_id).unwrap();

        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        assert!(!diags.has_errors());

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        assert!(!diags.has_errors());
        assert_eq!(program.items.len(), 3);

        match &program.items[0] {
            Item::Trait(t) => {
                assert_eq!(t.name.name, "Iterator");
                assert_eq!(t.type_params.len(), 1);
                assert_eq!(t.type_params[0].name.name, "T");
            }
            _ => panic!("Expected Trait item"),
        }

        match &program.items[1] {
            Item::Impl(i) => {
                assert_eq!(i.trait_name.as_ref().unwrap().name, "Iterator");
                assert_eq!(i.trait_args.len(), 1);
                assert_eq!(i.target.name, "RangeI64");
            }
            _ => panic!("Expected Impl item"),
        }

        match &program.items[2] {
            Item::Function(f) => {
                assert_eq!(f.body.stmts.len(), 1);
                match &f.body.stmts[0] {
                    Stmt::For { var, .. } => {
                        assert_eq!(var.name, "x");
                    }
                    _ => panic!("Expected Stmt::For"),
                }
            }
            _ => panic!("Expected Function item"),
        }
    }

    #[test]
    fn test_parse_lifetimes() {
        let src = r#"
        struct View<'a, T> {
            val: &'a T,
        }

        fn first<'a>(values: &'a [i64]) -> Option<&'a i64> {
            return Option::None;
        }

        impl<'a, T> View<'a, T> {
            fn get(self: &'a Self) -> &'a T {
                return self.val;
            }
        }
        "#;
        let mut sm = SourceMap::new();
        let file_id = sm.add_file("lifetime_test.va".into(), src.into());
        let file = sm.get_file(file_id).unwrap();

        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        assert!(!diags.has_errors());

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        assert!(!diags.has_errors());
        assert_eq!(program.items.len(), 3);

        match &program.items[0] {
            Item::Struct(s) => {
                assert_eq!(s.name.name, "View");
                assert_eq!(s.lifetime_params.len(), 1);
                assert_eq!(s.lifetime_params[0].name, "'a");
                assert_eq!(s.type_params.len(), 1);
                assert_eq!(s.type_params[0].name.name, "T");
                assert_eq!(s.fields.len(), 1);
                match &s.fields[0].ty.kind {
                    TypeExprKind::Reference {
                        lifetime,
                        is_mut,
                        inner,
                    } => {
                        assert_eq!(lifetime.as_ref().unwrap().name, "'a");
                        assert!(!*is_mut);
                        assert_eq!(inner.kind, TypeExprKind::Named("T".into()));
                    }
                    _ => panic!("Expected reference type with lifetime"),
                }
            }
            _ => panic!("Expected Struct item"),
        }

        match &program.items[1] {
            Item::Function(f) => {
                assert_eq!(f.name.name, "first");
                assert_eq!(f.lifetime_params.len(), 1);
                assert_eq!(f.lifetime_params[0].name, "'a");
                assert_eq!(f.params.len(), 1);
                match &f.params[0].ty.kind {
                    TypeExprKind::Reference { lifetime, .. } => {
                        assert_eq!(lifetime.as_ref().unwrap().name, "'a");
                    }
                    _ => panic!("Expected reference param"),
                }
            }
            _ => panic!("Expected Function item"),
        }

        match &program.items[2] {
            Item::Impl(i) => {
                assert_eq!(i.lifetime_params.len(), 1);
                assert_eq!(i.lifetime_params[0].name, "'a");
                assert_eq!(i.type_params.len(), 1);
                assert_eq!(i.type_params[0].name.name, "T");
                assert_eq!(i.target.name, "View");
                assert_eq!(i.target_args.len(), 2);
            }
            _ => panic!("Expected Impl item"),
        }
    }
}
