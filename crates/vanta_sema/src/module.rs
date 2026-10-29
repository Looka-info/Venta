//! Module tree representation, file resolution, and module loading for Vanta.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use vanta_diagnostics::{codes, Diagnostic, DiagnosticBag};
use vanta_lexer::Lexer;
use vanta_parser::ast::*;
use vanta_parser::Parser;
use vanta_source::{FileId, SourceMap};

const EMBEDDED_STD_MOD: &str = include_str!("../../../library/std/mod.va");
const EMBEDDED_STD_OPTION: &str = include_str!("../../../library/std/option.va");
const EMBEDDED_STD_RESULT: &str = include_str!("../../../library/std/result.va");
const EMBEDDED_STD_ITER: &str = include_str!("../../../library/std/iter.va");
const EMBEDDED_STD_STRING: &str = include_str!("../../../library/std/string.va");
const EMBEDDED_STD_IO: &str = include_str!("../../../library/std/io.va");
const EMBEDDED_STD_FS: &str = include_str!("../../../library/std/fs.va");
const EMBEDDED_STD_ENV: &str = include_str!("../../../library/std/env.va");
const EMBEDDED_STD_NET: &str = include_str!("../../../library/std/net.va");
const EMBEDDED_STD_GPU: &str = include_str!("../../../library/std/gpu.va");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ModuleId(pub usize);

#[derive(Debug, Clone)]
pub struct ModuleNode {
    pub id: ModuleId,
    pub name: String,
    pub path: Vec<String>,
    pub file_path: PathBuf,
    pub file_id: FileId,
    pub parent: Option<ModuleId>,
    pub children: HashMap<String, ModuleId>,
    pub is_pub: bool,
    pub items: Vec<Item>,
}

impl ModuleNode {
    pub fn canonical_prefix(&self) -> String {
        if self.path.is_empty() {
            String::new()
        } else {
            format!("{}__", self.path.join("__"))
        }
    }

    pub fn path_str(&self) -> String {
        if self.path.is_empty() {
            self.name.clone()
        } else {
            self.path.join("::")
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleTree {
    pub root: ModuleId,
    pub modules: Vec<ModuleNode>,
    pub path_to_module: HashMap<Vec<String>, ModuleId>,
}

impl ModuleTree {
    pub fn get(&self, id: ModuleId) -> &ModuleNode {
        &self.modules[id.0]
    }

    pub fn get_mut(&mut self, id: ModuleId) -> &mut ModuleNode {
        &mut self.modules[id.0]
    }

    pub fn find_by_path(&self, path: &[String]) -> Option<ModuleId> {
        self.path_to_module.get(path).copied()
    }

    /// Resolve a module path (e.g. ["math"] or ["math", "geometry"]) from a caller module.
    pub fn resolve_module_path(
        &self,
        from: ModuleId,
        path: &[String],
    ) -> Result<ModuleId, (String, Option<&'static str>)> {
        if path.is_empty() {
            return Ok(from);
        }

        let first = &path[0];
        let from_node = self.get(from);

        // 1. Check if first segment is a child of the current module
        let mut current_id = if let Some(&child_id) = from_node.children.get(first) {
            child_id
        } else if let Some(&top_id) = self.modules[self.root.0].children.get(first) {
            // 2. Check if first segment is a child of root module
            top_id
        } else if first == &from_node.name && path.len() == 1 {
            // Self reference
            from
        } else {
            return Err((
                format!("Cannot find module '{}' in module tree", first),
                Some(codes::E0601_MODULE_NOT_FOUND),
            ));
        };

        // 2. Traverse subsequent segments
        for seg in &path[1..] {
            let curr_node = self.get(current_id);
            if let Some(&child_id) = curr_node.children.get(seg) {
                let child_node = self.get(child_id);
                // Check module visibility: child module must be pub if accessed outside parent
                if !child_node.is_pub && from != current_id {
                    return Err((
                        format!("Module '{}' in module '{}' is private", seg, curr_node.name),
                        Some(codes::E0604_PRIVATE_ITEM),
                    ));
                }
                current_id = child_id;
            } else {
                return Err((
                    format!(
                        "Cannot find module '{}' in module '{}'",
                        seg, curr_node.name
                    ),
                    Some(codes::E0601_MODULE_NOT_FOUND),
                ));
            }
        }

        Ok(current_id)
    }
}

pub struct ModuleLoader<'a> {
    source_map: &'a mut SourceMap,
    diagnostics: &'a mut DiagnosticBag,
    modules: Vec<ModuleNode>,
    path_to_module: HashMap<Vec<String>, ModuleId>,
    loading_stack: Vec<PathBuf>,
    seen_canonical_paths: HashMap<PathBuf, ModuleId>,
}

impl<'a> ModuleLoader<'a> {
    pub fn new(source_map: &'a mut SourceMap, diagnostics: &'a mut DiagnosticBag) -> Self {
        Self {
            source_map,
            diagnostics,
            modules: Vec::new(),
            path_to_module: HashMap::new(),
            loading_stack: Vec::new(),
            seen_canonical_paths: HashMap::new(),
        }
    }

    /// Load the entire declared module tree starting from the root source file.
    pub fn load_module_tree(
        root_path: &Path,
        source_map: &'a mut SourceMap,
        diagnostics: &'a mut DiagnosticBag,
    ) -> Result<ModuleTree, String> {
        Self::load_module_tree_with_std(root_path, None, source_map, diagnostics)
    }

    /// Load the declared module tree with a configurable standard library root path.
    pub fn load_module_tree_with_std(
        root_path: &Path,
        std_path: Option<&Path>,
        source_map: &'a mut SourceMap,
        diagnostics: &'a mut DiagnosticBag,
    ) -> Result<ModuleTree, String> {
        let mut loader = ModuleLoader::new(source_map, diagnostics);
        let root_id = loader.load_root(root_path)?;
        loader.load_std(root_id, std_path)?;

        Ok(ModuleTree {
            root: root_id,
            modules: loader.modules,
            path_to_module: loader.path_to_module,
        })
    }

    fn find_std_dir(&self, root_path: &Path, std_path: Option<&Path>) -> Option<PathBuf> {
        if let Some(p) = std_path {
            if p.join("mod.va").is_file() {
                return Some(p.to_path_buf());
            } else if p.is_file() && p.file_name().and_then(|s| s.to_str()) == Some("mod.va") {
                return p.parent().map(|p| p.to_path_buf());
            }
        }

        if let Ok(env_path) = std::env::var("VANTA_STD_PATH") {
            let p = PathBuf::from(env_path);
            if p.join("mod.va").is_file() {
                return Some(p);
            } else if p.is_file() {
                return p.parent().map(|p| p.to_path_buf());
            }
        }

        if let Ok(vanta_home) = std::env::var("VANTA_HOME") {
            let p = PathBuf::from(vanta_home);
            let cand1 = p.join("lib").join("vanta").join("std");
            if cand1.join("mod.va").is_file() {
                return Some(cand1);
            }
            let cand2 = p.join("library").join("std");
            if cand2.join("mod.va").is_file() {
                return Some(cand2);
            }
        }

        let mut curr = root_path.parent();
        while let Some(dir) = curr {
            let candidate = dir.join("library").join("std");
            if candidate.join("mod.va").is_file() {
                return Some(candidate);
            }
            curr = dir.parent();
        }

        let cwd_candidate = Path::new("library").join("std");
        if cwd_candidate.join("mod.va").is_file() {
            return Some(cwd_candidate);
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let cand0 = parent.join("../lib/vanta/std");
                if cand0.join("mod.va").is_file() {
                    return Some(cand0);
                }
                let cand1 = parent.join("library").join("std");
                if cand1.join("mod.va").is_file() {
                    return Some(cand1);
                }
                let cand2 = parent.join("../library/std");
                if cand2.join("mod.va").is_file() {
                    return Some(cand2);
                }
                let cand3 = parent.join("../../library/std");
                if cand3.join("mod.va").is_file() {
                    return Some(cand3);
                }
            }
        }

        let manifest_candidate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library/std");
        if manifest_candidate.join("mod.va").is_file() {
            return Some(manifest_candidate);
        }

        None
    }

    fn load_std(&mut self, root_id: ModuleId, std_path: Option<&Path>) -> Result<(), String> {
        let root_file_path = self.modules[root_id.0].file_path.clone();
        let std_dir_opt = self.find_std_dir(&root_file_path, std_path);

        let (
            std_mod_path,
            mod_src,
            option_src,
            result_src,
            iter_src,
            string_src,
            io_src,
            fs_src,
            env_src,
            net_src,
            gpu_src,
        ) = if let Some(std_dir) = std_dir_opt {
            let mod_file = std_dir.join("mod.va");
            let canon_mod = mod_file.canonicalize().unwrap_or(mod_file);
            let m_src = fs::read_to_string(&canon_mod).unwrap_or_else(|_| EMBEDDED_STD_MOD.into());
            let opt_file = std_dir.join("option.va");
            let o_src =
                fs::read_to_string(&opt_file).unwrap_or_else(|_| EMBEDDED_STD_OPTION.into());
            let res_file = std_dir.join("result.va");
            let r_src =
                fs::read_to_string(&res_file).unwrap_or_else(|_| EMBEDDED_STD_RESULT.into());
            let iter_file = std_dir.join("iter.va");
            let i_src = fs::read_to_string(&iter_file).unwrap_or_else(|_| EMBEDDED_STD_ITER.into());
            let str_file = std_dir.join("string.va");
            let s_src =
                fs::read_to_string(&str_file).unwrap_or_else(|_| EMBEDDED_STD_STRING.into());
            let io_file = std_dir.join("io.va");
            let io_src = fs::read_to_string(&io_file).unwrap_or_else(|_| EMBEDDED_STD_IO.into());
            let fs_file = std_dir.join("fs.va");
            let fs_src = fs::read_to_string(&fs_file).unwrap_or_else(|_| EMBEDDED_STD_FS.into());
            let env_file = std_dir.join("env.va");
            let env_src = fs::read_to_string(&env_file).unwrap_or_else(|_| EMBEDDED_STD_ENV.into());
            let net_file = std_dir.join("net.va");
            let net_src = fs::read_to_string(&net_file).unwrap_or_else(|_| EMBEDDED_STD_NET.into());
            let gpu_file = std_dir.join("gpu.va");
            let gpu_src = fs::read_to_string(&gpu_file).unwrap_or_else(|_| EMBEDDED_STD_GPU.into());
            (
                canon_mod, m_src, o_src, r_src, i_src, s_src, io_src, fs_src, env_src, net_src, gpu_src,
            )
        } else {
            (
                PathBuf::from("<embedded>/std/mod.va"),
                EMBEDDED_STD_MOD.into(),
                EMBEDDED_STD_OPTION.into(),
                EMBEDDED_STD_RESULT.into(),
                EMBEDDED_STD_ITER.into(),
                EMBEDDED_STD_STRING.into(),
                EMBEDDED_STD_IO.into(),
                EMBEDDED_STD_FS.into(),
                EMBEDDED_STD_ENV.into(),
                EMBEDDED_STD_NET.into(),
                EMBEDDED_STD_GPU.into(),
            )
        };

        // 1. Add and parse std/mod.va
        let std_file_id = self
            .source_map
            .add_file(std_mod_path.to_string_lossy().to_string(), mod_src);
        let file = self.source_map.get_file(std_file_id).unwrap();
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(self.diagnostics);
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(self.diagnostics);

        let std_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: std_id,
            name: "std".into(),
            path: vec!["std".into()],
            file_path: std_mod_path.clone(),
            file_id: std_file_id,
            parent: Some(root_id),
            children: HashMap::new(),
            is_pub: true,
            items: program.items,
        });
        self.path_to_module.insert(vec!["std".into()], std_id);
        self.modules[root_id.0]
            .children
            .insert("std".into(), std_id);
        self.seen_canonical_paths
            .insert(std_mod_path.clone(), std_id);

        // 2. Add and parse std/option.va
        let opt_path = std_mod_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("option.va");
        let opt_file_id = self
            .source_map
            .add_file(opt_path.to_string_lossy().to_string(), option_src);
        let opt_file = self.source_map.get_file(opt_file_id).unwrap();
        let mut opt_lexer = Lexer::new(&opt_file);
        let opt_tokens = opt_lexer.tokenize(self.diagnostics);
        let mut opt_parser = Parser::new(opt_tokens);
        let opt_program = opt_parser.parse_program(self.diagnostics);

        let opt_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: opt_id,
            name: "option".into(),
            path: vec!["std".into(), "option".into()],
            file_path: opt_path.clone(),
            file_id: opt_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: opt_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "option".into()], opt_id);
        self.modules[std_id.0]
            .children
            .insert("option".into(), opt_id);
        self.seen_canonical_paths.insert(opt_path, opt_id);

        // 3. Add and parse std/result.va
        let res_path = std_mod_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("result.va");
        let res_file_id = self
            .source_map
            .add_file(res_path.to_string_lossy().to_string(), result_src);
        let res_file = self.source_map.get_file(res_file_id).unwrap();
        let mut res_lexer = Lexer::new(&res_file);
        let res_tokens = res_lexer.tokenize(self.diagnostics);
        let mut res_parser = Parser::new(res_tokens);
        let res_program = res_parser.parse_program(self.diagnostics);

        let res_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: res_id,
            name: "result".into(),
            path: vec!["std".into(), "result".into()],
            file_path: res_path.clone(),
            file_id: res_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: res_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "result".into()], res_id);
        self.modules[std_id.0]
            .children
            .insert("result".into(), res_id);
        self.seen_canonical_paths.insert(res_path, res_id);

        // 4. Add and parse std/iter.va
        let iter_path = std_mod_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("iter.va");
        let iter_file_id = self
            .source_map
            .add_file(iter_path.to_string_lossy().to_string(), iter_src);
        let iter_file = self.source_map.get_file(iter_file_id).unwrap();
        let mut iter_lexer = Lexer::new(&iter_file);
        let iter_tokens = iter_lexer.tokenize(self.diagnostics);
        let mut iter_parser = Parser::new(iter_tokens);
        let iter_program = iter_parser.parse_program(self.diagnostics);

        let iter_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: iter_id,
            name: "iter".into(),
            path: vec!["std".into(), "iter".into()],
            file_path: iter_path.clone(),
            file_id: iter_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: iter_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "iter".into()], iter_id);
        self.modules[std_id.0]
            .children
            .insert("iter".into(), iter_id);
        self.seen_canonical_paths.insert(iter_path, iter_id);

        // 5. Add and parse std/string.va
        let str_path = std_mod_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("string.va");
        let str_file_id = self
            .source_map
            .add_file(str_path.to_string_lossy().to_string(), string_src);
        let str_file = self.source_map.get_file(str_file_id).unwrap();
        let mut str_lexer = Lexer::new(&str_file);
        let str_tokens = str_lexer.tokenize(self.diagnostics);
        let mut str_parser = Parser::new(str_tokens);
        let str_program = str_parser.parse_program(self.diagnostics);

        let str_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: str_id,
            name: "string".into(),
            path: vec!["std".into(), "string".into()],
            file_path: str_path.clone(),
            file_id: str_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: str_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "string".into()], str_id);
        self.modules[std_id.0]
            .children
            .insert("string".into(), str_id);
        self.seen_canonical_paths.insert(str_path, str_id);

        // 6. Add and parse std/io.va
        let io_path = std_mod_path.parent().unwrap_or(Path::new("")).join("io.va");
        let io_file_id = self
            .source_map
            .add_file(io_path.to_string_lossy().to_string(), io_src);
        let io_file = self.source_map.get_file(io_file_id).unwrap();
        let mut io_lexer = Lexer::new(&io_file);
        let io_tokens = io_lexer.tokenize(self.diagnostics);
        let mut io_parser = Parser::new(io_tokens);
        let io_program = io_parser.parse_program(self.diagnostics);

        let io_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: io_id,
            name: "io".into(),
            path: vec!["std".into(), "io".into()],
            file_path: io_path.clone(),
            file_id: io_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: io_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "io".into()], io_id);
        self.modules[std_id.0].children.insert("io".into(), io_id);
        self.seen_canonical_paths.insert(io_path, io_id);

        // 7. Add and parse std/fs.va
        let fs_path = std_mod_path.parent().unwrap_or(Path::new("")).join("fs.va");
        let fs_file_id = self
            .source_map
            .add_file(fs_path.to_string_lossy().to_string(), fs_src);
        let fs_file = self.source_map.get_file(fs_file_id).unwrap();
        let mut fs_lexer = Lexer::new(&fs_file);
        let fs_tokens = fs_lexer.tokenize(self.diagnostics);
        let mut fs_parser = Parser::new(fs_tokens);
        let fs_program = fs_parser.parse_program(self.diagnostics);

        let fs_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: fs_id,
            name: "fs".into(),
            path: vec!["std".into(), "fs".into()],
            file_path: fs_path.clone(),
            file_id: fs_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: fs_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "fs".into()], fs_id);
        self.modules[std_id.0].children.insert("fs".into(), fs_id);
        self.seen_canonical_paths.insert(fs_path, fs_id);

        // 8. Add and parse std/env.va
        let env_path = std_mod_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("env.va");
        let env_file_id = self
            .source_map
            .add_file(env_path.to_string_lossy().to_string(), env_src);
        let env_file = self.source_map.get_file(env_file_id).unwrap();
        let mut env_lexer = Lexer::new(&env_file);
        let env_tokens = env_lexer.tokenize(self.diagnostics);
        let mut env_parser = Parser::new(env_tokens);
        let env_program = env_parser.parse_program(self.diagnostics);

        let env_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: env_id,
            name: "env".into(),
            path: vec!["std".into(), "env".into()],
            file_path: env_path.clone(),
            file_id: env_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: env_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "env".into()], env_id);
        self.modules[std_id.0].children.insert("env".into(), env_id);
        self.seen_canonical_paths.insert(env_path, env_id);

        // 9. Add and parse std/net.va
        let net_path = std_mod_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("net.va");
        let net_file_id = self
            .source_map
            .add_file(net_path.to_string_lossy().to_string(), net_src);
        let net_file = self.source_map.get_file(net_file_id).unwrap();
        let mut net_lexer = Lexer::new(&net_file);
        let net_tokens = net_lexer.tokenize(self.diagnostics);
        let mut net_parser = Parser::new(net_tokens);
        let net_program = net_parser.parse_program(self.diagnostics);

        let net_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: net_id,
            name: "net".into(),
            path: vec!["std".into(), "net".into()],
            file_path: net_path.clone(),
            file_id: net_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: net_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "net".into()], net_id);
        self.modules[std_id.0].children.insert("net".into(), net_id);
        self.seen_canonical_paths.insert(net_path, net_id);

        // 10. Add and parse std/gpu.va
        let gpu_path = std_mod_path
            .parent()
            .unwrap_or(Path::new(""))
            .join("gpu.va");
        let gpu_file_id = self
            .source_map
            .add_file(gpu_path.to_string_lossy().to_string(), gpu_src);
        let gpu_file = self.source_map.get_file(gpu_file_id).unwrap();
        let mut gpu_lexer = Lexer::new(&gpu_file);
        let gpu_tokens = gpu_lexer.tokenize(self.diagnostics);
        let mut gpu_parser = Parser::new(gpu_tokens);
        let gpu_program = gpu_parser.parse_program(self.diagnostics);

        let gpu_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: gpu_id,
            name: "gpu".into(),
            path: vec!["std".into(), "gpu".into()],
            file_path: gpu_path.clone(),
            file_id: gpu_file_id,
            parent: Some(std_id),
            children: HashMap::new(),
            is_pub: true,
            items: gpu_program.items,
        });
        self.path_to_module
            .insert(vec!["std".into(), "gpu".into()], gpu_id);
        self.modules[std_id.0].children.insert("gpu".into(), gpu_id);
        self.seen_canonical_paths.insert(gpu_path, gpu_id);

        Ok(())
    }

    fn load_root(&mut self, root_path: &Path) -> Result<ModuleId, String> {
        let canonical_root = root_path
            .canonicalize()
            .unwrap_or_else(|_| root_path.to_path_buf());

        let src = fs::read_to_string(&canonical_root).map_err(|e| {
            let msg = format!(
                "Failed to read root module file {:?}: {}",
                canonical_root, e
            );
            self.diagnostics
                .emit(Diagnostic::error(&msg).with_code(codes::E0608_UNREADABLE_MODULE));
            msg
        })?;

        let file_id = self
            .source_map
            .add_file(canonical_root.to_string_lossy().to_string(), src);
        let file = self.source_map.get_file(file_id).unwrap();

        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(self.diagnostics);
        if self.diagnostics.has_errors() {
            return Err("Lexing root module failed with errors".into());
        }

        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(self.diagnostics);
        if self.diagnostics.has_errors() {
            return Err("Parsing root module failed with errors".into());
        }

        let root_id = ModuleId(self.modules.len());
        self.modules.push(ModuleNode {
            id: root_id,
            name: "main".into(),
            path: Vec::new(),
            file_path: canonical_root.clone(),
            file_id,
            parent: None,
            children: HashMap::new(),
            is_pub: true,
            items: program.items,
        });
        self.path_to_module.insert(Vec::new(), root_id);
        self.seen_canonical_paths
            .insert(canonical_root.clone(), root_id);

        self.loading_stack.push(canonical_root.clone());

        // Recursively discover child modules declared in root items
        self.discover_children(root_id)?;

        self.loading_stack.pop();
        Ok(root_id)
    }

    fn discover_children(&mut self, parent_id: ModuleId) -> Result<(), String> {
        let items = self.modules[parent_id.0].items.clone();
        let parent_file = self.modules[parent_id.0].file_path.clone();
        let is_root = parent_id == ModuleId(0);

        // Determine base directory for child modules
        let child_base_dir = if is_root {
            parent_file.parent().unwrap_or(Path::new("")).to_path_buf()
        } else {
            let parent_dir = parent_file.parent().unwrap_or(Path::new(""));
            let stem = parent_file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if stem == "mod" {
                parent_dir.to_path_buf()
            } else {
                parent_dir.join(stem)
            }
        };

        let mut seen_in_file: HashSet<String> = HashSet::new();

        for item in &items {
            if let Item::Mod(mod_decl) = item {
                let mod_name = mod_decl.name.name.clone();

                if is_root
                    && mod_name == "std"
                    && self.modules[parent_id.0].children.contains_key("std")
                {
                    continue;
                }

                // 1. Check duplicate declaration in the same module file
                if !seen_in_file.insert(mod_name.clone()) {
                    self.diagnostics.emit(
                        Diagnostic::error(format!(
                            "Duplicate module declaration '{}' in file",
                            mod_name
                        ))
                        .with_code(codes::E0605_DUPLICATE_MODULE)
                        .with_span(mod_decl.name.span),
                    );
                    continue;
                }

                // 2. Candidate file paths: child_base_dir/name.va vs child_base_dir/name/mod.va
                let cand1 = child_base_dir.join(format!("{}.va", mod_name));
                let cand2 = child_base_dir.join(&mod_name).join("mod.va");

                let cand1_exists = cand1.is_file();
                let cand2_exists = cand2.is_file();

                if cand1_exists && cand2_exists {
                    self.diagnostics.emit(
                        Diagnostic::error(format!(
                            "Ambiguous module file for '{}': both '{:?}' and '{:?}' exist",
                            mod_name, cand1, cand2
                        ))
                        .with_code(codes::E0607_AMBIGUOUS_MODULE_FILE)
                        .with_span(mod_decl.name.span),
                    );
                    continue;
                }

                let target_file = if cand1_exists {
                    cand1
                } else if cand2_exists {
                    cand2
                } else {
                    self.diagnostics.emit(
                        Diagnostic::error(format!(
                            "Cannot find module file for '{}'; expected at '{:?}' or '{:?}'",
                            mod_name, cand1, cand2
                        ))
                        .with_code(codes::E0601_MODULE_NOT_FOUND)
                        .with_span(mod_decl.name.span),
                    );
                    continue;
                };

                let canonical_target = target_file
                    .canonicalize()
                    .unwrap_or_else(|_| target_file.clone());

                // 3. Cycle detection
                if self.loading_stack.contains(&canonical_target) {
                    let mut cycle_names = self
                        .loading_stack
                        .iter()
                        .map(|p| {
                            p.file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string()
                        })
                        .collect::<Vec<_>>();
                    cycle_names.push(mod_name.clone());
                    self.diagnostics.emit(
                        Diagnostic::error(format!(
                            "Cyclic module loading detected: {}",
                            cycle_names.join(" -> ")
                        ))
                        .with_code(codes::E0602_MODULE_CYCLE)
                        .with_span(mod_decl.name.span),
                    );
                    continue;
                }

                // 4. Duplicate load under conflicting identities
                if let Some(&existing_id) = self.seen_canonical_paths.get(&canonical_target) {
                    let existing_mod = &self.modules[existing_id.0];
                    self.diagnostics.emit(
                        Diagnostic::error(format!(
                            "Module file '{:?}' is already loaded under module identity '{:?}'",
                            canonical_target, existing_mod.path
                        ))
                        .with_code(codes::E0605_DUPLICATE_MODULE)
                        .with_span(mod_decl.name.span),
                    );
                    continue;
                }

                // 5. Read child file
                let src = match fs::read_to_string(&canonical_target) {
                    Ok(s) => s,
                    Err(e) => {
                        self.diagnostics.emit(
                            Diagnostic::error(format!(
                                "Failed to read module file '{:?}': {}",
                                canonical_target, e
                            ))
                            .with_code(codes::E0608_UNREADABLE_MODULE)
                            .with_span(mod_decl.name.span),
                        );
                        continue;
                    }
                };

                let file_id = self
                    .source_map
                    .add_file(canonical_target.to_string_lossy().to_string(), src);
                let file = self.source_map.get_file(file_id).unwrap();

                let mut lexer = Lexer::new(&file);
                let tokens = lexer.tokenize(self.diagnostics);
                if self.diagnostics.has_errors() {
                    continue;
                }

                let mut parser = Parser::new(tokens);
                let program = parser.parse_program(self.diagnostics);
                if self.diagnostics.has_errors() {
                    continue;
                }

                // Construct child module node
                let mut child_path = self.modules[parent_id.0].path.clone();
                child_path.push(mod_name.clone());

                let child_id = ModuleId(self.modules.len());
                self.modules.push(ModuleNode {
                    id: child_id,
                    name: mod_name.clone(),
                    path: child_path.clone(),
                    file_path: canonical_target.clone(),
                    file_id,
                    parent: Some(parent_id),
                    children: HashMap::new(),
                    is_pub: mod_decl.is_pub,
                    items: program.items,
                });

                self.modules[parent_id.0]
                    .children
                    .insert(mod_name.clone(), child_id);
                self.path_to_module.insert(child_path, child_id);
                self.seen_canonical_paths
                    .insert(canonical_target.clone(), child_id);

                // Recurse into child
                self.loading_stack.push(canonical_target);
                self.discover_children(child_id)?;
                self.loading_stack.pop();
            }
        }

        Ok(())
    }
}
