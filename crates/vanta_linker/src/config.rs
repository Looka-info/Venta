//! Configuration for the Venta native linker.

use std::path::PathBuf;
use vanta_target::TargetTriple;

/// Desired output binary type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Executable,
    RelocatableObject,
    SharedLibrary,
}

/// Link mode policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkMode {
    #[default]
    Static,
    Dynamic,
}

/// Complete configuration for a linker session.
#[derive(Debug, Clone)]
pub struct LinkerConfig {
    pub target: TargetTriple,
    pub output_path: PathBuf,
    pub output_format: OutputFormat,
    pub link_mode: LinkMode,
    pub entry_symbol: Option<String>,
    pub input_objects: Vec<PathBuf>,
    pub input_buffers: Vec<(String, Vec<u8>)>,
    pub archives: Vec<PathBuf>,
    pub archive_buffers: Vec<(String, Vec<u8>)>,
    pub library_search_paths: Vec<PathBuf>,
    pub libraries: Vec<String>,
    pub map_file: Option<PathBuf>,
    pub debug_info: bool,
    pub strip_dead: bool,
    pub verbose: bool,
}

impl LinkerConfig {
    pub fn new(target: TargetTriple, output_path: PathBuf) -> Self {
        Self {
            target,
            output_path,
            output_format: OutputFormat::Executable,
            link_mode: LinkMode::Static,
            entry_symbol: None,
            input_objects: Vec::new(),
            input_buffers: Vec::new(),
            archives: Vec::new(),
            archive_buffers: Vec::new(),
            library_search_paths: Vec::new(),
            libraries: Vec::new(),
            map_file: None,
            debug_info: false,
            strip_dead: false,
            verbose: false,
        }
    }

    /// Resolve default entry point symbol for target if not explicitly specified.
    pub fn resolved_entry_symbol(&self) -> &str {
        if let Some(ref e) = self.entry_symbol {
            return e.as_str();
        }
        match self.target {
            TargetTriple::X86_64UnknownLinuxGnu => "_start",
            TargetTriple::X86_64PcWindowsMsvc => "_start",
            TargetTriple::Aarch64AppleDarwin => "_main",
        }
    }
}
