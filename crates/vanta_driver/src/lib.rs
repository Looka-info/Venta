//! Vanta compiler pipeline driver and linker orchestrator.

pub mod linker_resolver;
pub use linker_resolver::*;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
pub use vanta_build::{
    ArtifactType, BuildConfiguration, BuildManifest, BuildNode, BuildStats, CacheCleanReport,
    CacheEntryMetadata, CacheManager, CacheStats, CacheVerificationReport, CompilerIdentity,
    ContentFingerprint, DependencyKind, Domain, FingerprintBuilder, FunctionFragment,
    IncrementalBuildEngine, IncrementalOptions, LinkConfiguration, LinkInputKind, ManifestFunction,
    ManifestModule, ManifestSourceFile, ManifestType, NodeId, NodeKind, ResolvedInvocation,
    ResolvedLibrary, ResolvedLinkInput, TargetFeaturesConfig,
};
use vanta_codegen_aarch64::AArch64Backend;
pub use vanta_codegen_x86_64::TargetFeatures;
use vanta_codegen_x86_64::X86_64Backend;
use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
pub use vanta_mir::opt::OptLevel;
use vanta_mir::{MirLowerer, MirVerifier, OptimizationPipeline};
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;
pub use vanta_target::{Arch, ObjectFormat, Os, TargetAbi, TargetSpec, TargetTriple};

const RUNTIME_C: &str = include_str!("../../../runtime/vanta_rt.c");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkerKind {
    #[default]
    Internal,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiagnosticFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitTarget {
    Tokens,
    Ast,
    Mir,
    Obj,
    Exe,
}

#[derive(Debug, Clone)]
pub struct CompilerOptions {
    pub input_file: PathBuf,
    pub output_file: PathBuf,
    pub emit_target: EmitTarget,
    pub target: TargetTriple,
    pub opt_level: OptLevel,
    pub verbose: bool,
    pub diagnostic_format: DiagnosticFormat,
    pub std_path: Option<PathBuf>,
    pub runtime_path: Option<PathBuf>,
    pub enable_reg_alloc: bool,
    pub enable_mem2reg: bool,
    pub enable_gvn: bool,
    pub enable_inline: bool,
    pub enable_ipa_constprop: bool,
    pub enable_licm: bool,
    pub enable_indvar: bool,
    pub enable_strength_reduction: bool,
    pub enable_sroa: bool,
    pub enable_loop_peel: bool,
    pub enable_loop_unroll: bool,
    pub enable_vectorize: bool,
    pub enable_instruction_schedule: bool,
    pub enable_global_dce: bool,
    pub enable_tail_calls: bool,
    pub enable_block_layout: bool,
    pub enable_code_align: bool,
    pub target_features: TargetFeatures,
    pub debug_info: bool,
    pub linker: LinkerKind,
    pub linker_flags: Vec<String>,
    pub link_archives: Vec<PathBuf>,
    pub additional_inputs: Vec<PathBuf>,
    pub map_file: Option<PathBuf>,
    pub lib_search_paths: Vec<PathBuf>,
    pub libs: Vec<String>,
    pub incremental: bool,
    pub cache_dir: Option<PathBuf>,
    pub build_stats: bool,
    pub explain_rebuild: bool,
}

impl CompilerOptions {
    pub fn new(input_file: PathBuf, output_file: PathBuf) -> Self {
        Self {
            input_file,
            output_file,
            emit_target: EmitTarget::Exe,
            target: TargetTriple::default(),
            opt_level: OptLevel::O1,
            verbose: false,
            diagnostic_format: DiagnosticFormat::Human,
            std_path: None,
            runtime_path: None,
            enable_reg_alloc: true,
            enable_mem2reg: true,
            enable_gvn: true,
            enable_inline: true,
            enable_ipa_constprop: true,
            enable_licm: true,
            enable_indvar: true,
            enable_strength_reduction: true,
            enable_sroa: true,
            enable_loop_peel: true,
            enable_loop_unroll: true,
            enable_vectorize: true,
            enable_instruction_schedule: true,
            enable_global_dce: true,
            enable_tail_calls: true,
            enable_block_layout: true,
            enable_code_align: true,
            target_features: TargetFeatures::baseline(),
            debug_info: false,
            linker: LinkerKind::System,
            linker_flags: Vec::new(),
            link_archives: Vec::new(),
            additional_inputs: Vec::new(),
            map_file: None,
            lib_search_paths: Vec::new(),
            libs: Vec::new(),
            incremental: false,
            cache_dir: None,
            build_stats: false,
            explain_rebuild: false,
        }
    }
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self::new(PathBuf::new(), PathBuf::new())
    }
}

pub struct CompilerSession {
    pub source_map: SourceMap,
    pub diagnostics: DiagnosticBag,
    pub last_build_stats: Option<BuildStats>,
    pub rebuild_explanations: Vec<(String, String)>,
}

impl Default for CompilerSession {
    fn default() -> Self {
        Self::new()
    }
}

impl CompilerSession {
    pub fn new() -> Self {
        Self {
            source_map: SourceMap::new(),
            diagnostics: DiagnosticBag::new(),
            last_build_stats: None,
            rebuild_explanations: Vec::new(),
        }
    }

    /// Render collected diagnostics according to the specified format.
    pub fn render_diagnostics(&self, format: DiagnosticFormat) -> String {
        match format {
            DiagnosticFormat::Human => self.diagnostics.render_all(&self.source_map, true),
            DiagnosticFormat::Json => self.diagnostics.to_json(&self.source_map),
        }
    }

    /// Run the compiler pipeline up to MIR optimization and verification without native codegen or linking.
    pub fn check(&mut self, options: &CompilerOptions) -> Result<(), String> {
        let mut module_tree = vanta_sema::module::ModuleLoader::load_module_tree_with_std(
            &options.input_file,
            options.std_path.as_deref(),
            &mut self.source_map,
            &mut self.diagnostics,
        )?;

        if self.diagnostics.has_errors() {
            return Err("Module loading failed with errors".into());
        }

        // Semantic Analysis (Type checking & Borrow checking)
        let sema = SemanticAnalyzer::new();
        let sema_ctx = sema.analyze_modules(&mut module_tree, &mut self.diagnostics);
        if self.diagnostics.has_errors() {
            return Err("Semantic analysis failed with errors".into());
        }

        // MIR Lowering & Verification
        let mut lowerer = MirLowerer::new(&sema_ctx);
        let mut mir = lowerer.lower_module_tree(&module_tree);
        MirVerifier::verify_program(&mir, &mut self.diagnostics);
        if self.diagnostics.has_errors() {
            return Err("MIR verification failed with errors".into());
        }

        // MIR Optimization Pipeline Verification
        let is_executable = options.emit_target == EmitTarget::Exe;
        let target_config = vanta_mir::opt::VectorizeTargetConfig {
            has_avx: options.target_features.avx,
            has_avx2: options.target_features.avx2,
            has_fma: options.target_features.fma,
            has_avx512f: options.target_features.avx512f,
        };
        OptimizationPipeline::optimize_program_full_with_target(
            &mut mir,
            options.opt_level,
            options.enable_mem2reg,
            options.enable_gvn,
            options.enable_inline,
            options.enable_ipa_constprop,
            options.enable_licm,
            options.enable_indvar,
            options.enable_strength_reduction,
            options.enable_sroa,
            options.enable_loop_peel,
            options.enable_loop_unroll,
            options.enable_vectorize,
            target_config,
            options.enable_instruction_schedule,
            options.enable_global_dce,
            options.enable_tail_calls,
            is_executable,
            &mut self.diagnostics,
        );
        if self.diagnostics.has_errors() {
            return Err("Optimized MIR verification failed with errors".into());
        }

        Ok(())
    }

    fn build_configuration(options: &CompilerOptions) -> BuildConfiguration {
        let target_spec = options.target.spec();
        BuildConfiguration {
            target_triple: target_spec.triple.to_string(),
            arch: format!("{:?}", target_spec.arch),
            os: format!("{:?}", target_spec.os),
            abi: format!("{:?}", target_spec.abi),
            object_format: format!("{:?}", target_spec.object_format),
            opt_level: format!("{:?}", options.opt_level),
            target_features: TargetFeaturesConfig {
                avx: options.target_features.avx,
                avx2: options.target_features.avx2,
                fma: options.target_features.fma,
                avx512f: options.target_features.avx512f,
                bmi1: false,
                bmi2: false,
            },
            enable_reg_alloc: options.enable_reg_alloc,
            enable_tail_calls: options.enable_tail_calls,
            enable_block_layout: options.enable_block_layout,
            enable_code_align: options.enable_code_align,
            enable_mem2reg: options.enable_mem2reg,
            enable_gvn: options.enable_gvn,
            enable_inline: options.enable_inline,
            enable_ipa_constprop: options.enable_ipa_constprop,
            enable_licm: options.enable_licm,
            enable_indvar: options.enable_indvar,
            enable_strength_reduction: options.enable_strength_reduction,
            enable_sroa: options.enable_sroa,
            enable_loop_peel: options.enable_loop_peel,
            enable_loop_unroll: options.enable_loop_unroll,
            enable_vectorize: options.enable_vectorize,
            enable_instruction_schedule: options.enable_instruction_schedule,
            enable_global_dce: options.enable_global_dce,
            debug_info: options.debug_info,
            std_fingerprint: None,
            runtime_fingerprint: None,
        }
    }

    /// Full compilation pipeline from source to requested emit target.
    pub fn compile(&mut self, options: &CompilerOptions) -> Result<Option<Vec<u8>>, String> {
        if options.emit_target == EmitTarget::Tokens {
            let src = fs::read_to_string(&options.input_file).map_err(|e| {
                format!("Failed to read input file {:?}: {}", options.input_file, e)
            })?;
            let file_id = self
                .source_map
                .add_file(options.input_file.to_string_lossy().to_string(), src);
            let file = self.source_map.get_file(file_id).unwrap();
            let mut lexer = Lexer::new(&file);
            let tokens = lexer.tokenize(&mut self.diagnostics);
            if self.diagnostics.has_errors() {
                return Err("Lexing failed with errors".into());
            }
            let output = format!("{:#?}", tokens);
            return Ok(Some(output.into_bytes()));
        }

        let is_codegen_or_link = matches!(options.emit_target, EmitTarget::Obj | EmitTarget::Exe);

        // Incremental fast-path: check if we can reuse cached object or entire executable
        let mut incremental_engine = if options.incremental && is_codegen_or_link {
            let build_config = Self::build_configuration(options);
            let inc_opts = IncrementalOptions {
                enabled: true,
                cache_dir: options
                    .cache_dir
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(CacheManager::DEFAULT_CACHE_REL_PATH)),
                target_triple: options.target.spec().triple.to_string(),
                opt_level: format!("{:?}", options.opt_level),
                target_features: format!("{:?}", options.target_features),
                debug_info: options.debug_info,
                explain_rebuild: options.explain_rebuild,
                build_stats: options.build_stats,
                build_config: Some(build_config.clone()),
            };
            let mut engine = IncrementalBuildEngine::new(inc_opts)?;

            let prev_manifest_opt = engine.load_manifest();

            // Check if we can take the complete no-op or link-only fast-path using the persisted manifest
            if let Some(prev_manifest) = &prev_manifest_opt {
                let current_compiler_id = engine.cache().compiler_identity().fingerprint().to_hex();
                let current_config_fp =
                    build_config.fingerprint(engine.cache().compiler_identity());

                let mut can_fast_path = prev_manifest.compiler_id == current_compiler_id
                    && prev_manifest.config_fingerprint == current_config_fp
                    && prev_manifest.target_triple == options.target.spec().triple.to_string()
                    && prev_manifest.root_file == options.input_file.to_string_lossy().to_string();

                if !can_fast_path {
                    if prev_manifest.compiler_id != current_compiler_id {
                        engine.add_rebuild_reason("compiler", "compiler identity changed");
                    } else if prev_manifest.config_fingerprint != current_config_fp {
                        engine.add_rebuild_reason("config", "build configuration changed");
                    } else if prev_manifest.target_triple
                        != options.target.spec().triple.to_string()
                    {
                        engine.add_rebuild_reason("target", "target triple changed");
                    } else {
                        engine.add_rebuild_reason("root", "root file changed");
                    }
                } else {
                    for sf in &prev_manifest.source_files {
                        let path = Path::new(&sf.path);
                        if !path.exists() {
                            can_fast_path = false;
                            engine.add_rebuild_reason(&sf.path, "file removed");
                            break;
                        }
                        if let Ok(content) = fs::read_to_string(path) {
                            let curr_fp = engine.compute_source_fingerprint(path, &content);
                            if curr_fp != sf.fingerprint {
                                can_fast_path = false;
                                engine.add_rebuild_reason(&sf.path, "source content modified");
                                break;
                            }
                        } else {
                            can_fast_path = false;
                            engine.add_rebuild_reason(&sf.path, "unable to read file");
                            break;
                        }
                    }
                }

                if can_fast_path {
                    let root_src = fs::read_to_string(&options.input_file).unwrap_or_default();
                    let root_fp = engine.compute_source_fingerprint(&options.input_file, &root_src);
                    if let Some(cached_obj_bytes) =
                        engine.query_cached_object(&options.input_file, &root_fp)
                    {
                        engine.stats_mut().source_files_total = prev_manifest.source_files.len();
                        engine.stats_mut().source_files_unchanged =
                            prev_manifest.source_files.len();
                        engine.stats_mut().functions_total = prev_manifest.functions.len();
                        engine.stats_mut().functions_cached = prev_manifest.functions.len();
                        engine.stats_mut().functions_recompiled = 0;

                        if options.emit_target == EmitTarget::Obj {
                            fs::write(&options.output_file, &cached_obj_bytes)
                                .map_err(|e| format!("Failed to write object file: {}", e))?;
                            self.last_build_stats = Some(engine.stats().clone());
                            self.rebuild_explanations = engine.explanations().to_vec();
                            return Ok(Some(cached_obj_bytes));
                        }

                        if options.emit_target == EmitTarget::Exe {
                            let target_spec = options.target.spec();
                            let link_plan = Self::resolve_link_plan(options, &target_spec)?;
                            let obj_fp = prev_manifest.object_fingerprint;
                            let link_fp = engine
                                .compute_link_configuration_fingerprint(&link_plan, &[obj_fp])?;
                            if engine.check_link_reusable(&link_fp, &options.output_file) {
                                // Full no-op hit: zero compilation, zero relinking!
                                self.last_build_stats = Some(engine.stats().clone());
                                self.rebuild_explanations = engine.explanations().to_vec();
                                return Ok(Some(cached_obj_bytes));
                            }

                            // Relink using cached object artifact (Link-only rebuild)
                            Self::link_target_executable_with_plan(
                                options,
                                &target_spec,
                                &cached_obj_bytes,
                                &link_plan,
                            )?;
                            let _ = engine.record_linked_executable(
                                &link_fp,
                                &options.output_file,
                                &[obj_fp],
                            );
                            self.last_build_stats = Some(engine.stats().clone());
                            self.rebuild_explanations = engine.explanations().to_vec();
                            return Ok(Some(cached_obj_bytes));
                        }
                    }
                }
            } else {
                engine.add_rebuild_reason(
                    options.input_file.to_string_lossy().as_ref(),
                    "cold build - no prior manifest",
                );
            }

            Some((engine, prev_manifest_opt))
        } else {
            None
        };

        let mut module_tree = vanta_sema::module::ModuleLoader::load_module_tree_with_std(
            &options.input_file,
            options.std_path.as_deref(),
            &mut self.source_map,
            &mut self.diagnostics,
        )?;

        if self.diagnostics.has_errors() {
            return Err("Module loading failed with errors".into());
        }

        if options.emit_target == EmitTarget::Ast {
            let output = format!("{:#?}", module_tree);
            return Ok(Some(output.into_bytes()));
        }

        // Semantic Analysis (Type checking & Borrow checking)
        let sema = SemanticAnalyzer::new();
        let sema_ctx = sema.analyze_modules(&mut module_tree, &mut self.diagnostics);
        if self.diagnostics.has_errors() {
            return Err("Semantic analysis failed with errors".into());
        }

        // MIR Lowering & Verification
        let mut lowerer = MirLowerer::new(&sema_ctx);
        let mut mir = lowerer.lower_module_tree(&module_tree);
        MirVerifier::verify_program(&mir, &mut self.diagnostics);
        if self.diagnostics.has_errors() {
            return Err("MIR verification failed with errors".into());
        }

        // Run MIR optimization pipeline
        let is_executable = options.emit_target == EmitTarget::Exe;
        let target_config = vanta_mir::opt::VectorizeTargetConfig {
            has_avx: options.target_features.avx,
            has_avx2: options.target_features.avx2,
            has_fma: options.target_features.fma,
            has_avx512f: options.target_features.avx512f,
        };
        OptimizationPipeline::optimize_program_full_with_target(
            &mut mir,
            options.opt_level,
            options.enable_mem2reg,
            options.enable_gvn,
            options.enable_inline,
            options.enable_ipa_constprop,
            options.enable_licm,
            options.enable_indvar,
            options.enable_strength_reduction,
            options.enable_sroa,
            options.enable_loop_peel,
            options.enable_loop_unroll,
            options.enable_vectorize,
            target_config,
            options.enable_instruction_schedule,
            options.enable_global_dce,
            options.enable_tail_calls,
            is_executable,
            &mut self.diagnostics,
        );
        if self.diagnostics.has_errors() {
            return Err("Optimized MIR verification failed with errors".into());
        }

        if options.emit_target == EmitTarget::Mir {
            let output = format!("{:#?}", mir);
            return Ok(Some(output.into_bytes()));
        }

        // Native Code Generation & Relocatable Object Emission
        let target_spec = options.target.spec();
        let obj_bytes = if let Some((ref mut engine, _)) = incremental_engine {
            match target_spec.arch {
                Arch::X86_64 => match X86_64Backend::compile_to_object_incremental(
                    &mir,
                    &target_spec,
                    options.enable_reg_alloc,
                    options.enable_tail_calls,
                    options.enable_block_layout,
                    options.enable_code_align,
                    options.target_features,
                    options.debug_info,
                    if options.debug_info {
                        Some(&self.source_map)
                    } else {
                        None
                    },
                    engine,
                ) {
                    Ok(bytes) => bytes,
                    Err(e) => return Err(format!("Backend code generation failed: {}", e)),
                },
                Arch::AArch64 => match AArch64Backend::compile_to_macho(&mir) {
                    Ok(bytes) => bytes,
                    Err(e) => return Err(format!("AArch64 backend code generation failed: {}", e)),
                },
            }
        } else {
            match target_spec.arch {
                Arch::X86_64 => match X86_64Backend::compile_to_object_full_with_debug(
                    &mir,
                    &target_spec,
                    options.enable_reg_alloc,
                    options.enable_tail_calls,
                    options.enable_block_layout,
                    options.enable_code_align,
                    options.target_features,
                    options.debug_info,
                    if options.debug_info {
                        Some(&self.source_map)
                    } else {
                        None
                    },
                ) {
                    Ok(bytes) => bytes,
                    Err(e) => return Err(format!("Backend code generation failed: {}", e)),
                },
                Arch::AArch64 => match AArch64Backend::compile_to_macho(&mir) {
                    Ok(bytes) => bytes,
                    Err(e) => return Err(format!("AArch64 backend code generation failed: {}", e)),
                },
            }
        };

        if let Some((mut engine, prev_manifest_opt)) = incremental_engine {
            let root_src = fs::read_to_string(&options.input_file).unwrap_or_default();
            let source_fp = engine.compute_source_fingerprint(&options.input_file, &root_src);
            let _ = engine.store_compiled_object(&source_fp, &obj_bytes);
            let obj_fp = engine.compute_object_fingerprint(&source_fp);

            // Populate build graph with nodes and dependencies
            engine
                .graph_mut()
                .add_node(BuildNode::new(NodeId::workspace(), NodeKind::Workspace));

            let mut manifest_source_files = Vec::new();
            let mut fid = 1u32;
            while let Some(file) = self.source_map.get_file(vanta_source::FileId(fid)) {
                let is_user_source =
                    !file.path.starts_with("<embedded>") && !file.path.contains("library/std");
                if is_user_source {
                    let s_fp = engine.compute_source_fingerprint(Path::new(&file.path), &file.src);
                    let src_node_id = NodeId::source(&file.path);
                    engine.graph_mut().add_node(BuildNode::new(
                        src_node_id.clone(),
                        NodeKind::SourceFile {
                            path: file.path.clone(),
                            fingerprint: s_fp,
                        },
                    ));
                    manifest_source_files.push(ManifestSourceFile {
                        path: file.path.clone(),
                        fingerprint: s_fp,
                    });
                }
                fid += 1;
            }

            engine.stats_mut().source_files_total = manifest_source_files.len();
            let mut changed = 0;
            let mut unchanged = 0;
            if let Some(prev) = &prev_manifest_opt {
                for sf in &manifest_source_files {
                    if let Some(prev_sf) = prev.source_files.iter().find(|p| p.path == sf.path) {
                        if prev_sf.fingerprint == sf.fingerprint {
                            unchanged += 1;
                        } else {
                            changed += 1;
                        }
                    } else {
                        changed += 1;
                    }
                }
            } else {
                changed = manifest_source_files.len();
            }
            engine.stats_mut().source_files_changed = changed;
            engine.stats_mut().source_files_unchanged = unchanged;

            if engine.explanations().is_empty() {
                engine.add_rebuild_reason(
                    options.input_file.to_string_lossy().as_ref(),
                    "cold build - no prior manifest",
                );
            }

            let mut manifest_modules = Vec::new();
            for m in &module_tree.modules {
                let mod_node_id = NodeId::module(&m.name);
                let iface_fp =
                    engine.compute_source_fingerprint(&m.file_path, &format!("{:?}", m.items));
                engine.graph_mut().add_node(BuildNode::new(
                    mod_node_id.clone(),
                    NodeKind::Module {
                        name: m.name.clone(),
                        file_id: m.file_id.0,
                        interface_fingerprint: iface_fp,
                    },
                ));
                let src_node_id = NodeId::source(&m.file_path.to_string_lossy());
                engine
                    .graph_mut()
                    .add_dependency(mod_node_id, src_node_id, DependencyKind::Body);
                manifest_modules.push(ManifestModule {
                    name: m.name.clone(),
                    path_str: m.path_str(),
                    file_path: m.file_path.to_string_lossy().to_string(),
                    interface_fingerprint: iface_fp,
                });
            }

            let mut manifest_functions = Vec::new();
            let build_config = Self::build_configuration(options);
            let compiler_id = engine.cache().compiler_identity().clone();

            for f in &mir.functions {
                let (sig_fp, body_fp) = X86_64Backend::compute_function_fingerprints(
                    f,
                    &mir,
                    Some(&build_config),
                    &compiler_id,
                );
                let fn_node_id = NodeId::function("main", &f.name);
                engine.graph_mut().add_node(BuildNode::new(
                    fn_node_id.clone(),
                    NodeKind::Function {
                        name: f.name.clone(),
                        module: "main".into(),
                        body_fingerprint: body_fp,
                        sig_fingerprint: sig_fp,
                    },
                ));
                manifest_functions.push(ManifestFunction {
                    name: f.name.clone(),
                    module: "main".into(),
                    body_fingerprint: body_fp,
                    sig_fingerprint: sig_fp,
                });
            }

            let mut manifest_types = Vec::new();
            let mut sorted_structs: Vec<_> = mir.structs.iter().collect();
            sorted_structs.sort_by_key(|(k, _)| *k);
            for (_sname, s) in sorted_structs {
                let type_fp = engine.compute_source_fingerprint(
                    Path::new(&options.input_file),
                    &format!("{:?}", s),
                );
                let type_node_id = NodeId::type_interface(&s.name);
                engine.graph_mut().add_node(BuildNode::new(
                    type_node_id,
                    NodeKind::TypeInterface {
                        name: s.name.clone(),
                        layout_fingerprint: type_fp,
                    },
                ));
                manifest_types.push(ManifestType {
                    name: s.name.clone(),
                    layout_fingerprint: type_fp,
                });
            }

            let link_plan = Self::resolve_link_plan(options, &target_spec)?;
            let link_fp = engine.compute_link_configuration_fingerprint(&link_plan, &[obj_fp])?;

            let manifest = BuildManifest {
                schema_version: CompilerIdentity::CURRENT_CACHE_SCHEMA_VERSION,
                compiler_id: compiler_id.fingerprint().to_hex(),
                target_triple: target_spec.triple.to_string(),
                root_file: options.input_file.to_string_lossy().to_string(),
                config_fingerprint: build_config.fingerprint(&compiler_id),
                source_files: manifest_source_files,
                modules: manifest_modules,
                functions: manifest_functions,
                types: manifest_types,
                graph_edges: Vec::new(),
                object_fingerprint: obj_fp,
                link_fingerprint: link_fp,
            };

            let _ = engine.save_manifest(&manifest);

            if options.emit_target == EmitTarget::Obj {
                fs::write(&options.output_file, &obj_bytes)
                    .map_err(|e| format!("Failed to write object file: {}", e))?;
                self.last_build_stats = Some(engine.stats().clone());
                self.rebuild_explanations = engine.explanations().to_vec();
                return Ok(Some(obj_bytes));
            }

            // Linking to Native Executable
            engine.stats_mut().linking_performed = true;
            engine.stats_mut().linking_reason = Some("cold build or modified object".into());
            Self::link_target_executable_with_plan(options, &target_spec, &obj_bytes, &link_plan)?;
            let _ = engine.record_linked_executable(&link_fp, &options.output_file, &[obj_fp]);
            self.last_build_stats = Some(engine.stats().clone());
            self.rebuild_explanations = engine.explanations().to_vec();
            return Ok(Some(obj_bytes));
        }

        if options.emit_target == EmitTarget::Obj {
            fs::write(&options.output_file, &obj_bytes)
                .map_err(|e| format!("Failed to write object file: {}", e))?;
            return Ok(Some(obj_bytes));
        }

        // Linking to Native Executable
        let link_plan = Self::resolve_link_plan(options, &target_spec)?;
        Self::link_target_executable_with_plan(options, &target_spec, &obj_bytes, &link_plan)?;
        Ok(Some(obj_bytes))
    }

    /// Resolves all inputs, libraries, runtime archives, and linker options once into a canonical plan.
    pub fn resolve_link_plan(
        options: &CompilerOptions,
        target_spec: &TargetSpec,
    ) -> Result<LinkConfiguration, String> {
        Self::resolve_link_plan_with_resolver(options, target_spec, &DefaultLinkerResolver)
    }

    /// Resolves all inputs, libraries, runtime archives, and linker options using an injected LinkerResolver.
    pub fn resolve_link_plan_with_resolver<R: LinkerResolver>(
        options: &CompilerOptions,
        target_spec: &TargetSpec,
        resolver: &R,
    ) -> Result<LinkConfiguration, String> {
        let mut plan = LinkConfiguration::new(
            target_spec.triple.to_string(),
            format!("{:?}", target_spec.object_format),
            match options.linker {
                LinkerKind::Internal => "internal",
                LinkerKind::System => "system",
            },
            options.output_file.to_string_lossy(),
        );

        plan.debug_info = options.debug_info;
        plan.map_file = options
            .map_file
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        plan.library_search_paths = options.lib_search_paths.clone();
        plan.requested_libraries = options.libs.clone();

        // 1. Linker Executable, Delegated Downstream Linker & Flags
        match options.linker {
            LinkerKind::Internal => {
                plan.linker_version = Some(format!(
                    "vanta-internal-linker-{}",
                    env!("CARGO_PKG_VERSION")
                ));
                plan.linker_driver = None;
                plan.downstream_linker = None;
            }
            LinkerKind::System => {
                let (driver_name, default_flags) = match target_spec.os {
                    Os::Linux => ("gcc", vec!["-no-pie".to_string()]),
                    Os::Windows => {
                        let has_link = resolver.resolve_in_path("link.exe").is_ok();
                        if has_link {
                            (
                                "link.exe",
                                vec![
                                    "/NOLOGO".to_string(),
                                    format!("/OUT:{}", options.output_file.display()),
                                    "/ENTRY:main".to_string(),
                                    "/SUBSYSTEM:CONSOLE".to_string(),
                                ],
                            )
                        } else {
                            (
                                "lld-link",
                                vec![
                                    format!("/OUT:{}", options.output_file.display()),
                                    "/ENTRY:main".to_string(),
                                    "/SUBSYSTEM:CONSOLE".to_string(),
                                ],
                            )
                        }
                    }
                    Os::MacOS => (
                        "clang",
                        vec![
                            "-arch".to_string(),
                            "arm64".to_string(),
                            "-o".to_string(),
                            options.output_file.to_string_lossy().to_string(),
                        ],
                    ),
                };

                let driver_path = resolver.resolve_in_path(driver_name)?;
                let driver_bytes = resolver.read_executable_bytes(&driver_path)?;
                let driver_input = ResolvedLinkInput::from_bytes(
                    LinkInputKind::LinkerDriver,
                    &driver_path,
                    &driver_bytes,
                );
                plan.linker_path = Some(driver_path.to_string_lossy().to_string());
                plan.linker_driver = Some(driver_input.clone());
                plan.linker_version = resolver.query_version(&driver_path);

                let mut combined_flags = default_flags;
                for flag in &options.linker_flags {
                    if !combined_flags.contains(flag) {
                        combined_flags.push(flag.clone());
                    }
                }
                plan.linker_flags = combined_flags.clone();

                let env_vars = collect_linker_environment_variables();

                if let Some(downstream_path) = resolver.resolve_downstream_backend(
                    &driver_path,
                    driver_name,
                    &plan.linker_flags,
                    &env_vars,
                )? {
                    // Authoritatively verify downstream backend via dry-run probe
                    resolver.verify_driver_invocation(
                        &driver_path,
                        driver_name,
                        &plan.linker_flags,
                        &downstream_path,
                        &env_vars,
                    )?;

                    let downstream_bytes = resolver.read_executable_bytes(&downstream_path)?;
                    let downstream_input = ResolvedLinkInput::from_bytes(
                        LinkInputKind::LinkerBackend,
                        &downstream_path,
                        &downstream_bytes,
                    );
                    plan.downstream_linker = Some(downstream_input.clone());

                    plan.resolved_invocation = Some(ResolvedInvocation::new(
                        driver_path,
                        driver_input.content_fingerprint,
                        Some(downstream_path),
                        Some(downstream_input.content_fingerprint),
                        combined_flags,
                        env_vars,
                    ));
                } else {
                    plan.resolved_invocation = Some(ResolvedInvocation::new(
                        driver_path,
                        driver_input.content_fingerprint,
                        None,
                        None,
                        combined_flags,
                        env_vars,
                    ));
                }
            }
        }

        // 2. Runtime archive
        match options.linker {
            LinkerKind::System => {
                if target_spec.os == Os::Linux {
                    let rt_archive_path = Self::find_runtime_archive(options)?;
                    let rt_input = ResolvedLinkInput::from_path(
                        LinkInputKind::RuntimeArchive,
                        &rt_archive_path,
                    )?;
                    plan.runtime_archive = Some(rt_input);
                }
            }
            LinkerKind::Internal => {
                if let Some(ref rt_path) = options.runtime_path {
                    let rt_input =
                        ResolvedLinkInput::from_path(LinkInputKind::RuntimeArchive, rt_path)?;
                    plan.runtime_archive = Some(rt_input);
                }
            }
        }

        // 3. Requested libraries resolution against search paths (order preserved!)
        for lib_name in &options.libs {
            let mut resolved = None;
            for search_dir in &options.lib_search_paths {
                let candidates = match target_spec.os {
                    Os::Windows => vec![
                        format!("{}.lib", lib_name),
                        format!("lib{}.a", lib_name),
                        format!("{}.dll", lib_name),
                    ],
                    _ => vec![
                        format!("lib{}.a", lib_name),
                        format!("lib{}.so", lib_name),
                        format!("lib{}.dylib", lib_name),
                    ],
                };

                for cand in candidates {
                    let cand_path = search_dir.join(&cand);
                    if cand_path.is_file() {
                        let input = ResolvedLinkInput::from_path(
                            LinkInputKind::ResolvedLibrary,
                            &cand_path,
                        )?;
                        resolved = Some((cand_path, input));
                        break;
                    }
                }
                if resolved.is_some() {
                    break;
                }
            }

            if let Some((p, input)) = resolved {
                plan.resolved_libraries.push(ResolvedLibrary {
                    name: lib_name.clone(),
                    resolved_path: Some(p),
                    input: Some(input),
                });
            } else {
                plan.resolved_libraries.push(ResolvedLibrary {
                    name: lib_name.clone(),
                    resolved_path: None,
                    input: None,
                });
            }
        }

        // 4. Static archives (order preserved!)
        for arch in &options.link_archives {
            let input = ResolvedLinkInput::from_path(LinkInputKind::StaticArchive, arch)?;
            plan.static_archives.push(input);
        }

        // 5. Additional inputs (classified by extension, order preserved!)
        for extra in &options.additional_inputs {
            let ext = extra.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext == "o" || ext == "obj" {
                let input = ResolvedLinkInput::from_path(LinkInputKind::AdditionalObject, extra)?;
                plan.additional_objects.push(input);
            } else if ext == "a" || ext == "lib" {
                let input = ResolvedLinkInput::from_path(LinkInputKind::StaticArchive, extra)?;
                plan.static_archives.push(input);
            } else {
                let input = ResolvedLinkInput::from_path(LinkInputKind::Other, extra)?;
                plan.additional_inputs.push(input);
            }
        }

        Ok(plan)
    }

    /// Revalidates that the compiler driver and downstream linker backend on disk still match
    /// the content fingerprints recorded in the plan before execution.
    pub fn verify_plan_executables_integrity<R: LinkerResolver>(
        plan: &LinkConfiguration,
        resolver: &R,
    ) -> Result<(), String> {
        if let Some(ref driver) = plan.linker_driver {
            let current_bytes = resolver.read_executable_bytes(&driver.path).map_err(|e| {
                format!(
                    "Pre-link executable integrity check failed for driver '{}': {}",
                    driver.path.display(),
                    e
                )
            })?;
            if current_bytes.len() as u64 != driver.content_length {
                return Err(format!(
                    "Pre-link executable integrity check failed for driver '{}': length changed from {} to {}",
                    driver.path.display(),
                    driver.content_length,
                    current_bytes.len()
                ));
            }
            let current_fp =
                ContentFingerprint::for_bytes(LinkInputKind::LinkerDriver.domain(), &current_bytes);
            if current_fp != driver.content_fingerprint {
                return Err(format!(
                    "Pre-link executable integrity check failed: linker driver '{}' content modified between resolution and execution (expected {}, found {})",
                    driver.path.display(),
                    driver.content_fingerprint,
                    current_fp
                ));
            }
        }

        if let Some(ref downstream) = plan.downstream_linker {
            let current_bytes = resolver
                .read_executable_bytes(&downstream.path)
                .map_err(|e| {
                    format!(
                        "Pre-link executable integrity check failed for backend '{}': {}",
                        downstream.path.display(),
                        e
                    )
                })?;
            if current_bytes.len() as u64 != downstream.content_length {
                return Err(format!(
                    "Pre-link executable integrity check failed for backend '{}': length changed from {} to {}",
                    downstream.path.display(),
                    downstream.content_length,
                    current_bytes.len()
                ));
            }
            let current_fp = ContentFingerprint::for_bytes(
                LinkInputKind::LinkerBackend.domain(),
                &current_bytes,
            );
            if current_fp != downstream.content_fingerprint {
                return Err(format!(
                    "Pre-link executable integrity check failed: downstream linker backend '{}' content modified between resolution and execution (expected {}, found {})",
                    downstream.path.display(),
                    downstream.content_fingerprint,
                    current_fp
                ));
            }
        }

        Ok(())
    }

    /// Link executable using a resolved LinkConfiguration plan.
    pub fn link_target_executable_with_plan(
        options: &CompilerOptions,
        target_spec: &TargetSpec,
        obj_bytes: &[u8],
        plan: &LinkConfiguration,
    ) -> Result<(), String> {
        Self::link_target_executable_with_plan_and_resolver(
            options,
            target_spec,
            obj_bytes,
            plan,
            &DefaultLinkerResolver,
        )
    }

    /// Link executable using a resolved LinkConfiguration plan with an injected resolver.
    pub fn link_target_executable_with_plan_and_resolver<R: LinkerResolver>(
        options: &CompilerOptions,
        target_spec: &TargetSpec,
        obj_bytes: &[u8],
        plan: &LinkConfiguration,
        resolver: &R,
    ) -> Result<(), String> {
        if options.linker == LinkerKind::Internal {
            let mut link_config =
                vanta_linker::LinkerConfig::new(options.target, options.output_file.clone());
            link_config
                .input_buffers
                .push(("main.o".into(), obj_bytes.to_vec()));
            link_config.debug_info = plan.debug_info;
            link_config.map_file = options.map_file.clone();
            link_config.library_search_paths = plan.library_search_paths.clone();
            link_config.libraries = options.libs.clone();

            // Include all resolved static archives from plan (preserving exact order!)
            for arch in &plan.static_archives {
                link_config.archives.push(arch.path.clone());
            }

            // Include any resolved libraries that are archives
            for lib in &plan.resolved_libraries {
                if let Some(ref p) = lib.resolved_path {
                    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
                    if (ext == "a" || ext == "lib") && !link_config.archives.contains(p) {
                        link_config.archives.push(p.clone());
                    }
                }
            }

            // Include additional object files from plan
            for obj in &plan.additional_objects {
                link_config.input_objects.push(obj.path.clone());
            }

            // Include runtime archive if present
            if let Some(ref rt) = plan.runtime_archive {
                if !link_config.archives.contains(&rt.path) {
                    link_config.archives.push(rt.path.clone());
                }
            }

            vanta_linker::link(&link_config)
                .map_err(|e| format!("Internal linking failed: {}", e))?;
            return Ok(());
        }

        // Before any external linking execution, verify executable content integrity on disk!
        Self::verify_plan_executables_integrity(plan, resolver)?;

        match target_spec.os {
            Os::Linux => {
                let rt_archive = match plan.runtime_archive {
                    Some(ref rt) => rt.path.clone(),
                    None => Self::find_runtime_archive(options)?,
                };
                let temp_obj_path = options
                    .output_file
                    .with_extension(target_spec.obj_extension);
                fs::write(&temp_obj_path, obj_bytes)
                    .map_err(|e| format!("Failed to write temporary object file: {}", e))?;

                let driver_path = if let Some(ref d) = plan.linker_driver {
                    d.path.clone()
                } else if let Some(ref p) = plan.linker_path {
                    PathBuf::from(p)
                } else {
                    PathBuf::from("gcc")
                };

                let mut args = plan.linker_flags.clone();
                args.push(temp_obj_path.to_string_lossy().to_string());
                args.push(rt_archive.to_string_lossy().to_string());
                args.push("-o".to_string());
                args.push(options.output_file.to_string_lossy().to_string());

                let env_vars = if let Some(ref inv) = plan.resolved_invocation {
                    inv.process_environment.clone()
                } else {
                    collect_linker_environment_variables()
                };

                let expected_backend = plan.downstream_linker.as_ref().map(|d| d.path.as_path());

                let link_res =
                    resolver.execute_link(&driver_path, &args, &env_vars, expected_backend);

                if link_res.is_ok() {
                    let _ = fs::remove_file(&temp_obj_path);
                }
                match link_res {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("Linking failed: {}", e)),
                }
            }
            Os::Windows => {
                let temp_obj_path = options
                    .output_file
                    .with_extension(target_spec.obj_extension);
                fs::write(&temp_obj_path, obj_bytes)
                    .map_err(|e| format!("Failed to write temporary object file: {}", e))?;

                let driver_path = if let Some(ref d) = plan.linker_driver {
                    d.path.clone()
                } else if let Some(ref p) = plan.linker_path {
                    PathBuf::from(p)
                } else {
                    PathBuf::from("link.exe")
                };

                let mut args = plan.linker_flags.clone();
                args.push(temp_obj_path.to_string_lossy().to_string());

                let env_vars = if let Some(ref inv) = plan.resolved_invocation {
                    inv.process_environment.clone()
                } else {
                    collect_linker_environment_variables()
                };

                let expected_backend = plan.downstream_linker.as_ref().map(|d| d.path.as_path());

                let link_res =
                    resolver.execute_link(&driver_path, &args, &env_vars, expected_backend);

                if link_res.is_ok() {
                    let _ = fs::remove_file(&temp_obj_path);
                    Ok(())
                } else {
                    Err(format!(
                        "Linker '{}' failed for target {}. Relocatable COFF object written to '{}': {}",
                        driver_path.display(),
                        target_spec.triple,
                        temp_obj_path.display(),
                        link_res.unwrap_err()
                    ))
                }
            }
            Os::MacOS => {
                let temp_obj_path = options
                    .output_file
                    .with_extension(target_spec.obj_extension);
                fs::write(&temp_obj_path, obj_bytes)
                    .map_err(|e| format!("Failed to write temporary object file: {}", e))?;

                let driver_path = if let Some(ref d) = plan.linker_driver {
                    d.path.clone()
                } else if let Some(ref p) = plan.linker_path {
                    PathBuf::from(p)
                } else {
                    PathBuf::from("clang")
                };

                let mut args = plan.linker_flags.clone();
                args.push(temp_obj_path.to_string_lossy().to_string());

                let env_vars = if let Some(ref inv) = plan.resolved_invocation {
                    inv.process_environment.clone()
                } else {
                    collect_linker_environment_variables()
                };

                let expected_backend = plan.downstream_linker.as_ref().map(|d| d.path.as_path());

                let link_res =
                    resolver.execute_link(&driver_path, &args, &env_vars, expected_backend);

                if link_res.is_ok() {
                    let _ = fs::remove_file(&temp_obj_path);
                    Ok(())
                } else {
                    Err(format!(
                        "Linker '{}' failed for target {}. Relocatable Mach-O 64-bit object written to '{}': {}",
                        driver_path.display(),
                        target_spec.triple,
                        temp_obj_path.display(),
                        link_res.unwrap_err()
                    ))
                }
            }
        }
    }

    pub fn link_target_executable(
        options: &CompilerOptions,
        target_spec: &TargetSpec,
        obj_bytes: &[u8],
    ) -> Result<(), String> {
        let plan = Self::resolve_link_plan(options, target_spec)?;
        Self::link_target_executable_with_plan(options, target_spec, obj_bytes, &plan)
    }

    /// Locate the precompiled `libvanta_rt.a` archive using the standard toolchain search hierarchy.
    pub fn find_runtime_archive(options: &CompilerOptions) -> Result<PathBuf, String> {
        // 1. Explicit CLI option
        if let Some(ref p) = options.runtime_path {
            if p.is_file() {
                return Ok(p.clone());
            }
            return Err(format!(
                "Specified runtime path does not exist: {}",
                p.display()
            ));
        }

        // 2. VANTA_RUNTIME_PATH env var
        if let Ok(env_path) = std::env::var("VANTA_RUNTIME_PATH") {
            let p = PathBuf::from(env_path);
            if p.is_file() {
                return Ok(p);
            }
        }

        // 3. VANTA_HOME env var
        if let Ok(vanta_home) = std::env::var("VANTA_HOME") {
            let base = PathBuf::from(vanta_home);
            let cand1 = base.join("lib").join("vanta").join("libvanta_rt.a");
            if cand1.is_file() {
                return Ok(cand1);
            }
            let cand2 = base.join("runtime").join("libvanta_rt.a");
            if cand2.is_file() {
                return Ok(cand2);
            }
        }

        // 4. Executable-relative path
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let cand_installed = parent.join("../lib/vanta/libvanta_rt.a");
                if cand_installed.is_file() {
                    return Ok(cand_installed);
                }
                let cand_dev = parent.join("../../runtime/libvanta_rt.a");
                if cand_dev.is_file() {
                    return Ok(cand_dev);
                }
            }
        }

        // 5. CWD dev path
        let cwd_candidate = Path::new("runtime").join("libvanta_rt.a");
        if cwd_candidate.is_file() {
            return Ok(cwd_candidate);
        }

        // 6. Cargo manifest candidate
        let manifest_candidate =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/libvanta_rt.a");
        if manifest_candidate.is_file() {
            return Ok(manifest_candidate);
        }

        // 7. Dynamic fallback: compile and cache libvanta_rt.a
        Self::build_cached_runtime_archive()
    }

    fn build_cached_runtime_archive() -> Result<PathBuf, String> {
        static BUILD_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = BUILD_MUTEX.lock().unwrap_or_else(|p| p.into_inner());

        let repo_rt_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime");
        let target_archive = if repo_rt_dir.is_dir() {
            repo_rt_dir.join("libvanta_rt.a")
        } else {
            let temp_dir = std::env::temp_dir().join("vanta_rt_cache");
            let _ = fs::create_dir_all(&temp_dir);
            temp_dir.join("libvanta_rt.a")
        };

        if target_archive.is_file() {
            return Ok(target_archive);
        }

        let pid = std::process::id();
        let temp_c = std::env::temp_dir().join(format!("vanta_rt_{}.c", pid));
        fs::write(&temp_c, RUNTIME_C)
            .map_err(|e| format!("Failed to write runtime source: {}", e))?;
        let temp_o = std::env::temp_dir().join(format!("vanta_rt_{}.o", pid));

        let gcc_status = Command::new("gcc")
            .arg("-c")
            .arg("-O2")
            .arg("-fPIC")
            .arg(&temp_c)
            .arg("-o")
            .arg(&temp_o)
            .status()
            .map_err(|e| format!("Could not invoke gcc to compile runtime: {}", e))?;

        let _ = fs::remove_file(&temp_c);

        if !gcc_status.success() {
            let _ = fs::remove_file(&temp_o);
            return Err(format!(
                "Failed to compile native runtime (gcc exited with {})",
                gcc_status
            ));
        }

        let ar_status = Command::new("ar")
            .arg("rcs")
            .arg(&target_archive)
            .arg(&temp_o)
            .status()
            .map_err(|e| format!("Could not invoke ar to create runtime archive: {}", e))?;

        let _ = fs::remove_file(&temp_o);

        if !ar_status.success() {
            return Err(format!(
                "Failed to package runtime archive (ar exited with {})",
                ar_status
            ));
        }

        Ok(target_archive)
    }

    fn invoke_system_linker_with_plan(
        plan: &LinkConfiguration,
        obj_path: &Path,
        out_path: &Path,
        rt_archive: &Path,
    ) -> Result<(), String> {
        let driver_path = if let Some(ref d) = plan.linker_driver {
            d.path.clone()
        } else if let Some(ref p) = plan.linker_path {
            PathBuf::from(p)
        } else {
            PathBuf::from("gcc")
        };

        let mut args = plan.linker_flags.clone();
        args.push(obj_path.to_string_lossy().to_string());
        args.push(rt_archive.to_string_lossy().to_string());
        args.push("-o".to_string());
        args.push(out_path.to_string_lossy().to_string());

        let env_vars = if let Some(ref inv) = plan.resolved_invocation {
            inv.process_environment.clone()
        } else {
            collect_linker_environment_variables()
        };

        let expected_backend = plan.downstream_linker.as_ref().map(|d| d.path.as_path());
        DefaultLinkerResolver.execute_link(&driver_path, &args, &env_vars, expected_backend)
    }

    #[allow(dead_code)]
    fn invoke_system_linker(
        obj_path: &Path,
        out_path: &Path,
        rt_archive: &Path,
    ) -> Result<(), String> {
        let default_plan = LinkConfiguration::new(
            "x86_64-unknown-linux-gnu",
            "elf64",
            "system",
            out_path.to_string_lossy(),
        );
        Self::invoke_system_linker_with_plan(&default_plan, obj_path, out_path, rt_archive)
    }
}

#[derive(Debug, Clone)]
pub struct FmtOptions {
    pub files: Vec<PathBuf>,
    pub check: bool,
    pub stdout: bool,
    pub verbose: bool,
    pub diagnostic_format: DiagnosticFormat,
}

impl Default for FmtOptions {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            check: false,
            stdout: false,
            verbose: false,
            diagnostic_format: DiagnosticFormat::Human,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FmtResult {
    Success,
    NeedsFormatting,
    SyntaxError,
    IoError,
}

pub fn run_fmt(options: &FmtOptions) -> (FmtResult, String, String) {
    if options.files.is_empty() {
        return (
            FmtResult::IoError,
            String::new(),
            "vantac fmt error: No input files specified\n".into(),
        );
    }

    if options.stdout {
        if options.files.len() > 1 {
            return (
                FmtResult::IoError,
                String::new(),
                "vantac fmt error: --stdout supports exactly one input file\n".into(),
            );
        }
        let file = &options.files[0];
        let src = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                return (
                    FmtResult::IoError,
                    String::new(),
                    format!(
                        "vantac fmt error: Failed to read file '{}': {}\n",
                        file.display(),
                        e
                    ),
                );
            }
        };

        match vanta_fmt::format_source(&src, &file.to_string_lossy()) {
            Ok(formatted) => (FmtResult::Success, formatted, String::new()),
            Err(diags) => {
                let mut sm = SourceMap::new();
                sm.add_file(file.to_string_lossy().to_string(), src);
                let err_str = match options.diagnostic_format {
                    DiagnosticFormat::Human => diags.render_all(&sm, true),
                    DiagnosticFormat::Json => diags.to_json(&sm),
                };
                (FmtResult::SyntaxError, String::new(), err_str)
            }
        }
    } else if options.check {
        let mut diff_files = Vec::new();
        for file in &options.files {
            let src = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    return (
                        FmtResult::IoError,
                        String::new(),
                        format!(
                            "vantac fmt error: Failed to read file '{}': {}\n",
                            file.display(),
                            e
                        ),
                    );
                }
            };

            match vanta_fmt::check_formatted(&src, &file.to_string_lossy()) {
                Ok(is_clean) => {
                    if !is_clean {
                        diff_files.push(file.to_string_lossy().to_string());
                    }
                }
                Err(diags) => {
                    let mut sm = SourceMap::new();
                    sm.add_file(file.to_string_lossy().to_string(), src);
                    let err_str = match options.diagnostic_format {
                        DiagnosticFormat::Human => diags.render_all(&sm, true),
                        DiagnosticFormat::Json => diags.to_json(&sm),
                    };
                    return (FmtResult::SyntaxError, String::new(), err_str);
                }
            }
        }

        if !diff_files.is_empty() {
            let msg = if options.diagnostic_format == DiagnosticFormat::Json {
                format!(
                    "{{\"status\":\"needs_formatting\",\"files\":[{}]}}\n",
                    diff_files
                        .iter()
                        .map(|f| format!("\"{}\"", f))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            } else {
                let mut out = String::new();
                for f in &diff_files {
                    out.push_str(&format!("Diff in {}\n", f));
                }
                out
            };
            (FmtResult::NeedsFormatting, String::new(), msg)
        } else {
            let msg = if options.diagnostic_format == DiagnosticFormat::Json {
                "{\"status\":\"ok\",\"files\":[]}\n".into()
            } else if options.verbose {
                "All files conform to canonical formatting.\n".into()
            } else {
                String::new()
            };
            (FmtResult::Success, msg, String::new())
        }
    } else {
        // Default mode: atomic in-place formatting
        let mut formatted_files = Vec::new();
        for file in &options.files {
            let src = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    return (
                        FmtResult::IoError,
                        String::new(),
                        format!(
                            "vantac fmt error: Failed to read file '{}': {}\n",
                            file.display(),
                            e
                        ),
                    );
                }
            };

            let formatted = match vanta_fmt::format_source(&src, &file.to_string_lossy()) {
                Ok(f) => f,
                Err(diags) => {
                    let mut sm = SourceMap::new();
                    sm.add_file(file.to_string_lossy().to_string(), src);
                    let err_str = match options.diagnostic_format {
                        DiagnosticFormat::Human => diags.render_all(&sm, true),
                        DiagnosticFormat::Json => diags.to_json(&sm),
                    };
                    return (FmtResult::SyntaxError, String::new(), err_str);
                }
            };

            if formatted != src {
                // Safe atomic write using temporary file
                let tmp_path = file.with_extension("va.tmp_fmt");
                if let Err(e) = fs::write(&tmp_path, formatted.as_bytes()) {
                    return (
                        FmtResult::IoError,
                        String::new(),
                        format!(
                            "vantac fmt error: Failed to write temporary file '{}': {}\n",
                            tmp_path.display(),
                            e
                        ),
                    );
                }
                if let Err(_e) = fs::rename(&tmp_path, file) {
                    if let Err(copy_err) = fs::copy(&tmp_path, file) {
                        let _ = fs::remove_file(&tmp_path);
                        return (
                            FmtResult::IoError,
                            String::new(),
                            format!(
                                "vantac fmt error: Failed to update file '{}': {}\n",
                                file.display(),
                                copy_err
                            ),
                        );
                    }
                    let _ = fs::remove_file(&tmp_path);
                }
                formatted_files.push(file.to_string_lossy().to_string());
            }
        }

        let msg = if options.diagnostic_format == DiagnosticFormat::Json {
            format!(
                "{{\"status\":\"formatted\",\"files\":[{}]}}\n",
                formatted_files
                    .iter()
                    .map(|f| format!("\"{}\"", f))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else if options.verbose {
            let mut out = String::new();
            for f in &formatted_files {
                out.push_str(&format!("Formatted {}\n", f));
            }
            out
        } else {
            String::new()
        };

        (FmtResult::Success, msg, String::new())
    }
}
