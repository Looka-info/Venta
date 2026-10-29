//! Vanta compiler pipeline driver and linker orchestrator.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
pub use vanta_codegen_x86_64::TargetFeatures;
use vanta_codegen_x86_64::X86_64Backend;
use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
pub use vanta_mir::opt::OptLevel;
use vanta_mir::{MirLowerer, MirVerifier, OptimizationPipeline};
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

const RUNTIME_C: &str = include_str!("../../../runtime/vanta_rt.c");

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
}

impl CompilerOptions {
    pub fn new(input_file: PathBuf, output_file: PathBuf) -> Self {
        Self {
            input_file,
            output_file,
            emit_target: EmitTarget::Exe,
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

        // Native Code Generation & ELF64 Emission
        let elf_bytes = match X86_64Backend::compile_to_elf_full_with_debug(
            &mir,
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
            Err(e) => {
                return Err(format!("Backend code generation failed: {}", e));
            }
        };

        if options.emit_target == EmitTarget::Obj {
            fs::write(&options.output_file, &elf_bytes)
                .map_err(|e| format!("Failed to write object file: {}", e))?;
            return Ok(Some(elf_bytes));
        }

        // Linking to Native Executable (System V AMD64 ABI on Linux)
        let rt_archive = Self::find_runtime_archive(options)?;

        let temp_obj_path = options.output_file.with_extension("o");
        fs::write(&temp_obj_path, &elf_bytes)
            .map_err(|e| format!("Failed to write temporary object file: {}", e))?;

        let link_res =
            Self::invoke_system_linker(&temp_obj_path, &options.output_file, &rt_archive);
        let _ = fs::remove_file(&temp_obj_path);

        match link_res {
            Ok(_) => Ok(Some(elf_bytes)),
            Err(e) => Err(format!("Linking failed: {}", e)),
        }
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

    fn invoke_system_linker(
        obj_path: &Path,
        out_path: &Path,
        rt_archive: &Path,
    ) -> Result<(), String> {
        // Use gcc as linker driver to link standard runtime startup, libc, and libvanta_rt.a
        let status = Command::new("gcc")
            .arg("-no-pie") // Produce standard executable layout
            .arg(obj_path)
            .arg(rt_archive)
            .arg("-o")
            .arg(out_path)
            .status();

        let status = status.map_err(|e| format!("Could not invoke gcc linker: {}", e))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("Linker exited with status {}", status))
        }
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
