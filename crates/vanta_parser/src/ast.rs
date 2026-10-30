//! Abstract Syntax Tree for Vanta.

use vanta_source::Span;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl Ident {
    pub fn new(name: impl Into<String>, span: Span) -> Self {
        Self {
            name: name.into(),
            span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimType {
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
    Bool,
    Str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExprKind {
    Primitive(PrimType),
    Reference {
        lifetime: Option<Ident>,
        is_mut: bool,
        inner: Box<TypeExpr>,
    },
    Array {
        inner: Box<TypeExpr>,
        len: usize,
    },
    Slice {
        lifetime: Option<Ident>,
        inner: Box<TypeExpr>,
    },
    Named(String),
    Generic {
        name: String,
        type_args: Vec<TypeExpr>,
    },
    TraitObject {
        trait_name: String,
        lifetime: Option<Ident>,
    },
    Lifetime(Ident),
    Unit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

impl TypeExpr {
    pub fn new(kind: TypeExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Function(FnDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Impl(ImplBlock),
    Mod(ModDecl),
    Import(ImportDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub name: Ident,
    pub bounds: Vec<Ident>,
    pub bound: Option<Ident>,
    pub span: Span,
}

impl std::ops::Deref for TypeParam {
    type Target = Ident;
    fn deref(&self) -> &Self::Target {
        &self.name
    }
}

impl TypeParam {
    pub fn new(name: Ident, bounds: Vec<Ident>, span: Span) -> Self {
        let bound = bounds.first().cloned();
        Self {
            name,
            bounds,
            bound,
            span,
        }
    }
}

impl From<Ident> for TypeParam {
    fn from(name: Ident) -> Self {
        let span = name.span;
        Self {
            name,
            bounds: Vec::new(),
            bound: None,
            span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub lifetime_params: Vec<Ident>,
    pub type_params: Vec<TypeParam>,
    pub methods: Vec<TraitMethodDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethodDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret_type: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub is_pub: bool,
    pub path: Vec<Ident>,
    pub items: Vec<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub lifetime_params: Vec<Ident>,
    pub type_params: Vec<TypeParam>,
    pub variants: Vec<VariantDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantDecl {
    pub name: Ident,
    pub payload: Vec<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImplBlock {
    pub lifetime_params: Vec<Ident>,
    pub type_params: Vec<TypeParam>,
    pub trait_name: Option<Ident>,
    pub trait_args: Vec<TypeExpr>,
    pub target: Ident,
    pub target_args: Vec<TypeExpr>,
    pub methods: Vec<FnDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub lifetime_params: Vec<Ident>,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub ret_type: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub is_mut: bool,
    pub name: Ident,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub lifetime_params: Vec<Ident>,
    pub type_params: Vec<TypeParam>,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub expr: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        is_mut: bool,
        name: Ident,
        ty: Option<TypeExpr>,
        init: Option<Expr>,
        span: Span,
    },
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    Loop {
        body: Block,
        span: Span,
    },
    For {
        var: Ident,
        iter: Expr,
        body: Block,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    String(String),
    Ident(String),
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Ref {
        is_mut: bool,
        expr: Box<Expr>,
    },
    Deref(Box<Expr>),
    Call {
        callee: String,
        type_args: Vec<TypeExpr>,
        args: Vec<Expr>,
    },
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    FieldAccess {
        target: Box<Expr>,
        field: Ident,
    },
    MethodCall {
        target: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
    },
    If {
        cond: Box<Expr>,
        then_branch: Block,
        else_branch: Option<ElseBranch>,
    },
    Block(Block),
    ArrayLiteral(Vec<Expr>),
    StructLiteral {
        name: Ident,
        type_args: Vec<TypeExpr>,
        fields: Vec<(Ident, Expr)>,
    },
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    VariantConstructor {
        enum_name: Option<Ident>,
        variant: Ident,
        type_args: Vec<TypeExpr>,
        args: Vec<Expr>,
    },
    Try(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

impl Pattern {
    pub fn new(kind: PatternKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternKind {
    Wildcard,
    Ident(String),
    Variant {
        enum_name: Option<Ident>,
        variant: Ident,
        subpatterns: Vec<Pattern>,
    },
    Bool(bool),
    Int(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElseBranch {
    If(Box<Expr>),
    Block(Block),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}
