//! Semantic analysis: Symbol resolution, Type checking, and Borrow checking for Vanta.

pub mod types;

use std::collections::HashMap;
pub use types::Type;
use vanta_diagnostics::{Diagnostic, DiagnosticBag};
use vanta_parser::ast::*;
use vanta_source::Span;

/// A unique ID for a resolved variable symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub ty: Type,
    pub is_mut: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnSignature {
    pub name: String,
    pub params: Vec<(String, Type, bool)>, // (name, type, is_mut)
    pub ret_type: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, Type)>,
    pub span: Span,
}

/// Status of a variable in the borrow checker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitState {
    Uninitialized,
    Initialized,
    Moved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BorrowState {
    None,
    Shared(usize),
    Exclusive,
}

#[derive(Debug, Clone)]
pub struct VarState {
    pub init: InitState,
    pub borrow: BorrowState,
}

pub struct SemanticContext {
    pub functions: HashMap<String, FnSignature>,
    pub structs: HashMap<String, StructDef>,
    pub symbols: HashMap<SymbolId, Symbol>,
    pub expr_types: HashMap<*const Expr, Type>,
    pub resolved_symbols: HashMap<*const Expr, SymbolId>,
    next_sym_id: u32,
    scopes: Vec<HashMap<String, SymbolId>>,
    var_states: HashMap<SymbolId, VarState>,
    current_fn_ret_type: Option<Type>,
}

impl Default for SemanticContext {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticContext {
    pub fn new() -> Self {
        let mut functions = HashMap::new();
        functions.insert(
            "print_i64".into(),
            FnSignature {
                name: "print_i64".into(),
                params: vec![("value".into(), Type::I64, false)],
                ret_type: Type::Unit,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "print_str".into(),
            FnSignature {
                name: "print_str".into(),
                params: vec![(
                    "value".into(),
                    Type::Ref {
                        is_mut: false,
                        inner: Box::new(Type::Str),
                    },
                    false,
                )],
                ret_type: Type::Unit,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );

        Self {
            functions,
            structs: HashMap::new(),
            symbols: HashMap::new(),
            expr_types: HashMap::new(),
            resolved_symbols: HashMap::new(),
            next_sym_id: 1,
            scopes: vec![HashMap::new()],
            var_states: HashMap::new(),
            current_fn_ret_type: None,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            // Drop variables going out of scope
            for (_, sym_id) in scope {
                self.var_states.remove(&sym_id);
            }
        }
    }

    fn declare_symbol(
        &mut self,
        name: String,
        ty: Type,
        is_mut: bool,
        span: Span,
        init: InitState,
        diags: &mut DiagnosticBag,
    ) -> SymbolId {
        if ty == Type::Str {
            diags.emit(
                Diagnostic::error(format!(
                    "Variable '{}' has type 'str'; 'str' cannot be used directly as a sized value, use '&str'",
                    name
                ))
                .with_span(span),
            );
        }

        let current_scope = self.scopes.last_mut().expect("At least one scope exists");
        if current_scope.contains_key(&name) {
            diags.emit(
                Diagnostic::error(format!(
                    "Identifier '{}' is already declared in this scope",
                    name
                ))
                .with_span(span),
            );
        }

        let id = SymbolId(self.next_sym_id);
        self.next_sym_id += 1;

        let sym = Symbol {
            id,
            name: name.clone(),
            ty,
            is_mut,
            span,
        };
        self.symbols.insert(id, sym);
        current_scope.insert(name, id);
        self.var_states.insert(
            id,
            VarState {
                init,
                borrow: BorrowState::None,
            },
        );
        id
    }

    pub fn lookup_symbol(&self, name: &str) -> Option<SymbolId> {
        for scope in self.scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(id);
            }
        }
        None
    }

    pub fn type_from_ast(&self, ty_expr: &TypeExpr, diags: &mut DiagnosticBag) -> Type {
        match &ty_expr.kind {
            TypeExprKind::Primitive(p) => match p {
                PrimType::I8 => Type::I8,
                PrimType::I16 => Type::I16,
                PrimType::I32 => Type::I32,
                PrimType::I64 => Type::I64,
                PrimType::U8 => Type::U8,
                PrimType::U16 => Type::U16,
                PrimType::U32 => Type::U32,
                PrimType::U64 => Type::U64,
                PrimType::F32 => Type::F32,
                PrimType::F64 => Type::F64,
                PrimType::Bool => Type::Bool,
                PrimType::Str => Type::Str,
            },
            TypeExprKind::Reference { is_mut, inner } => {
                let inner_ty = self.type_from_ast(inner, diags);
                if *is_mut && inner_ty == Type::Str {
                    diags.emit(
                        Diagnostic::error(
                            "Mutable string slices '&mut str' are not supported; string slices must be immutable '&str'",
                        )
                        .with_span(ty_expr.span),
                    );
                }
                Type::Ref {
                    is_mut: *is_mut,
                    inner: Box::new(inner_ty),
                }
            }
            TypeExprKind::Array { inner, len } => Type::Array {
                inner: Box::new(self.type_from_ast(inner, diags)),
                len: *len,
            },
            TypeExprKind::Named(name) => {
                if name == "str" {
                    Type::Str
                } else if self.structs.contains_key(name) {
                    Type::Struct(name.clone())
                } else {
                    diags.emit(
                        Diagnostic::error(format!("Unknown type '{}'", name))
                            .with_span(ty_expr.span),
                    );
                    Type::Unit
                }
            }
            TypeExprKind::Unit => Type::Unit,
        }
    }
}

pub struct SemanticAnalyzer {
    ctx: SemanticContext,
}

impl Default for SemanticAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            ctx: SemanticContext::new(),
        }
    }

    pub fn analyze(mut self, program: &Program, diags: &mut DiagnosticBag) -> SemanticContext {
        // Pass 1: Collect struct declarations
        for item in &program.items {
            if let Item::Struct(s) = item {
                let mut fields = Vec::new();
                for f in &s.fields {
                    let f_ty = self.ctx.type_from_ast(&f.ty, diags);
                    fields.push((f.name.name.clone(), f_ty));
                }
                self.ctx.structs.insert(
                    s.name.name.clone(),
                    StructDef {
                        name: s.name.name.clone(),
                        fields,
                        span: s.span,
                    },
                );
            }
        }

        // Pass 2: Collect function signatures
        for item in &program.items {
            if let Item::Function(f) = item {
                if f.name.name == "print_i64" || f.name.name == "print_str" {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Cannot redefine built-in intrinsic '{}'",
                            f.name.name
                        ))
                        .with_span(f.name.span),
                    );
                    continue;
                }

                let mut params = Vec::new();
                for p in &f.params {
                    let p_ty = self.ctx.type_from_ast(&p.ty, diags);
                    if p_ty == Type::Str {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Parameter '{}' has type 'str'; 'str' cannot be used directly as a sized value, use '&str'",
                                p.name.name
                            ))
                            .with_span(p.span),
                        );
                    }
                    params.push((p.name.name.clone(), p_ty, p.is_mut));
                }
                let ret_type = match &f.ret_type {
                    Some(t) => self.ctx.type_from_ast(t, diags),
                    None => Type::Unit,
                };
                if ret_type.is_str_slice() {
                    diags.emit(
                        Diagnostic::error(
                            "Returning '&str' from user-defined functions is not yet supported in Milestone 3",
                        )
                        .with_span(f.span),
                    );
                }

                if self.ctx.functions.contains_key(&f.name.name) {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Duplicate function definition '{}'",
                            f.name.name
                        ))
                        .with_span(f.name.span),
                    );
                } else {
                    self.ctx.functions.insert(
                        f.name.name.clone(),
                        FnSignature {
                            name: f.name.name.clone(),
                            params,
                            ret_type,
                            span: f.span,
                        },
                    );
                }
            }
        }

        // Pass 3: Check function bodies
        for item in &program.items {
            if let Item::Function(f) = item {
                self.check_function(f, diags);
            }
        }

        self.ctx
    }

    fn check_function(&mut self, f: &FnDecl, diags: &mut DiagnosticBag) {
        let sig = self.ctx.functions.get(&f.name.name).cloned().unwrap();
        self.ctx.current_fn_ret_type = Some(sig.ret_type.clone());
        self.ctx.push_scope();

        for (name, ty, is_mut) in &sig.params {
            self.ctx.declare_symbol(
                name.clone(),
                ty.clone(),
                *is_mut,
                f.span,
                InitState::Initialized,
                diags,
            );
        }

        let body_type = self.check_block(&f.body, diags);

        // Verify return type consistency
        if sig.ret_type != Type::Unit && body_type != sig.ret_type {
            // Check if there was an explicit return statement or if tail expression matches
            if !self.block_has_return(&f.body) {
                diags.emit(
                    Diagnostic::error(format!(
                        "Function '{}' expects return type {:?}, but body produces {:?}",
                        f.name.name, sig.ret_type, body_type
                    ))
                    .with_span(f.body.span),
                );
            }
        }

        self.ctx.pop_scope();
        self.ctx.current_fn_ret_type = None;
    }

    fn block_has_return(&self, block: &Block) -> bool {
        for stmt in &block.stmts {
            if matches!(stmt, Stmt::Return { .. }) {
                return true;
            }
        }
        false
    }

    fn check_block(&mut self, block: &Block, diags: &mut DiagnosticBag) -> Type {
        self.ctx.push_scope();
        for stmt in &block.stmts {
            self.check_stmt(stmt, diags);
        }

        let tail_type = if let Some(expr) = &block.expr {
            self.check_expr(expr, diags)
        } else {
            Type::Unit
        };

        self.ctx.pop_scope();
        tail_type
    }

    fn check_stmt(&mut self, stmt: &Stmt, diags: &mut DiagnosticBag) {
        match stmt {
            Stmt::Let {
                is_mut,
                name,
                ty,
                init,
                span,
            } => {
                let declared_ty = ty.as_ref().map(|t| self.ctx.type_from_ast(t, diags));

                let (inferred_ty, init_state) = if let Some(init_expr) = init {
                    let expr_ty = self.check_expr(init_expr, diags);
                    if let Some(dec_ty) = &declared_ty {
                        if expr_ty != *dec_ty {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Type mismatch in let binding: expected {:?}, found {:?}",
                                    dec_ty, expr_ty
                                ))
                                .with_span(init_expr.span),
                            );
                        }
                    }
                    (declared_ty.unwrap_or(expr_ty), InitState::Initialized)
                } else {
                    let ty = declared_ty.unwrap_or_else(|| {
                        diags.emit(
                            Diagnostic::error(
                                "Type annotation required when let binding is not initialized",
                            )
                            .with_span(*span),
                        );
                        Type::I64
                    });
                    (ty, InitState::Uninitialized)
                };

                self.ctx.declare_symbol(
                    name.name.clone(),
                    inferred_ty,
                    *is_mut,
                    *span,
                    init_state,
                    diags,
                );
            }
            Stmt::Assign {
                target,
                op: _,
                value,
                span,
            } => {
                let val_ty = self.check_expr(value, diags);

                // Borrow checking & mutability check on target
                match &target.kind {
                    ExprKind::Ident(name) => {
                        if let Some(sym_id) = self.ctx.lookup_symbol(name) {
                            let sym = self.ctx.symbols.get(&sym_id).cloned().unwrap();
                            if !sym.is_mut {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Cannot assign to immutable variable '{}'",
                                        name
                                    ))
                                    .with_span(*span),
                                );
                            }

                            // Check borrow state
                            if let Some(vstate) = self.ctx.var_states.get(&sym_id) {
                                match vstate.borrow {
                                    BorrowState::Shared(_) => {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Cannot assign to '{}' because it is borrowed",
                                                name
                                            ))
                                            .with_span(*span),
                                        );
                                    }
                                    BorrowState::Exclusive => {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Cannot assign to '{}' because it is mutably borrowed",
                                                name
                                            ))
                                            .with_span(*span),
                                        );
                                    }
                                    BorrowState::None => {}
                                }
                            }

                            if sym.ty != val_ty {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Cannot assign value of type {:?} to variable of type {:?}",
                                        val_ty, sym.ty
                                    ))
                                    .with_span(*span),
                                );
                            }

                            // Mark initialized
                            if let Some(vstate) = self.ctx.var_states.get_mut(&sym_id) {
                                vstate.init = InitState::Initialized;
                            }
                        } else {
                            diags.emit(
                                Diagnostic::error(format!("Undefined variable '{}'", name))
                                    .with_span(target.span),
                            );
                        }
                    }
                    ExprKind::Deref(inner) => {
                        let inner_ty = self.check_expr(inner, diags);
                        match inner_ty {
                            Type::Ref {
                                is_mut: true,
                                inner: ref_target_ty,
                            } => {
                                if *ref_target_ty != val_ty {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Type mismatch in dereference assignment: expected {:?}, found {:?}",
                                            ref_target_ty, val_ty
                                        ))
                                        .with_span(*span),
                                    );
                                }
                            }
                            _ => {
                                diags.emit(
                                    Diagnostic::error("Cannot assign through immutable reference")
                                        .with_span(*span),
                                );
                            }
                        }
                    }
                    _ => {
                        let target_ty = self.check_expr(target, diags);
                        if target_ty != val_ty {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Type mismatch in assignment: target {:?}, value {:?}",
                                    target_ty, val_ty
                                ))
                                .with_span(*span),
                            );
                        }
                    }
                }
            }
            Stmt::While {
                cond,
                body,
                span: _,
            } => {
                let cond_ty = self.check_expr(cond, diags);
                if cond_ty != Type::Bool {
                    diags.emit(
                        Diagnostic::error(format!(
                            "While condition must be 'bool', found {:?}",
                            cond_ty
                        ))
                        .with_span(cond.span),
                    );
                }
                self.check_block(body, diags);
            }
            Stmt::Loop { body, span: _ } => {
                self.check_block(body, diags);
            }
            Stmt::Return { value, span } => {
                let ret_ty = if let Some(expr) = value {
                    self.check_expr(expr, diags)
                } else {
                    Type::Unit
                };

                if let Some(expected) = &self.ctx.current_fn_ret_type {
                    if *expected != ret_ty {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Return type mismatch: expected {:?}, found {:?}",
                                expected, ret_ty
                            ))
                            .with_span(*span),
                        );
                    }
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::Expr(expr) => {
                self.check_expr(expr, diags);
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr, diags: &mut DiagnosticBag) -> Type {
        let ty = match &expr.kind {
            ExprKind::Int(_) => Type::I64,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::String(_) => Type::Ref {
                is_mut: false,
                inner: Box::new(Type::Str),
            },
            ExprKind::Ident(name) => {
                if let Some(sym_id) = self.ctx.lookup_symbol(name) {
                    let sym = self.ctx.symbols.get(&sym_id).cloned().unwrap();

                    // Check definite initialization
                    if let Some(vstate) = self.ctx.var_states.get(&sym_id) {
                        match vstate.init {
                            InitState::Uninitialized => {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Use of uninitialized variable '{}'",
                                        name
                                    ))
                                    .with_span(expr.span),
                                );
                            }
                            InitState::Moved => {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Use of moved-out variable '{}'",
                                        name
                                    ))
                                    .with_span(expr.span),
                                );
                            }
                            InitState::Initialized => {}
                        }
                    }

                    sym.ty
                } else {
                    diags.emit(
                        Diagnostic::error(format!("Undefined identifier '{}'", name))
                            .with_span(expr.span),
                    );
                    Type::I64
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs_ty = self.check_expr(lhs, diags);
                let rhs_ty = self.check_expr(rhs, diags);

                match op {
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Rem => {
                        if !lhs_ty.is_integer() || lhs_ty != rhs_ty {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Arithmetic operator '{:?}' requires matching integer types, found {:?} and {:?}",
                                    op, lhs_ty, rhs_ty
                                ))
                                .with_span(expr.span),
                            );
                        }
                        lhs_ty
                    }
                    BinaryOp::Eq | BinaryOp::NotEq => {
                        if lhs_ty != rhs_ty {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Equality comparison requires matching types, found {:?} and {:?}",
                                    lhs_ty, rhs_ty
                                ))
                                .with_span(expr.span),
                            );
                        }
                        Type::Bool
                    }
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        if !lhs_ty.is_integer() || lhs_ty != rhs_ty {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Relational comparison requires matching integer types, found {:?} and {:?}",
                                    lhs_ty, rhs_ty
                                ))
                                .with_span(expr.span),
                            );
                        }
                        Type::Bool
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        if lhs_ty != Type::Bool || rhs_ty != Type::Bool {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Logical operator '{:?}' requires 'bool', found {:?} and {:?}",
                                    op, lhs_ty, rhs_ty
                                ))
                                .with_span(expr.span),
                            );
                        }
                        Type::Bool
                    }
                    BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                        if !lhs_ty.is_integer() || lhs_ty != rhs_ty {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Bitwise operator '{:?}' requires matching integer types, found {:?} and {:?}",
                                    op, lhs_ty, rhs_ty
                                ))
                                .with_span(expr.span),
                            );
                        }
                        lhs_ty
                    }
                    BinaryOp::Shl | BinaryOp::Shr => {
                        if !lhs_ty.is_integer() || !rhs_ty.is_integer() {
                            diags.emit(
                                Diagnostic::error("Shift operations require integer operands")
                                    .with_span(expr.span),
                            );
                        }
                        lhs_ty
                    }
                }
            }
            ExprKind::Unary { op, expr: inner } => {
                let inner_ty = self.check_expr(inner, diags);
                match op {
                    UnaryOp::Neg => {
                        if !inner_ty.is_signed_integer() {
                            diags.emit(
                                Diagnostic::error("Negation '-' requires a signed integer type")
                                    .with_span(expr.span),
                            );
                        }
                        inner_ty
                    }
                    UnaryOp::Not => {
                        if inner_ty != Type::Bool && !inner_ty.is_integer() {
                            diags.emit(
                                Diagnostic::error(
                                    "Logical/Bitwise NOT '!' requires 'bool' or integer type",
                                )
                                .with_span(expr.span),
                            );
                        }
                        inner_ty
                    }
                }
            }
            ExprKind::Ref {
                is_mut,
                expr: inner,
            } => {
                // Borrow Checker: Borrowing an identifier
                if let ExprKind::Ident(name) = &inner.kind {
                    if let Some(sym_id) = self.ctx.lookup_symbol(name) {
                        let sym = self.ctx.symbols.get(&sym_id).cloned().unwrap();
                        if *is_mut && !sym.is_mut {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Cannot borrow immutable variable '{}' as mutable",
                                    name
                                ))
                                .with_span(expr.span),
                            );
                        }

                        if let Some(vstate) = self.ctx.var_states.get_mut(&sym_id) {
                            if *is_mut {
                                match vstate.borrow {
                                    BorrowState::None => {
                                        vstate.borrow = BorrowState::Exclusive;
                                    }
                                    BorrowState::Shared(_) | BorrowState::Exclusive => {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Cannot borrow '{}' as mutable more than once at a time",
                                                name
                                            ))
                                            .with_span(expr.span),
                                        );
                                    }
                                }
                            } else {
                                match vstate.borrow {
                                    BorrowState::None => {
                                        vstate.borrow = BorrowState::Shared(1);
                                    }
                                    BorrowState::Shared(n) => {
                                        vstate.borrow = BorrowState::Shared(n + 1);
                                    }
                                    BorrowState::Exclusive => {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Cannot borrow '{}' as immutable because it is already borrowed as mutable",
                                                name
                                            ))
                                            .with_span(expr.span),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                let inner_ty = self.check_expr(inner, diags);
                Type::Ref {
                    is_mut: *is_mut,
                    inner: Box::new(inner_ty),
                }
            }
            ExprKind::Deref(inner) => {
                let inner_ty = self.check_expr(inner, diags);
                match inner_ty {
                    Type::Ref {
                        inner: target_ty, ..
                    } => *target_ty,
                    _ => {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Cannot dereference non-reference type {:?}",
                                inner_ty
                            ))
                            .with_span(expr.span),
                        );
                        Type::I64
                    }
                }
            }
            ExprKind::Call { callee, args } => {
                if let Some(sig) = self.ctx.functions.get(callee).cloned() {
                    if sig.params.len() != args.len() {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Function '{}' expects {} arguments, but {} were provided",
                                callee,
                                sig.params.len(),
                                args.len()
                            ))
                            .with_span(expr.span),
                        );
                    }

                    for (idx, arg) in args.iter().enumerate() {
                        let arg_ty = self.check_expr(arg, diags);
                        if let Some((_, param_ty, _)) = sig.params.get(idx) {
                            if arg_ty != *param_ty {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Argument {} to function '{}' expects {}, found {}",
                                        idx + 1,
                                        callee,
                                        param_ty,
                                        arg_ty
                                    ))
                                    .with_span(arg.span),
                                );
                            }
                        }
                    }

                    sig.ret_type
                } else {
                    diags.emit(
                        Diagnostic::error(format!("Call to undefined function '{}'", callee))
                            .with_span(expr.span),
                    );
                    Type::I64
                }
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.check_expr(cond, diags);
                if cond_ty != Type::Bool {
                    diags.emit(
                        Diagnostic::error(format!(
                            "'if' condition must be 'bool', found {:?}",
                            cond_ty
                        ))
                        .with_span(cond.span),
                    );
                }

                let then_ty = self.check_block(then_branch, diags);
                let else_ty = match else_branch {
                    Some(ElseBranch::If(else_if)) => self.check_expr(else_if, diags),
                    Some(ElseBranch::Block(else_block)) => self.check_block(else_block, diags),
                    None => Type::Unit,
                };

                if then_ty != else_ty {
                    diags.emit(
                        Diagnostic::error(format!(
                            "'if' branches have incompatible types: then has {:?}, else has {:?}",
                            then_ty, else_ty
                        ))
                        .with_span(expr.span),
                    );
                }

                then_ty
            }
            ExprKind::Block(block) => self.check_block(block, diags),
            ExprKind::ArrayLiteral(elements) => {
                if elements.is_empty() {
                    Type::Array {
                        inner: Box::new(Type::I64),
                        len: 0,
                    }
                } else {
                    let first_ty = self.check_expr(&elements[0], diags);
                    for (i, elem) in elements.iter().enumerate().skip(1) {
                        let elem_ty = self.check_expr(elem, diags);
                        if elem_ty != first_ty {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Array element {} has type {:?}, expected {:?}",
                                    i, elem_ty, first_ty
                                ))
                                .with_span(elem.span),
                            );
                        }
                    }
                    Type::Array {
                        inner: Box::new(first_ty),
                        len: elements.len(),
                    }
                }
            }
            ExprKind::Index { array, index } => {
                let arr_ty = self.check_expr(array, diags);
                let idx_ty = self.check_expr(index, diags);

                if !idx_ty.is_integer() {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Array index must be integer, found {:?}",
                            idx_ty
                        ))
                        .with_span(index.span),
                    );
                }

                match arr_ty {
                    Type::Array { inner, .. } => *inner,
                    Type::Ref { inner, .. } => match *inner {
                        Type::Array { inner: elem_ty, .. } => *elem_ty,
                        _ => {
                            diags.emit(
                                Diagnostic::error("Indexing requires an array or array reference")
                                    .with_span(array.span),
                            );
                            Type::I64
                        }
                    },
                    _ => {
                        diags.emit(
                            Diagnostic::error("Indexing requires an array type")
                                .with_span(array.span),
                        );
                        Type::I64
                    }
                }
            }
            ExprKind::FieldAccess { target, field } => {
                let target_ty = self.check_expr(target, diags);
                match target_ty {
                    Type::Struct(struct_name) => {
                        if let Some(s_def) = self.ctx.structs.get(&struct_name) {
                            if let Some((_, f_ty)) =
                                s_def.fields.iter().find(|(name, _)| name == &field.name)
                            {
                                f_ty.clone()
                            } else {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Struct '{}' has no field named '{}'",
                                        struct_name, field.name
                                    ))
                                    .with_span(field.span),
                                );
                                Type::I64
                            }
                        } else {
                            Type::I64
                        }
                    }
                    _ => {
                        diags.emit(
                            Diagnostic::error("Field access requires struct type")
                                .with_span(target.span),
                        );
                        Type::I64
                    }
                }
            }
            ExprKind::StructLiteral { name, fields } => {
                if let Some(s_def) = self.ctx.structs.get(&name.name).cloned() {
                    for (f_ident, f_expr) in fields {
                        let val_ty = self.check_expr(f_expr, diags);
                        if let Some((_, expected_ty)) =
                            s_def.fields.iter().find(|(n, _)| n == &f_ident.name)
                        {
                            if val_ty != *expected_ty {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Field '{}' expects type {:?}, found {:?}",
                                        f_ident.name, expected_ty, val_ty
                                    ))
                                    .with_span(f_expr.span),
                                );
                            }
                        } else {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "No field named '{}' in struct '{}'",
                                    f_ident.name, name.name
                                ))
                                .with_span(f_ident.span),
                            );
                        }
                    }
                    Type::Struct(name.name.clone())
                } else {
                    diags.emit(
                        Diagnostic::error(format!("Undefined struct '{}'", name.name))
                            .with_span(name.span),
                    );
                    Type::I64
                }
            }
        };

        ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_lexer::Lexer;
    use vanta_parser::Parser;
    use vanta_source::SourceMap;

    #[test]
    fn test_typecheck_and_borrowcheck_valid() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "valid.va".into(),
            r#"
            fn add(a: i64, b: i64) -> i64 {
                let mut x: i64 = a;
                x += b;
                return x;
            }
            "#
            .into(),
        );
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        assert!(!diags.has_errors());

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        assert!(!diags.has_errors());

        let analyzer = SemanticAnalyzer::new();
        let _ctx = analyzer.analyze(&program, &mut diags);
        assert!(!diags.has_errors());
    }

    #[test]
    fn test_borrowcheck_cannot_borrow_mut_immutable() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "bad_borrow.va".into(),
            r#"
            fn test() {
                let x: i64 = 42;
                let r = &mut x;
            }
            "#
            .into(),
        );
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);

        let analyzer = SemanticAnalyzer::new();
        let _ctx = analyzer.analyze(&program, &mut diags);
        assert!(diags.has_errors());
        assert!(diags.diagnostics().iter().any(|d| d
            .message
            .contains("Cannot borrow immutable variable 'x' as mutable")));
    }
}
