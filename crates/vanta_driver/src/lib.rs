//! Vanta compiler pipeline driver and linker orchestrator.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use vanta_codegen_x86_64::X86_64Backend;
use vanta_diagnostics::DiagnosticBag;
use vanta_lexer::Lexer;
pub use vanta_mir::opt::OptLevel;
use vanta_mir::{MirLowerer, MirVerifier, OptimizationPipeline};
use vanta_parser::Parser;
use vanta_sema::SemanticAnalyzer;
use vanta_source::SourceMap;

const RUNTIME_C: &str = include_str!("../../../runtime/vanta_rt.c");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitTarget {
    Tokens,
    Ast,
    Mir,
    Obj,
    Exe,
}

pub struct CompilerOptions {
    pub input_file: PathBuf,
    pub output_file: PathBuf,
    pub emit_target: EmitTarget,
    pub opt_level: OptLevel,
    pub verbose: bool,
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

    pub fn compile(&mut self, options: &CompilerOptions) -> Result<Option<Vec<u8>>, String> {
        let src = fs::read_to_string(&options.input_file)
            .map_err(|e| format!("Failed to read input file {:?}: {}", options.input_file, e))?;

        let file_id = self
            .source_map
            .add_file(options.input_file.to_string_lossy().to_string(), src);
        let file = self.source_map.get_file(file_id).unwrap();

        // 1. Lexing
        let mut lexer = Lexer::new(&file);
        let tokens = lexer.tokenize(&mut self.diagnostics);
        if self.diagnostics.has_errors() {
            eprintln!("{}", self.diagnostics.render_all(&self.source_map, true));
            return Err("Lexing failed with errors".into());
        }

        if options.emit_target == EmitTarget::Tokens {
            let output = format!("{:#?}", tokens);
            return Ok(Some(output.into_bytes()));
        }

        // 2. Parsing
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program(&mut self.diagnostics);
        if self.diagnostics.has_errors() {
            eprintln!("{}", self.diagnostics.render_all(&self.source_map, true));
            return Err("Parsing failed with errors".into());
        }

        if options.emit_target == EmitTarget::Ast {
            let output = format!("{:#?}", program);
            return Ok(Some(output.into_bytes()));
        }

        // 3. Semantic Analysis (Type checking & Borrow checking)
        let sema = SemanticAnalyzer::new();
        let sema_ctx = sema.analyze(&program, &mut self.diagnostics);
        if self.diagnostics.has_errors() {
            eprintln!("{}", self.diagnostics.render_all(&self.source_map, true));
            return Err("Semantic analysis failed with errors".into());
        }

        // 4. MIR Lowering & Verification
        let mut lowerer = MirLowerer::new(&sema_ctx);
        let mut mir = lowerer.lower_program(&program);
        MirVerifier::verify_program(&mir, &mut self.diagnostics);
        if self.diagnostics.has_errors() {
            eprintln!("{}", self.diagnostics.render_all(&self.source_map, true));
            return Err("MIR verification failed with errors".into());
        }

        // Run MIR optimization pipeline
        OptimizationPipeline::optimize_program(&mut mir, options.opt_level, &mut self.diagnostics);
        if self.diagnostics.has_errors() {
            eprintln!("{}", self.diagnostics.render_all(&self.source_map, true));
            return Err("Optimized MIR verification failed with errors".into());
        }

        if options.emit_target == EmitTarget::Mir {
            let output = format!("{:#?}", mir);
            return Ok(Some(output.into_bytes()));
        }

        // 5. Native Code Generation & ELF64 Emission
        let elf_bytes = X86_64Backend::compile_to_elf(&mir);

        if options.emit_target == EmitTarget::Obj {
            fs::write(&options.output_file, &elf_bytes)
                .map_err(|e| format!("Failed to write object file: {}", e))?;
            return Ok(Some(elf_bytes));
        }

        // 6. Linking to Native Executable (System V AMD64 ABI on Linux)
        let temp_obj_path = options.output_file.with_extension("o");
        fs::write(&temp_obj_path, &elf_bytes)
            .map_err(|e| format!("Failed to write temporary object file: {}", e))?;

        let link_res = Self::invoke_system_linker(&temp_obj_path, &options.output_file);
        let _ = fs::remove_file(&temp_obj_path);

        match link_res {
            Ok(_) => Ok(Some(elf_bytes)),
            Err(e) => Err(format!("Linking failed: {}", e)),
        }
    }

    fn invoke_system_linker(obj_path: &Path, out_path: &Path) -> Result<(), String> {
        // Prepare temporary runtime C source file for standard I/O intrinsics
        let temp_rt_path = obj_path.with_extension("rt.c");
        fs::write(&temp_rt_path, RUNTIME_C)
            .map_err(|e| format!("Failed to write runtime helper source: {}", e))?;

        // Use gcc as linker driver to link standard runtime startup, libc, and vanta_rt
        let status = Command::new("gcc")
            .arg("-no-pie") // Produce standard executable layout
            .arg(obj_path)
            .arg(&temp_rt_path)
            .arg("-o")
            .arg(out_path)
            .status();

        let _ = fs::remove_file(&temp_rt_path);

        let status = status.map_err(|e| format!("Could not invoke gcc linker: {}", e))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("Linker exited with status {}", status))
        }
    }
}
