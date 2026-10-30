//! Canonical Source Formatter for the Vanta Programming Language.
//!
//! Provides deterministic, idempotent source code formatting that preserves
//! all comments, literals, and escapes while enforcing canonical Vanta style.

use std::collections::HashSet;
use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::{Comment, CommentKind, Lexer, Token, TokenKind};
use vanta_parser::ast::*;
use vanta_parser::Parser;
use vanta_source::{SourceFile, Span};

/// Check if a Vanta source string is already in canonical format.
/// Returns `Ok(true)` if already formatted, `Ok(false)` if formatting would change it,
/// or `Err(DiagnosticBag)` if the source contains syntax or lexical errors.
pub fn check_formatted(src: &str, file_name: &str) -> Result<bool, DiagnosticBag> {
    let formatted = format_source(src, file_name)?;
    Ok(formatted == src)
}

/// Formats a Vanta source string into its canonical representation.
/// Returns `Ok(String)` containing formatted source code or `Err(DiagnosticBag)`
/// if the source contains lexical or parse errors.
pub fn format_source(src: &str, file_name: &str) -> Result<String, DiagnosticBag> {
    if src.trim().is_empty() {
        return Ok(String::new());
    }

    let file = SourceFile::new(
        vanta_source::FileId(1),
        file_name.to_string(),
        src.to_string(),
    );
    let mut diags = DiagnosticBag::new();

    let mut lexer = Lexer::new(&file);
    let (tokens, comments) = lexer.tokenize_with_comments(&mut diags);
    if diags.has_errors() {
        return Err(diags);
    }

    // Verify program parses into valid AST
    let mut parser = Parser::new(tokens.clone());
    let program = parser.parse_program(&mut diags);
    if diags.has_errors() {
        return Err(diags);
    }

    let tags = collect_syntax_tags(&program, &tokens);

    let mut formatter = Formatter::new(&file, tokens, comments, tags);
    let formatted = formatter.format();

    Ok(formatted)
}

#[derive(Default)]
struct SyntaxTags {
    generic_bracket_spans: HashSet<Span>,
    struct_decl_ranges: Vec<Span>,
    match_closing_braces: HashSet<Span>,
    match_arm_commas: HashSet<Span>,
    top_level_closing_braces: HashSet<Span>,
    impl_method_closing_braces: HashSet<Span>,
    import_brace_spans: HashSet<Span>,
}

fn collect_syntax_tags(program: &Program, tokens: &[Token]) -> SyntaxTags {
    let mut tags = SyntaxTags::default();

    for item in &program.items {
        match item {
            Item::Function(f) => {
                // Top-level closing brace
                find_closing_brace_in_span(f.body.span, tokens, &mut tags.top_level_closing_braces);

                // Generic type params: fn foo<T, U>(...)
                if !f.type_params.is_empty() || !f.lifetime_params.is_empty() {
                    mark_generic_brackets_between(
                        f.name.span.end,
                        f.params_start(),
                        tokens,
                        &mut tags.generic_bracket_spans,
                    );
                }
                for param in &f.params {
                    collect_type_generics(&param.ty, tokens, &mut tags.generic_bracket_spans);
                }
                if let Some(ret_ty) = &f.ret_type {
                    collect_type_generics(ret_ty, tokens, &mut tags.generic_bracket_spans);
                }
                collect_block_generics(&f.body, tokens, &mut tags);
            }
            Item::Struct(s) => {
                tags.struct_decl_ranges.push(s.span);
                find_closing_brace_in_span(s.span, tokens, &mut tags.top_level_closing_braces);

                if !s.type_params.is_empty() || !s.lifetime_params.is_empty() {
                    mark_generic_brackets_between(
                        s.name.span.end,
                        s.span.end,
                        tokens,
                        &mut tags.generic_bracket_spans,
                    );
                }
                for field in &s.fields {
                    collect_type_generics(&field.ty, tokens, &mut tags.generic_bracket_spans);
                }
            }
            Item::Enum(e) => {
                tags.struct_decl_ranges.push(e.span);
                find_closing_brace_in_span(e.span, tokens, &mut tags.top_level_closing_braces);

                if !e.type_params.is_empty() || !e.lifetime_params.is_empty() {
                    mark_generic_brackets_between(
                        e.name.span.end,
                        e.span.end,
                        tokens,
                        &mut tags.generic_bracket_spans,
                    );
                }
                for v in &e.variants {
                    for p in &v.payload {
                        collect_type_generics(p, tokens, &mut tags.generic_bracket_spans);
                    }
                }
            }
            Item::Impl(impl_block) => {
                find_closing_brace_in_span(
                    impl_block.span,
                    tokens,
                    &mut tags.top_level_closing_braces,
                );

                if !impl_block.type_params.is_empty() || !impl_block.lifetime_params.is_empty() {
                    let first_ident_start = impl_block
                        .trait_name
                        .as_ref()
                        .map(|id| id.span.start)
                        .unwrap_or(impl_block.target.span.start);
                    mark_generic_brackets_between(
                        impl_block.span.start,
                        first_ident_start,
                        tokens,
                        &mut tags.generic_bracket_spans,
                    );
                }

                if let Some(trait_ident) = &impl_block.trait_name {
                    if !impl_block.trait_args.is_empty() {
                        mark_generic_brackets_between(
                            trait_ident.span.end,
                            impl_block.target.span.start,
                            tokens,
                            &mut tags.generic_bracket_spans,
                        );
                    }
                }
                if !impl_block.target_args.is_empty() {
                    mark_generic_brackets_between(
                        impl_block.target.span.end,
                        impl_block.span.end,
                        tokens,
                        &mut tags.generic_bracket_spans,
                    );
                }

                for t in &impl_block.trait_args {
                    collect_type_generics(t, tokens, &mut tags.generic_bracket_spans);
                }
                for t in &impl_block.target_args {
                    collect_type_generics(t, tokens, &mut tags.generic_bracket_spans);
                }

                let method_count = impl_block.methods.len();
                for (idx, m) in impl_block.methods.iter().enumerate() {
                    if idx + 1 < method_count {
                        find_closing_brace_in_span(
                            m.body.span,
                            tokens,
                            &mut tags.impl_method_closing_braces,
                        );
                    }
                    if !m.type_params.is_empty() || !m.lifetime_params.is_empty() {
                        mark_generic_brackets_between(
                            m.name.span.end,
                            m.params_start(),
                            tokens,
                            &mut tags.generic_bracket_spans,
                        );
                    }
                    for param in &m.params {
                        collect_type_generics(&param.ty, tokens, &mut tags.generic_bracket_spans);
                    }
                    if let Some(ret_ty) = &m.ret_type {
                        collect_type_generics(ret_ty, tokens, &mut tags.generic_bracket_spans);
                    }
                    collect_block_generics(&m.body, tokens, &mut tags);
                }
            }
            Item::Import(imp) => {
                for tok in tokens {
                    if tok.span.start >= imp.span.start
                        && tok.span.end <= imp.span.end
                        && matches!(tok.kind, TokenKind::LBrace | TokenKind::RBrace)
                    {
                        tags.import_brace_spans.insert(tok.span);
                    }
                }
            }
            Item::Trait(t) => {
                find_closing_brace_in_span(t.span, tokens, &mut tags.top_level_closing_braces);
                if !t.type_params.is_empty() || !t.lifetime_params.is_empty() {
                    mark_generic_brackets_between(
                        t.name.span.end,
                        t.span.end,
                        tokens,
                        &mut tags.generic_bracket_spans,
                    );
                }
                for m in &t.methods {
                    for param in &m.params {
                        collect_type_generics(&param.ty, tokens, &mut tags.generic_bracket_spans);
                    }
                    if let Some(ret_ty) = &m.ret_type {
                        collect_type_generics(ret_ty, tokens, &mut tags.generic_bracket_spans);
                    }
                }
            }
            Item::Mod(_) => {}
        }
    }

    tags
}

trait FnDeclExt {
    fn params_start(&self) -> u32;
}

impl FnDeclExt for FnDecl {
    fn params_start(&self) -> u32 {
        if let Some(first) = self.params.first() {
            first.span.start
        } else {
            self.body.span.start
        }
    }
}

fn find_closing_brace_in_span(span: Span, tokens: &[Token], dest: &mut HashSet<Span>) {
    // Search backward from end of span for RBrace
    for tok in tokens.iter().rev() {
        if tok.span.end <= span.end && tok.span.start >= span.start && tok.kind == TokenKind::RBrace
        {
            dest.insert(tok.span);
            break;
        }
    }
}

fn mark_generic_brackets_between(start: u32, end: u32, tokens: &[Token], dest: &mut HashSet<Span>) {
    let mut depth = 0;
    for tok in tokens {
        if tok.span.start >= start && tok.span.end <= end {
            if tok.kind == TokenKind::Lt {
                dest.insert(tok.span);
                depth += 1;
            } else if tok.kind == TokenKind::Gt && depth > 0 {
                dest.insert(tok.span);
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
        }
    }
}

fn collect_type_generics(ty: &TypeExpr, tokens: &[Token], dest: &mut HashSet<Span>) {
    match &ty.kind {
        TypeExprKind::Generic { type_args, .. } => {
            mark_generic_brackets_between(ty.span.start, ty.span.end, tokens, dest);
            for arg in type_args {
                collect_type_generics(arg, tokens, dest);
            }
        }
        TypeExprKind::Reference { inner, .. }
        | TypeExprKind::Array { inner, .. }
        | TypeExprKind::Slice { inner, .. } => {
            collect_type_generics(inner, tokens, dest);
        }
        _ => {}
    }
}

fn collect_block_generics(block: &Block, tokens: &[Token], tags: &mut SyntaxTags) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let { ty, init, .. } => {
                if let Some(t) = ty {
                    collect_type_generics(t, tokens, &mut tags.generic_bracket_spans);
                }
                if let Some(e) = init {
                    collect_expr_generics(e, tokens, tags);
                }
            }
            Stmt::Assign { target, value, .. } => {
                collect_expr_generics(target, tokens, tags);
                collect_expr_generics(value, tokens, tags);
            }
            Stmt::While { cond, body, .. } => {
                collect_expr_generics(cond, tokens, tags);
                collect_block_generics(body, tokens, tags);
            }
            Stmt::Loop { body, .. } => {
                collect_block_generics(body, tokens, tags);
            }
            Stmt::For { iter, body, .. } => {
                collect_expr_generics(iter, tokens, tags);
                collect_block_generics(body, tokens, tags);
            }
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    collect_expr_generics(e, tokens, tags);
                }
            }
            Stmt::Expr(e) => {
                collect_expr_generics(e, tokens, tags);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    if let Some(expr) = &block.expr {
        collect_expr_generics(expr, tokens, tags);
    }
}

fn collect_expr_generics(expr: &Expr, tokens: &[Token], tags: &mut SyntaxTags) {
    match &expr.kind {
        ExprKind::Call {
            type_args, args, ..
        } => {
            if !type_args.is_empty() {
                mark_generic_brackets_between(
                    expr.span.start,
                    expr.span.end,
                    tokens,
                    &mut tags.generic_bracket_spans,
                );
                for arg in type_args {
                    collect_type_generics(arg, tokens, &mut tags.generic_bracket_spans);
                }
            }
            for a in args {
                collect_expr_generics(a, tokens, tags);
            }
        }
        ExprKind::StructLiteral {
            type_args, fields, ..
        } => {
            if !type_args.is_empty() {
                mark_generic_brackets_between(
                    expr.span.start,
                    expr.span.end,
                    tokens,
                    &mut tags.generic_bracket_spans,
                );
                for arg in type_args {
                    collect_type_generics(arg, tokens, &mut tags.generic_bracket_spans);
                }
            }
            for (_, f_expr) in fields {
                collect_expr_generics(f_expr, tokens, tags);
            }
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_expr_generics(lhs, tokens, tags);
            collect_expr_generics(rhs, tokens, tags);
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::Ref { expr: inner, .. }
        | ExprKind::Deref(inner)
        | ExprKind::Try(inner)
        | ExprKind::FieldAccess { target: inner, .. } => {
            collect_expr_generics(inner, tokens, tags);
        }
        ExprKind::MethodCall { target, args, .. } => {
            collect_expr_generics(target, tokens, tags);
            for a in args {
                collect_expr_generics(a, tokens, tags);
            }
        }
        ExprKind::Index { array, index } => {
            collect_expr_generics(array, tokens, tags);
            collect_expr_generics(index, tokens, tags);
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_generics(cond, tokens, tags);
            collect_block_generics(then_branch, tokens, tags);
            if let Some(eb) = else_branch {
                match eb {
                    ElseBranch::If(if_expr) => collect_expr_generics(if_expr, tokens, tags),
                    ElseBranch::Block(b) => collect_block_generics(b, tokens, tags),
                }
            }
        }
        ExprKind::Block(b) => {
            collect_block_generics(b, tokens, tags);
        }
        ExprKind::ArrayLiteral(elements) => {
            for el in elements {
                collect_expr_generics(el, tokens, tags);
            }
        }
        ExprKind::Match { expr: scrut, arms } => {
            find_closing_brace_in_span(expr.span, tokens, &mut tags.match_closing_braces);
            collect_expr_generics(scrut, tokens, tags);
            for arm in arms {
                collect_expr_generics(&arm.body, tokens, tags);
                for tok in tokens {
                    if tok.span.start >= arm.body.span.end {
                        if tok.kind == TokenKind::Comma {
                            tags.match_arm_commas.insert(tok.span);
                        }
                        break;
                    }
                }
            }
        }
        ExprKind::VariantConstructor {
            type_args, args, ..
        } => {
            if !type_args.is_empty() {
                mark_generic_brackets_between(
                    expr.span.start,
                    expr.span.end,
                    tokens,
                    &mut tags.generic_bracket_spans,
                );
                for arg in type_args {
                    collect_type_generics(arg, tokens, &mut tags.generic_bracket_spans);
                }
            }
            for a in args {
                collect_expr_generics(a, tokens, tags);
            }
        }
        _ => {}
    }
}

struct Formatter<'a> {
    file: &'a SourceFile,
    tokens: Vec<Token>,
    comments: Vec<Comment>,
    tags: SyntaxTags,
    output: String,
    current_indent: usize,
    at_line_start: bool,
    prev_tok_kind: Option<TokenKind>,
    prev_tok_span: Option<Span>,
    last_source_line: usize,
    comment_idx: usize,
    paren_depth: usize,
    bracket_depth: usize,
    generic_depth: usize,
    in_struct_decl_depth: usize,
}

impl<'a> Formatter<'a> {
    fn new(
        file: &'a SourceFile,
        tokens: Vec<Token>,
        comments: Vec<Comment>,
        tags: SyntaxTags,
    ) -> Self {
        Self {
            file,
            tokens,
            comments,
            tags,
            output: String::new(),
            current_indent: 0,
            at_line_start: true,
            prev_tok_kind: None,
            prev_tok_span: None,
            last_source_line: 0,
            comment_idx: 0,
            paren_depth: 0,
            bracket_depth: 0,
            generic_depth: 0,
            in_struct_decl_depth: 0,
        }
    }

    fn emit_indent(&mut self) {
        if self.at_line_start {
            for _ in 0..self.current_indent {
                self.output.push_str("    ");
            }
            self.at_line_start = false;
        }
    }

    fn flush_comments_before(&mut self, next_span: Span) {
        while self.comment_idx < self.comments.len() {
            let comment = &self.comments[self.comment_idx];
            if comment.span.start >= next_span.start {
                break;
            }
            self.emit_comment(self.comment_idx);
            self.comment_idx += 1;
        }
    }

    fn emit_comment(&mut self, idx: usize) {
        let comment = self.comments[idx].clone();
        let (comment_start_line, _) = self.file.line_col(comment.span.start);
        let (comment_end_line, _) = self.file.line_col(comment.span.end);

        if !self.at_line_start {
            // Check if trailing comment on same line as previous emitted code
            if self.last_source_line > 0 && comment_start_line == self.last_source_line {
                self.output.push(' ');
                self.output.push_str(&comment.text);
                if comment.kind == CommentKind::Line {
                    self.output.push('\n');
                    self.at_line_start = true;
                }
                self.last_source_line = comment_end_line;
                return;
            } else {
                // Comment is on a later line, end previous line
                self.output.push('\n');
                self.at_line_start = true;
            }
        }

        // We are at line start. Check if there was a blank line before comment
        if self.last_source_line > 0
            && comment_start_line > self.last_source_line + 1
            && !self.output.ends_with("\n\n")
            && !self.output.is_empty()
        {
            self.output.push('\n');
        }

        self.emit_indent();
        self.output.push_str(&comment.text);

        if comment.kind == CommentKind::Line {
            self.output.push('\n');
            self.at_line_start = true;
        } else {
            // Block comment: check if followed by newline in original source
            let next_tok_line = self.peek_next_line(idx);
            if next_tok_line > comment_end_line {
                self.output.push('\n');
                self.at_line_start = true;
            } else {
                self.output.push(' ');
                self.at_line_start = false;
            }
        }

        self.last_source_line = comment_end_line;
    }

    fn flush_trailing_comments_on_line(&mut self, line: usize) {
        while self.comment_idx < self.comments.len() {
            let comment = &self.comments[self.comment_idx];
            let (c_start_line, _) = self.file.line_col(comment.span.start);
            if c_start_line == line {
                let comment = self.comments[self.comment_idx].clone();
                self.output.push(' ');
                self.output.push_str(&comment.text);
                self.last_source_line = self.file.line_col(comment.span.end).0;
                self.comment_idx += 1;
                if comment.kind == CommentKind::Line {
                    self.output.push('\n');
                    self.at_line_start = true;
                    return;
                }
            } else {
                break;
            }
        }
    }

    fn peek_next_line(&self, comment_idx: usize) -> usize {
        if comment_idx + 1 < self.comments.len() {
            let next_c = &self.comments[comment_idx + 1];
            return self.file.line_col(next_c.span.start).0;
        }
        // Else peek next token
        for tok in &self.tokens {
            if tok.span.start > self.comments[comment_idx].span.end {
                return self.file.line_col(tok.span.start).0;
            }
        }
        usize::MAX
    }

    fn is_generic_bracket(&self, span: Span) -> bool {
        self.tags.generic_bracket_spans.contains(&span)
    }

    fn is_unary_operator(&self, kind: &TokenKind) -> bool {
        match kind {
            TokenKind::Minus | TokenKind::Bang | TokenKind::Star | TokenKind::Ampersand => {
                match &self.prev_tok_kind {
                    None => true,
                    Some(prev) => matches!(
                        prev,
                        TokenKind::Eq
                            | TokenKind::PlusEq
                            | TokenKind::MinusEq
                            | TokenKind::StarEq
                            | TokenKind::SlashEq
                            | TokenKind::PercentEq
                            | TokenKind::Plus
                            | TokenKind::Minus
                            | TokenKind::Star
                            | TokenKind::Slash
                            | TokenKind::Percent
                            | TokenKind::AmpAmp
                            | TokenKind::PipePipe
                            | TokenKind::EqEq
                            | TokenKind::BangEq
                            | TokenKind::Lt
                            | TokenKind::LtEq
                            | TokenKind::Gt
                            | TokenKind::GtEq
                            | TokenKind::LParen
                            | TokenKind::LBracket
                            | TokenKind::LBrace
                            | TokenKind::Comma
                            | TokenKind::Colon
                            | TokenKind::Semicolon
                            | TokenKind::Arrow
                            | TokenKind::Return
                            | TokenKind::Let
                            | TokenKind::Mut
                            | TokenKind::If
                            | TokenKind::While
                            | TokenKind::As
                    ),
                }
            }
            _ => false,
        }
    }

    fn format(&mut self) -> String {
        let count = self.tokens.len();
        for i in 0..count {
            let tok = self.tokens[i].clone();
            if tok.kind == TokenKind::Eof {
                // Flush remaining comments
                self.flush_comments_before(tok.span);
                break;
            }

            self.format_token(i);
        }

        // Flush any trailing comments after the last non-EOF token
        let file_end_span = Span::new(
            self.file.id,
            self.file.src.len() as u32,
            self.file.src.len() as u32,
        );
        self.flush_comments_before(file_end_span);

        // Ensure exactly one trailing newline if non-empty
        let mut res = self.output.clone();
        while res.ends_with("\n\n") {
            res.pop();
        }
        if !res.is_empty() && !res.ends_with('\n') {
            res.push('\n');
        }

        res
    }

    fn format_token(&mut self, i: usize) {
        let tok = self.tokens[i].clone();
        let next_tok = self.tokens.get(i + 1).cloned();

        // 1. Flush any comments that appear before this token
        self.flush_comments_before(tok.span);

        let (tok_line, _) = self.file.line_col(tok.span.start);

        // 2. Check if there was an original blank line before this token
        if self.at_line_start
            && self.last_source_line > 0
            && tok_line > self.last_source_line + 1
            && !self.output.ends_with("\n\n")
            && !self.output.is_empty()
        {
            self.output.push('\n');
        }

        // Check if entering a struct or enum declaration
        if matches!(tok.kind, TokenKind::Struct | TokenKind::Enum) {
            for range in &self.tags.struct_decl_ranges {
                if tok.span.start >= range.start && tok.span.end <= range.end {
                    self.in_struct_decl_depth += 1;
                    break;
                }
            }
        }

        // Special handling for RBrace: decrease indentation before emitting
        if tok.kind == TokenKind::RBrace && !self.tags.import_brace_spans.contains(&tok.span) {
            if self.in_struct_decl_depth > 0 || self.tags.match_closing_braces.contains(&tok.span) {
                // Ensure trailing comma on final struct field, enum variant, or match arm if omitted
                if let Some(prev) = &self.prev_tok_kind {
                    if !matches!(prev, TokenKind::Comma | TokenKind::LBrace) {
                        self.output.push(',');
                    }
                }
            }
            if !self.at_line_start {
                self.output.push('\n');
                self.at_line_start = true;
            }
            self.current_indent = self.current_indent.saturating_sub(1);
        }

        // 3. Emit indentation if starting a line
        self.emit_indent();

        // 4. Determine spacing before token
        self.ensure_spacing_before(&tok);

        // 5. Emit token content
        let text = self.token_text(&tok);
        self.output.push_str(&text);
        self.at_line_start = false;

        // 6. State updates and after-token spacing/newlines
        self.handle_token_after(&tok, next_tok.as_ref());

        self.last_source_line = self.file.line_col(tok.span.end).0;
        self.prev_tok_kind = Some(tok.kind);
        self.prev_tok_span = Some(tok.span);
    }

    fn token_text(&self, tok: &Token) -> String {
        match &tok.kind {
            TokenKind::StringLiteral(_) | TokenKind::IntLiteral(_) | TokenKind::FloatLiteral(_) => {
                // Exact verbatim representation preserving all escape sequences and radix/underscores
                self.file.src[tok.span.start as usize..tok.span.end as usize].to_string()
            }
            TokenKind::Ident(name) => name.clone(),
            TokenKind::Lifetime(name) => name.clone(),
            TokenKind::BoolLiteral(b) => {
                if *b {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            TokenKind::Fn => "fn".into(),
            TokenKind::Let => "let".into(),
            TokenKind::Mut => "mut".into(),
            TokenKind::If => "if".into(),
            TokenKind::Else => "else".into(),
            TokenKind::While => "while".into(),
            TokenKind::Loop => "loop".into(),
            TokenKind::Return => "return".into(),
            TokenKind::Break => "break".into(),
            TokenKind::Continue => "continue".into(),
            TokenKind::Struct => "struct".into(),
            TokenKind::Impl => "impl".into(),
            TokenKind::Pub => "pub".into(),
            TokenKind::Mod => "mod".into(),
            TokenKind::Import => "import".into(),
            TokenKind::Trait => "trait".into(),
            TokenKind::For => "for".into(),
            TokenKind::In => "in".into(),
            TokenKind::Dyn => "dyn".into(),
            TokenKind::As => "as".into(),
            TokenKind::Const => "const".into(),
            TokenKind::Unsafe => "unsafe".into(),
            TokenKind::Enum => "enum".into(),
            TokenKind::Match => "match".into(),
            TokenKind::I8 => "i8".into(),
            TokenKind::I16 => "i16".into(),
            TokenKind::I32 => "i32".into(),
            TokenKind::I64 => "i64".into(),
            TokenKind::U8 => "u8".into(),
            TokenKind::U16 => "u16".into(),
            TokenKind::U32 => "u32".into(),
            TokenKind::U64 => "u64".into(),
            TokenKind::F32 => "f32".into(),
            TokenKind::F64 => "f64".into(),
            TokenKind::BoolType => "bool".into(),
            TokenKind::Str => "str".into(),
            TokenKind::Plus => "+".into(),
            TokenKind::Minus => "-".into(),
            TokenKind::Star => "*".into(),
            TokenKind::Slash => "/".into(),
            TokenKind::Percent => "%".into(),
            TokenKind::Ampersand => "&".into(),
            TokenKind::Pipe => "|".into(),
            TokenKind::Caret => "^".into(),
            TokenKind::Tilde => "~".into(),
            TokenKind::Shl => "<<".into(),
            TokenKind::Shr => ">>".into(),
            TokenKind::AmpAmp => "&&".into(),
            TokenKind::PipePipe => "||".into(),
            TokenKind::Bang => "!".into(),
            TokenKind::EqEq => "==".into(),
            TokenKind::BangEq => "!=".into(),
            TokenKind::Lt => "<".into(),
            TokenKind::LtEq => "<=".into(),
            TokenKind::Gt => ">".into(),
            TokenKind::GtEq => ">=".into(),
            TokenKind::Eq => "=".into(),
            TokenKind::PlusEq => "+=".into(),
            TokenKind::MinusEq => "-=".into(),
            TokenKind::StarEq => "*=".into(),
            TokenKind::SlashEq => "/=".into(),
            TokenKind::PercentEq => "%=".into(),
            TokenKind::LParen => "(".into(),
            TokenKind::RParen => ")".into(),
            TokenKind::LBrace => "{".into(),
            TokenKind::RBrace => "}".into(),
            TokenKind::LBracket => "[".into(),
            TokenKind::RBracket => "]".into(),
            TokenKind::Semicolon => ";".into(),
            TokenKind::Colon => ":".into(),
            TokenKind::ColonColon => "::".into(),
            TokenKind::Comma => ",".into(),
            TokenKind::Dot => ".".into(),
            TokenKind::Arrow => "->".into(),
            TokenKind::FatArrow => "=>".into(),
            TokenKind::Underscore => "_".into(),
            TokenKind::Question => "?".into(),
            TokenKind::Eof => String::new(),
            TokenKind::Error(s) => s.clone(),
        }
    }

    fn ensure_spacing_before(&mut self, tok: &Token) {
        if self.at_line_start
            || self.output.is_empty()
            || self.output.ends_with(' ')
            || self.output.ends_with('\n')
        {
            return;
        }

        let is_generic = self.is_generic_bracket(tok.span);
        let is_unary = self.is_unary_operator(&tok.kind);

        match &tok.kind {
            TokenKind::LBrace => {
                // Opening brace always preceded by a single space: "fn foo() {" or "struct Point {"
                // unless it is an import brace list e.g. import path::{a, b}
                if !self.tags.import_brace_spans.contains(&tok.span) {
                    self.output.push(' ');
                }
            }
            TokenKind::Arrow | TokenKind::FatArrow => {
                // " -> " or " => "
                self.output.push(' ');
            }
            TokenKind::Eq
            | TokenKind::PlusEq
            | TokenKind::MinusEq
            | TokenKind::StarEq
            | TokenKind::SlashEq
            | TokenKind::PercentEq
            | TokenKind::Plus
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::Pipe
            | TokenKind::Caret
            | TokenKind::Shl
            | TokenKind::Shr
            | TokenKind::AmpAmp
            | TokenKind::PipePipe
            | TokenKind::EqEq
            | TokenKind::BangEq
            | TokenKind::LtEq
            | TokenKind::GtEq
            | TokenKind::For
            | TokenKind::In
            | TokenKind::As => {
                self.output.push(' ');
            }
            TokenKind::Minus | TokenKind::Star | TokenKind::Ampersand => {
                if !is_unary {
                    self.output.push(' ');
                } else if let Some(prev) = &self.prev_tok_kind {
                    if matches!(
                        prev,
                        TokenKind::Colon
                            | TokenKind::Comma
                            | TokenKind::Eq
                            | TokenKind::PlusEq
                            | TokenKind::MinusEq
                            | TokenKind::StarEq
                            | TokenKind::SlashEq
                            | TokenKind::PercentEq
                            | TokenKind::Arrow
                            | TokenKind::Return
                            | TokenKind::Let
                            | TokenKind::Mut
                            | TokenKind::If
                            | TokenKind::While
                            | TokenKind::Plus
                            | TokenKind::Minus
                            | TokenKind::Star
                            | TokenKind::Slash
                            | TokenKind::Percent
                            | TokenKind::EqEq
                            | TokenKind::BangEq
                            | TokenKind::Lt
                            | TokenKind::LtEq
                            | TokenKind::Gt
                            | TokenKind::GtEq
                            | TokenKind::AmpAmp
                            | TokenKind::PipePipe
                    ) {
                        self.output.push(' ');
                    }
                }
            }
            TokenKind::Lt | TokenKind::Gt => {
                if !is_generic {
                    self.output.push(' ');
                }
            }
            TokenKind::Colon => {
                // No space before colon: "field: i64"
            }
            TokenKind::Semicolon | TokenKind::Comma | TokenKind::Dot | TokenKind::ColonColon => {
                // No space before punctuation
            }
            TokenKind::RParen | TokenKind::RBracket => {
                // No space before closing delimiter
            }
            TokenKind::LParen => {
                // Space before '(' only if preceded by keywords like if, while, loop
                if let Some(prev) = &self.prev_tok_kind {
                    if matches!(
                        prev,
                        TokenKind::If | TokenKind::While | TokenKind::Loop | TokenKind::Match
                    ) {
                        self.output.push(' ');
                    }
                }
            }
            TokenKind::LBracket => {
                // Space before '[' unless indexing into array or slice: "arr[0]"
                if let Some(prev) = &self.prev_tok_kind {
                    if !matches!(
                        prev,
                        TokenKind::Ident(_)
                            | TokenKind::RParen
                            | TokenKind::RBracket
                            | TokenKind::Dot
                    ) {
                        self.output.push(' ');
                    }
                }
            }
            _ => {
                // Default: space after keywords, binary ops, punctuation
                if let Some(prev) = &self.prev_tok_kind {
                    let prev_was_unary = self.is_unary_operator(prev);
                    let prev_was_generic = self
                        .prev_tok_span
                        .is_some_and(|s| self.is_generic_bracket(s));

                    if prev_was_unary {
                        if prev == &TokenKind::Ampersand && tok.kind == TokenKind::Mut {
                            self.output.push(' ');
                        }
                    } else if prev_was_generic {
                        // After generic '>', space only if followed by keyword, ident, '{', or binary op
                        if matches!(
                            tok.kind,
                            TokenKind::LBrace | TokenKind::Arrow | TokenKind::Eq | TokenKind::For
                        ) {
                            self.output.push(' ');
                        }
                    } else if matches!(
                        prev,
                        TokenKind::Fn
                            | TokenKind::Let
                            | TokenKind::Mut
                            | TokenKind::If
                            | TokenKind::Else
                            | TokenKind::While
                            | TokenKind::Loop
                            | TokenKind::Return
                            | TokenKind::Struct
                            | TokenKind::Impl
                            | TokenKind::Pub
                            | TokenKind::Mod
                            | TokenKind::Import
                            | TokenKind::Trait
                            | TokenKind::For
                            | TokenKind::In
                            | TokenKind::As
                            | TokenKind::Const
                            | TokenKind::Unsafe
                            | TokenKind::Enum
                            | TokenKind::Match
                            | TokenKind::FatArrow
                            | TokenKind::Comma
                            | TokenKind::Colon
                            | TokenKind::Arrow
                            | TokenKind::Plus
                            | TokenKind::Minus
                            | TokenKind::Star
                            | TokenKind::Slash
                            | TokenKind::Percent
                            | TokenKind::Eq
                            | TokenKind::PlusEq
                            | TokenKind::MinusEq
                            | TokenKind::StarEq
                            | TokenKind::SlashEq
                            | TokenKind::PercentEq
                            | TokenKind::EqEq
                            | TokenKind::BangEq
                            | TokenKind::Lt
                            | TokenKind::LtEq
                            | TokenKind::Gt
                            | TokenKind::GtEq
                            | TokenKind::AmpAmp
                            | TokenKind::PipePipe
                            | TokenKind::Pipe
                            | TokenKind::Caret
                            | TokenKind::Shl
                            | TokenKind::Shr
                    ) || (matches!(prev, TokenKind::Lifetime(_))
                        && !matches!(tok.kind, TokenKind::Comma | TokenKind::Gt))
                    {
                        self.output.push(' ');
                    }
                }
            }
        }
    }

    fn handle_token_after(&mut self, tok: &Token, next_tok: Option<&Token>) {
        match tok.kind {
            TokenKind::LParen => {
                self.paren_depth += 1;
            }
            TokenKind::RParen => {
                self.paren_depth = self.paren_depth.saturating_sub(1);
            }
            TokenKind::LBracket => {
                self.bracket_depth += 1;
            }
            TokenKind::RBracket => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
            }
            TokenKind::Lt => {
                if self.is_generic_bracket(tok.span) {
                    self.generic_depth += 1;
                }
            }
            TokenKind::Gt => {
                if self.is_generic_bracket(tok.span) {
                    self.generic_depth = self.generic_depth.saturating_sub(1);
                }
            }
            TokenKind::LBrace => {
                if !self.tags.import_brace_spans.contains(&tok.span) {
                    self.current_indent += 1;
                    // If block is immediately empty: "{}"
                    if next_tok.is_some_and(|n| n.kind == TokenKind::RBrace) {
                        // Empty block on same line
                    } else {
                        let (tok_line, _) = self.file.line_col(tok.span.end);
                        self.flush_trailing_comments_on_line(tok_line);
                        if !self.at_line_start {
                            self.output.push('\n');
                            self.at_line_start = true;
                        }
                    }
                }
            }
            TokenKind::RBrace => {
                if !self.tags.import_brace_spans.contains(&tok.span) {
                    // If closing a struct declaration, decrement struct depth
                    if self.in_struct_decl_depth > 0 {
                        for range in &self.tags.struct_decl_ranges {
                            if tok.span.end == range.end
                                || tok.span.start >= range.start && tok.span.end <= range.end
                            {
                                self.in_struct_decl_depth =
                                    self.in_struct_decl_depth.saturating_sub(1);
                                break;
                            }
                        }
                    }

                    // Check what comes after RBrace
                    let is_top_level = self.tags.top_level_closing_braces.contains(&tok.span);
                    let is_impl_method = self.tags.impl_method_closing_braces.contains(&tok.span);

                    if let Some(next) = next_tok {
                        if next.kind == TokenKind::Else {
                            // "} else" stays on same line with space
                            self.output.push(' ');
                        } else if next.kind == TokenKind::Semicolon || next.kind == TokenKind::Comma
                        {
                            // Semicolon/comma immediately attached to brace: "};" or "},"
                        } else if is_top_level {
                            if next.kind != TokenKind::Eof {
                                self.output.push_str("\n\n");
                                self.at_line_start = true;
                            } else {
                                self.output.push('\n');
                                self.at_line_start = true;
                            }
                        } else if is_impl_method {
                            self.output.push_str("\n\n");
                            self.at_line_start = true;
                        } else {
                            self.output.push('\n');
                            self.at_line_start = true;
                        }
                    } else {
                        self.output.push('\n');
                        self.at_line_start = true;
                    }
                }
            }
            TokenKind::Semicolon => {
                let (tok_line, _) = self.file.line_col(tok.span.end);
                self.flush_trailing_comments_on_line(tok_line);
                // Semicolon ending a statement causes a newline unless inside array type [i64; 4]
                if self.paren_depth == 0 && self.bracket_depth == 0 && !self.at_line_start {
                    self.output.push('\n');
                    self.at_line_start = true;
                } else if !self.at_line_start {
                    self.output.push(' ');
                }
            }
            TokenKind::Comma => {
                let (tok_line, _) = self.file.line_col(tok.span.end);
                self.flush_trailing_comments_on_line(tok_line);
                // Inside struct declaration field list or match arms: -> newline
                if (self.in_struct_decl_depth > 0 || self.tags.match_arm_commas.contains(&tok.span))
                    && self.paren_depth == 0
                    && self.bracket_depth == 0
                    && self.generic_depth == 0
                    && !self.at_line_start
                {
                    self.output.push('\n');
                    self.at_line_start = true;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_empty() {
        assert_eq!(format_source("", "test.va").unwrap(), "");
        assert_eq!(format_source("   \n\n  \t ", "test.va").unwrap(), "");
    }

    #[test]
    fn test_format_simple_function() {
        let input = "fn add(a:i64,b:i64)->i64{return a+b;}";
        let expected = "fn add(a: i64, b: i64) -> i64 {\n    return a + b;\n}\n";
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, expected);
        // Idempotence
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_format_struct_and_impl() {
        let input = "struct Point{x:i64,y:i64}impl Point{fn move_by(self:&mut Point,dx:i64,dy:i64){self.x=self.x+dx;self.y=self.y+dy;}}";
        let expected = r#"struct Point {
    x: i64,
    y: i64,
}

impl Point {
    fn move_by(self: &mut Point, dx: i64, dy: i64) {
        self.x = self.x + dx;
        self.y = self.y + dy;
    }
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, expected);
        // Idempotence
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_comment_preservation() {
        let input = r#"// Header comment
// Line two

fn main() -> i64 {
    // initialize x
    let mut x: i64 = 42; // trailing comment
    /* block comment */
    return x;
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, input);
        // Idempotence
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_generic_syntax_and_comparison() {
        let input = r#"fn test<T>(val: T, x: i64) -> i64 {
    if x < 10 {
        return x;
    }
    return 0;
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, input);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_string_escapes_and_numbers() {
        let input = r#"pub fn main() -> i64 {
    let s: &str = "Hello\nWorld\t\"Vanta\"\0!";
    let hex = 0xDEAD_BEEF;
    let bin = 0b1010_0101;
    let big = 1_000_000;
    return 0;
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, input);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_if_else_chaining_and_loops() {
        let input = r#"pub fn check_val(x: i64) -> i64 {
    if x > 10 {
        return 1;
    } else if x == 0 {
        return 0;
    } else {
        let mut i: i64 = 0;
        while i < 10 {
            i += 1;
        }
        return -1;
    }
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, input);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_arrays_and_indexing() {
        let input = r#"pub fn main() -> i64 {
        let mut arr: [i64; 4] = [10, 20, 30, 40];
        arr[2] = 99;
        return arr[0] + arr[2];
}
"#;
        let expected = r#"pub fn main() -> i64 {
    let mut arr: [i64; 4] = [10, 20, 30, 40];
    arr[2] = 99;
    return arr[0] + arr[2];
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, expected);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_nested_block_comments() {
        let input = r#"/* outer /* nested */ comment */
pub fn main() -> i64 {
    return 42;
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, input);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_all_repo_example_files_idempotent() {
        let examples = [
            include_str!("../../../examples/01_return_42.va"),
            include_str!("../../../examples/02_arithmetic.va"),
            include_str!("../../../examples/03_while_loop.va"),
            include_str!("../../../examples/04_functions.va"),
            include_str!("../../../examples/05_pointers_and_references.va"),
            include_str!("../../../examples/06_strings_and_stdout.va"),
            include_str!("../../../examples/07_ai_first_check.va"),
            include_str!("../../../examples/09_array_bounds_safe.va"),
            include_str!("../../../examples/10_panic_handling.va"),
            include_str!("../../../examples/11_enums_and_patterns.va"),
            include_str!("../../../benchmarks/micro/branch_folding.va"),
            include_str!("../../../benchmarks/micro/constant_arithmetic.va"),
            include_str!("../../../benchmarks/micro/dead_code.va"),
            include_str!("../../../benchmarks/micro/loop_computation.va"),
        ];

        for (i, code) in examples.iter().enumerate() {
            let res = format_source(code, &format!("example_{}.va", i));
            assert!(
                res.is_ok(),
                "Failed to format example {}: {:?}",
                i,
                res.err()
            );
            let formatted1 = res.unwrap();
            let formatted2 = format_source(&formatted1, &format!("example_{}.va", i)).unwrap();
            assert_eq!(
                formatted1, formatted2,
                "Idempotence failed for example {}:\nDiff:\nFirst:\n{}\nSecond:\n{}",
                i, formatted1, formatted2
            );
        }
    }

    #[test]
    fn test_format_enum_and_match() {
        let input = r#"
enum Color {
Red,
Green,
Blue
}

pub fn check_color(c: Color) -> i64 {
match c {
Color::Red => 1,
Color::Green => 2,
Color::Blue => 3,
_ => 0
}
}
"#;
        let expected = r#"enum Color {
    Red,
    Green,
    Blue,
}

pub fn check_color(c: Color) -> i64 {
    match c {
        Color::Red => 1,
        Color::Green => 2,
        Color::Blue => 3,
        _ => 0,
    }
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, expected);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_format_mod_and_import() {
        let input = r#"
mod math;
pub mod models;

import math::{add, multiply};
pub import models::Point;

pub fn main() {
let res = add(1, 2);
}
"#;
        let expected = r#"mod math;
pub mod models;

import math::{add, multiply};
pub import models::Point;

pub fn main() {
    let res = add(1, 2);
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, expected);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_format_trait_and_impl() {
        let input = r#"
pub trait Describe {
fn describe(self: &Self) -> i64;
}

impl Describe for User {
pub fn describe(self: &User) -> i64 {
return 42;
}
}

fn print_val<T: Describe>(val: &T) {
val.describe();
}
"#;
        let expected = r#"pub trait Describe {
    fn describe(self: &Self) -> i64;
}

impl Describe for User {
    pub fn describe(self: &User) -> i64 {
        return 42;
    }
}

fn print_val<T: Describe>(val: &T) {
    val.describe();
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, expected);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }

    #[test]
    fn test_format_for_loop_and_iterator() {
        let input = r#"
pub trait Iterator<T> {
fn next(self: &mut Self) -> Option<T>;
}

impl Iterator<i64> for RangeI64 {
pub fn next(self: &mut RangeI64) -> Option<i64> {
return Option::None;
}
}

pub fn main() {
for x in range_i64(0, 10) {
print_i64(x);
}
}
"#;
        let expected = r#"pub trait Iterator<T> {
    fn next(self: &mut Self) -> Option<T>;
}

impl Iterator<i64> for RangeI64 {
    pub fn next(self: &mut RangeI64) -> Option<i64> {
        return Option::None;
    }
}

pub fn main() {
    for x in range_i64(0, 10) {
        print_i64(x);
    }
}
"#;
        let actual = format_source(input, "test.va").unwrap();
        assert_eq!(actual, expected);
        assert_eq!(format_source(&actual, "test.va").unwrap(), actual);
    }
}
