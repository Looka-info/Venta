//! Semantic analysis: Symbol resolution, Type checking, and Borrow checking for Vanta.

pub mod module;
pub mod types;

pub use module::{ModuleId, ModuleNode, ModuleTree};
use std::collections::{HashMap, HashSet};
pub use types::{EnumDef, StructDef, StructField, Type, VariantDef};
use vanta_diagnostics::{codes, Diagnostic, DiagnosticBag};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Param,
    Local,
    Static,
}

#[derive(Debug, Clone)]
pub struct VarState {
    pub init: InitState,
    pub borrow: BorrowState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitMethodSig {
    pub name: String,
    pub is_mut_self: bool,
    pub params: Vec<(String, Type, bool)>, // all parameters including self
    pub ret_type: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitDef {
    pub name: String,
    pub canonical_name: String,
    pub is_pub: bool,
    pub type_params: Vec<TypeParam>,
    pub methods: HashMap<String, TraitMethodSig>,
    pub ast_methods: Vec<TraitMethodDecl>,
    pub defining_module: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitImplDef {
    pub trait_canonical: String,
    pub target_canonical: String,
    pub type_args: Vec<Type>,
    pub defining_module: String,
    pub methods: HashMap<String, String>, // method_name -> mangled_name
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct ModuleScope {
    pub id: ModuleId,
    pub name: String,
    pub path: Vec<String>,
    pub canonical_prefix: String,
    pub functions: HashMap<String, String>, // local name -> canonical mangled name
    pub structs: HashMap<String, String>,   // local name -> canonical mangled name
    pub enums: HashMap<String, String>,     // local name -> canonical mangled name
    pub traits: HashMap<String, String>,    // local name -> canonical mangled name
    pub pub_items: HashSet<String>,         // local names of public items
    pub imported_items: HashSet<String>,    // items imported via `import`
}

pub struct SemanticContext {
    pub functions: HashMap<String, FnSignature>,
    pub structs: HashMap<String, StructDef>,
    pub declared_struct_names: HashSet<String>,
    pub generic_structs: HashMap<String, StructDecl>,
    pub enums: HashMap<String, EnumDef>,
    pub declared_enum_names: HashSet<String>,
    pub generic_enums: HashMap<String, EnumDecl>,
    pub traits: HashMap<String, TraitDef>,
    pub declared_trait_names: HashSet<String>,
    pub trait_impls: HashMap<(String, String), TraitImplDef>,
    pub trait_methods: HashMap<(String, String, String), String>, // (target_canon, trait_canon, method_name) -> mangled_name
    pub variant_constructor_enum_names: HashMap<Span, String>,
    pub generic_functions: HashMap<String, FnDecl>,
    pub monomorphized_functions: Vec<FnDecl>,
    pub call_callees: HashMap<Span, String>,
    pub struct_literal_names: HashMap<Span, String>,
    pub generic_depth: usize,
    pub methods: HashMap<(String, String), String>,
    pub symbols: HashMap<SymbolId, Symbol>,
    pub expr_types: HashMap<*const Expr, Type>,
    pub resolved_symbols: HashMap<*const Expr, SymbolId>,
    pub current_impl_target: Option<String>,
    pub module_scopes: HashMap<ModuleId, ModuleScope>,
    pub current_module: ModuleId,
    pub pub_methods: HashSet<String>,
    pub module_tree: Option<ModuleTree>,
    next_sym_id: u32,
    scopes: Vec<HashMap<String, SymbolId>>,
    var_states: HashMap<SymbolId, VarState>,
    current_fn_ret_type: Option<Type>,
    pub expected_type: Option<Type>,
    pub generic_enum_type_args: HashMap<String, (String, Vec<Type>)>,
    pub active_lifetime_params: HashSet<String>,
    pub local_storage_symbols: HashSet<SymbolId>,
    pub ref_provenances: HashMap<SymbolId, Provenance>,
    pub borrowed_sources: HashMap<SymbolId, SymbolId>,
}

impl Default for SemanticContext {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticContext {
    pub fn can_coerce(&self, from: &Type, to: &Type) -> bool {
        if from == to {
            return true;
        }
        if from.is_integer() && to.is_integer() {
            return true;
        }
        if let (
            Type::Ref {
                is_mut: false,
                inner: from_inner,
            },
            Type::Ref {
                is_mut: false,
                inner: to_inner,
            },
        ) = (from, to)
        {
            if let (
                Type::Array {
                    inner: from_elem, ..
                },
                Type::Slice(to_elem),
            ) = (&**from_inner, &**to_inner)
            {
                return from_elem == to_elem;
            }
        }
        false
    }

    pub fn is_option_type(&self, ty: &Type) -> Option<Type> {
        if let Type::Enum(name) = ty {
            if let Some(e_def) = self.enums.get(name) {
                let is_opt_name = name == "Option"
                    || name.starts_with("Option__")
                    || name.ends_with("__Option")
                    || name.contains("__Option__");
                if is_opt_name {
                    let some_v = e_def
                        .variants
                        .iter()
                        .find(|v| v.name == "Some" && v.payload.len() == 1);
                    let none_v = e_def
                        .variants
                        .iter()
                        .find(|v| v.name == "None" && v.payload.is_empty());
                    if let (Some(some), Some(_)) = (some_v, none_v) {
                        return Some(some.payload[0].clone());
                    }
                }
            }
        }
        None
    }

    pub fn is_result_type(&self, ty: &Type) -> Option<(Type, Type)> {
        if let Type::Enum(name) = ty {
            if let Some(e_def) = self.enums.get(name) {
                let is_res_name = name == "Result"
                    || name.starts_with("Result__")
                    || name.ends_with("__Result")
                    || name.contains("__Result__");
                if is_res_name {
                    let ok_v = e_def
                        .variants
                        .iter()
                        .find(|v| v.name == "Ok" && v.payload.len() == 1);
                    let err_v = e_def
                        .variants
                        .iter()
                        .find(|v| v.name == "Err" && v.payload.len() == 1);
                    if let (Some(ok), Some(err)) = (ok_v, err_v) {
                        return Some((ok.payload[0].clone(), err.payload[0].clone()));
                    }
                }
            }
        }
        None
    }

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
        functions.insert(
            "panic".into(),
            FnSignature {
                name: "panic".into(),
                params: vec![(
                    "msg".into(),
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
        functions.insert(
            "vanta_alloc".into(),
            FnSignature {
                name: "vanta_alloc".into(),
                params: vec![("size".into(), Type::I64, false)],
                ret_type: Type::I64,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "vanta_realloc".into(),
            FnSignature {
                name: "vanta_realloc".into(),
                params: vec![
                    ("ptr".into(), Type::I64, false),
                    ("new_size".into(), Type::I64, false),
                ],
                ret_type: Type::I64,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "vanta_free".into(),
            FnSignature {
                name: "vanta_free".into(),
                params: vec![("ptr".into(), Type::I64, false)],
                ret_type: Type::Unit,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "vanta_active_allocs".into(),
            FnSignature {
                name: "vanta_active_allocs".into(),
                params: vec![],
                ret_type: Type::I64,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "vanta_io_write_stdout".into(),
            FnSignature {
                name: "vanta_io_write_stdout".into(),
                params: vec![(
                    "s".into(),
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
        functions.insert(
            "vanta_io_write_stderr".into(),
            FnSignature {
                name: "vanta_io_write_stderr".into(),
                params: vec![(
                    "s".into(),
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
        functions.insert(
            "vanta_io_read_stdin".into(),
            FnSignature {
                name: "vanta_io_read_stdin".into(),
                params: vec![(
                    "dest".into(),
                    Type::Ref {
                        is_mut: true,
                        inner: Box::new(Type::Struct("String".into())),
                    },
                    false,
                )],
                ret_type: Type::I64,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "vanta_fs_read_to_string".into(),
            FnSignature {
                name: "vanta_fs_read_to_string".into(),
                params: vec![
                    (
                        "path".into(),
                        Type::Ref {
                            is_mut: false,
                            inner: Box::new(Type::Str),
                        },
                        false,
                    ),
                    (
                        "dest".into(),
                        Type::Ref {
                            is_mut: true,
                            inner: Box::new(Type::Struct("String".into())),
                        },
                        false,
                    ),
                ],
                ret_type: Type::I64,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "vanta_fs_write".into(),
            FnSignature {
                name: "vanta_fs_write".into(),
                params: vec![
                    (
                        "path".into(),
                        Type::Ref {
                            is_mut: false,
                            inner: Box::new(Type::Str),
                        },
                        false,
                    ),
                    (
                        "contents".into(),
                        Type::Ref {
                            is_mut: false,
                            inner: Box::new(Type::Str),
                        },
                        false,
                    ),
                ],
                ret_type: Type::I64,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "vanta_env_args_count".into(),
            FnSignature {
                name: "vanta_env_args_count".into(),
                params: vec![],
                ret_type: Type::I64,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        functions.insert(
            "vanta_env_arg_get".into(),
            FnSignature {
                name: "vanta_env_arg_get".into(),
                params: vec![
                    ("idx".into(), Type::I64, false),
                    (
                        "dest".into(),
                        Type::Ref {
                            is_mut: true,
                            inner: Box::new(Type::Struct("String".into())),
                        },
                        false,
                    ),
                ],
                ret_type: Type::I64,
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );

        let mut generic_structs = HashMap::new();
        generic_structs.insert(
            "Vec".to_string(),
            StructDecl {
                is_pub: true,
                name: Ident::new("Vec", Span::new(vanta_source::FileId(0), 0, 0)),
                lifetime_params: Vec::new(),
                type_params: vec![Ident::new("T", Span::new(vanta_source::FileId(0), 0, 0)).into()],
                fields: vec![
                    FieldDecl {
                        is_pub: true,
                        name: Ident::new("data_ptr", Span::new(vanta_source::FileId(0), 0, 0)),
                        ty: TypeExpr::new(
                            TypeExprKind::Primitive(PrimType::I64),
                            Span::new(vanta_source::FileId(0), 0, 0),
                        ),
                        span: Span::new(vanta_source::FileId(0), 0, 0),
                    },
                    FieldDecl {
                        is_pub: true,
                        name: Ident::new("len", Span::new(vanta_source::FileId(0), 0, 0)),
                        ty: TypeExpr::new(
                            TypeExprKind::Primitive(PrimType::I64),
                            Span::new(vanta_source::FileId(0), 0, 0),
                        ),
                        span: Span::new(vanta_source::FileId(0), 0, 0),
                    },
                    FieldDecl {
                        is_pub: true,
                        name: Ident::new("cap", Span::new(vanta_source::FileId(0), 0, 0)),
                        ty: TypeExpr::new(
                            TypeExprKind::Primitive(PrimType::I64),
                            Span::new(vanta_source::FileId(0), 0, 0),
                        ),
                        span: Span::new(vanta_source::FileId(0), 0, 0),
                    },
                ],
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );
        generic_structs.insert(
            "Iter".to_string(),
            StructDecl {
                is_pub: true,
                name: Ident::new("Iter", Span::new(vanta_source::FileId(0), 0, 0)),
                lifetime_params: vec![Ident::new("'a", Span::new(vanta_source::FileId(0), 0, 0))],
                type_params: vec![Ident::new("T", Span::new(vanta_source::FileId(0), 0, 0)).into()],
                fields: vec![
                    FieldDecl {
                        is_pub: true,
                        name: Ident::new("vec", Span::new(vanta_source::FileId(0), 0, 0)),
                        ty: TypeExpr::new(
                            TypeExprKind::Reference {
                                lifetime: Some(Ident::new(
                                    "'a",
                                    Span::new(vanta_source::FileId(0), 0, 0),
                                )),
                                is_mut: false,
                                inner: Box::new(TypeExpr::new(
                                    TypeExprKind::Generic {
                                        name: "Vec".to_string(),
                                        type_args: vec![TypeExpr::new(
                                            TypeExprKind::Named("T".to_string()),
                                            Span::new(vanta_source::FileId(0), 0, 0),
                                        )],
                                    },
                                    Span::new(vanta_source::FileId(0), 0, 0),
                                )),
                            },
                            Span::new(vanta_source::FileId(0), 0, 0),
                        ),
                        span: Span::new(vanta_source::FileId(0), 0, 0),
                    },
                    FieldDecl {
                        is_pub: true,
                        name: Ident::new("index", Span::new(vanta_source::FileId(0), 0, 0)),
                        ty: TypeExpr::new(
                            TypeExprKind::Primitive(PrimType::I64),
                            Span::new(vanta_source::FileId(0), 0, 0),
                        ),
                        span: Span::new(vanta_source::FileId(0), 0, 0),
                    },
                ],
                span: Span::new(vanta_source::FileId(0), 0, 0),
            },
        );

        let mut structs = HashMap::new();
        let mut declared_struct_names = HashSet::new();

        let string_def = StructDef {
            name: "String".to_string(),
            fields: vec![
                StructField {
                    name: "data_ptr".to_string(),
                    ty: Type::I64,
                    offset: 0,
                    size: 8,
                    is_pub: true,
                },
                StructField {
                    name: "len".to_string(),
                    ty: Type::I64,
                    offset: 8,
                    size: 8,
                    is_pub: true,
                },
                StructField {
                    name: "cap".to_string(),
                    ty: Type::I64,
                    offset: 16,
                    size: 8,
                    is_pub: true,
                },
            ],
            total_size: 24,
            alignment: 8,
            is_copy: false,
            defining_module: String::new(),
            span: Span::new(vanta_source::FileId(0), 0, 0),
        };
        structs.insert("String".to_string(), string_def.clone());
        structs.insert("std__string__String".to_string(), string_def);
        declared_struct_names.insert("String".to_string());
        declared_struct_names.insert("std__string__String".to_string());

        Self {
            functions,
            structs,
            declared_struct_names,
            generic_structs,
            enums: HashMap::new(),
            declared_enum_names: HashSet::new(),
            generic_enums: HashMap::new(),
            traits: HashMap::new(),
            declared_trait_names: HashSet::new(),
            trait_impls: HashMap::new(),
            trait_methods: HashMap::new(),
            variant_constructor_enum_names: HashMap::new(),
            generic_functions: HashMap::new(),
            monomorphized_functions: Vec::new(),
            call_callees: HashMap::new(),
            struct_literal_names: HashMap::new(),
            generic_depth: 0,
            methods: HashMap::new(),
            symbols: HashMap::new(),
            expr_types: HashMap::new(),
            resolved_symbols: HashMap::new(),
            current_impl_target: None,
            module_scopes: HashMap::new(),
            current_module: ModuleId(0),
            pub_methods: HashSet::new(),
            module_tree: None,
            next_sym_id: 1,
            scopes: vec![HashMap::new()],
            var_states: HashMap::new(),
            current_fn_ret_type: None,
            expected_type: None,
            generic_enum_type_args: HashMap::new(),
            active_lifetime_params: HashSet::new(),
            local_storage_symbols: HashSet::new(),
            ref_provenances: HashMap::new(),
            borrowed_sources: HashMap::new(),
        }
    }

    pub fn is_valid_lifetime(&self, name: &str) -> bool {
        name == "'static" || name == "static" || self.active_lifetime_params.contains(name)
    }

    pub fn current_module_name(&self) -> String {
        if let Some(scope) = self.module_scopes.get(&self.current_module) {
            if scope.canonical_prefix.is_empty() {
                "main".to_string()
            } else {
                scope.canonical_prefix.trim_end_matches("__").to_string()
            }
        } else {
            "main".to_string()
        }
    }

    pub fn resolve_fn_name(
        &self,
        name_or_path: &str,
        span: Span,
        diags: &mut DiagnosticBag,
    ) -> Option<String> {
        if name_or_path == "print_i64"
            || name_or_path == "print_str"
            || name_or_path == "panic"
            || name_or_path == "vanta_alloc"
            || name_or_path == "vanta_realloc"
            || name_or_path == "vanta_free"
            || name_or_path == "vanta_active_allocs"
            || name_or_path == "vanta_io_write_stdout"
            || name_or_path == "vanta_io_write_stderr"
            || name_or_path == "vanta_io_read_stdin"
            || name_or_path == "vanta_fs_read_to_string"
            || name_or_path == "vanta_fs_write"
            || name_or_path == "vanta_env_args_count"
            || name_or_path == "vanta_env_arg_get"
            || name_or_path == "Vec::new"
            || name_or_path == "Vec_new"
            || name_or_path == "String::new"
            || name_or_path == "String_new"
            || name_or_path == "String::from_str"
            || name_or_path == "String_from_str"
            || name_or_path == "std::string::new"
            || name_or_path == "std::string::from_str"
        {
            return Some(name_or_path.to_string());
        }

        if name_or_path.contains("::") {
            let segments: Vec<&str> = name_or_path.split("::").collect();
            if segments.len() >= 2 {
                let mod_path: Vec<String> = segments[..segments.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let item_name = *segments.last().unwrap();
                if let Some(tree) = &self.module_tree {
                    match tree.resolve_module_path(self.current_module, &mod_path) {
                        Ok(target_mod_id) => {
                            if let Some(target_scope) = self.module_scopes.get(&target_mod_id) {
                                if let Some(canon) = target_scope.functions.get(item_name) {
                                    if target_mod_id != self.current_module
                                        && !target_scope.pub_items.contains(item_name)
                                    {
                                        let target_node = tree.get(target_mod_id);
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Item '{}' in module '{}' is private",
                                                item_name, target_node.name
                                            ))
                                            .with_code(codes::E0604_PRIVATE_ITEM)
                                            .with_span(span),
                                        );
                                    }
                                    return Some(canon.clone());
                                }
                            }
                        }
                        Err((msg, code)) => {
                            if self.functions.contains_key(name_or_path)
                                || self.generic_functions.contains_key(name_or_path)
                            {
                                return Some(name_or_path.to_string());
                            }
                            let mut d = Diagnostic::error(msg).with_span(span);
                            if let Some(c) = code {
                                d = d.with_code(c);
                            }
                            diags.emit(d);
                            return None;
                        }
                    }
                }
            }
        }

        // Unqualified lookup in current module scope
        if let Some(scope) = self.module_scopes.get(&self.current_module) {
            if let Some(canon) = scope.functions.get(name_or_path) {
                return Some(canon.clone());
            }
        }

        // Fallback for single-file tests or builtins
        if self.functions.contains_key(name_or_path)
            || self.generic_functions.contains_key(name_or_path)
        {
            Some(name_or_path.to_string())
        } else {
            None
        }
    }

    pub fn resolve_struct_name(
        &self,
        name_or_path: &str,
        span: Span,
        diags: &mut DiagnosticBag,
    ) -> Option<String> {
        if name_or_path == "Vec" {
            return Some("Vec".to_string());
        }
        if name_or_path == "String" || name_or_path == "std::string::String" {
            return Some("String".to_string());
        }

        if name_or_path.contains("::") {
            let segments: Vec<&str> = name_or_path.split("::").collect();
            if segments.len() >= 2 {
                let mod_path: Vec<String> = segments[..segments.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let item_name = *segments.last().unwrap();
                if let Some(tree) = &self.module_tree {
                    match tree.resolve_module_path(self.current_module, &mod_path) {
                        Ok(target_mod_id) => {
                            if let Some(target_scope) = self.module_scopes.get(&target_mod_id) {
                                if let Some(canon) = target_scope.structs.get(item_name) {
                                    if target_mod_id != self.current_module
                                        && !target_scope.pub_items.contains(item_name)
                                    {
                                        let target_node = tree.get(target_mod_id);
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Item '{}' in module '{}' is private",
                                                item_name, target_node.name
                                            ))
                                            .with_code(codes::E0604_PRIVATE_ITEM)
                                            .with_span(span),
                                        );
                                    }
                                    return Some(canon.clone());
                                }
                            }
                        }
                        Err((msg, code)) => {
                            if self.functions.contains_key(name_or_path)
                                || self.generic_functions.contains_key(name_or_path)
                            {
                                return Some(name_or_path.to_string());
                            }
                            let mut d = Diagnostic::error(msg).with_span(span);
                            if let Some(c) = code {
                                d = d.with_code(c);
                            }
                            diags.emit(d);
                            return None;
                        }
                    }
                }
            }
        }

        // Unqualified lookup in current module scope
        if let Some(scope) = self.module_scopes.get(&self.current_module) {
            if let Some(canon) = scope.structs.get(name_or_path) {
                return Some(canon.clone());
            }
        }

        // Fallback for single-file tests or builtins
        if self.structs.contains_key(name_or_path)
            || self.generic_structs.contains_key(name_or_path)
            || self.declared_struct_names.contains(name_or_path)
        {
            Some(name_or_path.to_string())
        } else {
            None
        }
    }

    pub fn resolve_enum_name(
        &self,
        name_or_path: &str,
        span: Span,
        diags: &mut DiagnosticBag,
    ) -> Option<String> {
        if name_or_path.contains("::") {
            let segments: Vec<&str> = name_or_path.split("::").collect();
            if segments.len() >= 2 {
                let mod_path: Vec<String> = segments[..segments.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let item_name = *segments.last().unwrap();
                if let Some(tree) = &self.module_tree {
                    match tree.resolve_module_path(self.current_module, &mod_path) {
                        Ok(target_mod_id) => {
                            if let Some(target_scope) = self.module_scopes.get(&target_mod_id) {
                                if let Some(canon) = target_scope.enums.get(item_name) {
                                    if target_mod_id != self.current_module
                                        && !target_scope.pub_items.contains(item_name)
                                    {
                                        let target_node = tree.get(target_mod_id);
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Item '{}' in module '{}' is private",
                                                item_name, target_node.name
                                            ))
                                            .with_code(codes::E0604_PRIVATE_ITEM)
                                            .with_span(span),
                                        );
                                    }
                                    return Some(canon.clone());
                                }
                            }
                        }
                        Err((msg, code)) => {
                            if self.functions.contains_key(name_or_path)
                                || self.generic_functions.contains_key(name_or_path)
                            {
                                return Some(name_or_path.to_string());
                            }
                            let mut d = Diagnostic::error(msg).with_span(span);
                            if let Some(c) = code {
                                d = d.with_code(c);
                            }
                            diags.emit(d);
                            return None;
                        }
                    }
                }
            }
        }

        // Unqualified lookup in current module scope
        if let Some(scope) = self.module_scopes.get(&self.current_module) {
            if let Some(canon) = scope.enums.get(name_or_path) {
                return Some(canon.clone());
            }
        }

        // Fallback for single-file tests or builtins
        if self.enums.contains_key(name_or_path)
            || self.generic_enums.contains_key(name_or_path)
            || self.declared_enum_names.contains(name_or_path)
        {
            Some(name_or_path.to_string())
        } else {
            None
        }
    }

    pub fn resolve_trait_name(
        &self,
        name_or_path: &str,
        span: Span,
        diags: &mut DiagnosticBag,
    ) -> Option<String> {
        if name_or_path.contains("::") {
            let segments: Vec<&str> = name_or_path.split("::").collect();
            if segments.len() >= 2 {
                let mod_path: Vec<String> = segments[..segments.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let item_name = *segments.last().unwrap();
                if let Some(tree) = &self.module_tree {
                    match tree.resolve_module_path(self.current_module, &mod_path) {
                        Ok(target_mod_id) => {
                            if let Some(target_scope) = self.module_scopes.get(&target_mod_id) {
                                if let Some(canon) = target_scope.traits.get(item_name) {
                                    if target_mod_id != self.current_module
                                        && !target_scope.pub_items.contains(item_name)
                                    {
                                        let target_node = tree.get(target_mod_id);
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Trait '{}' in module '{}' is private",
                                                item_name, target_node.name
                                            ))
                                            .with_code(codes::E0604_PRIVATE_ITEM)
                                            .with_span(span),
                                        );
                                    }
                                    return Some(canon.clone());
                                }
                            }
                        }
                        Err((msg, code)) => {
                            if self.traits.contains_key(name_or_path)
                                || self.declared_trait_names.contains(name_or_path)
                            {
                                return Some(name_or_path.to_string());
                            }
                            let mut d = Diagnostic::error(msg).with_span(span);
                            if let Some(c) = code {
                                d = d.with_code(c);
                            }
                            diags.emit(d);
                            return None;
                        }
                    }
                }
            }
        }

        // Unqualified lookup in current module scope
        if let Some(scope) = self.module_scopes.get(&self.current_module) {
            if let Some(canon) = scope.traits.get(name_or_path) {
                return Some(canon.clone());
            }
        }

        if self.traits.contains_key(name_or_path)
            || self.declared_trait_names.contains(name_or_path)
        {
            Some(name_or_path.to_string())
        } else {
            None
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
                self.local_storage_symbols.remove(&sym_id);
                self.ref_provenances.remove(&sym_id);
                if let Some(src_id) = self.borrowed_sources.remove(&sym_id) {
                    if let Some(vs) = self.var_states.get_mut(&src_id) {
                        if let BorrowState::Shared(n) = vs.borrow {
                            if n <= 1 {
                                vs.borrow = BorrowState::None;
                            } else {
                                vs.borrow = BorrowState::Shared(n - 1);
                            }
                        } else if vs.borrow == BorrowState::Exclusive {
                            vs.borrow = BorrowState::None;
                        }
                    }
                }
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
                .with_code(codes::E0202_UNSIZED_TYPE_VALUE)
                .with_expected("&str")
                .with_actual("str")
                .with_suggestion("use immutable slice '&str'", Some(span), "&str")
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
                .with_code(codes::E0101_DUPLICATE_SYMBOL)
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

    pub fn instantiate_generic_struct(
        &mut self,
        name: &str,
        type_args: &[Type],
        span: Span,
        diags: &mut DiagnosticBag,
    ) -> Option<String> {
        let generic_s = self
            .generic_structs
            .get(name)
            .cloned()
            .or_else(|| {
                let canon = self.resolve_struct_name(name, span, diags)?;
                self.generic_structs.get(&canon).cloned()
            })
            .or_else(|| {
                self.generic_structs
                    .iter()
                    .find(|(k, _)| {
                        k.as_str() == name
                            || k.ends_with(&format!("__{}", name))
                            || k.ends_with(&format!("::{}", name))
                    })
                    .map(|(_, v)| v.clone())
            })?;

        if generic_s.type_params.len() != type_args.len() {
            diags.emit(
                Diagnostic::error(format!(
                    "Generic struct '{}' expects {} type arguments, but {} were provided",
                    name,
                    generic_s.type_params.len(),
                    type_args.len()
                ))
                .with_code(codes::E0209_GENERIC_ARITY_MISMATCH)
                .with_span(span),
            );
            return None;
        }

        let mangled_name = format!(
            "{}__{}",
            name,
            type_args
                .iter()
                .map(|t| t.to_mangled_string())
                .collect::<Vec<_>>()
                .join("_")
        );

        if self.structs.contains_key(&mangled_name) {
            return Some(mangled_name);
        }

        let mut substs = HashMap::new();
        for (param, concrete) in generic_s.type_params.iter().zip(type_args.iter()) {
            substs.insert(param.name.name.clone(), concrete.clone());
        }

        let prev_lts = self.active_lifetime_params.clone();
        for lt in &generic_s.lifetime_params {
            self.active_lifetime_params.insert(lt.name.clone());
        }

        let mut fields = Vec::new();
        for f in &generic_s.fields {
            let subst_ty = subst_type_expr(&f.ty, &substs);
            let f_ty = self.type_from_ast(&subst_ty, diags);
            fields.push((f.name.name.clone(), f_ty));
        }

        self.active_lifetime_params = prev_lts;

        let def =
            StructDef::compute_layout(mangled_name.clone(), fields, &self.structs, generic_s.span);
        self.structs.insert(mangled_name.clone(), def);
        Some(mangled_name)
    }

    pub fn instantiate_generic_enum(
        &mut self,
        name: &str,
        concrete_types: &[Type],
        span: Span,
        diags: &mut DiagnosticBag,
    ) -> Option<String> {
        let generic_e = self
            .generic_enums
            .get(name)
            .cloned()
            .or_else(|| {
                let canon = self.resolve_enum_name(name, span, diags)?;
                self.generic_enums.get(&canon).cloned()
            })
            .or_else(|| {
                self.generic_enums
                    .iter()
                    .find(|(k, _)| {
                        k.as_str() == name
                            || k.ends_with(&format!("__{}", name))
                            || k.ends_with(&format!("::{}", name))
                    })
                    .map(|(_, v)| v.clone())
            })?;
        if concrete_types.len() != generic_e.type_params.len() {
            diags.emit(
                Diagnostic::error(format!(
                    "Generic enum '{}' expects {} type arguments, but {} were provided",
                    name,
                    generic_e.type_params.len(),
                    concrete_types.len()
                ))
                .with_code(codes::E0209_GENERIC_ARITY_MISMATCH)
                .with_span(span),
            );
            return None;
        }

        let mangled_name = format!(
            "{}__{}",
            name,
            concrete_types
                .iter()
                .map(|t| t.to_mangled_string())
                .collect::<Vec<_>>()
                .join("_")
        );

        self.generic_enum_type_args.insert(
            mangled_name.clone(),
            (name.to_string(), concrete_types.to_vec()),
        );

        if self.enums.contains_key(&mangled_name) {
            return Some(mangled_name);
        }

        if self.generic_depth >= 16 {
            diags.emit(
                Diagnostic::error(format!(
                    "Recursion limit exceeded while instantiating generic enum '{}'",
                    name
                ))
                .with_code(codes::E0211_RECURSIVE_GENERIC_INSTANTIATION)
                .with_span(span),
            );
            return None;
        }

        let mut substs = HashMap::new();
        for (tp, ct) in generic_e.type_params.iter().zip(concrete_types.iter()) {
            substs.insert(tp.name.name.clone(), ct.clone());
        }

        self.generic_depth += 1;
        let prev_lts = self.active_lifetime_params.clone();
        for lt in &generic_e.lifetime_params {
            self.active_lifetime_params.insert(lt.name.clone());
        }
        let mut variants = Vec::new();
        for v in &generic_e.variants {
            let mut payload = Vec::new();
            for p in &v.payload {
                let subst_p = subst_type_expr(p, &substs);
                let p_ty = self.type_from_ast(&subst_p, diags);
                payload.push(p_ty);
            }
            variants.push((v.name.name.clone(), payload));
        }
        self.active_lifetime_params = prev_lts;
        self.generic_depth -= 1;

        match EnumDef::compute_layout(
            mangled_name.clone(),
            variants,
            &self.structs,
            &self.enums,
            generic_e.span,
        ) {
            Ok(def) => {
                self.enums.insert(mangled_name.clone(), def);
                Some(mangled_name)
            }
            Err(e) => {
                diags.emit(
                    Diagnostic::error(format!(
                        "Failed to compute layout for enum '{}': {}",
                        mangled_name, e
                    ))
                    .with_code(codes::E0507_RECURSIVE_ENUM)
                    .with_span(span),
                );
                None
            }
        }
    }

    pub fn type_from_ast(&mut self, ty_expr: &TypeExpr, diags: &mut DiagnosticBag) -> Type {
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
            TypeExprKind::Reference {
                is_mut,
                inner,
                lifetime,
            } => {
                if let Some(lt) = lifetime {
                    if !self.is_valid_lifetime(&lt.name) {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Undeclared lifetime parameter '{}'",
                                lt.name
                            ))
                            .with_code(codes::E0311_UNDECLARED_LIFETIME)
                            .with_span(lt.span),
                        );
                    }
                }
                let inner_ty = self.type_from_ast(inner, diags);
                if *is_mut && inner_ty == Type::Str {
                    diags.emit(
                        Diagnostic::error(
                            "Mutable string slices '&mut str' are not supported; string slices must be immutable '&str'",
                        )
                        .with_code(codes::E0203_MUTABLE_STR_SLICE)
                        .with_expected("&str")
                        .with_actual("&mut str")
                        .with_suggestion("use immutable slice '&str'", Some(ty_expr.span), "&str")
                        .with_span(ty_expr.span),
                    );
                } else if *is_mut && matches!(inner_ty, Type::Slice(_)) {
                    diags.emit(
                        Diagnostic::error(
                            "Mutable slices '&mut [T]' are not supported in this milestone; slices must be immutable '&[T]'",
                        )
                        .with_code(codes::E0203_MUTABLE_STR_SLICE)
                        .with_expected("&[T]")
                        .with_actual("&mut [T]")
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
            TypeExprKind::Slice { inner, lifetime } => {
                if let Some(lt) = lifetime {
                    if !self.is_valid_lifetime(&lt.name) {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Undeclared lifetime parameter '{}'",
                                lt.name
                            ))
                            .with_code(codes::E0311_UNDECLARED_LIFETIME)
                            .with_span(lt.span),
                        );
                    }
                }
                let inner_ty = self.type_from_ast(inner, diags);
                Type::Slice(Box::new(inner_ty))
            }
            TypeExprKind::Generic { name, type_args } => {
                let resolved_name = self
                    .resolve_enum_name(name, ty_expr.span, diags)
                    .or_else(|| self.resolve_struct_name(name, ty_expr.span, diags))
                    .unwrap_or_else(|| name.clone());

                if let Some(generic_s) = self.generic_structs.get(&resolved_name).cloned() {
                    let num_lts = generic_s.lifetime_params.len();
                    let num_tps = generic_s.type_params.len();
                    let type_args_slice = if type_args.len() == num_lts + num_tps {
                        for lt_arg in &type_args[..num_lts] {
                            if let TypeExprKind::Lifetime(lt) = &lt_arg.kind {
                                if !self.is_valid_lifetime(&lt.name) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Undeclared lifetime parameter '{}'",
                                            lt.name
                                        ))
                                        .with_code(codes::E0311_UNDECLARED_LIFETIME)
                                        .with_span(lt.span),
                                    );
                                }
                            } else {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Expected lifetime parameter in generic argument position for '{}'",
                                        name
                                    ))
                                    .with_code(codes::E0315_INVALID_LIFETIME_ARG)
                                    .with_span(lt_arg.span),
                                );
                            }
                        }
                        &type_args[num_lts..]
                    } else if type_args.len() == num_tps {
                        &type_args[..]
                    } else {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Generic struct '{}' expects {} arguments, but {} were provided",
                                name,
                                num_lts + num_tps,
                                type_args.len()
                            ))
                            .with_code(codes::E0209_GENERIC_ARITY_MISMATCH)
                            .with_span(ty_expr.span),
                        );
                        return Type::Unit;
                    };

                    let mut concrete_types = Vec::new();
                    for t in type_args_slice {
                        concrete_types.push(self.type_from_ast(t, diags));
                    }

                    if let Some(mangled) = self.instantiate_generic_struct(
                        &resolved_name,
                        &concrete_types,
                        ty_expr.span,
                        diags,
                    ) {
                        Type::Struct(mangled)
                    } else {
                        Type::Unit
                    }
                } else if self.generic_enums.contains_key(&resolved_name) {
                    let mut concrete_types = Vec::new();
                    for t in type_args {
                        concrete_types.push(self.type_from_ast(t, diags));
                    }
                    if let Some(mangled) = self.instantiate_generic_enum(
                        &resolved_name,
                        &concrete_types,
                        ty_expr.span,
                        diags,
                    ) {
                        Type::Enum(mangled)
                    } else {
                        Type::Unit
                    }
                } else {
                    diags.emit(
                        Diagnostic::error(format!("Unknown generic type '{}'", name))
                            .with_code(codes::E0104_UNKNOWN_STRUCT)
                            .with_span(ty_expr.span),
                    );
                    Type::Unit
                }
            }
            TypeExprKind::Named(name) => {
                if name == "str" {
                    Type::Str
                } else if name == "Self" {
                    if let Some(target) = &self.current_impl_target {
                        if self.enums.contains_key(target)
                            || self.declared_enum_names.contains(target)
                        {
                            Type::Enum(target.clone())
                        } else {
                            Type::Struct(target.clone())
                        }
                    } else {
                        diags.emit(
                            Diagnostic::error("'Self' type is only valid within an impl block")
                                .with_code(codes::E0104_UNKNOWN_STRUCT)
                                .with_span(ty_expr.span),
                        );
                        Type::Unit
                    }
                } else if let Some(canon) = self.resolve_struct_name(name, ty_expr.span, diags) {
                    Type::Struct(canon)
                } else if let Some(canon) = self.resolve_enum_name(name, ty_expr.span, diags) {
                    Type::Enum(canon)
                } else if self.structs.contains_key(name) || name.starts_with("Vec__") {
                    Type::Struct(name.clone())
                } else if self.enums.contains_key(name) {
                    Type::Enum(name.clone())
                } else if let Some(target) = &self.current_impl_target {
                    if target == name {
                        if self.enums.contains_key(target)
                            || self.declared_enum_names.contains(target)
                        {
                            Type::Enum(name.clone())
                        } else {
                            Type::Struct(name.clone())
                        }
                    } else {
                        diags.emit(
                            Diagnostic::error(format!("Unknown type '{}'", name))
                                .with_code(codes::E0104_UNKNOWN_STRUCT)
                                .with_span(ty_expr.span),
                        );
                        Type::Unit
                    }
                } else {
                    diags.emit(
                        Diagnostic::error(format!("Unknown type '{}'", name))
                            .with_code(codes::E0104_UNKNOWN_STRUCT)
                            .with_span(ty_expr.span),
                    );
                    Type::Unit
                }
            }
            TypeExprKind::Unit => Type::Unit,
            TypeExprKind::Lifetime(id) => {
                diags.emit(
                    Diagnostic::error(format!(
                        "Lifetime '{}' cannot be used as a value type",
                        id.name
                    ))
                    .with_code(codes::E0315_INVALID_LIFETIME_ARG)
                    .with_span(ty_expr.span),
                );
                Type::Unit
            }
        }
    }
}

pub fn type_to_type_expr(ty: &Type, span: Span) -> TypeExpr {
    let kind = match ty {
        Type::I8 => TypeExprKind::Primitive(PrimType::I8),
        Type::I16 => TypeExprKind::Primitive(PrimType::I16),
        Type::I32 => TypeExprKind::Primitive(PrimType::I32),
        Type::I64 => TypeExprKind::Primitive(PrimType::I64),
        Type::U8 => TypeExprKind::Primitive(PrimType::U8),
        Type::U16 => TypeExprKind::Primitive(PrimType::U16),
        Type::U32 => TypeExprKind::Primitive(PrimType::U32),
        Type::U64 => TypeExprKind::Primitive(PrimType::U64),
        Type::F32 => TypeExprKind::Primitive(PrimType::F32),
        Type::F64 => TypeExprKind::Primitive(PrimType::F64),
        Type::Bool => TypeExprKind::Primitive(PrimType::Bool),
        Type::Str => TypeExprKind::Primitive(PrimType::Str),
        Type::Unit => TypeExprKind::Unit,
        Type::Ref { is_mut, inner } => TypeExprKind::Reference {
            lifetime: None,
            is_mut: *is_mut,
            inner: Box::new(type_to_type_expr(inner, span)),
        },
        Type::Array { inner, len } => TypeExprKind::Array {
            inner: Box::new(type_to_type_expr(inner, span)),
            len: *len,
        },
        Type::Slice(inner) => TypeExprKind::Slice {
            lifetime: None,
            inner: Box::new(type_to_type_expr(inner, span)),
        },
        Type::Struct(name) | Type::Enum(name) => TypeExprKind::Named(name.clone()),
    };
    TypeExpr::new(kind, span)
}

pub fn subst_type_expr(ty: &TypeExpr, substs: &HashMap<String, Type>) -> TypeExpr {
    let kind = match &ty.kind {
        TypeExprKind::Named(name) => {
            if let Some(concrete) = substs.get(name) {
                return type_to_type_expr(concrete, ty.span);
            } else {
                TypeExprKind::Named(name.clone())
            }
        }
        TypeExprKind::Generic { name, type_args } => {
            let new_args = type_args
                .iter()
                .map(|a| subst_type_expr(a, substs))
                .collect();
            TypeExprKind::Generic {
                name: name.clone(),
                type_args: new_args,
            }
        }
        TypeExprKind::Reference {
            is_mut,
            inner,
            lifetime,
        } => TypeExprKind::Reference {
            lifetime: lifetime.clone(),
            is_mut: *is_mut,
            inner: Box::new(subst_type_expr(inner, substs)),
        },
        TypeExprKind::Array { inner, len } => TypeExprKind::Array {
            inner: Box::new(subst_type_expr(inner, substs)),
            len: *len,
        },
        TypeExprKind::Slice { inner, lifetime } => TypeExprKind::Slice {
            lifetime: lifetime.clone(),
            inner: Box::new(subst_type_expr(inner, substs)),
        },
        TypeExprKind::Lifetime(lt) => TypeExprKind::Lifetime(lt.clone()),
        TypeExprKind::Primitive(p) => TypeExprKind::Primitive(*p),
        TypeExprKind::Unit => TypeExprKind::Unit,
    };
    TypeExpr::new(kind, ty.span)
}

pub fn subst_expr(expr: &Expr, substs: &HashMap<String, Type>) -> Expr {
    let kind = match &expr.kind {
        ExprKind::Int(v) => ExprKind::Int(*v),
        ExprKind::Bool(v) => ExprKind::Bool(*v),
        ExprKind::String(v) => ExprKind::String(v.clone()),
        ExprKind::Ident(v) => ExprKind::Ident(v.clone()),
        ExprKind::Binary { op, lhs, rhs } => ExprKind::Binary {
            op: *op,
            lhs: Box::new(subst_expr(lhs, substs)),
            rhs: Box::new(subst_expr(rhs, substs)),
        },
        ExprKind::Unary { op, expr } => ExprKind::Unary {
            op: *op,
            expr: Box::new(subst_expr(expr, substs)),
        },
        ExprKind::Ref { is_mut, expr } => ExprKind::Ref {
            is_mut: *is_mut,
            expr: Box::new(subst_expr(expr, substs)),
        },
        ExprKind::Deref(expr) => ExprKind::Deref(Box::new(subst_expr(expr, substs))),
        ExprKind::Call {
            callee,
            type_args,
            args,
        } => ExprKind::Call {
            callee: callee.clone(),
            type_args: type_args
                .iter()
                .map(|a| subst_type_expr(a, substs))
                .collect(),
            args: args.iter().map(|a| subst_expr(a, substs)).collect(),
        },
        ExprKind::Index { array, index } => ExprKind::Index {
            array: Box::new(subst_expr(array, substs)),
            index: Box::new(subst_expr(index, substs)),
        },
        ExprKind::FieldAccess { target, field } => ExprKind::FieldAccess {
            target: Box::new(subst_expr(target, substs)),
            field: field.clone(),
        },
        ExprKind::MethodCall {
            target,
            method,
            args,
        } => ExprKind::MethodCall {
            target: Box::new(subst_expr(target, substs)),
            method: method.clone(),
            args: args.iter().map(|a| subst_expr(a, substs)).collect(),
        },
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => ExprKind::If {
            cond: Box::new(subst_expr(cond, substs)),
            then_branch: subst_block(then_branch, substs),
            else_branch: else_branch.as_ref().map(|eb| match eb {
                ElseBranch::If(e) => ElseBranch::If(Box::new(subst_expr(e, substs))),
                ElseBranch::Block(b) => ElseBranch::Block(subst_block(b, substs)),
            }),
        },
        ExprKind::Block(b) => ExprKind::Block(subst_block(b, substs)),
        ExprKind::ArrayLiteral(elements) => {
            ExprKind::ArrayLiteral(elements.iter().map(|e| subst_expr(e, substs)).collect())
        }
        ExprKind::StructLiteral {
            name,
            type_args,
            fields,
        } => ExprKind::StructLiteral {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|a| subst_type_expr(a, substs))
                .collect(),
            fields: fields
                .iter()
                .map(|(id, e)| (id.clone(), subst_expr(e, substs)))
                .collect(),
        },
        ExprKind::Match { expr, arms } => ExprKind::Match {
            expr: Box::new(subst_expr(expr, substs)),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    pattern: arm.pattern.clone(),
                    body: subst_expr(&arm.body, substs),
                    span: arm.span,
                })
                .collect(),
        },
        ExprKind::VariantConstructor {
            enum_name,
            variant,
            type_args,
            args,
        } => ExprKind::VariantConstructor {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            type_args: type_args
                .iter()
                .map(|a| subst_type_expr(a, substs))
                .collect(),
            args: args.iter().map(|a| subst_expr(a, substs)).collect(),
        },
        ExprKind::Try(e) => ExprKind::Try(Box::new(subst_expr(e, substs))),
    };
    Expr::new(kind, expr.span)
}

pub fn subst_stmt(stmt: &Stmt, substs: &HashMap<String, Type>) -> Stmt {
    match stmt {
        Stmt::Let {
            is_mut,
            name,
            ty,
            init,
            span,
        } => Stmt::Let {
            is_mut: *is_mut,
            name: name.clone(),
            ty: ty.as_ref().map(|t| subst_type_expr(t, substs)),
            init: init.as_ref().map(|e| subst_expr(e, substs)),
            span: *span,
        },
        Stmt::Assign {
            target,
            op,
            value,
            span,
        } => Stmt::Assign {
            target: subst_expr(target, substs),
            op: *op,
            value: subst_expr(value, substs),
            span: *span,
        },
        Stmt::While { cond, body, span } => Stmt::While {
            cond: subst_expr(cond, substs),
            body: subst_block(body, substs),
            span: *span,
        },
        Stmt::Loop { body, span } => Stmt::Loop {
            body: subst_block(body, substs),
            span: *span,
        },
        Stmt::For {
            var,
            iter,
            body,
            span,
        } => Stmt::For {
            var: var.clone(),
            iter: subst_expr(iter, substs),
            body: subst_block(body, substs),
            span: *span,
        },
        Stmt::Return { value, span } => Stmt::Return {
            value: value.as_ref().map(|e| subst_expr(e, substs)),
            span: *span,
        },
        Stmt::Break { span } => Stmt::Break { span: *span },
        Stmt::Continue { span } => Stmt::Continue { span: *span },
        Stmt::Expr(e) => Stmt::Expr(subst_expr(e, substs)),
    }
}

pub fn subst_block(block: &Block, substs: &HashMap<String, Type>) -> Block {
    Block {
        stmts: block.stmts.iter().map(|s| subst_stmt(s, substs)).collect(),
        expr: block.expr.as_ref().map(|e| Box::new(subst_expr(e, substs))),
        span: block.span,
    }
}

pub fn subst_fn_decl(f: &FnDecl, mangled_name: String, substs: &HashMap<String, Type>) -> FnDecl {
    FnDecl {
        is_pub: f.is_pub,
        name: Ident::new(mangled_name, f.name.span),
        lifetime_params: f.lifetime_params.clone(),
        type_params: Vec::new(),
        params: f
            .params
            .iter()
            .map(|p| Param {
                is_mut: p.is_mut,
                name: p.name.clone(),
                ty: subst_type_expr(&p.ty, substs),
                span: p.span,
            })
            .collect(),
        ret_type: f.ret_type.as_ref().map(|t| subst_type_expr(t, substs)),
        body: subst_block(&f.body, substs),
        span: f.span,
    }
}

fn unify_type_arg(
    param_ty: &TypeExpr,
    arg_ty: &Type,
    inferred: &mut HashMap<String, Type>,
    type_params: &[TypeParam],
) {
    match &param_ty.kind {
        TypeExprKind::Named(name) => {
            if type_params.iter().any(|p| &p.name.name == name) {
                inferred
                    .entry(name.clone())
                    .or_insert_with(|| arg_ty.clone());
            }
        }
        TypeExprKind::Reference {
            is_mut: _, inner, ..
        } => {
            if let Type::Ref {
                inner: arg_inner, ..
            } = arg_ty
            {
                unify_type_arg(inner, arg_inner, inferred, type_params);
            }
        }
        TypeExprKind::Slice { inner, .. } => {
            if let Type::Slice(arg_inner) = arg_ty {
                unify_type_arg(inner, arg_inner, inferred, type_params);
            } else if let Type::Ref {
                inner: ref_inner, ..
            } = arg_ty
            {
                if let Type::Slice(arg_inner) = ref_inner.as_ref() {
                    unify_type_arg(inner, arg_inner, inferred, type_params);
                }
            }
        }
        TypeExprKind::Array { inner, .. } => {
            if let Type::Array {
                inner: arg_inner, ..
            } = arg_ty
            {
                unify_type_arg(inner, arg_inner, inferred, type_params);
            }
        }
        _ => {}
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

fn get_by_value_aggregate_deps(ty: &Type) -> Vec<String> {
    match ty {
        Type::Struct(name) => {
            if ty.is_vec() || ty.is_string() {
                vec![]
            } else {
                vec![name.clone()]
            }
        }
        Type::Enum(name) => {
            if name.starts_with("Option__")
                || name.starts_with("Result__")
                || name == "Option"
                || name == "Result"
            {
                vec![]
            } else {
                vec![name.clone()]
            }
        }
        Type::Array { inner, .. } => get_by_value_aggregate_deps(inner),
        _ => vec![],
    }
}

fn detect_cycle(
    start: &str,
    current: &str,
    adj: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    path.push(current.to_string());
    if let Some(neighbors) = adj.get(current) {
        for next in neighbors {
            if next == start {
                let mut cycle = path.clone();
                cycle.push(next.clone());
                return Some(cycle);
            }
            if !visited.contains(next) {
                visited.insert(next.clone());
                if let Some(c) = detect_cycle(start, next, adj, visited, path) {
                    return Some(c);
                }
            }
        }
    }
    path.pop();
    None
}

fn check_duplicate_lifetimes(
    lts: &[Ident],
    item_name: &str,
    item_kind: &str,
    diags: &mut DiagnosticBag,
) {
    let mut seen = HashSet::new();
    for lt in lts {
        if !seen.insert(&lt.name) {
            diags.emit(
                Diagnostic::error(format!(
                    "Duplicate lifetime parameter '{}' in {} '{}'",
                    lt.name, item_kind, item_name
                ))
                .with_code(codes::E0310_DUPLICATE_LIFETIME)
                .with_span(lt.span),
            );
        }
    }
}

fn collect_lifetimes_in_type(ty: &TypeExpr, out: &mut Vec<Ident>) {
    match &ty.kind {
        TypeExprKind::Reference {
            lifetime, inner, ..
        } => {
            if let Some(lt) = lifetime {
                out.push(lt.clone());
            }
            collect_lifetimes_in_type(inner, out);
        }
        TypeExprKind::Slice { lifetime, inner } => {
            if let Some(lt) = lifetime {
                out.push(lt.clone());
            }
            collect_lifetimes_in_type(inner, out);
        }
        TypeExprKind::Generic { type_args, .. } => {
            for arg in type_args {
                if let TypeExprKind::Lifetime(lt) = &arg.kind {
                    out.push(lt.clone());
                } else {
                    collect_lifetimes_in_type(arg, out);
                }
            }
        }
        TypeExprKind::Lifetime(lt) => {
            out.push(lt.clone());
        }
        _ => {}
    }
}

fn is_reference_type(ty: &TypeExpr) -> bool {
    match &ty.kind {
        TypeExprKind::Reference { .. } | TypeExprKind::Slice { .. } => true,
        TypeExprKind::Generic { type_args, .. } => type_args
            .iter()
            .any(|arg| matches!(arg.kind, TypeExprKind::Lifetime(_)) || is_reference_type(arg)),
        _ => false,
    }
}

fn extract_return_reference_lifetime(ty: &TypeExpr) -> Option<Option<Ident>> {
    match &ty.kind {
        TypeExprKind::Reference { lifetime, .. } | TypeExprKind::Slice { lifetime, .. } => {
            Some(lifetime.clone())
        }
        TypeExprKind::Generic { name, type_args } if name == "Option" && !type_args.is_empty() => {
            extract_return_reference_lifetime(&type_args[0])
        }
        _ => None,
    }
}

fn extract_borrow_source(expr: &Expr, ctx: &SemanticContext) -> Option<SymbolId> {
    match &expr.kind {
        ExprKind::Ident(name) => {
            let sym_id = ctx.lookup_symbol(name)?;
            if let Some(src) = ctx.borrowed_sources.get(&sym_id) {
                Some(*src)
            } else {
                Some(sym_id)
            }
        }
        ExprKind::Ref { expr: inner, .. } => extract_borrow_source(inner, ctx),
        ExprKind::MethodCall { target, .. } => extract_borrow_source(target, ctx),
        _ => None,
    }
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        Self {
            ctx: SemanticContext::new(),
        }
    }

    fn validate_fn_signature_lifetimes(&mut self, f: &FnDecl, diags: &mut DiagnosticBag) {
        check_duplicate_lifetimes(&f.lifetime_params, &f.name.name, "function", diags);

        let mut fn_lts = HashSet::new();
        for lt in &f.lifetime_params {
            fn_lts.insert(lt.name.clone());
        }

        let mut all_active = self.ctx.active_lifetime_params.clone();
        for lt in &fn_lts {
            all_active.insert(lt.clone());
        }

        let mut input_all_lifetimes: HashSet<String> = HashSet::new();
        let mut ref_param_count = 0;

        for p in &f.params {
            let mut lts = Vec::new();
            collect_lifetimes_in_type(&p.ty, &mut lts);
            for lt in &lts {
                if lt.name != "'static" && lt.name != "static" && !all_active.contains(&lt.name) {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Undeclared lifetime parameter '{}' in parameter '{}'",
                            lt.name, p.name.name
                        ))
                        .with_code(codes::E0311_UNDECLARED_LIFETIME)
                        .with_span(lt.span),
                    );
                }
                input_all_lifetimes.insert(lt.name.clone());
            }

            if is_reference_type(&p.ty) {
                ref_param_count += 1;
            }
        }

        if let Some(ret_type_expr) = &f.ret_type {
            let mut ret_lts = Vec::new();
            collect_lifetimes_in_type(ret_type_expr, &mut ret_lts);
            for lt in &ret_lts {
                if lt.name != "'static" && lt.name != "static" && !all_active.contains(&lt.name) {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Undeclared lifetime parameter '{}' in return type of function '{}'",
                            lt.name, f.name.name
                        ))
                        .with_code(codes::E0311_UNDECLARED_LIFETIME)
                        .with_span(lt.span),
                    );
                }
            }

            if let Some(ret_lt_opt) = extract_return_reference_lifetime(ret_type_expr) {
                match ret_lt_opt {
                    Some(ret_lt) => {
                        if ret_lt.name != "'static"
                            && ret_lt.name != "static"
                            && !input_all_lifetimes.contains(&ret_lt.name)
                        {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Lifetime parameter '{}' in return type of function '{}' does not match any input parameter",
                                    ret_lt.name, f.name.name
                                ))
                                .with_code(codes::E0312_LIFETIME_OUTLIVES_ORIGIN)
                                .with_span(ret_lt.span),
                            );
                        }
                    }
                    None => {
                        if ref_param_count == 0 {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Function '{}' returns a reference, but does not take any reference parameters to borrow from",
                                    f.name.name
                                ))
                                .with_code(codes::E0314_RETURNING_LOCAL_REFERENCE)
                                .with_span(ret_type_expr.span),
                            );
                        } else if ref_param_count > 1 {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Ambiguous lifetime elision in function '{}': multiple reference parameters exist, so return reference must specify an explicit lifetime parameter",
                                    f.name.name
                                ))
                                .with_code(codes::E0313_AMBIGUOUS_LIFETIME_ELISION)
                                .with_span(ret_type_expr.span),
                            );
                        }
                    }
                }
            }
        }
    }

    fn compute_expr_provenance(&self, expr: &Expr) -> Provenance {
        match &expr.kind {
            ExprKind::Ref { expr: inner, .. } => match &inner.kind {
                ExprKind::Ident(_) => Provenance::Local,
                ExprKind::String(_) => Provenance::Static,
                ExprKind::FieldAccess { target, .. } => self.compute_expr_provenance(target),
                ExprKind::Index { array, .. } => self.compute_expr_provenance(array),
                ExprKind::Deref(base) => self.compute_expr_provenance(base),
                _ => Provenance::Local,
            },
            ExprKind::Ident(name) => {
                if let Some(sym_id) = self.ctx.lookup_symbol(name) {
                    self.ctx
                        .ref_provenances
                        .get(&sym_id)
                        .copied()
                        .unwrap_or(Provenance::Local)
                } else {
                    Provenance::Local
                }
            }
            ExprKind::String(_) => Provenance::Static,
            ExprKind::Block(b) => {
                if let Some(ref e) = b.expr {
                    self.compute_expr_provenance(e)
                } else if let Some(Stmt::Expr(e)) = b.stmts.last() {
                    self.compute_expr_provenance(e)
                } else {
                    Provenance::Local
                }
            }
            ExprKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let p1 = match &then_branch.expr {
                    Some(e) => self.compute_expr_provenance(e),
                    None => Provenance::Local,
                };
                let p2 = match else_branch {
                    Some(ElseBranch::Block(b)) => match &b.expr {
                        Some(e) => self.compute_expr_provenance(e),
                        None => Provenance::Local,
                    },
                    Some(ElseBranch::If(e)) => self.compute_expr_provenance(e),
                    None => Provenance::Local,
                };
                if p1 == Provenance::Local || p2 == Provenance::Local {
                    Provenance::Local
                } else if p1 == Provenance::Param || p2 == Provenance::Param {
                    Provenance::Param
                } else {
                    Provenance::Static
                }
            }
            ExprKind::Match { arms, .. } => {
                let mut has_local = false;
                let mut has_param = false;
                for arm in arms {
                    let p = self.compute_expr_provenance(&arm.body);
                    if p == Provenance::Local {
                        has_local = true;
                    } else if p == Provenance::Param {
                        has_param = true;
                    }
                }
                if has_local {
                    Provenance::Local
                } else if has_param {
                    Provenance::Param
                } else {
                    Provenance::Static
                }
            }
            ExprKind::Call { args, .. } => {
                let mut has_local = false;
                let mut has_param = false;
                for arg in args {
                    let p = self.compute_expr_provenance(arg);
                    if p == Provenance::Local {
                        has_local = true;
                    } else if p == Provenance::Param {
                        has_param = true;
                    }
                }
                if has_local {
                    Provenance::Local
                } else if has_param {
                    Provenance::Param
                } else {
                    Provenance::Local
                }
            }
            ExprKind::MethodCall { target, args, .. } => {
                let mut has_local = false;
                let mut has_param = false;
                let p_target = self.compute_expr_provenance(target);
                if p_target == Provenance::Local {
                    has_local = true;
                } else if p_target == Provenance::Param {
                    has_param = true;
                }
                for arg in args {
                    let p = self.compute_expr_provenance(arg);
                    if p == Provenance::Local {
                        has_local = true;
                    } else if p == Provenance::Param {
                        has_param = true;
                    }
                }
                if has_local {
                    Provenance::Local
                } else if has_param {
                    Provenance::Param
                } else {
                    Provenance::Local
                }
            }
            _ => Provenance::Local,
        }
    }

    fn check_return_expr_provenance(&self, expr: &Expr, diags: &mut DiagnosticBag) {
        let prov = self.compute_expr_provenance(expr);
        if prov == Provenance::Local {
            diags.emit(
                Diagnostic::error(
                    "Cannot return reference to local variable or temporary stack storage",
                )
                .with_code(codes::E0314_RETURNING_LOCAL_REFERENCE)
                .with_span(expr.span),
            );
        }
    }

    fn mark_var_moved(&mut self, src_name: &str, span: Span, diags: &mut DiagnosticBag) {
        if let Some(src_id) = self.ctx.lookup_symbol(src_name) {
            if let Some(vs) = self.ctx.var_states.get_mut(&src_id) {
                if vs.borrow != BorrowState::None {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Cannot move '{}' because it is borrowed",
                            src_name
                        ))
                        .with_code(codes::E0306_MUTATE_WHILE_BORROWED)
                        .with_note("values cannot be moved or transferred while borrowed by a reference or iterator")
                        .with_span(span),
                    );
                }
                vs.init = InitState::Moved;
            }
        }
    }

    fn check_type_visibility(
        &self,
        ty: &Type,
        _mod_name: &str,
        span: Span,
        diags: &mut DiagnosticBag,
    ) {
        match ty {
            Type::Struct(sname) => {
                if let Some(s_def) = self.ctx.structs.get(sname) {
                    if let Some(scope) = self.ctx.module_scopes.get(&self.ctx.current_module) {
                        if s_def.defining_module == scope.name {
                            for (loc, canon) in &scope.structs {
                                if canon == sname && !scope.pub_items.contains(loc) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Type '{}' is private and cannot be used in a public signature",
                                            loc
                                        ))
                                        .with_code(codes::E0604_PRIVATE_ITEM)
                                        .with_span(span),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Type::Enum(ename) => {
                if let Some(e_def) = self.ctx.enums.get(ename) {
                    if let Some(scope) = self.ctx.module_scopes.get(&self.ctx.current_module) {
                        if e_def.defining_module == scope.name {
                            for (loc, canon) in &scope.enums {
                                if canon == ename && !scope.pub_items.contains(loc) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Type '{}' is private and cannot be used in a public signature",
                                            loc
                                        ))
                                        .with_code(codes::E0604_PRIVATE_ITEM)
                                        .with_span(span),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Type::Ref { inner, .. } => self.check_type_visibility(inner, _mod_name, span, diags),
            Type::Array { inner, .. } => self.check_type_visibility(inner, _mod_name, span, diags),
            Type::Slice(inner) => self.check_type_visibility(inner, _mod_name, span, diags),
            _ => {}
        }
    }

    pub fn analyze(self, program: &Program, diags: &mut DiagnosticBag) -> SemanticContext {
        let root_id = ModuleId(0);
        let modules = vec![ModuleNode {
            id: root_id,
            name: "main".into(),
            path: Vec::new(),
            file_path: std::path::PathBuf::from("main.va"),
            file_id: vanta_source::FileId(1),
            parent: None,
            children: HashMap::new(),
            is_pub: true,
            items: program.items.clone(),
        }];
        let mut path_to_module = HashMap::new();
        path_to_module.insert(Vec::new(), root_id);

        let mut tree = ModuleTree {
            root: root_id,
            modules,
            path_to_module,
        };

        self.analyze_modules(&mut tree, diags)
    }

    pub fn analyze_modules(
        mut self,
        module_tree: &mut ModuleTree,
        diags: &mut DiagnosticBag,
    ) -> SemanticContext {
        self.ctx.module_tree = Some(module_tree.clone());

        // Pass 1: Initialize ModuleScopes and register local declarations
        for mod_node in &module_tree.modules {
            let is_root = mod_node.id == module_tree.root;
            let prefix = mod_node.canonical_prefix();

            let mut scope = ModuleScope {
                id: mod_node.id,
                name: mod_node.name.clone(),
                path: mod_node.path.clone(),
                canonical_prefix: prefix.clone(),
                functions: HashMap::new(),
                structs: HashMap::new(),
                enums: HashMap::new(),
                traits: HashMap::new(),
                pub_items: HashSet::new(),
                imported_items: HashSet::new(),
            };

            for item in &mod_node.items {
                match item {
                    Item::Struct(s) => {
                        let canon_name = if is_root {
                            s.name.name.clone()
                        } else {
                            format!("{}{}", prefix, s.name.name)
                        };
                        scope
                            .structs
                            .insert(s.name.name.clone(), canon_name.clone());
                        if s.is_pub {
                            scope.pub_items.insert(s.name.name.clone());
                        }
                        check_duplicate_lifetimes(
                            &s.lifetime_params,
                            &s.name.name,
                            "struct",
                            diags,
                        );
                        if s.type_params.is_empty() && s.lifetime_params.is_empty() {
                            self.ctx.declared_struct_names.insert(canon_name.clone());
                        } else {
                            let mut seen_tps = HashSet::new();
                            for tp in &s.type_params {
                                if !seen_tps.insert(&tp.name.name) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Duplicate type parameter '{}' in struct '{}'",
                                            tp.name.name, s.name.name
                                        ))
                                        .with_code(codes::E0101_DUPLICATE_SYMBOL)
                                        .with_span(tp.span),
                                    );
                                }
                            }
                            self.ctx
                                .generic_structs
                                .insert(canon_name.clone(), s.clone());
                        }
                    }
                    Item::Enum(e) => {
                        let canon_name = if is_root {
                            e.name.name.clone()
                        } else {
                            format!("{}{}", prefix, e.name.name)
                        };
                        scope.enums.insert(e.name.name.clone(), canon_name.clone());
                        if e.is_pub {
                            scope.pub_items.insert(e.name.name.clone());
                        }
                        check_duplicate_lifetimes(&e.lifetime_params, &e.name.name, "enum", diags);
                        if e.type_params.is_empty() && e.lifetime_params.is_empty() {
                            self.ctx.declared_enum_names.insert(canon_name.clone());
                        } else {
                            let mut seen_tps = HashSet::new();
                            for tp in &e.type_params {
                                if !seen_tps.insert(&tp.name.name) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Duplicate type parameter '{}' in enum '{}'",
                                            tp.name.name, e.name.name
                                        ))
                                        .with_code(codes::E0101_DUPLICATE_SYMBOL)
                                        .with_span(tp.span),
                                    );
                                }
                            }
                            self.ctx.generic_enums.insert(canon_name.clone(), e.clone());
                        }
                    }
                    Item::Function(f) => {
                        let canon_name = if is_root && f.name.name == "main" {
                            "main".to_string()
                        } else if is_root {
                            f.name.name.clone()
                        } else {
                            format!("{}{}", prefix, f.name.name)
                        };
                        scope
                            .functions
                            .insert(f.name.name.clone(), canon_name.clone());
                        if f.is_pub {
                            scope.pub_items.insert(f.name.name.clone());
                        }
                        check_duplicate_lifetimes(
                            &f.lifetime_params,
                            &f.name.name,
                            "function",
                            diags,
                        );
                        if !f.type_params.is_empty() {
                            let mut seen_tps = HashSet::new();
                            for tp in &f.type_params {
                                if !seen_tps.insert(&tp.name.name) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Duplicate type parameter '{}' in function '{}'",
                                            tp.name.name, f.name.name
                                        ))
                                        .with_code(codes::E0101_DUPLICATE_SYMBOL)
                                        .with_span(tp.span),
                                    );
                                }
                            }
                            self.ctx
                                .generic_functions
                                .insert(canon_name.clone(), f.clone());
                        }
                    }
                    Item::Mod(m) => {
                        if m.is_pub {
                            scope.pub_items.insert(m.name.name.clone());
                        }
                    }
                    Item::Trait(t) => {
                        let canon_name = if is_root {
                            t.name.name.clone()
                        } else {
                            format!("{}{}", prefix, t.name.name)
                        };
                        check_duplicate_lifetimes(&t.lifetime_params, &t.name.name, "trait", diags);
                        let prev_t_lts = self.ctx.active_lifetime_params.clone();
                        for lt in &t.lifetime_params {
                            self.ctx.active_lifetime_params.insert(lt.name.clone());
                        }
                        if scope.traits.contains_key(&t.name.name) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Duplicate trait '{}' in module '{}'",
                                    t.name.name, mod_node.name
                                ))
                                .with_code(codes::E0708_DUPLICATE_TRAIT)
                                .with_span(t.name.span),
                            );
                            self.ctx.active_lifetime_params = prev_t_lts;
                            continue;
                        }
                        scope.traits.insert(t.name.name.clone(), canon_name.clone());
                        if t.is_pub {
                            scope.pub_items.insert(t.name.name.clone());
                        }
                        self.ctx.declared_trait_names.insert(canon_name.clone());

                        let mut methods = HashMap::new();
                        let mut seen_method_names = HashSet::new();
                        for m in &t.methods {
                            if !seen_method_names.insert(m.name.name.clone()) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Duplicate method '{}' in trait '{}'",
                                        m.name.name, t.name.name
                                    ))
                                    .with_code(codes::E0101_DUPLICATE_SYMBOL)
                                    .with_span(m.name.span),
                                );
                                continue;
                            }

                            if m.params.is_empty() || m.params[0].name.name != "self" {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Trait method '{}' must have 'self' as its first parameter",
                                        m.name.name
                                    ))
                                    .with_code(codes::E0709_INVALID_TRAIT_RECEIVER)
                                    .with_span(m.span),
                                );
                                continue;
                            }

                            let is_mut_self = match &m.params[0].ty.kind {
                                TypeExprKind::Reference { is_mut, inner, .. } => {
                                    if let TypeExprKind::Named(ref n) = inner.kind {
                                        if n == "Self" {
                                            *is_mut
                                        } else {
                                            diags.emit(
                                                Diagnostic::error(format!(
                                                    "Receiver for trait method '{}' must be '&Self' or '&mut Self', found '&{}'",
                                                    m.name.name, n
                                                ))
                                                .with_code(codes::E0709_INVALID_TRAIT_RECEIVER)
                                                .with_span(m.params[0].span),
                                            );
                                            continue;
                                        }
                                    } else {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Receiver for trait method '{}' must be '&Self' or '&mut Self'",
                                                m.name.name
                                            ))
                                            .with_code(codes::E0709_INVALID_TRAIT_RECEIVER)
                                            .with_span(m.params[0].span),
                                        );
                                        continue;
                                    }
                                }
                                _ => {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Receiver for trait method '{}' must be '&Self' or '&mut Self'",
                                            m.name.name
                                        ))
                                        .with_code(codes::E0709_INVALID_TRAIT_RECEIVER)
                                        .with_span(m.params[0].span),
                                    );
                                    continue;
                                }
                            };

                            let is_generic_trait = !t.type_params.is_empty();
                            let mut params = Vec::new();
                            params.push((
                                "self".to_string(),
                                Type::Ref {
                                    is_mut: is_mut_self,
                                    inner: Box::new(Type::Unit),
                                },
                                is_mut_self,
                            ));
                            if !is_generic_trait {
                                for p in m.params.iter().skip(1) {
                                    let pty = self.ctx.type_from_ast(&p.ty, diags);
                                    params.push((p.name.name.clone(), pty, p.is_mut));
                                }
                            }
                            let ret_type = if !is_generic_trait {
                                match &m.ret_type {
                                    Some(t) => self.ctx.type_from_ast(t, diags),
                                    None => Type::Unit,
                                }
                            } else {
                                Type::Unit
                            };

                            methods.insert(
                                m.name.name.clone(),
                                TraitMethodSig {
                                    name: m.name.name.clone(),
                                    is_mut_self,
                                    params,
                                    ret_type,
                                    span: m.span,
                                },
                            );
                        }

                        self.ctx.traits.insert(
                            canon_name.clone(),
                            TraitDef {
                                name: t.name.name.clone(),
                                canonical_name: canon_name,
                                is_pub: t.is_pub,
                                type_params: t.type_params.clone(),
                                methods,
                                ast_methods: t.methods.clone(),
                                defining_module: mod_node.name.clone(),
                                span: t.span,
                            },
                        );
                        self.ctx.active_lifetime_params = prev_t_lts;
                    }
                    Item::Impl(impl_block) => {
                        check_duplicate_lifetimes(
                            &impl_block.lifetime_params,
                            &impl_block.target.name,
                            "impl block",
                            diags,
                        );
                        for m in &impl_block.methods {
                            check_duplicate_lifetimes(
                                &m.lifetime_params,
                                &m.name.name,
                                "method",
                                diags,
                            );
                        }
                    }
                    _ => {}
                }
            }

            self.ctx.module_scopes.insert(mod_node.id, scope);
        }

        // Pass 2a: Fixed-point resolution of imports & re-exports
        let max_passes = 16;
        for _pass in 0..max_passes {
            let mut any_new_import = false;

            let num_modules = module_tree.modules.len();
            for mod_idx in 0..num_modules {
                let mod_id = ModuleId(mod_idx);
                self.ctx.current_module = mod_id;
                let items = module_tree.modules[mod_idx].items.clone();

                for item in &items {
                    if let Item::Import(imp) = item {
                        let mod_path: Vec<String> =
                            imp.path.iter().map(|id| id.name.clone()).collect();
                        if let Ok(target_mod_id) =
                            module_tree.resolve_module_path(mod_id, &mod_path)
                        {
                            let target_mod = module_tree.get(target_mod_id).clone();
                            let target_scope =
                                self.ctx.module_scopes.get(&target_mod_id).unwrap().clone();

                            for item_ident in &imp.items {
                                let item_name = &item_ident.name;

                                let curr_scope = self.ctx.module_scopes.get(&mod_id).unwrap();
                                if curr_scope.imported_items.contains(item_name) {
                                    continue;
                                }

                                let is_fn = target_scope.functions.contains_key(item_name);
                                let is_struct = target_scope.structs.contains_key(item_name);
                                let is_enum = target_scope.enums.contains_key(item_name);
                                let is_trait = target_scope.traits.contains_key(item_name);
                                let is_child_mod = target_mod.children.get(item_name).copied();

                                if !is_fn
                                    && !is_struct
                                    && !is_enum
                                    && !is_trait
                                    && is_child_mod.is_none()
                                {
                                    continue;
                                }

                                if target_mod_id != mod_id {
                                    if let Some(child_id) = is_child_mod {
                                        let child_node = module_tree.get(child_id);
                                        if !child_node.is_pub {
                                            continue;
                                        }
                                    } else if !target_scope.pub_items.contains(item_name) {
                                        continue;
                                    }
                                }

                                let curr_scope = self.ctx.module_scopes.get_mut(&mod_id).unwrap();

                                curr_scope.imported_items.insert(item_name.clone());
                                if let Some(canon) = target_scope.functions.get(item_name) {
                                    curr_scope
                                        .functions
                                        .insert(item_name.clone(), canon.clone());
                                }
                                if let Some(canon) = target_scope.structs.get(item_name) {
                                    curr_scope.structs.insert(item_name.clone(), canon.clone());
                                }
                                if let Some(canon) = target_scope.enums.get(item_name) {
                                    curr_scope.enums.insert(item_name.clone(), canon.clone());
                                }
                                if let Some(canon) = target_scope.traits.get(item_name) {
                                    curr_scope.traits.insert(item_name.clone(), canon.clone());
                                }
                                if let Some(child_id) = is_child_mod {
                                    module_tree
                                        .get_mut(mod_id)
                                        .children
                                        .insert(item_name.clone(), child_id);
                                }
                                if imp.is_pub {
                                    curr_scope.pub_items.insert(item_name.clone());
                                }
                                any_new_import = true;
                            }
                        }
                    }
                }
            }

            if !any_new_import {
                break;
            }
        }

        // Pass 2b: Validate all imports and report errors
        let num_modules = module_tree.modules.len();
        for mod_idx in 0..num_modules {
            let mod_id = ModuleId(mod_idx);
            self.ctx.current_module = mod_id;
            let items = module_tree.modules[mod_idx].items.clone();

            for item in &items {
                if let Item::Import(imp) = item {
                    let mod_path: Vec<String> = imp.path.iter().map(|id| id.name.clone()).collect();
                    match module_tree.resolve_module_path(mod_id, &mod_path) {
                        Ok(target_mod_id) => {
                            let target_mod = module_tree.get(target_mod_id).clone();
                            let target_scope =
                                self.ctx.module_scopes.get(&target_mod_id).unwrap().clone();

                            for item_ident in &imp.items {
                                let item_name = &item_ident.name;
                                let is_fn = target_scope.functions.contains_key(item_name);
                                let is_struct = target_scope.structs.contains_key(item_name);
                                let is_enum = target_scope.enums.contains_key(item_name);
                                let is_trait = target_scope.traits.contains_key(item_name);
                                let is_child_mod = target_mod.children.get(item_name).copied();

                                if !is_fn
                                    && !is_struct
                                    && !is_enum
                                    && !is_trait
                                    && is_child_mod.is_none()
                                {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Cannot find item '{}' in module '{}'",
                                            item_name, target_mod.name
                                        ))
                                        .with_code(codes::E0603_UNKNOWN_IMPORT)
                                        .with_span(item_ident.span),
                                    );
                                    continue;
                                }

                                if target_mod_id != mod_id {
                                    if let Some(child_id) = is_child_mod {
                                        let child_node = module_tree.get(child_id);
                                        if !child_node.is_pub {
                                            diags.emit(
                                                Diagnostic::error(format!(
                                                    "Module '{}' in module '{}' is private",
                                                    item_name, target_mod.name
                                                ))
                                                .with_code(codes::E0604_PRIVATE_ITEM)
                                                .with_span(item_ident.span),
                                            );
                                        }
                                    } else if !target_scope.pub_items.contains(item_name) {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Item '{}' in module '{}' is private",
                                                item_name, target_mod.name
                                            ))
                                            .with_code(codes::E0604_PRIVATE_ITEM)
                                            .with_span(item_ident.span),
                                        );
                                    }
                                }

                                let curr_scope = self.ctx.module_scopes.get(&mod_id).unwrap();

                                // Check collisions in current module with local declarations
                                if (curr_scope.functions.contains_key(item_name)
                                    || curr_scope.structs.contains_key(item_name)
                                    || curr_scope.enums.contains_key(item_name)
                                    || curr_scope.traits.contains_key(item_name))
                                    && !curr_scope.imported_items.contains(item_name)
                                {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Identifier '{}' is already declared or imported in this module",
                                            item_name
                                        ))
                                        .with_code(codes::E0101_DUPLICATE_SYMBOL)
                                        .with_span(item_ident.span),
                                    );
                                }
                            }
                        }
                        Err((msg, code)) => {
                            let mut d = Diagnostic::error(msg).with_span(imp.span);
                            if let Some(c) = code {
                                d = d.with_code(c);
                            }
                            diags.emit(d);
                        }
                    }
                }
            }
        }

        self.ctx.module_tree = Some(module_tree.clone());

        // Pass 3: Struct and enum layout computation
        struct RawStruct<'a> {
            decl: &'a StructDecl,
            canonical_name: String,
            defining_module: String,
            fields: Vec<(String, Type, bool)>, // (name, ty, is_pub)
        }

        struct RawEnum<'a> {
            decl: &'a EnumDecl,
            canonical_name: String,
            defining_module: String,
            variants: Vec<(String, Vec<Type>)>,
        }

        let mut raw_structs = Vec::new();
        let mut raw_enums = Vec::new();
        let mut declared_canonical_names = HashSet::new();

        for mod_node in &module_tree.modules {
            self.ctx.current_module = mod_node.id;
            let mod_name = mod_node.name.clone();
            let scope = self.ctx.module_scopes.get(&mod_node.id).unwrap().clone();

            for item in &mod_node.items {
                match item {
                    Item::Struct(s) => {
                        if !s.type_params.is_empty() {
                            continue;
                        }
                        let canon_name = scope.structs.get(&s.name.name).unwrap().clone();
                        if !declared_canonical_names.insert(canon_name.clone()) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Duplicate symbol definition '{}'",
                                    s.name.name
                                ))
                                .with_code(codes::E0101_DUPLICATE_SYMBOL)
                                .with_span(s.name.span),
                            );
                            continue;
                        }

                        let prev_s_lts = self.ctx.active_lifetime_params.clone();
                        for lt in &s.lifetime_params {
                            self.ctx.active_lifetime_params.insert(lt.name.clone());
                        }

                        let mut seen_fields = HashSet::new();
                        let mut fields = Vec::new();
                        for f in &s.fields {
                            if !seen_fields.insert(f.name.name.clone()) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Duplicate field '{}' in struct '{}'",
                                        f.name.name, s.name.name
                                    ))
                                    .with_code(codes::E0106_DUPLICATE_FIELD)
                                    .with_span(f.name.span),
                                );
                            }
                            let f_ty = self.ctx.type_from_ast(&f.ty, diags);
                            fields.push((f.name.name.clone(), f_ty, f.is_pub));
                        }
                        self.ctx.active_lifetime_params = prev_s_lts;

                        raw_structs.push(RawStruct {
                            decl: s,
                            canonical_name: canon_name,
                            defining_module: mod_name.clone(),
                            fields,
                        });
                    }
                    Item::Enum(e) => {
                        if !e.type_params.is_empty() {
                            continue;
                        }
                        let canon_name = scope.enums.get(&e.name.name).unwrap().clone();
                        if !declared_canonical_names.insert(canon_name.clone()) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Duplicate symbol definition '{}'",
                                    e.name.name
                                ))
                                .with_code(codes::E0101_DUPLICATE_SYMBOL)
                                .with_span(e.name.span),
                            );
                            continue;
                        }

                        let prev_e_lts = self.ctx.active_lifetime_params.clone();
                        for lt in &e.lifetime_params {
                            self.ctx.active_lifetime_params.insert(lt.name.clone());
                        }

                        let mut seen_variants = HashSet::new();
                        let mut variants = Vec::new();
                        for v in &e.variants {
                            if !seen_variants.insert(v.name.name.clone()) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Duplicate variant '{}' in enum '{}'",
                                        v.name.name, e.name.name
                                    ))
                                    .with_code(codes::E0505_DUPLICATE_VARIANT)
                                    .with_span(v.name.span),
                                );
                            }
                            let mut payload = Vec::new();
                            for p in &v.payload {
                                payload.push(self.ctx.type_from_ast(p, diags));
                            }
                            variants.push((v.name.name.clone(), payload));
                        }
                        self.ctx.active_lifetime_params = prev_e_lts;

                        raw_enums.push(RawEnum {
                            decl: e,
                            canonical_name: canon_name,
                            defining_module: mod_name.clone(),
                            variants,
                        });
                    }
                    Item::Impl(impl_block) => {
                        check_duplicate_lifetimes(
                            &impl_block.lifetime_params,
                            &impl_block.target.name,
                            "impl block",
                            diags,
                        );
                        for m in &impl_block.methods {
                            check_duplicate_lifetimes(
                                &m.lifetime_params,
                                &m.name.name,
                                "method",
                                diags,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for raw in &raw_structs {
            let mut deps = Vec::new();
            for (_, f_ty, _) in &raw.fields {
                for dep in get_by_value_aggregate_deps(f_ty) {
                    deps.push(dep);
                }
            }
            adj.insert(raw.canonical_name.clone(), deps);
        }
        for raw in &raw_enums {
            let mut deps = Vec::new();
            for (_, payload) in &raw.variants {
                for p_ty in payload {
                    for dep in get_by_value_aggregate_deps(p_ty) {
                        deps.push(dep);
                    }
                }
            }
            adj.insert(raw.canonical_name.clone(), deps);
        }

        // Cycle detection
        let mut cyclic_types = HashSet::new();
        for raw in &raw_structs {
            let name = &raw.canonical_name;
            let mut visited = HashSet::new();
            let mut path = Vec::new();
            if let Some(cycle) = detect_cycle(name, name, &adj, &mut visited, &mut path) {
                cyclic_types.insert(name.clone());
                let cycle_str = cycle.join(" -> ");
                diags.emit(
                    Diagnostic::error(format!(
                        "Struct '{}' has infinite size due to recursive cycle: {}",
                        raw.decl.name.name, cycle_str
                    ))
                    .with_code(codes::E0208_RECURSIVE_STRUCT)
                    .with_span(raw.decl.span),
                );
            }
        }
        for raw in &raw_enums {
            let name = &raw.canonical_name;
            let mut visited = HashSet::new();
            let mut path = Vec::new();
            if let Some(cycle) = detect_cycle(name, name, &adj, &mut visited, &mut path) {
                cyclic_types.insert(name.clone());
                let cycle_str = cycle.join(" -> ");
                diags.emit(
                    Diagnostic::error(format!(
                        "Enum '{}' has infinite size due to recursive cycle: {}",
                        raw.decl.name.name, cycle_str
                    ))
                    .with_code(codes::E0507_RECURSIVE_ENUM)
                    .with_span(raw.decl.span),
                );
            }
        }

        // Compute layouts in topological / iterative order
        let mut pending_structs: Vec<_> = raw_structs
            .into_iter()
            .filter(|r| !cyclic_types.contains(&r.canonical_name))
            .collect();
        let mut pending_enums: Vec<_> = raw_enums
            .into_iter()
            .filter(|r| !cyclic_types.contains(&r.canonical_name))
            .collect();

        let mut changed = true;
        while changed && (!pending_structs.is_empty() || !pending_enums.is_empty()) {
            changed = false;

            let mut still_pending_structs = Vec::new();
            for raw in pending_structs {
                let all_deps_resolved = raw.fields.iter().all(|(_, fty, _)| {
                    for dep in get_by_value_aggregate_deps(fty) {
                        if !self.ctx.structs.contains_key(&dep)
                            && !self.ctx.enums.contains_key(&dep)
                        {
                            return false;
                        }
                    }
                    true
                });

                if all_deps_resolved {
                    let def = StructDef::compute_layout_with_enums_and_module(
                        raw.canonical_name.clone(),
                        raw.fields,
                        &self.ctx.structs,
                        &self.ctx.enums,
                        raw.defining_module.clone(),
                        raw.decl.span,
                    );
                    self.ctx.structs.insert(raw.canonical_name.clone(), def);
                    changed = true;
                } else {
                    still_pending_structs.push(raw);
                }
            }
            pending_structs = still_pending_structs;

            let mut still_pending_enums = Vec::new();
            for raw in pending_enums {
                let all_deps_resolved = raw.variants.iter().all(|(_, payload)| {
                    for pty in payload {
                        for dep in get_by_value_aggregate_deps(pty) {
                            if !self.ctx.structs.contains_key(&dep)
                                && !self.ctx.enums.contains_key(&dep)
                            {
                                return false;
                            }
                        }
                    }
                    true
                });

                if all_deps_resolved {
                    match EnumDef::compute_layout_with_module(
                        raw.canonical_name.clone(),
                        raw.variants,
                        &self.ctx.structs,
                        &self.ctx.enums,
                        raw.defining_module.clone(),
                        raw.decl.span,
                    ) {
                        Ok(def) => {
                            self.ctx.enums.insert(raw.canonical_name.clone(), def);
                            changed = true;
                        }
                        Err(e) => {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Failed to compute layout for enum '{}': {}",
                                    raw.decl.name.name, e
                                ))
                                .with_code(codes::E0507_RECURSIVE_ENUM)
                                .with_span(raw.decl.span),
                            );
                        }
                    }
                } else {
                    still_pending_enums.push(raw);
                }
            }
            pending_enums = still_pending_enums;
        }

        // Pass 4: Function and method signatures
        for mod_node in &module_tree.modules {
            self.ctx.current_module = mod_node.id;
            let scope = self.ctx.module_scopes.get(&mod_node.id).unwrap().clone();

            for item in &mod_node.items {
                match item {
                    Item::Function(f) => {
                        if !f.type_params.is_empty() {
                            for tp in &f.type_params {
                                if let Some(bound) = &tp.bound {
                                    if self
                                        .ctx
                                        .resolve_trait_name(&bound.name, bound.span, diags)
                                        .is_none()
                                    {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Unknown trait '{}' in bound for type parameter '{}'",
                                                bound.name, tp.name.name
                                            ))
                                            .with_code(codes::E0701_UNKNOWN_TRAIT)
                                            .with_span(bound.span),
                                        );
                                    }
                                }
                            }
                            continue;
                        }
                        if f.name.name == "print_i64"
                            || f.name.name == "print_str"
                            || f.name.name == "panic"
                            || f.name.name == "vanta_alloc"
                            || f.name.name == "vanta_realloc"
                            || f.name.name == "vanta_free"
                            || f.name.name == "vanta_active_allocs"
                            || f.name.name == "vanta_io_write_stdout"
                            || f.name.name == "vanta_io_write_stderr"
                            || f.name.name == "vanta_io_read_stdin"
                            || f.name.name == "vanta_fs_read_to_string"
                            || f.name.name == "vanta_fs_write"
                            || f.name.name == "vanta_env_args_count"
                            || f.name.name == "vanta_env_arg_get"
                        {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Cannot redefine built-in intrinsic '{}'",
                                    f.name.name
                                ))
                                .with_code(codes::E0103_REDEFINED_INTRINSIC)
                                .with_note(
                                    "built-in standard output and runtime intrinsics are globally reserved",
                                )
                                .with_span(f.name.span),
                            );
                            continue;
                        }

                        let canon_name = scope.functions.get(&f.name.name).unwrap().clone();
                        let prev_fn_lts = self.ctx.active_lifetime_params.clone();
                        for lt in &f.lifetime_params {
                            self.ctx.active_lifetime_params.insert(lt.name.clone());
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
                                    .with_code(codes::E0202_UNSIZED_TYPE_VALUE)
                                    .with_expected("&str")
                                    .with_actual("str")
                                    .with_suggestion("change parameter type to '&str'", Some(p.ty.span), "&str")
                                    .with_span(p.span),
                                );
                            }
                            if f.is_pub {
                                self.check_type_visibility(
                                    &p_ty,
                                    &mod_node.name,
                                    f.name.span,
                                    diags,
                                );
                            }
                            params.push((p.name.name.clone(), p_ty, p.is_mut));
                        }
                        let ret_type = match &f.ret_type {
                            Some(t) => {
                                let rty = self.ctx.type_from_ast(t, diags);
                                if f.is_pub {
                                    self.check_type_visibility(
                                        &rty,
                                        &mod_node.name,
                                        f.name.span,
                                        diags,
                                    );
                                }
                                rty
                            }
                            None => Type::Unit,
                        };
                        self.validate_fn_signature_lifetimes(f, diags);
                        self.ctx.active_lifetime_params = prev_fn_lts;

                        if self.ctx.functions.contains_key(&canon_name) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Duplicate function definition '{}'",
                                    f.name.name
                                ))
                                .with_code(codes::E0101_DUPLICATE_SYMBOL)
                                .with_span(f.name.span),
                            );
                        } else {
                            self.ctx.functions.insert(
                                canon_name.clone(),
                                FnSignature {
                                    name: canon_name,
                                    params,
                                    ret_type,
                                    span: f.span,
                                },
                            );
                        }
                    }
                    Item::Impl(impl_block) => {
                        if !impl_block.type_params.is_empty() {
                            continue;
                        }
                        let prev_impl_lts = self.ctx.active_lifetime_params.clone();
                        for lt in &impl_block.lifetime_params {
                            self.ctx.active_lifetime_params.insert(lt.name.clone());
                        }
                        let target_name = &impl_block.target.name;
                        let target_canonical = scope
                            .structs
                            .get(target_name)
                            .cloned()
                            .or_else(|| scope.enums.get(target_name).cloned())
                            .or_else(|| {
                                self.ctx.resolve_struct_name(
                                    target_name,
                                    impl_block.target.span,
                                    diags,
                                )
                            })
                            .or_else(|| {
                                self.ctx.resolve_enum_name(
                                    target_name,
                                    impl_block.target.span,
                                    diags,
                                )
                            })
                            .unwrap_or_else(|| target_name.clone());

                        let is_struct = self.ctx.structs.contains_key(&target_canonical);
                        let is_enum = self.ctx.enums.contains_key(&target_canonical);

                        if !is_struct && !is_enum {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Cannot implement methods for unknown struct '{}'",
                                    impl_block.target.name
                                ))
                                .with_code(codes::E0104_UNKNOWN_STRUCT)
                                .with_span(impl_block.target.span),
                            );
                            self.ctx.active_lifetime_params = prev_impl_lts;
                            continue;
                        }

                        self.ctx.current_impl_target = Some(target_canonical.clone());

                        if let Some(trait_ident) = &impl_block.trait_name {
                            // --- TRAIT IMPLEMENTATION ---
                            let canon_trait = match self.ctx.resolve_trait_name(
                                &trait_ident.name,
                                trait_ident.span,
                                diags,
                            ) {
                                Some(t) => t,
                                None => {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Unknown trait '{}'",
                                            trait_ident.name
                                        ))
                                        .with_code(codes::E0701_UNKNOWN_TRAIT)
                                        .with_span(trait_ident.span),
                                    );
                                    self.ctx.current_impl_target = None;
                                    self.ctx.active_lifetime_params = prev_impl_lts;
                                    continue;
                                }
                            };

                            let impl_key = (canon_trait.clone(), target_canonical.clone());
                            if self.ctx.trait_impls.contains_key(&impl_key) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Conflicting implementation of trait '{}' for type '{}'",
                                        trait_ident.name, impl_block.target.name
                                    ))
                                    .with_code(codes::E0702_DUPLICATE_IMPLEMENTATION)
                                    .with_span(impl_block.span),
                                );
                                self.ctx.current_impl_target = None;
                                self.ctx.active_lifetime_params = prev_impl_lts;
                                continue;
                            }

                            let trait_def = match self.ctx.traits.get(&canon_trait).cloned() {
                                Some(td) => td,
                                None => {
                                    self.ctx.current_impl_target = None;
                                    continue;
                                }
                            };

                            let is_generic_trait = !trait_def.type_params.is_empty();
                            let mut concrete_args = Vec::new();
                            let mut substs = HashMap::new();
                            if is_generic_trait {
                                if impl_block.trait_args.len() != trait_def.type_params.len() {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Trait '{}' expects {} type arguments, but {} were provided",
                                            trait_ident.name,
                                            trait_def.type_params.len(),
                                            impl_block.trait_args.len()
                                        ))
                                        .with_code(codes::E0209_GENERIC_ARITY_MISMATCH)
                                        .with_span(trait_ident.span),
                                    );
                                    self.ctx.current_impl_target = None;
                                    continue;
                                }
                                for (tp, targ) in
                                    trait_def.type_params.iter().zip(&impl_block.trait_args)
                                {
                                    let cty = self.ctx.type_from_ast(targ, diags);
                                    concrete_args.push(cty.clone());
                                    substs.insert(tp.name.name.clone(), cty);
                                }
                            }

                            let trait_method_names: HashSet<String> = if is_generic_trait {
                                trait_def
                                    .ast_methods
                                    .iter()
                                    .map(|m| m.name.name.clone())
                                    .collect()
                            } else {
                                trait_def.methods.keys().cloned().collect()
                            };

                            // E0704_UNKNOWN_TRAIT_METHOD
                            for method in &impl_block.methods {
                                if !trait_method_names.contains(&method.name.name) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Method '{}' is not a member of trait '{}'",
                                            method.name.name, trait_ident.name
                                        ))
                                        .with_code(codes::E0704_UNKNOWN_TRAIT_METHOD)
                                        .with_span(method.span),
                                    );
                                }
                            }

                            // E0703_MISSING_TRAIT_METHOD
                            for m_name in &trait_method_names {
                                if !impl_block.methods.iter().any(|m| &m.name.name == m_name) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Missing implementation of trait method '{}' for type '{}'",
                                            m_name, impl_block.target.name
                                        ))
                                        .with_code(codes::E0703_MISSING_TRAIT_METHOD)
                                        .with_span(impl_block.span),
                                    );
                                }
                            }

                            let mut impl_methods = HashMap::new();

                            for method in &impl_block.methods {
                                if method.params.is_empty() || method.params[0].name.name != "self"
                                {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Method '{}' must have 'self' as its first parameter",
                                            method.name.name
                                        ))
                                        .with_code(codes::E0709_INVALID_TRAIT_RECEIVER)
                                        .with_span(method.span),
                                    );
                                    continue;
                                }

                                let receiver_ty =
                                    self.ctx.type_from_ast(&method.params[0].ty, diags);
                                let (is_ref, is_mut, inner_ty) = match &receiver_ty {
                                    Type::Ref { is_mut, inner } => (true, *is_mut, &**inner),
                                    _ => (false, false, &receiver_ty),
                                };

                                if !is_ref {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Receiver for method '{}' must be a reference (&Self or &mut Self)",
                                            method.name.name
                                        ))
                                        .with_code(codes::E0709_INVALID_TRAIT_RECEIVER)
                                        .with_span(method.params[0].span),
                                    );
                                    continue;
                                }

                                let is_target_match = match inner_ty {
                                    Type::Struct(s) => s == &target_canonical,
                                    Type::Enum(e) => e == &target_canonical,
                                    _ => false,
                                };

                                if !is_target_match {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Invalid receiver type for method '{}' of type '{}'",
                                            method.name.name, impl_block.target.name
                                        ))
                                        .with_code(codes::E0709_INVALID_TRAIT_RECEIVER)
                                        .with_span(method.params[0].span),
                                    );
                                    continue;
                                }

                                let (expected_is_mut, expected_params, expected_ret) =
                                    if is_generic_trait {
                                        if let Some(ast_m) = trait_def
                                            .ast_methods
                                            .iter()
                                            .find(|m| m.name.name == method.name.name)
                                        {
                                            let is_m = match &ast_m.params[0].ty.kind {
                                                TypeExprKind::Reference { is_mut, .. } => *is_mut,
                                                _ => false,
                                            };
                                            let mut ptys = Vec::new();
                                            for p in ast_m.params.iter().skip(1) {
                                                let subst_p = subst_type_expr(&p.ty, &substs);
                                                ptys.push(self.ctx.type_from_ast(&subst_p, diags));
                                            }
                                            let ret_ty = match &ast_m.ret_type {
                                                Some(t) => {
                                                    let subst_t = subst_type_expr(t, &substs);
                                                    self.ctx.type_from_ast(&subst_t, diags)
                                                }
                                                None => Type::Unit,
                                            };
                                            (Some(is_m), Some(ptys), Some(ret_ty))
                                        } else {
                                            (None, None, None)
                                        }
                                    } else if let Some(expected_sig) =
                                        trait_def.methods.get(&method.name.name)
                                    {
                                        let ptys = expected_sig
                                            .params
                                            .iter()
                                            .skip(1)
                                            .map(|(_, ty, _)| ty.clone())
                                            .collect();
                                        (
                                            Some(expected_sig.is_mut_self),
                                            Some(ptys),
                                            Some(expected_sig.ret_type.clone()),
                                        )
                                    } else {
                                        (None, None, None)
                                    };

                                if let (Some(exp_mut), Some(exp_ptys), Some(exp_ret)) =
                                    (expected_is_mut, expected_params, expected_ret)
                                {
                                    if is_mut != exp_mut {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Receiver mutability mismatch for method '{}': expected {}, found {}",
                                                method.name.name,
                                                if exp_mut { "&mut Self" } else { "&Self" },
                                                if is_mut { "&mut Self" } else { "&Self" }
                                            ))
                                            .with_code(codes::E0705_TRAIT_METHOD_SIGNATURE_MISMATCH)
                                            .with_span(method.params[0].span),
                                        );
                                        continue;
                                    }

                                    if method.params.len() != exp_ptys.len() + 1 {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Method '{}' expects {} parameters, found {}",
                                                method.name.name,
                                                exp_ptys.len() + 1,
                                                method.params.len()
                                            ))
                                            .with_code(codes::E0705_TRAIT_METHOD_SIGNATURE_MISMATCH)
                                            .with_span(method.span),
                                        );
                                        continue;
                                    }

                                    let mut params_match = true;
                                    for (p_idx, p) in method.params.iter().enumerate().skip(1) {
                                        let p_ty = self.ctx.type_from_ast(&p.ty, diags);
                                        let expected_ty = &exp_ptys[p_idx - 1];
                                        if p_ty != *expected_ty {
                                            diags.emit(
                                                Diagnostic::error(format!(
                                                    "Parameter '{}' of method '{}' has type {}, expected {}",
                                                    p.name.name, method.name.name, p_ty, expected_ty
                                                ))
                                                .with_code(codes::E0705_TRAIT_METHOD_SIGNATURE_MISMATCH)
                                                .with_span(p.span),
                                            );
                                            params_match = false;
                                            break;
                                        }
                                    }
                                    if !params_match {
                                        continue;
                                    }

                                    let ret_type = match &method.ret_type {
                                        Some(t) => self.ctx.type_from_ast(t, diags),
                                        None => Type::Unit,
                                    };

                                    if ret_type != exp_ret {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Return type mismatch for method '{}': expected {}, found {}",
                                                method.name.name, exp_ret, ret_type
                                            ))
                                            .with_code(codes::E0705_TRAIT_METHOD_SIGNATURE_MISMATCH)
                                            .with_span(method.span),
                                        );
                                        continue;
                                    }
                                }

                                let mut params = Vec::new();
                                for p in &method.params {
                                    let p_ty = self.ctx.type_from_ast(&p.ty, diags);
                                    params.push((p.name.name.clone(), p_ty, p.is_mut));
                                }
                                let ret_type = match &method.ret_type {
                                    Some(t) => self.ctx.type_from_ast(t, diags),
                                    None => Type::Unit,
                                };

                                let mangled_name = format!(
                                    "{}__{}_{}",
                                    target_canonical,
                                    canon_trait.replace("::", "_"),
                                    method.name.name
                                );
                                impl_methods.insert(method.name.name.clone(), mangled_name.clone());

                                self.ctx.trait_methods.insert(
                                    (
                                        target_canonical.clone(),
                                        canon_trait.clone(),
                                        method.name.name.clone(),
                                    ),
                                    mangled_name.clone(),
                                );

                                self.ctx.functions.insert(
                                    mangled_name.clone(),
                                    FnSignature {
                                        name: mangled_name,
                                        params,
                                        ret_type,
                                        span: method.span,
                                    },
                                );
                            }

                            self.ctx.trait_impls.insert(
                                impl_key,
                                TraitImplDef {
                                    trait_canonical: canon_trait,
                                    target_canonical: target_canonical.clone(),
                                    type_args: concrete_args,
                                    defining_module: mod_node.name.clone(),
                                    methods: impl_methods,
                                    span: impl_block.span,
                                },
                            );
                        } else {
                            // --- INHERENT IMPLEMENTATION ---
                            for method in &impl_block.methods {
                                if method.params.is_empty() || method.params[0].name.name != "self"
                                {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Method '{}' must have 'self' as its first parameter",
                                            method.name.name
                                        ))
                                        .with_code(codes::E0109_INVALID_RECEIVER)
                                        .with_span(method.span),
                                    );
                                    continue;
                                }

                                let receiver_ty =
                                    self.ctx.type_from_ast(&method.params[0].ty, diags);
                                let is_valid_receiver = match &receiver_ty {
                                    Type::Ref { inner, .. } => match &**inner {
                                        Type::Struct(ref s) => s == &target_canonical,
                                        Type::Enum(ref e) => e == &target_canonical,
                                        _ => false,
                                    },
                                    Type::Struct(ref s) => s == &target_canonical,
                                    Type::Enum(ref e) => e == &target_canonical,
                                    _ => false,
                                };

                                if !is_valid_receiver {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Invalid receiver type for method '{}' of struct '{}'",
                                            method.name.name, impl_block.target.name
                                        ))
                                        .with_code(codes::E0109_INVALID_RECEIVER)
                                        .with_span(method.params[0].span),
                                    );
                                    continue;
                                }

                                let mut params = Vec::new();
                                for p in &method.params {
                                    let p_ty = self.ctx.type_from_ast(&p.ty, diags);
                                    params.push((p.name.name.clone(), p_ty, p.is_mut));
                                }
                                let ret_type = match &method.ret_type {
                                    Some(t) => self.ctx.type_from_ast(t, diags),
                                    None => Type::Unit,
                                };

                                let mangled_name =
                                    format!("{}_{}", target_canonical, method.name.name);
                                if method.is_pub {
                                    self.ctx.pub_methods.insert(mangled_name.clone());
                                }
                                self.ctx.methods.insert(
                                    (target_canonical.clone(), method.name.name.clone()),
                                    mangled_name.clone(),
                                );
                                self.validate_fn_signature_lifetimes(method, diags);
                                self.ctx.functions.insert(
                                    mangled_name.clone(),
                                    FnSignature {
                                        name: mangled_name,
                                        params,
                                        ret_type,
                                        span: method.span,
                                    },
                                );
                            }
                        }

                        self.ctx.active_lifetime_params = prev_impl_lts;
                        self.ctx.current_impl_target = None;
                    }
                    _ => {}
                }
            }
        }

        // Pass 5: Check function and method bodies
        for mod_node in &module_tree.modules {
            self.ctx.current_module = mod_node.id;
            let scope = self.ctx.module_scopes.get(&mod_node.id).unwrap().clone();

            for item in &mod_node.items {
                match item {
                    Item::Function(f) => {
                        if f.type_params.is_empty() {
                            let canon_name = scope
                                .functions
                                .get(&f.name.name)
                                .cloned()
                                .unwrap_or_else(|| f.name.name.clone());
                            self.check_function(f, &canon_name, diags);
                        }
                    }
                    Item::Impl(impl_block) => {
                        if !impl_block.type_params.is_empty() {
                            continue;
                        }
                        let prev_impl_lts = self.ctx.active_lifetime_params.clone();
                        for lt in &impl_block.lifetime_params {
                            self.ctx.active_lifetime_params.insert(lt.name.clone());
                        }
                        let target_name = &impl_block.target.name;
                        let target_canonical = scope
                            .structs
                            .get(target_name)
                            .cloned()
                            .or_else(|| scope.enums.get(target_name).cloned())
                            .or_else(|| {
                                self.ctx.resolve_struct_name(
                                    target_name,
                                    impl_block.target.span,
                                    diags,
                                )
                            })
                            .or_else(|| {
                                self.ctx.resolve_enum_name(
                                    target_name,
                                    impl_block.target.span,
                                    diags,
                                )
                            })
                            .unwrap_or_else(|| target_name.clone());

                        self.ctx.current_impl_target = Some(target_canonical.clone());

                        if let Some(trait_ident) = &impl_block.trait_name {
                            if let Some(canon_trait) = self.ctx.resolve_trait_name(
                                &trait_ident.name,
                                trait_ident.span,
                                diags,
                            ) {
                                for method in &impl_block.methods {
                                    let mangled_name = format!(
                                        "{}__{}_{}",
                                        target_canonical,
                                        canon_trait.replace("::", "_"),
                                        method.name.name
                                    );
                                    self.check_method(method, &mangled_name, diags);
                                }
                            }
                        } else {
                            for method in &impl_block.methods {
                                let mangled_name =
                                    format!("{}_{}", target_canonical, method.name.name);
                                self.check_method(method, &mangled_name, diags);
                            }
                        }
                        self.ctx.active_lifetime_params = prev_impl_lts;
                        self.ctx.current_impl_target = None;
                    }
                    _ => {}
                }
            }
        }

        self.ctx
    }

    fn check_method(&mut self, m: &FnDecl, mangled_name: &str, diags: &mut DiagnosticBag) {
        if let Some(sig) = self.ctx.functions.get(mangled_name).cloned() {
            let prev_lts = self.ctx.active_lifetime_params.clone();
            for lt in &m.lifetime_params {
                self.ctx.active_lifetime_params.insert(lt.name.clone());
            }
            let prev_ret = self.ctx.current_fn_ret_type.take();
            self.ctx.current_fn_ret_type = Some(sig.ret_type.clone());
            self.ctx.push_scope();

            for (name, ty, is_mut) in &sig.params {
                let sym_id = self.ctx.declare_symbol(
                    name.clone(),
                    ty.clone(),
                    *is_mut,
                    m.span,
                    InitState::Initialized,
                    diags,
                );
                if matches!(ty, Type::Ref { .. }) || ty.is_str_slice() {
                    self.ctx.ref_provenances.insert(sym_id, Provenance::Param);
                } else {
                    self.ctx.local_storage_symbols.insert(sym_id);
                }
            }

            let body_type = self.check_block(&m.body, diags);

            if let Some(expr) = &m.body.expr {
                if matches!(sig.ret_type, Type::Ref { .. }) || sig.ret_type.is_str_slice() {
                    self.check_return_expr_provenance(expr, diags);
                }
            }

            if sig.ret_type != Type::Unit
                && body_type != sig.ret_type
                && !self.block_has_return(&m.body)
            {
                diags.emit(
                    Diagnostic::error(format!(
                        "Method '{}' expects return type {:?}, but body produces {:?}",
                        m.name.name, sig.ret_type, body_type
                    ))
                    .with_code(codes::E0207_RETURN_TYPE_MISMATCH)
                    .with_expected(format!("{}", sig.ret_type))
                    .with_actual(format!("{}", body_type))
                    .with_span(m.body.span),
                );
            }

            self.ctx.pop_scope();
            self.ctx.current_fn_ret_type = prev_ret;
            self.ctx.active_lifetime_params = prev_lts;
        }
    }

    fn check_function(&mut self, f: &FnDecl, canon_name: &str, diags: &mut DiagnosticBag) {
        let sig = match self.ctx.functions.get(canon_name).cloned() {
            Some(s) => s,
            None => return,
        };
        let prev_lts = self.ctx.active_lifetime_params.clone();
        for lt in &f.lifetime_params {
            self.ctx.active_lifetime_params.insert(lt.name.clone());
        }
        let prev_ret = self.ctx.current_fn_ret_type.take();
        self.ctx.current_fn_ret_type = Some(sig.ret_type.clone());
        self.ctx.push_scope();

        for (name, ty, is_mut) in &sig.params {
            let sym_id = self.ctx.declare_symbol(
                name.clone(),
                ty.clone(),
                *is_mut,
                f.span,
                InitState::Initialized,
                diags,
            );
            if matches!(ty, Type::Ref { .. }) || ty.is_str_slice() {
                self.ctx.ref_provenances.insert(sym_id, Provenance::Param);
            } else {
                self.ctx.local_storage_symbols.insert(sym_id);
            }
        }

        let body_type = self.check_block(&f.body, diags);

        if let Some(expr) = &f.body.expr {
            if matches!(sig.ret_type, Type::Ref { .. }) || sig.ret_type.is_str_slice() {
                self.check_return_expr_provenance(expr, diags);
            }
        }

        // Verify return type consistency
        if sig.ret_type != Type::Unit && body_type != sig.ret_type {
            // Check if there was an explicit return statement or if tail expression matches
            if !self.block_has_return(&f.body) {
                diags.emit(
                    Diagnostic::error(format!(
                        "Function '{}' expects return type {:?}, but body produces {:?}",
                        f.name.name, sig.ret_type, body_type
                    ))
                    .with_code(codes::E0207_RETURN_TYPE_MISMATCH)
                    .with_expected(format!("{}", sig.ret_type))
                    .with_actual(format!("{}", body_type))
                    .with_span(f.body.span),
                );
            }
        }

        self.ctx.pop_scope();
        self.ctx.current_fn_ret_type = prev_ret;
        self.ctx.active_lifetime_params = prev_lts;
    }

    fn block_has_return(&self, block: &Block) -> bool {
        if block.expr.is_some() {
            return true;
        }
        for stmt in &block.stmts {
            if matches!(stmt, Stmt::Return { .. }) {
                return true;
            }
            if let Stmt::Expr(expr) = stmt {
                if self.expr_has_return(expr) {
                    return true;
                }
            }
        }
        false
    }

    fn expr_has_return(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let then_ret = self.block_has_return(then_branch);
                let else_ret = match else_branch {
                    Some(ElseBranch::If(else_if)) => self.expr_has_return(else_if),
                    Some(ElseBranch::Block(else_b)) => self.block_has_return(else_b),
                    None => false,
                };
                then_ret || else_ret
            }
            ExprKind::Block(b) => self.block_has_return(b),
            _ => false,
        }
    }

    fn check_block(&mut self, block: &Block, diags: &mut DiagnosticBag) -> Type {
        self.ctx.push_scope();
        for stmt in &block.stmts {
            self.check_stmt(stmt, diags);
        }

        let tail_type = if let Some(expr) = &block.expr {
            let prev_expected = self.ctx.expected_type.clone();
            if self.ctx.expected_type.is_none() {
                self.ctx.expected_type = self.ctx.current_fn_ret_type.clone();
            }
            let ty = self.check_expr(expr, diags);
            self.ctx.expected_type = prev_expected;
            ty
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
                    let prev_expected = self.ctx.expected_type.take();
                    self.ctx.expected_type = declared_ty.clone();
                    let expr_ty = self.check_expr(init_expr, diags);
                    self.ctx.expected_type = prev_expected;
                    if let Some(dec_ty) = &declared_ty {
                        if !self.ctx.can_coerce(&expr_ty, dec_ty) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Type mismatch in let binding: expected {}, found {}",
                                    dec_ty, expr_ty
                                ))
                                .with_code(codes::E0201_TYPE_MISMATCH)
                                .with_expected(format!("{}", dec_ty))
                                .with_actual(format!("{}", expr_ty))
                                .with_span(init_expr.span),
                            );
                        }
                    }
                    let final_ty = declared_ty.unwrap_or(expr_ty);
                    if final_ty.needs_drop(&self.ctx.structs, &self.ctx.enums) {
                        if let ExprKind::Ident(src_name) = &init_expr.kind {
                            self.mark_var_moved(src_name, init_expr.span, diags);
                        }
                    }
                    (final_ty, InitState::Initialized)
                } else {
                    let ty = declared_ty.unwrap_or_else(|| {
                        diags.emit(
                            Diagnostic::error(
                                "Type annotation required when let binding is not initialized",
                            )
                            .with_code(codes::E0201_TYPE_MISMATCH)
                            .with_suggestion(
                                "provide an explicit type annotation, e.g. ': i64'",
                                None,
                                ": i64",
                            )
                            .with_span(*span),
                        );
                        Type::I64
                    });
                    (ty, InitState::Uninitialized)
                };

                let sym_id = self.ctx.declare_symbol(
                    name.name.clone(),
                    inferred_ty,
                    *is_mut,
                    name.span,
                    init_state,
                    diags,
                );
                if let Some(init_expr) = init {
                    if let Some(src_id) = extract_borrow_source(init_expr, &self.ctx) {
                        self.ctx.borrowed_sources.insert(sym_id, src_id);
                    }
                }
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
                                    .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                    .with_suggestion(
                                        "declare variable as mutable with 'let mut'",
                                        Some(sym.span),
                                        "let mut",
                                    )
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
                                            .with_code(codes::E0306_MUTATE_WHILE_BORROWED)
                                            .with_note("active shared borrow exists")
                                            .with_span(*span),
                                        );
                                    }
                                    BorrowState::Exclusive => {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Cannot assign to '{}' because it is mutably borrowed",
                                                name
                                            ))
                                            .with_code(codes::E0306_MUTATE_WHILE_BORROWED)
                                            .with_note("active exclusive borrow exists")
                                            .with_span(*span),
                                        );
                                    }
                                    BorrowState::None => {}
                                }
                            }

                            if !self.ctx.can_coerce(&val_ty, &sym.ty) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Cannot assign value of type {} to variable of type {}",
                                        val_ty, sym.ty
                                    ))
                                    .with_code(codes::E0201_TYPE_MISMATCH)
                                    .with_expected(format!("{}", sym.ty))
                                    .with_actual(format!("{}", val_ty))
                                    .with_span(*span),
                                );
                            }

                            // Mark initialized
                            if let Some(vstate) = self.ctx.var_states.get_mut(&sym_id) {
                                vstate.init = InitState::Initialized;
                            }
                            if val_ty.needs_drop(&self.ctx.structs, &self.ctx.enums) {
                                if let ExprKind::Ident(src_name) = &value.kind {
                                    if src_name != name {
                                        self.mark_var_moved(src_name, value.span, diags);
                                    }
                                }
                            }
                        } else {
                            diags.emit(
                                Diagnostic::error(format!("Undefined variable '{}'", name))
                                    .with_code(codes::E0102_UNDEFINED_SYMBOL)
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
                                if !self.ctx.can_coerce(&val_ty, &ref_target_ty) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Type mismatch in dereference assignment: expected {}, found {}",
                                            ref_target_ty, val_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_expected(format!("{}", ref_target_ty))
                                        .with_actual(format!("{}", val_ty))
                                        .with_span(*span),
                                    );
                                }
                            }
                            _ => {
                                diags.emit(
                                    Diagnostic::error("Cannot assign through immutable reference")
                                        .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                        .with_span(*span),
                                );
                            }
                        }
                    }
                    ExprKind::Index { array, .. } => {
                        let target_ty = self.check_expr(target, diags);
                        self.check_place_mutability(array, span, diags);
                        if !self.ctx.can_coerce(&val_ty, &target_ty) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Type mismatch in assignment: target {}, value {}",
                                    target_ty, val_ty
                                ))
                                .with_code(codes::E0201_TYPE_MISMATCH)
                                .with_expected(format!("{}", target_ty))
                                .with_actual(format!("{}", val_ty))
                                .with_span(*span),
                            );
                        }
                    }
                    ExprKind::FieldAccess {
                        target: base,
                        field: _,
                    } => {
                        let target_ty = self.check_expr(target, diags);
                        self.check_place_mutability(base, span, diags);
                        if !self.ctx.can_coerce(&val_ty, &target_ty) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Type mismatch in field assignment: expected {}, found {}",
                                    target_ty, val_ty
                                ))
                                .with_code(codes::E0201_TYPE_MISMATCH)
                                .with_expected(format!("{}", target_ty))
                                .with_actual(format!("{}", val_ty))
                                .with_span(*span),
                            );
                        }
                    }
                    _ => {
                        let target_ty = self.check_expr(target, diags);
                        if !self.ctx.can_coerce(&val_ty, &target_ty) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Type mismatch in assignment: target {}, value {}",
                                    target_ty, val_ty
                                ))
                                .with_code(codes::E0201_TYPE_MISMATCH)
                                .with_expected(format!("{}", target_ty))
                                .with_actual(format!("{}", val_ty))
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
                        .with_code(codes::E0201_TYPE_MISMATCH)
                        .with_expected("bool")
                        .with_actual(format!("{}", cond_ty))
                        .with_span(cond.span),
                    );
                }
                self.check_block(body, diags);
            }
            Stmt::Loop { body, span: _ } => {
                self.check_block(body, diags);
            }
            Stmt::For {
                var,
                iter,
                body,
                span: _,
            } => {
                let borrowed_src = extract_borrow_source(iter, &self.ctx);
                let iter_ty = self.check_expr(iter, diags);
                let target_name = match &iter_ty {
                    Type::Struct(s) => s.clone(),
                    Type::Enum(e) => e.clone(),
                    _ => {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Type '{}' is not iterable; it does not implement 'Iterator<T>'",
                                iter_ty
                            ))
                            .with_code(codes::E0901_TYPE_NOT_ITERABLE)
                            .with_span(iter.span),
                        );
                        return;
                    }
                };

                let iter_impl =
                    self.ctx
                        .trait_impls
                        .iter()
                        .find(|((trait_canon, target_canon), _)| {
                            (target_canon == &target_name
                                || target_canon.ends_with(&format!("::{}", target_name))
                                || target_name.ends_with(&format!("::{}", target_canon)))
                                && (trait_canon == "Iterator"
                                    || trait_canon.ends_with("::Iterator")
                                    || trait_canon == "std__iter__Iterator")
                        });

                let elem_ty = if let Some((_, impl_def)) = iter_impl {
                    if let Some(t) = impl_def.type_args.first() {
                        t.clone()
                    } else {
                        Type::I64
                    }
                } else {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Type '{}' is not iterable; it does not implement 'Iterator<T>'",
                            iter_ty
                        ))
                        .with_code(codes::E0901_TYPE_NOT_ITERABLE)
                        .with_span(iter.span),
                    );
                    return;
                };

                self.ctx.push_scope();
                let var_sym_id = self.ctx.declare_symbol(
                    var.name.clone(),
                    elem_ty,
                    false,
                    var.span,
                    InitState::Initialized,
                    diags,
                );
                if let Some(src_id) = borrowed_src {
                    self.ctx.borrowed_sources.insert(var_sym_id, src_id);
                }

                for s in &body.stmts {
                    self.check_stmt(s, diags);
                }
                if let Some(expr) = &body.expr {
                    self.check_expr(expr, diags);
                }
                self.ctx.pop_scope();
            }
            Stmt::Return { value, span } => {
                let prev_expected = self.ctx.expected_type.take();
                self.ctx.expected_type = self.ctx.current_fn_ret_type.clone();
                let ret_ty = if let Some(expr) = value {
                    let ty = self.check_expr(expr, diags);
                    if matches!(ty, Type::Ref { .. }) || ty.is_str_slice() {
                        self.check_return_expr_provenance(expr, diags);
                    }
                    ty
                } else {
                    Type::Unit
                };
                self.ctx.expected_type = prev_expected;

                if let Some(expected) = &self.ctx.current_fn_ret_type {
                    if *expected == Type::I64 && ret_ty.is_integer() {
                        // Integer widening to i64
                    } else if *expected != ret_ty {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Return type mismatch: expected {:?}, found {:?}",
                                expected, ret_ty
                            ))
                            .with_code(codes::E0207_RETURN_TYPE_MISMATCH)
                            .with_expected(format!("{}", expected))
                            .with_actual(format!("{}", ret_ty))
                            .with_span(*span),
                        );
                    }
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::Expr(expr) => {
                self.check_expr(expr, diags);
                let active_sources: HashSet<SymbolId> =
                    self.ctx.borrowed_sources.values().copied().collect();
                for (sym_id, vs) in &mut self.ctx.var_states {
                    if !active_sources.contains(sym_id) && vs.borrow != BorrowState::None {
                        vs.borrow = BorrowState::None;
                    }
                }
            }
        }
    }

    fn check_place_mutability(&self, expr: &Expr, span: &Span, diags: &mut DiagnosticBag) {
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some(sym_id) = self.ctx.lookup_symbol(name) {
                    let sym = self.ctx.symbols.get(&sym_id).cloned().unwrap();
                    if let Type::Ref { is_mut, .. } = &sym.ty {
                        if !*is_mut {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Cannot assign through immutable reference '{}'",
                                    name
                                ))
                                .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                .with_span(*span),
                            );
                        }
                    } else if !sym.is_mut {
                        if matches!(&sym.ty, Type::Array { .. }) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Cannot assign to indexed element of immutable variable '{}'",
                                    name
                                ))
                                .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                .with_suggestion(
                                    "declare variable as mutable with 'let mut'",
                                    Some(sym.span),
                                    "let mut",
                                )
                                .with_span(*span),
                            );
                        } else {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Cannot assign to field of immutable variable '{}'",
                                    name
                                ))
                                .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                .with_suggestion(
                                    "declare variable as mutable with 'let mut'",
                                    Some(sym.span),
                                    "let mut",
                                )
                                .with_span(*span),
                            );
                        }
                    }
                    if sym.ty.is_str_slice() || sym.ty == Type::Str {
                        diags.emit(
                            Diagnostic::error("String slices are immutable and cannot be modified through indexing")
                                .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                .with_span(*span),
                        );
                    } else if sym.ty.is_slice() {
                        diags.emit(
                            Diagnostic::error("Dynamic slices are immutable and cannot be modified through indexing")
                                .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                .with_span(*span),
                        );
                    }
                }
            }
            ExprKind::Index { array, .. } => {
                self.check_place_mutability(array, span, diags);
            }
            ExprKind::FieldAccess { target, .. } => {
                self.check_place_mutability(target, span, diags);
            }
            ExprKind::Deref(inner) => {
                if let Some(inner_ty) = self.ctx.expr_types.get(&(inner.as_ref() as *const Expr)) {
                    match inner_ty {
                        Type::Ref { is_mut: true, .. } => {}
                        _ => {
                            diags.emit(
                                Diagnostic::error("Cannot assign through immutable reference")
                                    .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                    .with_span(*span),
                            );
                        }
                    }
                }
            }
            _ => {
                if let Some(ty) = self.ctx.expr_types.get(&(expr as *const Expr)) {
                    if ty.is_str_slice() || *ty == Type::Str {
                        diags.emit(
                            Diagnostic::error("String slices are immutable and cannot be modified through indexing")
                                .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                .with_span(*span),
                        );
                    } else if ty.is_slice() {
                        diags.emit(
                            Diagnostic::error("Dynamic slices are immutable and cannot be modified through indexing")
                                .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                .with_span(*span),
                        );
                    }
                }
            }
        }
    }

    fn check_subpatterns_exhaustive(
        payload_types: &[Type],
        matrix: &[Vec<Pattern>],
        enums: &HashMap<String, EnumDef>,
    ) -> bool {
        if payload_types.is_empty() {
            return true;
        }
        for row in matrix {
            if row
                .iter()
                .all(|p| matches!(p.kind, PatternKind::Wildcard | PatternKind::Ident(_)))
            {
                return true;
            }
        }
        if payload_types.len() == 1 {
            let ty = &payload_types[0];
            match ty {
                Type::Bool => {
                    let mut has_true = false;
                    let mut has_false = false;
                    for row in matrix {
                        if let Some(p) = row.first() {
                            match &p.kind {
                                PatternKind::Bool(true) => has_true = true,
                                PatternKind::Bool(false) => has_false = true,
                                PatternKind::Wildcard | PatternKind::Ident(_) => return true,
                                _ => {}
                            }
                        }
                    }
                    return has_true && has_false;
                }
                Type::Enum(name) => {
                    if let Some(def) = enums.get(name) {
                        let mut covered_variants = HashSet::new();
                        for row in matrix {
                            if let Some(p) = row.first() {
                                match &p.kind {
                                    PatternKind::Wildcard | PatternKind::Ident(_) => return true,
                                    PatternKind::Variant {
                                        variant,
                                        subpatterns,
                                        ..
                                    } => {
                                        if let Some(v) = def.find_variant(&variant.name) {
                                            if v.payload.is_empty()
                                                || subpatterns.iter().all(|sp| {
                                                    matches!(
                                                        sp.kind,
                                                        PatternKind::Wildcard
                                                            | PatternKind::Ident(_)
                                                    )
                                                })
                                            {
                                                covered_variants.insert(variant.name.clone());
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        return covered_variants.len() == def.variants.len();
                    }
                }
                _ => {
                    for row in matrix {
                        if let Some(p) = row.first() {
                            if matches!(p.kind, PatternKind::Wildcard | PatternKind::Ident(_)) {
                                return true;
                            }
                        }
                    }
                    return false;
                }
            }
        }
        false
    }

    fn check_call_internal(
        &mut self,
        callee: &str,
        type_args: &[TypeExpr],
        args: &[Expr],
        span: Span,
        diags: &mut DiagnosticBag,
    ) -> Type {
        if callee == "Vec_new" || callee == "Vec::new" {
            let elem_ty = if !type_args.is_empty() {
                self.ctx.type_from_ast(&type_args[0], diags)
            } else if let Some(exp) = &self.ctx.expected_type {
                exp.vec_elem_type().unwrap_or(Type::I64)
            } else {
                Type::I64
            };
            let mangled_name = format!("Vec__{}", elem_ty.to_mangled_string());
            self.ctx
                .instantiate_generic_struct("Vec", &[elem_ty], span, diags);
            self.ctx.call_callees.insert(span, mangled_name.clone());
            return Type::Struct(mangled_name);
        }

        if callee == "String_new" || callee == "String::new" || callee == "std::string::new" {
            if !args.is_empty() {
                diags.emit(
                    Diagnostic::error("String::new expects 0 arguments")
                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                        .with_span(span),
                );
            }
            self.ctx
                .call_callees
                .insert(span, "String::new".to_string());
            return Type::Struct("String".into());
        }

        if callee == "String_from_str"
            || callee == "String::from_str"
            || callee == "std::string::from_str"
        {
            if args.len() != 1 {
                diags.emit(
                    Diagnostic::error("String::from_str expects 1 argument")
                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                        .with_span(span),
                );
            } else {
                let arg_ty = self.check_expr(&args[0], diags);
                let expected = Type::Ref {
                    is_mut: false,
                    inner: Box::new(Type::Str),
                };
                if !self.ctx.can_coerce(&arg_ty, &expected) {
                    diags.emit(
                        Diagnostic::error(format!(
                            "String::from_str expects argument of type &str, found {}",
                            arg_ty
                        ))
                        .with_code(codes::E0201_TYPE_MISMATCH)
                        .with_expected("&str".to_string())
                        .with_actual(format!("{}", arg_ty))
                        .with_span(args[0].span),
                    );
                }
            }
            self.ctx
                .call_callees
                .insert(span, "String::from_str".to_string());
            return Type::Struct("String".into());
        }

        let resolved_callee = self
            .ctx
            .resolve_fn_name(callee, span, diags)
            .unwrap_or_else(|| callee.to_string());

        if let Some(generic_f) = self.ctx.generic_functions.get(&resolved_callee).cloned() {
            let concrete_types = if !type_args.is_empty() {
                if type_args.len() != generic_f.type_params.len() {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Generic function '{}' expects {} type arguments, but {} were provided",
                            callee,
                            generic_f.type_params.len(),
                            type_args.len()
                        ))
                        .with_code(codes::E0209_GENERIC_ARITY_MISMATCH)
                        .with_span(span),
                    );
                    return Type::Unit;
                }
                let mut cts = Vec::new();
                for t in type_args {
                    cts.push(self.ctx.type_from_ast(t, diags));
                }
                cts
            } else {
                let mut arg_types = Vec::new();
                for arg in args {
                    arg_types.push(self.check_expr(arg, diags));
                }
                let mut inferred = HashMap::new();
                for (idx, p) in generic_f.params.iter().enumerate() {
                    if let Some(a_ty) = arg_types.get(idx) {
                        unify_type_arg(&p.ty, a_ty, &mut inferred, &generic_f.type_params);
                    }
                }
                let mut cts = Vec::new();
                let mut all_inferred = true;
                for tp in &generic_f.type_params {
                    if let Some(t) = inferred.get(&tp.name.name) {
                        cts.push(t.clone());
                    } else {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Cannot infer type parameter '{}' for function '{}'; specify type arguments explicitly",
                                tp.name.name, callee
                            ))
                            .with_code(codes::E0212_CANNOT_INFER_TYPE_PARAM)
                            .with_span(span),
                        );
                        all_inferred = false;
                    }
                }
                if !all_inferred {
                    return Type::Unit;
                }
                cts
            };

            // Check trait bounds on concrete types
            for (tp, ct) in generic_f.type_params.iter().zip(concrete_types.iter()) {
                if let Some(bound) = &tp.bound {
                    if let Some(canon_trait) =
                        self.ctx.resolve_trait_name(&bound.name, bound.span, diags)
                    {
                        let ct_name = match ct {
                            Type::Struct(s) => s.clone(),
                            Type::Enum(e) => e.clone(),
                            Type::Ref { inner, .. } => match &**inner {
                                Type::Struct(s) => s.clone(),
                                Type::Enum(e) => e.clone(),
                                _ => format!("{}", ct),
                            },
                            _ => format!("{}", ct),
                        };
                        let impl_key = (canon_trait.clone(), ct_name.clone());
                        if !self.ctx.trait_impls.contains_key(&impl_key) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Trait bound '{}' is not satisfied for type '{}'",
                                    bound.name, ct_name
                                ))
                                .with_code(codes::E0707_TRAIT_BOUND_NOT_SATISFIED)
                                .with_span(span),
                            );
                            return Type::Unit;
                        }
                    }
                }
            }

            let mangled_callee = format!(
                "{}__{}",
                resolved_callee,
                concrete_types
                    .iter()
                    .map(|t| t.to_mangled_string())
                    .collect::<Vec<_>>()
                    .join("_")
            );

            self.ctx.call_callees.insert(span, mangled_callee.clone());

            if !self.ctx.functions.contains_key(&mangled_callee) {
                if self.ctx.generic_depth >= 16 {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Recursion limit exceeded while instantiating generic function '{}'",
                            callee
                        ))
                        .with_code(codes::E0211_RECURSIVE_GENERIC_INSTANTIATION)
                        .with_span(span),
                    );
                    return Type::Unit;
                }

                let mut substs = HashMap::new();
                for (tp, ct) in generic_f.type_params.iter().zip(concrete_types.iter()) {
                    substs.insert(tp.name.name.clone(), ct.clone());
                }

                let specialized_f = subst_fn_decl(&generic_f, mangled_callee.clone(), &substs);

                let mut sig_params = Vec::new();
                for p in &specialized_f.params {
                    let p_ty = self.ctx.type_from_ast(&p.ty, diags);
                    sig_params.push((p.name.name.clone(), p_ty, p.is_mut));
                }
                let ret_type = match &specialized_f.ret_type {
                    Some(t) => self.ctx.type_from_ast(t, diags),
                    None => Type::Unit,
                };

                self.ctx.functions.insert(
                    mangled_callee.clone(),
                    FnSignature {
                        name: mangled_callee.clone(),
                        params: sig_params,
                        ret_type: ret_type.clone(),
                        span: specialized_f.span,
                    },
                );
                self.ctx.generic_depth += 1;
                self.check_function(&specialized_f, &mangled_callee, diags);
                self.ctx.generic_depth -= 1;
                self.ctx.monomorphized_functions.push(specialized_f);
            }

            let sig = self.ctx.functions.get(&mangled_callee).cloned().unwrap();
            if !type_args.is_empty() {
                for (idx, arg) in args.iter().enumerate() {
                    let arg_ty = self.check_expr(arg, diags);
                    if let Some((_, param_ty, _)) = sig.params.get(idx) {
                        if !matches!(param_ty, Type::Ref { .. })
                            && param_ty.needs_drop(&self.ctx.structs, &self.ctx.enums)
                        {
                            if let ExprKind::Ident(src_name) = &arg.kind {
                                self.mark_var_moved(src_name, arg.span, diags);
                            }
                        }
                        if !self.ctx.can_coerce(&arg_ty, param_ty) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Argument {} to function '{}' expects {}, found {}",
                                    idx + 1,
                                    callee,
                                    param_ty,
                                    arg_ty
                                ))
                                .with_code(codes::E0201_TYPE_MISMATCH)
                                .with_expected(format!("{}", param_ty))
                                .with_actual(format!("{}", arg_ty))
                                .with_span(arg.span),
                            );
                        }
                    }
                }
            } else {
                for (idx, arg) in args.iter().enumerate() {
                    if let Some((_, param_ty, _)) = sig.params.get(idx) {
                        if !matches!(param_ty, Type::Ref { .. })
                            && param_ty.needs_drop(&self.ctx.structs, &self.ctx.enums)
                        {
                            if let ExprKind::Ident(src_name) = &arg.kind {
                                self.mark_var_moved(src_name, arg.span, diags);
                            }
                        }
                    }
                }
            }

            return sig.ret_type;
        }

        if let Some(sig) = self.ctx.functions.get(&resolved_callee).cloned() {
            self.ctx.call_callees.insert(span, resolved_callee.clone());
            if sig.params.len() != args.len() {
                diags.emit(
                    Diagnostic::error(format!(
                        "Function '{}' expects {} arguments, but {} were provided",
                        callee,
                        sig.params.len(),
                        args.len()
                    ))
                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                    .with_span(span),
                );
            }

            for (idx, arg) in args.iter().enumerate() {
                let arg_ty = self.check_expr(arg, diags);
                if let Some((_, param_ty, _)) = sig.params.get(idx) {
                    if !matches!(param_ty, Type::Ref { .. })
                        && param_ty.needs_drop(&self.ctx.structs, &self.ctx.enums)
                    {
                        if let ExprKind::Ident(src_name) = &arg.kind {
                            self.mark_var_moved(src_name, arg.span, diags);
                        }
                    }
                    if !self.ctx.can_coerce(&arg_ty, param_ty) {
                        let mut diag = Diagnostic::error(format!(
                            "Argument {} to function '{}' expects {}, found {}",
                            idx + 1,
                            callee,
                            param_ty,
                            arg_ty
                        ))
                        .with_code(codes::E0201_TYPE_MISMATCH)
                        .with_expected(format!("{}", param_ty))
                        .with_actual(format!("{}", arg_ty))
                        .with_span(arg.span);

                        if callee == "print_i64" && arg_ty.is_str_slice() {
                            diag = diag
                                .with_note(
                                    "function 'print_i64' only accepts 64-bit signed integers",
                                )
                                .with_suggestion(
                                    "call 'print_str' to print string slices",
                                    None,
                                    "print_str",
                                );
                        } else if callee == "print_str" && arg_ty.is_integer() {
                            diag = diag
                                .with_note("function 'print_str' only accepts string slices '&str'")
                                .with_suggestion(
                                    "call 'print_i64' to print integers",
                                    None,
                                    "print_i64",
                                );
                        }

                        diags.emit(diag);
                    }
                }
            }

            sig.ret_type
        } else {
            diags.emit(
                Diagnostic::error(format!("Call to undefined function '{}'", callee))
                    .with_code(codes::E0102_UNDEFINED_SYMBOL)
                    .with_span(span),
            );
            Type::I64
        }
    }

    fn check_pattern(&mut self, pat: &Pattern, expected_ty: &Type, diags: &mut DiagnosticBag) {
        match &pat.kind {
            PatternKind::Wildcard => {}
            PatternKind::Ident(name) => {
                let _sym_id = self.ctx.declare_symbol(
                    name.clone(),
                    expected_ty.clone(),
                    false,
                    pat.span,
                    InitState::Initialized,
                    diags,
                );
            }
            PatternKind::Bool(_) => {
                if !expected_ty.is_bool() {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Mismatched pattern type: expected {}, found bool",
                            expected_ty
                        ))
                        .with_code(codes::E0508_INVALID_PATTERN)
                        .with_expected(expected_ty.to_string())
                        .with_actual("bool".to_string())
                        .with_span(pat.span),
                    );
                }
            }
            PatternKind::Int(_) => {
                if !expected_ty.is_integer() {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Mismatched pattern type: expected {}, found integer",
                            expected_ty
                        ))
                        .with_code(codes::E0508_INVALID_PATTERN)
                        .with_expected(expected_ty.to_string())
                        .with_actual("integer".to_string())
                        .with_span(pat.span),
                    );
                }
            }
            PatternKind::Variant {
                enum_name,
                variant,
                subpatterns,
            } => {
                let actual_enum_name = match expected_ty {
                    Type::Enum(name) => name.clone(),
                    _ => {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Mismatched pattern: expected non-enum type {}, found enum variant pattern '{}'",
                                expected_ty, variant.name
                            ))
                            .with_code(codes::E0508_INVALID_PATTERN)
                            .with_span(pat.span),
                        );
                        return;
                    }
                };

                let enum_def = match self.ctx.enums.get(&actual_enum_name).cloned() {
                    Some(def) => def,
                    None => {
                        diags.emit(
                            Diagnostic::error(format!("Unknown enum '{}'", actual_enum_name))
                                .with_code(codes::E0503_UNKNOWN_ENUM)
                                .with_span(pat.span),
                        );
                        return;
                    }
                };

                if let Some(e_name) = enum_name {
                    let resolved_e = self
                        .ctx
                        .resolve_enum_name(&e_name.name, e_name.span, diags)
                        .unwrap_or_else(|| e_name.name.clone());
                    let base_name = actual_enum_name
                        .split("__")
                        .next()
                        .unwrap_or(&actual_enum_name);
                    if resolved_e != actual_enum_name
                        && e_name.name != actual_enum_name
                        && e_name.name != base_name
                        && !actual_enum_name.ends_with(&format!("__{}", e_name.name))
                        && !actual_enum_name.starts_with(&format!("{resolved_e}__"))
                        && !actual_enum_name.contains(&format!("__{}__", e_name.name))
                        && !actual_enum_name.starts_with(&format!("{}__", e_name.name))
                    {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Variant '{}::{}' does not belong to expected enum '{}'",
                                e_name.name, variant.name, actual_enum_name
                            ))
                            .with_code(codes::E0504_UNKNOWN_VARIANT)
                            .with_span(e_name.span),
                        );
                        return;
                    }
                }

                let v_def = match enum_def.find_variant(&variant.name) {
                    Some(v) => v.clone(),
                    None => {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Unknown variant '{}' for enum '{}'",
                                variant.name, actual_enum_name
                            ))
                            .with_code(codes::E0504_UNKNOWN_VARIANT)
                            .with_span(variant.span),
                        );
                        return;
                    }
                };

                if subpatterns.len() != v_def.payload.len() {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Variant pattern '{}::{}' expects {} arguments, but {} were provided",
                            actual_enum_name,
                            variant.name,
                            v_def.payload.len(),
                            subpatterns.len()
                        ))
                        .with_code(codes::E0506_VARIANT_ARITY_MISMATCH)
                        .with_span(pat.span),
                    );
                    return;
                }

                for (subpat, field_ty) in subpatterns.iter().zip(v_def.payload.iter()) {
                    self.check_pattern(subpat, field_ty, diags);
                }
            }
        }
    }

    fn check_exhaustiveness(
        &mut self,
        scrutinee_ty: &Type,
        arms: &[MatchArm],
        match_span: Span,
        diags: &mut DiagnosticBag,
    ) {
        if arms.is_empty() {
            diags.emit(
                Diagnostic::error(format!(
                    "Non-exhaustive match on '{}': match has no arms",
                    scrutinee_ty
                ))
                .with_code(codes::E0501_NON_EXHAUSTIVE_MATCH)
                .with_span(match_span),
            );
            return;
        }

        match scrutinee_ty {
            Type::Bool => {
                let mut covered_true = false;
                let mut covered_false = false;
                let mut covered_all = false;
                let mut first_redundant: Option<Span> = None;

                for arm in arms {
                    if covered_all || (covered_true && covered_false) {
                        if first_redundant.is_none() {
                            first_redundant = Some(arm.span);
                        }
                        continue;
                    }

                    match &arm.pattern.kind {
                        PatternKind::Wildcard | PatternKind::Ident(_) => {
                            covered_all = true;
                        }
                        PatternKind::Bool(true) => {
                            if covered_true && first_redundant.is_none() {
                                first_redundant = Some(arm.span);
                            }
                            covered_true = true;
                        }
                        PatternKind::Bool(false) => {
                            if covered_false && first_redundant.is_none() {
                                first_redundant = Some(arm.span);
                            }
                            covered_false = true;
                        }
                        _ => {}
                    }
                }

                if let Some(span) = first_redundant {
                    diags.emit(
                        Diagnostic::error("Unreachable match arm")
                            .with_code(codes::E0502_UNREACHABLE_PATTERN)
                            .with_span(span),
                    );
                }

                if !covered_all {
                    if !covered_true && !covered_false {
                        diags.emit(
                            Diagnostic::error(
                                "Non-exhaustive match on 'bool': missing cases 'true' and 'false'",
                            )
                            .with_code(codes::E0501_NON_EXHAUSTIVE_MATCH)
                            .with_suggestion(
                                "add arms for 'true' and 'false' or a wildcard arm",
                                Some(match_span),
                                "true => ..., false => ...",
                            )
                            .with_span(match_span),
                        );
                    } else if !covered_true {
                        diags.emit(
                            Diagnostic::error(
                                "Non-exhaustive match on 'bool': missing case 'true'",
                            )
                            .with_code(codes::E0501_NON_EXHAUSTIVE_MATCH)
                            .with_suggestion(
                                "add a 'true' arm or a wildcard arm",
                                Some(match_span),
                                "true => ...",
                            )
                            .with_span(match_span),
                        );
                    } else if !covered_false {
                        diags.emit(
                            Diagnostic::error(
                                "Non-exhaustive match on 'bool': missing case 'false'",
                            )
                            .with_code(codes::E0501_NON_EXHAUSTIVE_MATCH)
                            .with_suggestion(
                                "add a 'false' arm or a wildcard arm",
                                Some(match_span),
                                "false => ...",
                            )
                            .with_span(match_span),
                        );
                    }
                }
            }
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64 => {
                let mut covered_all = false;
                let mut first_redundant: Option<Span> = None;
                let mut seen_ints = HashSet::new();

                for arm in arms {
                    if covered_all {
                        if first_redundant.is_none() {
                            first_redundant = Some(arm.span);
                        }
                        continue;
                    }
                    match &arm.pattern.kind {
                        PatternKind::Wildcard | PatternKind::Ident(_) => {
                            covered_all = true;
                        }
                        PatternKind::Int(v) => {
                            if !seen_ints.insert(*v) && first_redundant.is_none() {
                                first_redundant = Some(arm.span);
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(span) = first_redundant {
                    diags.emit(
                        Diagnostic::error("Unreachable match arm")
                            .with_code(codes::E0502_UNREACHABLE_PATTERN)
                            .with_span(span),
                    );
                }

                if !covered_all {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Non-exhaustive match on '{}': integer match requires a wildcard arm '_'",
                            scrutinee_ty
                        ))
                        .with_code(codes::E0501_NON_EXHAUSTIVE_MATCH)
                        .with_suggestion("add a wildcard arm '_ => ...'", Some(match_span), "_ => ...")
                        .with_span(match_span),
                    );
                }
            }
            Type::Enum(enum_name) => {
                let enum_def = match self.ctx.enums.get(enum_name).cloned() {
                    Some(def) => def,
                    None => return,
                };

                let mut fully_covered_variants: HashSet<String> = HashSet::new();
                let mut partially_covered_variants: HashMap<String, Vec<Vec<Pattern>>> =
                    HashMap::new();
                let mut covered_all = false;
                let mut first_redundant: Option<Span> = None;

                for arm in arms {
                    if covered_all || fully_covered_variants.len() == enum_def.variants.len() {
                        if first_redundant.is_none() {
                            first_redundant = Some(arm.span);
                        }
                        continue;
                    }

                    match &arm.pattern.kind {
                        PatternKind::Wildcard | PatternKind::Ident(_) => {
                            covered_all = true;
                        }
                        PatternKind::Variant {
                            variant,
                            subpatterns,
                            ..
                        } => {
                            let v_def = match enum_def.find_variant(&variant.name) {
                                Some(v) => v,
                                None => continue,
                            };

                            if fully_covered_variants.contains(&variant.name) {
                                if first_redundant.is_none() {
                                    first_redundant = Some(arm.span);
                                }
                                continue;
                            }

                            let is_wildcard_payload = subpatterns.iter().all(|p| {
                                matches!(p.kind, PatternKind::Wildcard | PatternKind::Ident(_))
                            });
                            if v_def.payload.is_empty() || is_wildcard_payload {
                                fully_covered_variants.insert(variant.name.clone());
                            } else {
                                let entries = partially_covered_variants
                                    .entry(variant.name.clone())
                                    .or_default();
                                entries.push(subpatterns.clone());
                                if Self::check_subpatterns_exhaustive(
                                    &v_def.payload,
                                    entries,
                                    &self.ctx.enums,
                                ) {
                                    fully_covered_variants.insert(variant.name.clone());
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(span) = first_redundant {
                    diags.emit(
                        Diagnostic::error("Unreachable match arm")
                            .with_code(codes::E0502_UNREACHABLE_PATTERN)
                            .with_span(span),
                    );
                }

                if !covered_all && fully_covered_variants.len() < enum_def.variants.len() {
                    let mut missing_cases = Vec::new();
                    for v in &enum_def.variants {
                        if !fully_covered_variants.contains(&v.name) {
                            let base_enum = enum_name.split("__").next().unwrap_or(enum_name);
                            if v.payload.is_empty() {
                                missing_cases.push(format!("{}::{}", base_enum, v.name));
                            } else {
                                let wildcards = vec!["_"; v.payload.len()].join(", ");
                                missing_cases
                                    .push(format!("{}::{}({})", base_enum, v.name, wildcards));
                            }
                        }
                    }

                    let missing_str = missing_cases
                        .iter()
                        .map(|s| format!("`{}`", s))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let first_missing = &missing_cases[0];
                    diags.emit(
                        Diagnostic::error(format!(
                            "Non-exhaustive match on `{}`: missing case {}",
                            enum_name, missing_str
                        ))
                        .with_code(codes::E0501_NON_EXHAUSTIVE_MATCH)
                        .with_suggestion(
                            format!("add a `{}` arm or a wildcard arm", first_missing),
                            Some(match_span),
                            format!("{} => ...", first_missing),
                        )
                        .with_span(match_span),
                    );
                }
            }
            _ => {
                let mut covered = false;
                for arm in arms {
                    if matches!(
                        arm.pattern.kind,
                        PatternKind::Wildcard | PatternKind::Ident(_)
                    ) {
                        covered = true;
                        break;
                    }
                }
                if !covered {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Non-exhaustive match on '{}': wildcard arm required",
                            scrutinee_ty
                        ))
                        .with_code(codes::E0501_NON_EXHAUSTIVE_MATCH)
                        .with_span(match_span),
                    );
                }
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
                                    .with_code(codes::E0301_UNINITIALIZED_READ)
                                    .with_span(expr.span),
                                );
                            }
                            InitState::Moved => {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Use of moved-out variable '{}'",
                                        name
                                    ))
                                    .with_code(codes::E0302_USE_OF_MOVED_VALUE)
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
                            .with_code(codes::E0102_UNDEFINED_SYMBOL)
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
                                .with_code(codes::E0204_INVALID_OPERAND_TYPE)
                                .with_span(expr.span),
                            );
                        }
                        lhs_ty
                    }
                    BinaryOp::Eq | BinaryOp::NotEq => {
                        if !(lhs_ty == rhs_ty || (lhs_ty.is_integer() && rhs_ty.is_integer())) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Equality comparison requires matching types, found {:?} and {:?}",
                                    lhs_ty, rhs_ty
                                ))
                                .with_code(codes::E0201_TYPE_MISMATCH)
                                .with_expected(format!("{}", lhs_ty))
                                .with_actual(format!("{}", rhs_ty))
                                .with_span(expr.span),
                            );
                        }
                        Type::Bool
                    }
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        if !lhs_ty.is_integer() || !rhs_ty.is_integer() {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Relational comparison requires integer operands, found {:?} and {:?}",
                                    lhs_ty, rhs_ty
                                ))
                                .with_code(codes::E0204_INVALID_OPERAND_TYPE)
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
                                .with_code(codes::E0204_INVALID_OPERAND_TYPE)
                                .with_expected("bool")
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
                                .with_code(codes::E0204_INVALID_OPERAND_TYPE)
                                .with_span(expr.span),
                            );
                        }
                        lhs_ty
                    }
                    BinaryOp::Shl | BinaryOp::Shr => {
                        if !lhs_ty.is_integer() || !rhs_ty.is_integer() {
                            diags.emit(
                                Diagnostic::error("Shift operations require integer operands")
                                    .with_code(codes::E0204_INVALID_OPERAND_TYPE)
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
                                    .with_code(codes::E0204_INVALID_OPERAND_TYPE)
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
                                .with_code(codes::E0204_INVALID_OPERAND_TYPE)
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
                                .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                .with_suggestion(
                                    "declare variable as mutable with 'let mut'",
                                    Some(sym.span),
                                    "let mut",
                                )
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
                                            .with_code(codes::E0305_MUT_BORROW_CONFLICT_MUT)
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
                                            .with_code(codes::E0304_MUT_BORROW_CONFLICT_SHARED)
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
                            .with_code(codes::E0204_INVALID_OPERAND_TYPE)
                            .with_span(expr.span),
                        );
                        Type::I64
                    }
                }
            }
            ExprKind::Call {
                callee,
                args,
                type_args,
            } => self.check_call_internal(callee, type_args, args, expr.span, diags),
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
                        .with_code(codes::E0201_TYPE_MISMATCH)
                        .with_expected("bool")
                        .with_actual(format!("{}", cond_ty))
                        .with_span(cond.span),
                    );
                }

                let prev_expected = self.ctx.expected_type.clone();
                let then_ty = self.check_block(then_branch, diags);
                if self.ctx.expected_type.is_none() && then_ty != Type::Unit {
                    self.ctx.expected_type = Some(then_ty.clone());
                }
                let else_ty = match else_branch {
                    Some(ElseBranch::If(else_if)) => self.check_expr(else_if, diags),
                    Some(ElseBranch::Block(else_block)) => self.check_block(else_block, diags),
                    None => Type::Unit,
                };
                self.ctx.expected_type = prev_expected;

                if then_ty != else_ty {
                    diags.emit(
                        Diagnostic::error(format!(
                            "'if' branches have incompatible types: then has {:?}, else has {:?}",
                            then_ty, else_ty
                        ))
                        .with_code(codes::E0201_TYPE_MISMATCH)
                        .with_expected(format!("{}", then_ty))
                        .with_actual(format!("{}", else_ty))
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
                                .with_code(codes::E0201_TYPE_MISMATCH)
                                .with_expected(format!("{}", first_ty))
                                .with_actual(format!("{}", elem_ty))
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
                        .with_code(codes::E0201_TYPE_MISMATCH)
                        .with_expected("integer")
                        .with_actual(format!("{}", idx_ty))
                        .with_span(index.span),
                    );
                }

                if arr_ty.is_vec() {
                    return arr_ty.vec_elem_type().unwrap_or(Type::I64);
                }
                if let Type::Ref { inner, .. } = &arr_ty {
                    if inner.is_vec() {
                        return inner.vec_elem_type().unwrap_or(Type::I64);
                    }
                }

                match &arr_ty {
                    Type::Array { inner, .. } => *inner.clone(),
                    Type::Ref { inner, .. } => match inner.as_ref() {
                        Type::Array { inner: elem_ty, .. } => *elem_ty.clone(),
                        Type::Slice(elem_ty) => *elem_ty.clone(),
                        Type::Str => Type::U8,
                        _ => {
                            diags.emit(
                                Diagnostic::error(
                                    "Indexing requires an array, array reference, or string slice",
                                )
                                .with_code(codes::E0204_INVALID_OPERAND_TYPE)
                                .with_span(array.span),
                            );
                            Type::I64
                        }
                    },
                    Type::Str => Type::U8,
                    _ => {
                        diags.emit(
                            Diagnostic::error("Indexing requires an array type or string slice")
                                .with_code(codes::E0204_INVALID_OPERAND_TYPE)
                                .with_span(array.span),
                        );
                        Type::I64
                    }
                }
            }
            ExprKind::FieldAccess { target, field } => {
                let target_ty = self.check_expr(target, diags);
                let struct_name = match &target_ty {
                    Type::Struct(sname) => Some(sname.clone()),
                    Type::Ref { inner, .. } => {
                        if let Type::Struct(sname) = &**inner {
                            Some(sname.clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                };

                if let Some(sname) = struct_name {
                    if let Some(s_def) = self.ctx.structs.get(&sname) {
                        if let Some(f) = s_def.find_field(&field.name) {
                            if !f.is_pub && s_def.defining_module != self.ctx.current_module_name()
                            {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Field '{}' of struct '{}' is private",
                                        field.name, sname
                                    ))
                                    .with_code(codes::E0606_PRIVATE_FIELD)
                                    .with_span(field.span),
                                );
                            }
                            f.ty.clone()
                        } else {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Struct '{}' has no field named '{}'",
                                    sname, field.name
                                ))
                                .with_code(codes::E0105_UNKNOWN_FIELD)
                                .with_span(field.span),
                            );
                            Type::I64
                        }
                    } else {
                        Type::I64
                    }
                } else {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Field access requires struct type or reference to struct, found {}",
                            target_ty
                        ))
                        .with_code(codes::E0204_INVALID_OPERAND_TYPE)
                        .with_span(target.span),
                    );
                    Type::I64
                }
            }
            ExprKind::StructLiteral {
                name,
                fields,
                type_args,
            } => {
                let resolved_struct = self
                    .ctx
                    .resolve_struct_name(&name.name, name.span, diags)
                    .unwrap_or_else(|| name.name.clone());

                if let Some(generic_s) = self.ctx.generic_structs.get(&resolved_struct).cloned() {
                    let concrete_types = if !type_args.is_empty() {
                        if type_args.len() != generic_s.type_params.len() {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Generic struct '{}' expects {} type arguments, but {} were provided",
                                    name.name,
                                    generic_s.type_params.len(),
                                    type_args.len()
                                ))
                                .with_code(codes::E0209_GENERIC_ARITY_MISMATCH)
                                .with_span(expr.span),
                            );
                            return Type::Unit;
                        }
                        let mut cts = Vec::new();
                        for t in type_args {
                            cts.push(self.ctx.type_from_ast(t, diags));
                        }
                        cts
                    } else {
                        // Infer from field value types
                        let mut field_types = Vec::new();
                        for (f_id, f_val) in fields {
                            field_types.push((f_id.name.clone(), self.check_expr(f_val, diags)));
                        }
                        let mut inferred = HashMap::new();
                        for (f_name, f_ty) in &field_types {
                            if let Some(decl_f) =
                                generic_s.fields.iter().find(|df| df.name.name == *f_name)
                            {
                                unify_type_arg(
                                    &decl_f.ty,
                                    f_ty,
                                    &mut inferred,
                                    &generic_s.type_params,
                                );
                            }
                        }
                        let mut cts = Vec::new();
                        let mut all_inferred = true;
                        for tp in &generic_s.type_params {
                            if let Some(t) = inferred.get(&tp.name.name) {
                                cts.push(t.clone());
                            } else {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Cannot infer type parameter '{}' for struct '{}'",
                                        tp.name.name, name.name
                                    ))
                                    .with_code(codes::E0212_CANNOT_INFER_TYPE_PARAM)
                                    .with_span(expr.span),
                                );
                                all_inferred = false;
                            }
                        }
                        if !all_inferred {
                            return Type::Unit;
                        }
                        cts
                    };

                    let mangled_name = self.ctx.instantiate_generic_struct(
                        &resolved_struct,
                        &concrete_types,
                        expr.span,
                        diags,
                    );
                    if let Some(mangled) = mangled_name {
                        self.ctx
                            .struct_literal_names
                            .insert(expr.span, mangled.clone());
                        let s_def = self.ctx.structs.get(&mangled).cloned().unwrap();

                        if s_def.defining_module != self.ctx.current_module_name() {
                            let has_private = s_def.fields.iter().any(|f| !f.is_pub);
                            if has_private {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Cannot construct struct '{}' with private fields outside its defining module",
                                        name.name
                                    ))
                                    .with_code(codes::E0606_PRIVATE_FIELD)
                                    .with_span(name.span),
                                );
                            }
                        }

                        let mut seen = HashSet::new();
                        for (f_ident, f_expr) in fields {
                            if !seen.insert(f_ident.name.clone()) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Duplicate field '{}' in struct literal '{}'",
                                        f_ident.name, name.name
                                    ))
                                    .with_code(codes::E0106_DUPLICATE_FIELD)
                                    .with_span(f_ident.span),
                                );
                            }
                            let val_ty = if type_args.is_empty() {
                                self.ctx
                                    .expr_types
                                    .get(&(f_expr as *const Expr))
                                    .cloned()
                                    .unwrap_or(Type::Unit)
                            } else {
                                self.check_expr(f_expr, diags)
                            };
                            if let Some(expected_field) = s_def.find_field(&f_ident.name) {
                                if !expected_field.is_pub
                                    && s_def.defining_module != self.ctx.current_module_name()
                                {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Field '{}' of struct '{}' is private",
                                            f_ident.name, name.name
                                        ))
                                        .with_code(codes::E0606_PRIVATE_FIELD)
                                        .with_span(f_ident.span),
                                    );
                                }
                                if expected_field
                                    .ty
                                    .needs_drop(&self.ctx.structs, &self.ctx.enums)
                                {
                                    if let ExprKind::Ident(src_name) = &f_expr.kind {
                                        self.mark_var_moved(src_name, f_expr.span, diags);
                                    }
                                }
                                if !self.ctx.can_coerce(&val_ty, &expected_field.ty) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Field '{}' expects type {}, found {}",
                                            f_ident.name, expected_field.ty, val_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_expected(format!("{}", expected_field.ty))
                                        .with_actual(format!("{}", val_ty))
                                        .with_span(f_expr.span),
                                    );
                                }
                            } else {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "No field named '{}' in struct '{}'",
                                        f_ident.name, name.name
                                    ))
                                    .with_code(codes::E0105_UNKNOWN_FIELD)
                                    .with_span(f_ident.span),
                                );
                            }
                        }

                        for declared in &s_def.fields {
                            if !seen.contains(&declared.name) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Missing field '{}' in initializer for struct '{}'",
                                        declared.name, name.name
                                    ))
                                    .with_code(codes::E0107_MISSING_FIELD)
                                    .with_span(name.span),
                                );
                            }
                        }

                        return Type::Struct(mangled);
                    } else {
                        return Type::Unit;
                    }
                }

                if let Some(s_def) = self.ctx.structs.get(&resolved_struct).cloned() {
                    self.ctx
                        .struct_literal_names
                        .insert(expr.span, resolved_struct.clone());

                    if s_def.defining_module != self.ctx.current_module_name() {
                        let has_private = s_def.fields.iter().any(|f| !f.is_pub);
                        if has_private {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Cannot construct struct '{}' with private fields outside its defining module",
                                    name.name
                                ))
                                .with_code(codes::E0606_PRIVATE_FIELD)
                                .with_span(name.span),
                            );
                        }
                    }

                    let mut seen = HashSet::new();
                    for (f_ident, f_expr) in fields {
                        if !seen.insert(f_ident.name.clone()) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Duplicate field '{}' in struct literal '{}'",
                                    f_ident.name, name.name
                                ))
                                .with_code(codes::E0106_DUPLICATE_FIELD)
                                .with_span(f_ident.span),
                            );
                        }
                        let val_ty = self.check_expr(f_expr, diags);
                        if let Some(expected_field) = s_def.find_field(&f_ident.name) {
                            if !expected_field.is_pub
                                && s_def.defining_module != self.ctx.current_module_name()
                            {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Field '{}' of struct '{}' is private",
                                        f_ident.name, name.name
                                    ))
                                    .with_code(codes::E0606_PRIVATE_FIELD)
                                    .with_span(f_ident.span),
                                );
                            }
                            if expected_field
                                .ty
                                .needs_drop(&self.ctx.structs, &self.ctx.enums)
                            {
                                if let ExprKind::Ident(src_name) = &f_expr.kind {
                                    self.mark_var_moved(src_name, f_expr.span, diags);
                                }
                            }
                            if !self.ctx.can_coerce(&val_ty, &expected_field.ty) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Field '{}' expects type {}, found {}",
                                        f_ident.name, expected_field.ty, val_ty
                                    ))
                                    .with_code(codes::E0201_TYPE_MISMATCH)
                                    .with_expected(format!("{}", expected_field.ty))
                                    .with_actual(format!("{}", val_ty))
                                    .with_span(f_expr.span),
                                );
                            }
                        } else {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "No field named '{}' in struct '{}'",
                                    f_ident.name, name.name
                                ))
                                .with_code(codes::E0105_UNKNOWN_FIELD)
                                .with_span(f_ident.span),
                            );
                        }
                    }

                    // Check for missing fields
                    for declared in &s_def.fields {
                        if !seen.contains(&declared.name) {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Missing field '{}' in initializer for struct '{}'",
                                    declared.name, name.name
                                ))
                                .with_code(codes::E0107_MISSING_FIELD)
                                .with_span(name.span),
                            );
                        }
                    }

                    Type::Struct(resolved_struct)
                } else {
                    diags.emit(
                        Diagnostic::error(format!("Undefined struct '{}'", name.name))
                            .with_code(codes::E0104_UNKNOWN_STRUCT)
                            .with_span(name.span),
                    );
                    Type::I64
                }
            }
            ExprKind::MethodCall {
                target,
                method,
                args,
            } => {
                let target_ty = self.check_expr(target, diags);

                let is_slice_or_array = target_ty.is_str_slice()
                    || target_ty.is_slice()
                    || matches!(&target_ty, Type::Ref { inner, .. } if matches!(inner.as_ref(), Type::Array { .. }));

                if target_ty.is_vec()
                    || matches!(&target_ty, Type::Ref { inner, .. } if inner.is_vec())
                {
                    let elem_ty = target_ty
                        .vec_elem_type()
                        .or_else(|| match &target_ty {
                            Type::Ref { inner, .. } => inner.vec_elem_type(),
                            _ => None,
                        })
                        .unwrap_or(Type::I64);

                    if method.name == "len" {
                        if !args.is_empty() {
                            diags.emit(
                                Diagnostic::error("Method 'len' expects 0 arguments")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        }
                        Type::I64
                    } else if method.name == "capacity" {
                        if !args.is_empty() {
                            diags.emit(
                                Diagnostic::error("Method 'capacity' expects 0 arguments")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        }
                        Type::I64
                    } else if method.name == "push" {
                        self.check_place_mutability(target, &target.span, diags);
                        if let ExprKind::Ident(target_name) = &target.kind {
                            if let Some(sym_id) = self.ctx.lookup_symbol(target_name) {
                                if let Some(vs) = self.ctx.var_states.get(&sym_id) {
                                    if vs.borrow != BorrowState::None {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Cannot mutate vector '{}' while an immutable borrow or iterator is active",
                                                target_name
                                            ))
                                            .with_code(codes::E0306_MUTATE_WHILE_BORROWED)
                                            .with_note("vector elements cannot be added or removed while a reference or iterator borrows the vector's buffer")
                                            .with_span(method.span),
                                        );
                                    }
                                }
                            }
                        }
                        if args.len() != 1 {
                            diags.emit(
                                Diagnostic::error("Method 'push' expects 1 argument")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        } else {
                            let arg_ty = self.check_expr(&args[0], diags);
                            if !self.ctx.can_coerce(&arg_ty, &elem_ty) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Method 'push' expects argument of type {}, found {}",
                                        elem_ty, arg_ty
                                    ))
                                    .with_code(codes::E0201_TYPE_MISMATCH)
                                    .with_expected(format!("{}", elem_ty))
                                    .with_actual(format!("{}", arg_ty))
                                    .with_span(args[0].span),
                                );
                            }
                        }
                        Type::Unit
                    } else if method.name == "pop" {
                        self.check_place_mutability(target, &target.span, diags);
                        if let ExprKind::Ident(target_name) = &target.kind {
                            if let Some(sym_id) = self.ctx.lookup_symbol(target_name) {
                                if let Some(vs) = self.ctx.var_states.get(&sym_id) {
                                    if vs.borrow != BorrowState::None {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Cannot mutate vector '{}' while an immutable borrow or iterator is active",
                                                target_name
                                            ))
                                            .with_code(codes::E0306_MUTATE_WHILE_BORROWED)
                                            .with_note("vector elements cannot be added or removed while a reference or iterator borrows the vector's buffer")
                                            .with_span(method.span),
                                        );
                                    }
                                }
                            }
                        }
                        if !args.is_empty() {
                            diags.emit(
                                Diagnostic::error("Method 'pop' expects 0 arguments")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        }
                        elem_ty
                    } else if method.name == "into_iter" {
                        diags.emit(
                            Diagnostic::error("Consuming vector iteration ('into_iter') is deferred: partial moves of heap-allocated elements require partial destruction tracking")
                                .with_code(codes::E0316_MUTATE_DURING_ITERATION)
                                .with_note("use 'iter()' for immutable borrowed iteration instead")
                                .with_span(method.span),
                        );
                        Type::Unit
                    } else if method.name == "iter_mut" {
                        diags.emit(
                            Diagnostic::error(
                                "Mutable vector iteration ('iter_mut') is deferred in Milestone 13",
                            )
                            .with_code(codes::E0316_MUTATE_DURING_ITERATION)
                            .with_note("use 'iter()' for immutable borrowed iteration instead")
                            .with_span(method.span),
                        );
                        Type::Unit
                    } else if method.name == "iter" {
                        if !args.is_empty() {
                            diags.emit(
                                Diagnostic::error("Method 'iter' expects 0 arguments")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        }
                        if let ExprKind::Ident(target_name) = &target.kind {
                            if let Some(sym_id) = self.ctx.lookup_symbol(target_name) {
                                if let Some(vs) = self.ctx.var_states.get_mut(&sym_id) {
                                    match vs.borrow {
                                        BorrowState::None => vs.borrow = BorrowState::Shared(1),
                                        BorrowState::Shared(n) => {
                                            vs.borrow = BorrowState::Shared(n + 1)
                                        }
                                        BorrowState::Exclusive => {
                                            diags.emit(
                                                Diagnostic::error(format!(
                                                    "Cannot borrow '{}' as immutable because it is already borrowed as mutable",
                                                    target_name
                                                ))
                                                .with_code(codes::E0304_MUT_BORROW_CONFLICT_SHARED)
                                                .with_span(target.span),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        let mangled_iter = format!("Iter__{}", elem_ty.to_mangled_string());
                        if !self.ctx.structs.contains_key(&mangled_iter) {
                            let _ = self.ctx.instantiate_generic_struct(
                                "Iter",
                                std::slice::from_ref(&elem_ty),
                                method.span,
                                diags,
                            );
                        }
                        let ref_elem_ty = Type::Ref {
                            is_mut: false,
                            inner: Box::new(elem_ty.clone()),
                        };
                        let opt_ref_ty = self
                            .ctx
                            .instantiate_generic_enum(
                                "Option",
                                std::slice::from_ref(&ref_elem_ty),
                                method.span,
                                diags,
                            )
                            .map(Type::Enum)
                            .unwrap_or(Type::Unit);

                        let method_mangled = format!("{}_std_iter_Iterator_next", mangled_iter);
                        let mut impl_def = TraitImplDef {
                            trait_canonical: "Iterator".to_string(),
                            target_canonical: mangled_iter.clone(),
                            type_args: vec![ref_elem_ty],
                            defining_module: "std::iter".to_string(),
                            methods: HashMap::new(),
                            span: method.span,
                        };
                        impl_def
                            .methods
                            .insert("next".to_string(), method_mangled.clone());
                        self.ctx.trait_impls.insert(
                            ("Iterator".to_string(), mangled_iter.clone()),
                            impl_def.clone(),
                        );
                        self.ctx.trait_impls.insert(
                            ("std::iter::Iterator".to_string(), mangled_iter.clone()),
                            impl_def.clone(),
                        );
                        self.ctx.trait_impls.insert(
                            ("std__iter__Iterator".to_string(), mangled_iter.clone()),
                            impl_def,
                        );
                        self.ctx.trait_methods.insert(
                            (
                                mangled_iter.clone(),
                                "Iterator".to_string(),
                                "next".to_string(),
                            ),
                            method_mangled.clone(),
                        );
                        self.ctx.trait_methods.insert(
                            (
                                mangled_iter.clone(),
                                "std::iter::Iterator".to_string(),
                                "next".to_string(),
                            ),
                            method_mangled.clone(),
                        );
                        self.ctx.trait_methods.insert(
                            (
                                mangled_iter.clone(),
                                "std__iter__Iterator".to_string(),
                                "next".to_string(),
                            ),
                            method_mangled.clone(),
                        );

                        self.ctx.functions.insert(
                            method_mangled.clone(),
                            FnSignature {
                                name: method_mangled,
                                params: vec![(
                                    "self".to_string(),
                                    Type::Ref {
                                        is_mut: true,
                                        inner: Box::new(Type::Struct(mangled_iter.clone())),
                                    },
                                    false,
                                )],
                                ret_type: opt_ref_ty,
                                span: method.span,
                            },
                        );

                        Type::Struct(mangled_iter)
                    } else {
                        diags.emit(
                            Diagnostic::error(format!("Unknown method '{}' on Vec", method.name))
                                .with_code(codes::E0108_UNKNOWN_METHOD)
                                .with_span(method.span),
                        );
                        Type::Unit
                    }
                } else if target_ty.is_string()
                    || matches!(&target_ty, Type::Ref { inner, .. } if inner.is_string())
                {
                    match method.name.as_str() {
                        "len_bytes" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'len_bytes' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            Type::I64
                        }
                        "capacity" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'capacity' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            Type::I64
                        }
                        "is_empty" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'is_empty' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            Type::Bool
                        }
                        "push_str" => {
                            self.check_place_mutability(target, &target.span, diags);
                            if let ExprKind::Ident(target_name) = &target.kind {
                                if let Some(sym_id) = self.ctx.lookup_symbol(target_name) {
                                    if let Some(vs) = self.ctx.var_states.get(&sym_id) {
                                        if vs.borrow != BorrowState::None {
                                            diags.emit(
                                                Diagnostic::error(format!(
                                                    "Cannot mutate string '{}' while an immutable borrow or slice view is active",
                                                    target_name
                                                ))
                                                .with_code(codes::E0306_MUTATE_WHILE_BORROWED)
                                                .with_note("string buffer cannot be modified while borrowed by a slice")
                                                .with_span(method.span),
                                            );
                                        }
                                    }
                                }
                            }
                            if args.len() != 1 {
                                diags.emit(
                                    Diagnostic::error("Method 'push_str' expects 1 argument")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            } else {
                                let arg_ty = self.check_expr(&args[0], diags);
                                let expected = Type::Ref {
                                    is_mut: false,
                                    inner: Box::new(Type::Str),
                                };
                                if !self.ctx.can_coerce(&arg_ty, &expected) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Method 'push_str' expects argument of type &str, found {}",
                                            arg_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_expected("&str".to_string())
                                        .with_actual(format!("{}", arg_ty))
                                        .with_span(args[0].span),
                                    );
                                }
                            }
                            Type::Unit
                        }
                        "clear" => {
                            self.check_place_mutability(target, &target.span, diags);
                            if let ExprKind::Ident(target_name) = &target.kind {
                                if let Some(sym_id) = self.ctx.lookup_symbol(target_name) {
                                    if let Some(vs) = self.ctx.var_states.get(&sym_id) {
                                        if vs.borrow != BorrowState::None {
                                            diags.emit(
                                                Diagnostic::error(format!(
                                                    "Cannot mutate string '{}' while an immutable borrow or slice view is active",
                                                    target_name
                                                ))
                                                .with_code(codes::E0306_MUTATE_WHILE_BORROWED)
                                                .with_note("string buffer cannot be cleared while borrowed by a slice")
                                                .with_span(method.span),
                                            );
                                        }
                                    }
                                }
                            }
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'clear' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            Type::Unit
                        }
                        "clone" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'clone' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            Type::Struct("String".into())
                        }
                        "equals" => {
                            if args.len() != 1 {
                                diags.emit(
                                    Diagnostic::error("Method 'equals' expects 1 argument")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            } else {
                                let arg_ty = self.check_expr(&args[0], diags);
                                let expected = Type::Ref {
                                    is_mut: false,
                                    inner: Box::new(Type::Str),
                                };
                                if !self.ctx.can_coerce(&arg_ty, &expected) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Method 'equals' expects argument of type &str, found {}",
                                            arg_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_expected("&str".to_string())
                                        .with_actual(format!("{}", arg_ty))
                                        .with_span(args[0].span),
                                    );
                                }
                            }
                            Type::Bool
                        }
                        "as_str" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'as_str' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            if let ExprKind::Ident(target_name) = &target.kind {
                                if let Some(sym_id) = self.ctx.lookup_symbol(target_name) {
                                    if let Some(vs) = self.ctx.var_states.get_mut(&sym_id) {
                                        match vs.borrow {
                                            BorrowState::None => vs.borrow = BorrowState::Shared(1),
                                            BorrowState::Shared(n) => {
                                                vs.borrow = BorrowState::Shared(n + 1)
                                            }
                                            BorrowState::Exclusive => {
                                                diags.emit(
                                                    Diagnostic::error(format!(
                                                        "Cannot borrow '{}' as immutable because it is already borrowed as mutable",
                                                        target_name
                                                    ))
                                                    .with_code(codes::E0304_MUT_BORROW_CONFLICT_SHARED)
                                                    .with_span(target.span),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Type::Ref {
                                is_mut: false,
                                inner: Box::new(Type::Str),
                            }
                        }
                        "as_bytes" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'as_bytes' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            if let ExprKind::Ident(target_name) = &target.kind {
                                if let Some(sym_id) = self.ctx.lookup_symbol(target_name) {
                                    if let Some(vs) = self.ctx.var_states.get_mut(&sym_id) {
                                        match vs.borrow {
                                            BorrowState::None => vs.borrow = BorrowState::Shared(1),
                                            BorrowState::Shared(n) => {
                                                vs.borrow = BorrowState::Shared(n + 1)
                                            }
                                            BorrowState::Exclusive => {
                                                diags.emit(
                                                    Diagnostic::error(format!(
                                                        "Cannot borrow '{}' as immutable because it is already borrowed as mutable",
                                                        target_name
                                                    ))
                                                    .with_code(codes::E0304_MUT_BORROW_CONFLICT_SHARED)
                                                    .with_span(target.span),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Type::Ref {
                                is_mut: false,
                                inner: Box::new(Type::Slice(Box::new(Type::U8))),
                            }
                        }
                        "slice_bytes" => {
                            if args.len() != 2 {
                                diags.emit(
                                    Diagnostic::error("Method 'slice_bytes' expects 2 arguments (start: i64, end: i64)")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            } else {
                                let start_ty = self.check_expr(&args[0], diags);
                                if !self.ctx.can_coerce(&start_ty, &Type::I64) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Argument 'start' to 'slice_bytes' must be i64, found {}",
                                            start_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_span(args[0].span),
                                    );
                                }
                                let end_ty = self.check_expr(&args[1], diags);
                                if !self.ctx.can_coerce(&end_ty, &Type::I64) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Argument 'end' to 'slice_bytes' must be i64, found {}",
                                            end_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_span(args[1].span),
                                    );
                                }
                            }
                            if let ExprKind::Ident(target_name) = &target.kind {
                                if let Some(sym_id) = self.ctx.lookup_symbol(target_name) {
                                    if let Some(vs) = self.ctx.var_states.get_mut(&sym_id) {
                                        match vs.borrow {
                                            BorrowState::None => vs.borrow = BorrowState::Shared(1),
                                            BorrowState::Shared(n) => {
                                                vs.borrow = BorrowState::Shared(n + 1)
                                            }
                                            BorrowState::Exclusive => {
                                                diags.emit(
                                                    Diagnostic::error(format!(
                                                        "Cannot borrow '{}' as immutable because it is already borrowed as mutable",
                                                        target_name
                                                    ))
                                                    .with_code(codes::E0304_MUT_BORROW_CONFLICT_SHARED)
                                                    .with_span(target.span),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            let ref_str_ty = Type::Ref {
                                is_mut: false,
                                inner: Box::new(Type::Str),
                            };
                            let opt_str_ty = self
                                .ctx
                                .instantiate_generic_enum(
                                    "Option",
                                    std::slice::from_ref(&ref_str_ty),
                                    method.span,
                                    diags,
                                )
                                .map(Type::Enum)
                                .unwrap_or_else(|| {
                                    let name = "Option__ref_str".to_string();
                                    if !self.ctx.enums.contains_key(&name) {
                                        let raw_variants = vec![
                                            ("None".to_string(), vec![]),
                                            ("Some".to_string(), vec![ref_str_ty.clone()]),
                                        ];
                                        if let Ok(def) = EnumDef::compute_layout(
                                            name.clone(),
                                            raw_variants,
                                            &self.ctx.structs,
                                            &self.ctx.enums,
                                            method.span,
                                        ) {
                                            self.ctx.enums.insert(name.clone(), def);
                                        }
                                    }
                                    Type::Enum(name)
                                });
                            opt_str_ty
                        }
                        _ => {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Unknown method '{}' on String",
                                    method.name
                                ))
                                .with_code(codes::E0108_UNKNOWN_METHOD)
                                .with_span(method.span),
                            );
                            Type::Unit
                        }
                    }
                } else if target_ty.is_str_slice() {
                    match method.name.as_str() {
                        "len" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'len' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            Type::I64
                        }
                        "is_empty" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'is_empty' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            Type::Bool
                        }
                        "as_bytes" => {
                            if !args.is_empty() {
                                diags.emit(
                                    Diagnostic::error("Method 'as_bytes' expects 0 arguments")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            }
                            Type::Ref {
                                is_mut: false,
                                inner: Box::new(Type::Slice(Box::new(Type::U8))),
                            }
                        }
                        "equals" => {
                            if args.len() != 1 {
                                diags.emit(
                                    Diagnostic::error("Method 'equals' expects 1 argument")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            } else {
                                let arg_ty = self.check_expr(&args[0], diags);
                                let expected = Type::Ref {
                                    is_mut: false,
                                    inner: Box::new(Type::Str),
                                };
                                if !self.ctx.can_coerce(&arg_ty, &expected) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Method 'equals' expects argument of type &str, found {}",
                                            arg_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_expected("&str".to_string())
                                        .with_actual(format!("{}", arg_ty))
                                        .with_span(args[0].span),
                                    );
                                }
                            }
                            Type::Bool
                        }
                        "slice_bytes" => {
                            if args.len() != 2 {
                                diags.emit(
                                    Diagnostic::error("Method 'slice_bytes' expects 2 arguments (start: i64, end: i64)")
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(method.span),
                                );
                            } else {
                                let start_ty = self.check_expr(&args[0], diags);
                                if !self.ctx.can_coerce(&start_ty, &Type::I64) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Argument 'start' to 'slice_bytes' must be i64, found {}",
                                            start_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_span(args[0].span),
                                    );
                                }
                                let end_ty = self.check_expr(&args[1], diags);
                                if !self.ctx.can_coerce(&end_ty, &Type::I64) {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Argument 'end' to 'slice_bytes' must be i64, found {}",
                                            end_ty
                                        ))
                                        .with_code(codes::E0201_TYPE_MISMATCH)
                                        .with_span(args[1].span),
                                    );
                                }
                            }
                            let ref_str_ty = Type::Ref {
                                is_mut: false,
                                inner: Box::new(Type::Str),
                            };
                            let opt_str_ty = self
                                .ctx
                                .instantiate_generic_enum(
                                    "Option",
                                    std::slice::from_ref(&ref_str_ty),
                                    method.span,
                                    diags,
                                )
                                .map(Type::Enum)
                                .unwrap_or_else(|| {
                                    let name = "Option__ref_str".to_string();
                                    if !self.ctx.enums.contains_key(&name) {
                                        let raw_variants = vec![
                                            ("None".to_string(), vec![]),
                                            ("Some".to_string(), vec![ref_str_ty.clone()]),
                                        ];
                                        if let Ok(def) = EnumDef::compute_layout(
                                            name.clone(),
                                            raw_variants,
                                            &self.ctx.structs,
                                            &self.ctx.enums,
                                            method.span,
                                        ) {
                                            self.ctx.enums.insert(name.clone(), def);
                                        }
                                    }
                                    Type::Enum(name)
                                });
                            opt_str_ty
                        }
                        _ => {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Unknown method '{}' on &str",
                                    method.name
                                ))
                                .with_code(codes::E0108_UNKNOWN_METHOD)
                                .with_span(method.span),
                            );
                            Type::Unit
                        }
                    }
                } else if is_slice_or_array && method.name == "len" {
                    if !args.is_empty() {
                        diags.emit(
                            Diagnostic::error("Method 'len' expects 0 arguments")
                                .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                .with_span(method.span),
                        );
                    }
                    Type::I64
                } else if let Some(elem_ty) = self.ctx.is_option_type(&match &target_ty {
                    Type::Ref { inner, .. } => (**inner).clone(),
                    _ => target_ty.clone(),
                }) {
                    if method.name == "is_some" || method.name == "is_none" {
                        if !args.is_empty() {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Method '{}' expects 0 arguments",
                                    method.name
                                ))
                                .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                .with_span(method.span),
                            );
                        }
                        Type::Bool
                    } else if method.name == "unwrap_or" {
                        if args.len() != 1 {
                            diags.emit(
                                Diagnostic::error("Method 'unwrap_or' expects 1 argument")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        } else {
                            let arg_ty = self.check_expr(&args[0], diags);
                            if !self.ctx.can_coerce(&arg_ty, &elem_ty) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Method 'unwrap_or' expects argument of type {}, found {}",
                                        elem_ty, arg_ty
                                    ))
                                    .with_code(codes::E0201_TYPE_MISMATCH)
                                    .with_expected(format!("{}", elem_ty))
                                    .with_actual(format!("{}", arg_ty))
                                    .with_span(args[0].span),
                                );
                            }
                        }
                        elem_ty
                    } else if method.name == "unwrap" {
                        if !args.is_empty() {
                            diags.emit(
                                Diagnostic::error("Method 'unwrap' expects 0 arguments")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        }
                        elem_ty
                    } else {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Unknown method '{}' on Option",
                                method.name
                            ))
                            .with_code(codes::E0108_UNKNOWN_METHOD)
                            .with_span(method.span),
                        );
                        Type::Unit
                    }
                } else if let Some((t_ty, _e_ty)) = self.ctx.is_result_type(&match &target_ty {
                    Type::Ref { inner, .. } => (**inner).clone(),
                    _ => target_ty.clone(),
                }) {
                    if method.name == "is_ok" || method.name == "is_err" {
                        if !args.is_empty() {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Method '{}' expects 0 arguments",
                                    method.name
                                ))
                                .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                .with_span(method.span),
                            );
                        }
                        Type::Bool
                    } else if method.name == "unwrap_or" {
                        if args.len() != 1 {
                            diags.emit(
                                Diagnostic::error("Method 'unwrap_or' expects 1 argument")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        } else {
                            let arg_ty = self.check_expr(&args[0], diags);
                            if !self.ctx.can_coerce(&arg_ty, &t_ty) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Method 'unwrap_or' expects argument of type {}, found {}",
                                        t_ty, arg_ty
                                    ))
                                    .with_code(codes::E0201_TYPE_MISMATCH)
                                    .with_expected(format!("{}", t_ty))
                                    .with_actual(format!("{}", arg_ty))
                                    .with_span(args[0].span),
                                );
                            }
                        }
                        t_ty
                    } else if method.name == "unwrap" {
                        if !args.is_empty() {
                            diags.emit(
                                Diagnostic::error("Method 'unwrap' expects 0 arguments")
                                    .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                    .with_span(method.span),
                            );
                        }
                        t_ty
                    } else {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Unknown method '{}' on Result",
                                method.name
                            ))
                            .with_code(codes::E0108_UNKNOWN_METHOD)
                            .with_span(method.span),
                        );
                        Type::Unit
                    }
                } else {
                    let type_name = match &target_ty {
                        Type::Struct(sname) => Some(sname.clone()),
                        Type::Enum(ename) => Some(ename.clone()),
                        Type::Ref { inner, .. } => match &**inner {
                            Type::Struct(sname) => Some(sname.clone()),
                            Type::Enum(ename) => Some(ename.clone()),
                            _ => None,
                        },
                        _ => None,
                    };

                    if let Some(tname) = type_name {
                        // 1. Inherent method resolution
                        let inherent_mangled = self
                            .ctx
                            .methods
                            .get(&(tname.clone(), method.name.clone()))
                            .cloned();

                        let resolved_mangled = if let Some(mangled) = inherent_mangled {
                            if let Some(s_def) = self.ctx.structs.get(&tname) {
                                if !self.ctx.pub_methods.contains(&mangled)
                                    && s_def.defining_module != self.ctx.current_module_name()
                                {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Method '{}' of struct '{}' is private",
                                            method.name, tname
                                        ))
                                        .with_code(codes::E0604_PRIVATE_ITEM)
                                        .with_span(method.span),
                                    );
                                }
                            }
                            Some(mangled)
                        } else {
                            // 2. Trait method resolution
                            let curr_mod_scope =
                                self.ctx.module_scopes.get(&self.ctx.current_module);
                            let mut candidates = Vec::new();

                            for ((trait_canon, target_canon), impl_def) in &self.ctx.trait_impls {
                                if target_canon == &tname
                                    && impl_def.methods.contains_key(&method.name)
                                {
                                    let is_in_scope = curr_mod_scope.is_some_and(|s| {
                                        s.traits.values().any(|c| c == trait_canon)
                                    });
                                    if is_in_scope {
                                        candidates.push((
                                            trait_canon.clone(),
                                            impl_def.methods[&method.name].clone(),
                                        ));
                                    }
                                }
                            }

                            if candidates.len() > 1 {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Ambiguous call to method '{}' on type '{}'; multiple traits in scope implement this method ({})",
                                        method.name,
                                        tname,
                                        candidates.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(", ")
                                    ))
                                    .with_code(codes::E0706_AMBIGUOUS_TRAIT_METHOD)
                                    .with_span(method.span),
                                );
                                None
                            } else if candidates.len() == 1 {
                                Some(candidates[0].1.clone())
                            } else {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "No method named '{}' found on type '{}'",
                                        method.name, tname
                                    ))
                                    .with_code(codes::E0108_UNKNOWN_METHOD)
                                    .with_span(method.span),
                                );
                                None
                            }
                        };

                        if let Some(mangled) = resolved_mangled {
                            self.ctx.call_callees.insert(method.span, mangled.clone());
                            if let Some(sig) = self.ctx.functions.get(&mangled).cloned() {
                                // Receiver checks
                                if let Some((_, receiver_ty, _)) = sig.params.first() {
                                    match receiver_ty {
                                        Type::Ref { is_mut: true, .. } => {
                                            self.check_place_mutability(
                                                target,
                                                &target.span,
                                                diags,
                                            );
                                            if let Type::Ref { is_mut: false, .. } = target_ty {
                                                diags.emit(
                                                    Diagnostic::error(format!(
                                                        "Cannot call method '{}' requiring '&mut Self' on immutable reference",
                                                        method.name
                                                    ))
                                                    .with_code(codes::E0303_MUTATE_IMMUTABLE)
                                                    .with_span(target.span),
                                                );
                                            }
                                        }
                                        Type::Ref { is_mut: false, .. } => {}
                                        Type::Struct(_) | Type::Enum(_) => {}
                                        _ => {}
                                    }
                                }

                                let expected_arg_count = sig.params.len().saturating_sub(1);
                                if args.len() != expected_arg_count {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Method '{}' expects {} arguments, but {} were provided",
                                            method.name,
                                            expected_arg_count,
                                            args.len()
                                        ))
                                        .with_code(codes::E0205_ARGUMENT_MISMATCH)
                                        .with_span(expr.span),
                                    );
                                }

                                for (idx, arg) in args.iter().enumerate() {
                                    let arg_ty = self.check_expr(arg, diags);
                                    if let Some((_, param_ty, _)) = sig.params.get(idx + 1) {
                                        if !self.ctx.can_coerce(&arg_ty, param_ty) {
                                            diags.emit(
                                                Diagnostic::error(format!(
                                                    "Argument {} to method '{}' expects {}, found {}",
                                                    idx + 1,
                                                    method.name,
                                                    param_ty,
                                                    arg_ty
                                                ))
                                                .with_code(codes::E0201_TYPE_MISMATCH)
                                                .with_expected(format!("{}", param_ty))
                                                .with_actual(format!("{}", arg_ty))
                                                .with_span(arg.span),
                                            );
                                        }
                                    }
                                }

                                sig.ret_type
                            } else {
                                Type::I64
                            }
                        } else {
                            Type::I64
                        }
                    } else {
                        diags.emit(
                            Diagnostic::error(format!(
                                "Method call '{}' on non-aggregate type {}",
                                method.name, target_ty
                            ))
                            .with_code(codes::E0204_INVALID_OPERAND_TYPE)
                            .with_span(target.span),
                        );
                        Type::I64
                    }
                }
            }
            ExprKind::VariantConstructor {
                enum_name,
                variant,
                type_args,
                args,
            } => {
                if let Some(e_id) = enum_name {
                    let resolved_enum = self
                        .ctx
                        .resolve_enum_name(&e_id.name, e_id.span, diags)
                        .unwrap_or_else(|| e_id.name.clone());

                    if self.ctx.enums.contains_key(&resolved_enum)
                        || self.ctx.generic_enums.contains_key(&resolved_enum)
                    {
                        let enum_name_str = &resolved_enum;
                        let (mangled_enum, v_def) = if let Some(generic_e) =
                            self.ctx.generic_enums.get(enum_name_str).cloned()
                        {
                            let concrete_types = if !type_args.is_empty() {
                                if type_args.len() != generic_e.type_params.len() {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Generic enum '{}' expects {} type arguments, but {} were provided",
                                            enum_name_str,
                                            generic_e.type_params.len(),
                                            type_args.len()
                                        ))
                                        .with_code(codes::E0209_GENERIC_ARITY_MISMATCH)
                                        .with_span(expr.span),
                                    );
                                    return Type::Unit;
                                }
                                let mut cts = Vec::new();
                                for t in type_args {
                                    cts.push(self.ctx.type_from_ast(t, diags));
                                }
                                cts
                            } else {
                                let mut arg_types = Vec::new();
                                for arg in args {
                                    arg_types.push(self.check_expr(arg, diags));
                                }
                                let v_decl = generic_e
                                    .variants
                                    .iter()
                                    .find(|v| v.name.name == variant.name);
                                if let Some(v_decl) = v_decl {
                                    let mut inferred = HashMap::new();
                                    for (idx, p) in v_decl.payload.iter().enumerate() {
                                        if let Some(a_ty) = arg_types.get(idx) {
                                            unify_type_arg(
                                                p,
                                                a_ty,
                                                &mut inferred,
                                                &generic_e.type_params,
                                            );
                                        }
                                    }
                                    let candidate_enum = match &self.ctx.expected_type {
                                        Some(Type::Enum(e)) => Some(e.clone()),
                                        _ => match &self.ctx.current_fn_ret_type {
                                            Some(Type::Enum(e)) => Some(e.clone()),
                                            _ => None,
                                        },
                                    };
                                    if let Some(expected_mangled) = candidate_enum {
                                        if let Some((base, expected_args)) = self
                                            .ctx
                                            .generic_enum_type_args
                                            .get(&expected_mangled)
                                            .cloned()
                                        {
                                            if (base == *enum_name_str
                                                || expected_mangled
                                                    .starts_with(&format!("{enum_name_str}__")))
                                                && expected_args.len()
                                                    == generic_e.type_params.len()
                                            {
                                                for (tp, exp_arg) in generic_e
                                                    .type_params
                                                    .iter()
                                                    .zip(expected_args.iter())
                                                {
                                                    inferred
                                                        .entry(tp.name.name.clone())
                                                        .or_insert_with(|| exp_arg.clone());
                                                }
                                            }
                                        }
                                    }

                                    let mut cts = Vec::new();
                                    let mut all_inferred = true;
                                    for tp in &generic_e.type_params {
                                        if let Some(t) = inferred.get(&tp.name.name) {
                                            cts.push(t.clone());
                                        } else {
                                            diags.emit(
                                                Diagnostic::error(format!(
                                                    "Cannot infer type parameter '{}' for enum '{}'; specify type arguments explicitly",
                                                    tp.name.name, enum_name_str
                                                ))
                                                .with_code(codes::E0212_CANNOT_INFER_TYPE_PARAM)
                                                .with_span(expr.span),
                                            );
                                            all_inferred = false;
                                        }
                                    }
                                    if !all_inferred {
                                        return Type::Unit;
                                    }
                                    cts
                                } else {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Unknown variant '{}' for enum '{}'",
                                            variant.name, enum_name_str
                                        ))
                                        .with_code(codes::E0504_UNKNOWN_VARIANT)
                                        .with_span(variant.span),
                                    );
                                    return Type::Unit;
                                }
                            };

                            if let Some(mangled) = self.ctx.instantiate_generic_enum(
                                enum_name_str,
                                &concrete_types,
                                expr.span,
                                diags,
                            ) {
                                let e_def = self.ctx.enums.get(&mangled).unwrap().clone();
                                let v_def = match e_def.find_variant(&variant.name) {
                                    Some(v) => v.clone(),
                                    None => {
                                        diags.emit(
                                            Diagnostic::error(format!(
                                                "Unknown variant '{}' for enum '{}'",
                                                variant.name, mangled
                                            ))
                                            .with_code(codes::E0504_UNKNOWN_VARIANT)
                                            .with_span(variant.span),
                                        );
                                        return Type::Unit;
                                    }
                                };
                                (mangled, v_def)
                            } else {
                                return Type::Unit;
                            }
                        } else {
                            let e_def = self.ctx.enums.get(enum_name_str).unwrap().clone();
                            let v_def = match e_def.find_variant(&variant.name) {
                                Some(v) => v.clone(),
                                None => {
                                    diags.emit(
                                        Diagnostic::error(format!(
                                            "Unknown variant '{}' for enum '{}'",
                                            variant.name, enum_name_str
                                        ))
                                        .with_code(codes::E0504_UNKNOWN_VARIANT)
                                        .with_span(variant.span),
                                    );
                                    return Type::Unit;
                                }
                            };
                            (enum_name_str.clone(), v_def)
                        };

                        self.ctx
                            .variant_constructor_enum_names
                            .insert(expr.span, mangled_enum.clone());

                        if args.len() != v_def.payload.len() {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Variant '{}::{}' expects {} arguments, but {} were provided",
                                    mangled_enum,
                                    variant.name,
                                    v_def.payload.len(),
                                    args.len()
                                ))
                                .with_code(codes::E0506_VARIANT_ARITY_MISMATCH)
                                .with_span(expr.span),
                            );
                            return Type::Unit;
                        }

                        for (idx, (arg, expected_ty)) in
                            args.iter().zip(v_def.payload.iter()).enumerate()
                        {
                            let actual_ty = self.check_expr(arg, diags);
                            if !matches!(expected_ty, Type::Ref { .. })
                                && expected_ty.needs_drop(&self.ctx.structs, &self.ctx.enums)
                            {
                                if let ExprKind::Ident(src_name) = &arg.kind {
                                    self.mark_var_moved(src_name, arg.span, diags);
                                }
                            }
                            if !self.ctx.can_coerce(&actual_ty, expected_ty) {
                                diags.emit(
                                    Diagnostic::error(format!(
                                        "Type mismatch in payload argument {} of '{}::{}': expected {}, found {}",
                                        idx + 1,
                                        mangled_enum,
                                        variant.name,
                                        expected_ty,
                                        actual_ty
                                    ))
                                    .with_code(codes::E0201_TYPE_MISMATCH)
                                    .with_expected(expected_ty.to_string())
                                    .with_actual(actual_ty.to_string())
                                    .with_span(arg.span),
                                );
                            }
                        }

                        Type::Enum(mangled_enum)
                    } else {
                        let mangled_callee = format!("{}_{}", e_id.name, variant.name);
                        self.check_call_internal(&mangled_callee, type_args, args, expr.span, diags)
                    }
                } else {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Unqualified variant construction '{}' is not supported; qualify with enum name (e.g. 'Enum::{}')",
                            variant.name, variant.name
                        ))
                        .with_code(codes::E0504_UNKNOWN_VARIANT)
                        .with_span(variant.span),
                    );
                    Type::Unit
                }
            }
            ExprKind::Match {
                expr: scrutinee,
                arms,
            } => {
                let scrutinee_ty = self.check_expr(scrutinee, diags);

                if scrutinee_ty.needs_drop(&self.ctx.structs, &self.ctx.enums) {
                    if let ExprKind::Ident(src_name) = &scrutinee.kind {
                        self.mark_var_moved(src_name, scrutinee.span, diags);
                    }
                }

                let mut arm_types = Vec::new();
                for arm in arms {
                    self.ctx.push_scope();
                    self.check_pattern(&arm.pattern, &scrutinee_ty, diags);
                    let arm_ty = self.check_expr(&arm.body, diags);
                    self.ctx.pop_scope();
                    arm_types.push(arm_ty);
                }

                self.check_exhaustiveness(&scrutinee_ty, arms, expr.span, diags);

                if arm_types.is_empty() {
                    Type::Unit
                } else {
                    let first_ty = arm_types[0].clone();
                    for (idx, aty) in arm_types.iter().enumerate().skip(1) {
                        if !self.ctx.can_coerce(aty, &first_ty)
                            && !self.ctx.can_coerce(&first_ty, aty)
                        {
                            diags.emit(
                                Diagnostic::error(format!(
                                    "Incompatible types in match arms: arm 1 has type {}, but arm {} has type {}",
                                    first_ty,
                                    idx + 1,
                                    aty
                                ))
                                .with_code(codes::E0201_TYPE_MISMATCH)
                                .with_expected(first_ty.to_string())
                                .with_actual(aty.to_string())
                                .with_span(arms[idx].span),
                            );
                        }
                    }
                    first_ty
                }
            }
            ExprKind::Try(inner) => self.check_try_expr(inner, expr.span, diags),
        };

        self.ctx.expr_types.insert(expr as *const Expr, ty.clone());
        ty
    }

    fn check_try_expr(&mut self, inner: &Expr, span: Span, diags: &mut DiagnosticBag) -> Type {
        let fn_ret_ty = match &self.ctx.current_fn_ret_type {
            Some(t) => t.clone(),
            None => {
                diags.emit(
                    Diagnostic::error("The '?' operator can only be used inside a function")
                        .with_code(codes::E0804_TRY_OUTSIDE_FUNCTION)
                        .with_span(span),
                );
                return Type::Unit;
            }
        };

        let inner_ty = self.check_expr(inner, diags);

        if inner_ty.needs_drop(&self.ctx.structs, &self.ctx.enums) {
            if let ExprKind::Ident(ref src_name) = inner.kind {
                self.mark_var_moved(src_name, inner.span, diags);
            }
        }

        let opt_inner = self.ctx.is_option_type(&inner_ty);
        let res_inner = self.ctx.is_result_type(&inner_ty);

        if let Some(t_val) = opt_inner {
            let fn_opt = self.ctx.is_option_type(&fn_ret_ty);
            let fn_res = self.ctx.is_result_type(&fn_ret_ty);

            if fn_opt.is_some() {
                t_val
            } else if fn_res.is_some() {
                diags.emit(
                    Diagnostic::error(
                        "Cannot propagate an 'Option' using '?' in a function returning 'Result'; convert the Option to a Result explicitly",
                    )
                    .with_code(codes::E0805_MIXED_PROPAGATION_TYPE)
                    .with_span(span),
                );
                t_val
            } else {
                diags.emit(
                    Diagnostic::error(format!(
                        "The '?' operator can only be used in a function that returns 'Option' or 'Result', but current function returns '{}'",
                        fn_ret_ty
                    ))
                    .with_code(codes::E0802_INCOMPATIBLE_PROPAGATION_RETURN)
                    .with_expected("Option<...> or Result<...>".to_string())
                    .with_actual(fn_ret_ty.to_string())
                    .with_span(span),
                );
                t_val
            }
        } else if let Some((t_val, e_val)) = res_inner {
            let fn_opt = self.ctx.is_option_type(&fn_ret_ty);
            let fn_res = self.ctx.is_result_type(&fn_ret_ty);

            if let Some((_, fn_e)) = fn_res {
                if e_val == fn_e {
                    t_val
                } else {
                    diags.emit(
                        Diagnostic::error(format!(
                            "Incompatible error types in '?' propagation: cannot propagate error of type '{}' from function returning error of type '{}'",
                            e_val, fn_e
                        ))
                        .with_code(codes::E0803_INCOMPATIBLE_ERROR_TYPE)
                        .with_expected(fn_e.to_string())
                        .with_actual(e_val.to_string())
                        .with_span(span),
                    );
                    t_val
                }
            } else if fn_opt.is_some() {
                diags.emit(
                    Diagnostic::error(
                        "Cannot propagate a 'Result' using '?' in a function returning 'Option'; convert the Result to an Option explicitly",
                    )
                    .with_code(codes::E0805_MIXED_PROPAGATION_TYPE)
                    .with_span(span),
                );
                t_val
            } else {
                diags.emit(
                    Diagnostic::error(format!(
                        "The '?' operator can only be used in a function that returns 'Result' or 'Option', but current function returns '{}'",
                        fn_ret_ty
                    ))
                    .with_code(codes::E0802_INCOMPATIBLE_PROPAGATION_RETURN)
                    .with_expected("Result<...> or Option<...>".to_string())
                    .with_actual(fn_ret_ty.to_string())
                    .with_span(span),
                );
                t_val
            }
        } else {
            diags.emit(
                Diagnostic::error(format!(
                    "The '?' operator can only be applied to 'Result' or 'Option' types, found '{}'",
                    inner_ty
                ))
                .with_code(codes::E0801_INVALID_TRY_OPERAND)
                .with_span(span),
            );
            Type::Unit
        }
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

    #[test]
    fn test_struct_layout_and_methods() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "struct_test.va".into(),
            r#"
            struct Point {
                x: i64,
                y: i64,
            }

            impl Point {
                fn translate(self: &mut Self, dx: i64, dy: i64) {
                    self.x = self.x + dx;
                    self.y = self.y + dy;
                }

                fn sum(self: &Self) -> i64 {
                    return self.x + self.y;
                }
            }

            fn main() {
                let mut p = Point { x: 10, y: 20 };
                p.translate(5, 7);
                let s = p.sum();
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
        assert!(!diags.has_errors());

        let analyzer = SemanticAnalyzer::new();
        let ctx = analyzer.analyze(&program, &mut diags);
        assert!(!diags.has_errors());

        let point_def = ctx.structs.get("Point").expect("Point struct exists");
        assert_eq!(point_def.total_size, 16);
        assert_eq!(point_def.alignment, 8);
        assert_eq!(point_def.fields[0].offset, 0);
        assert_eq!(point_def.fields[1].offset, 8);
    }

    #[test]
    fn test_recursive_struct_rejected() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "recursive.va".into(),
            r#"
            struct Node {
                val: i64,
                next: Node,
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
        assert!(diags
            .diagnostics()
            .iter()
            .any(|d| d.code == Some(codes::E0208_RECURSIVE_STRUCT)));
    }

    #[test]
    fn test_struct_missing_field_rejected() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "missing.va".into(),
            r#"
            struct Point {
                x: i64,
                y: i64,
            }

            fn main() {
                let p = Point { x: 10 };
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
        assert!(diags
            .diagnostics()
            .iter()
            .any(|d| d.code == Some(codes::E0107_MISSING_FIELD)));
    }

    #[test]
    fn test_dynamic_slice_coercion_and_len() {
        let mut sm = SourceMap::new();
        let id = sm.add_file(
            "slice.va".into(),
            r#"
            fn sum_slice(s: &[i64]) -> i64 {
                let n = s.len();
                let mut sum = 0;
                let mut i = 0;
                while i < n {
                    sum = sum + s[i];
                    i = i + 1;
                }
                return sum;
            }

            fn main() {
                let arr: [i64; 3] = [10, 20, 30];
                let view: &[i64] = &arr;
                let res = sum_slice(view);
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
        assert!(!diags.has_errors());

        let analyzer = SemanticAnalyzer::new();
        let _ctx = analyzer.analyze(&program, &mut diags);
        assert!(!diags.has_errors());
    }

    #[test]
    fn test_enum_layout_and_typecheck_valid() {
        let mut sm = SourceMap::new();
        let src = r#"
        enum Color {
            Red,
            Green,
            Blue,
        }

        enum Message {
            Quit,
            Move(i64, i64),
        }

        fn process(c: Color, m: Message) -> i64 {
            let res = match c {
                Color::Red => 1,
                Color::Green => 2,
                Color::Blue => 3,
            };

            let delta = match m {
                Message::Quit => 0,
                Message::Move(x, y) => x + y,
            };

            return res + delta;
        }
        "#;
        let id = sm.add_file("enum_test.va".into(), src.into());
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);
        assert!(!diags.has_errors());

        let analyzer = SemanticAnalyzer::new();
        let ctx = analyzer.analyze(&program, &mut diags);
        assert!(!diags.has_errors());

        let color_def = ctx.enums.get("Color").unwrap();
        assert_eq!(color_def.variants.len(), 3);
        assert!(color_def.is_copy);

        let msg_def = ctx.enums.get("Message").unwrap();
        assert_eq!(msg_def.variants.len(), 2);
        assert_eq!(msg_def.variants[1].payload.len(), 2);
        assert!(msg_def.is_copy);
    }

    #[test]
    fn test_non_exhaustive_match_rejected() {
        let mut sm = SourceMap::new();
        let src = r#"
        enum Color {
            Red,
            Green,
            Blue,
        }

        fn test(c: Color) -> i64 {
            return match c {
                Color::Red => 1,
                Color::Green => 2,
            };
        }
        "#;
        let id = sm.add_file("non_exh.va".into(), src.into());
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);

        let analyzer = SemanticAnalyzer::new();
        let _ctx = analyzer.analyze(&program, &mut diags);
        assert!(diags.has_errors());
        assert!(diags
            .diagnostics()
            .iter()
            .any(|d| d.code == Some(codes::E0501_NON_EXHAUSTIVE_MATCH)));
    }

    #[test]
    fn test_unreachable_pattern_rejected() {
        let mut sm = SourceMap::new();
        let src = r#"
        enum Color {
            Red,
            Green,
            Blue,
        }

        fn test(c: Color) -> i64 {
            return match c {
                _ => 0,
                Color::Red => 1,
            };
        }
        "#;
        let id = sm.add_file("unreach.va".into(), src.into());
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);

        let analyzer = SemanticAnalyzer::new();
        let _ctx = analyzer.analyze(&program, &mut diags);
        assert!(diags.has_errors());
        assert!(diags
            .diagnostics()
            .iter()
            .any(|d| d.code == Some(codes::E0502_UNREACHABLE_PATTERN)));
    }

    #[test]
    fn test_recursive_enum_rejected() {
        let mut sm = SourceMap::new();
        let src = r#"
        enum Node {
            Leaf(i64),
            Branch(Node),
        }
        "#;
        let id = sm.add_file("rec_enum.va".into(), src.into());
        let file = sm.get_file(id).unwrap();
        let mut diags = DiagnosticBag::new();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut diags);
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut diags);

        let analyzer = SemanticAnalyzer::new();
        let _ctx = analyzer.analyze(&program, &mut diags);
        assert!(diags.has_errors());
        assert!(diags
            .diagnostics()
            .iter()
            .any(|d| d.code == Some(codes::E0507_RECURSIVE_ENUM)));
    }
}
