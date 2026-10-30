//! # Venta Pure-Rust Self-Contained Native Linker & Static Archive Suite
//!
//! Provides multi-object linking, static archive creation and inspection, symbol resolution,
//! section merging, relocation application, and executable generation without external
//! linker dependencies (`gcc`, `clang`, `link.exe`, `ld`, `ar`).
//!
//! Supported targets:
//! - `x86_64-unknown-linux-gnu` (ELF64 static executable with self-contained runtime)
//! - `x86_64-pc-windows-msvc` (PE32+ executable with KERNEL32 imports and base relocations)
//! - `aarch64-apple-darwin` (Mach-O 64-bit dyld-compatible executable)

pub mod archive;
pub mod coff;
pub mod config;
pub mod elf;
pub mod error;
pub mod input;
pub mod layout;
pub mod macho;
pub mod map_file;
pub mod object;
pub mod relocation;
pub mod resolve;
pub mod section;
pub mod symbol;

pub use archive::{Archive, ArchiveMember, ArchiveWriter};
pub use config::{LinkMode, LinkerConfig, OutputFormat};
pub use error::{LinkResult, LinkerError};
pub use object::ObjectFile;
pub use relocation::RelocationRecord;
pub use section::{InputSection, MergedSection, SectionFlags, SectionKind};
pub use symbol::{Symbol, SymbolKind, SymbolVisibility};

use std::fs;
use std::path::{Path, PathBuf};
use vanta_target::TargetTriple;

/// Unified result of a successful linking session across any supported target.
#[derive(Debug, Clone)]
pub struct LinkerOutput {
    pub output_path: PathBuf,
    pub entry_point_address: u64,
    pub total_file_size: usize,
    pub input_objects_count: usize,
    pub extracted_archive_members_count: usize,
    pub map_content: Option<String>,
}

/// Dispatches linking according to the target triple configured in `LinkerConfig`.
pub fn link(config: &LinkerConfig) -> LinkResult<LinkerOutput> {
    match config.target {
        TargetTriple::X86_64UnknownLinuxGnu => {
            let res = elf::link_elf_executable(config)?;
            Ok(LinkerOutput {
                output_path: res.output_path,
                entry_point_address: res.entry_point_va,
                total_file_size: res.total_file_size,
                input_objects_count: res.input_objects_count,
                extracted_archive_members_count: res.extracted_archive_members_count,
                map_content: res.map_content,
            })
        }
        TargetTriple::X86_64PcWindowsMsvc => {
            let res = coff::link_coff_executable(config)?;
            Ok(LinkerOutput {
                output_path: res.output_path,
                entry_point_address: res.entry_point_rva as u64,
                total_file_size: res.total_file_size,
                input_objects_count: res.input_objects_count,
                extracted_archive_members_count: res.extracted_archive_members_count,
                map_content: res.map_content,
            })
        }
        TargetTriple::Aarch64AppleDarwin => {
            let res = macho::link_macho_executable(config)?;
            Ok(LinkerOutput {
                output_path: res.output_path,
                entry_point_address: res.entry_point_offset,
                total_file_size: res.total_file_size,
                input_objects_count: res.input_objects_count,
                extracted_archive_members_count: res.extracted_archive_members_count,
                map_content: res.map_content,
            })
        }
    }
}

/// Creates a deterministic static archive (.a or .lib) from the specified input object files.
pub fn create_archive<P: AsRef<Path>>(
    output_path: P,
    member_paths: &[PathBuf],
) -> LinkResult<usize> {
    let out_p = output_path.as_ref();
    let mut writer = ArchiveWriter::new();

    for p in member_paths {
        let data = fs::read(p).map_err(|e| LinkerError::Io {
            path: p.clone(),
            message: format!("Failed to read object for archive: {}", e),
        })?;
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("object.o")
            .to_string();
        writer.add_member(name, data);
    }

    let archive_bytes = writer.write();

    if let Some(parent) = out_p.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| LinkerError::Io {
                path: parent.to_path_buf(),
                message: format!("Failed to create output directory: {}", e),
            })?;
        }
    }

    fs::write(out_p, &archive_bytes).map_err(|e| LinkerError::Io {
        path: out_p.to_path_buf(),
        message: format!("Failed to write archive: {}", e),
    })?;

    Ok(archive_bytes.len())
}

/// Lists all member names stored in a static archive (.a or .lib).
pub fn list_archive<P: AsRef<Path>>(archive_path: P) -> LinkResult<Vec<String>> {
    let p = archive_path.as_ref();
    let data = fs::read(p).map_err(|e| LinkerError::Io {
        path: p.to_path_buf(),
        message: format!("Failed to read archive: {}", e),
    })?;

    let archive = Archive::parse(p, &data)?;
    Ok(archive.members.into_iter().map(|m| m.name).collect())
}

/// Diagnostic inspection details for an archive.
#[derive(Debug, Clone)]
pub struct ArchiveInspection {
    pub path: PathBuf,
    pub total_size: usize,
    pub members_count: usize,
    pub members: Vec<(String, usize)>, // (name, size)
    pub symbols_count: usize,
    pub symbols: Vec<(String, String)>, // (symbol_name, member_name)
}

/// Deeply inspects an archive, returning its members and full symbol index table.
pub fn inspect_archive<P: AsRef<Path>>(archive_path: P) -> LinkResult<ArchiveInspection> {
    let p = archive_path.as_ref().to_path_buf();
    let data = fs::read(&p).map_err(|e| LinkerError::Io {
        path: p.clone(),
        message: format!("Failed to read archive: {}", e),
    })?;

    let archive = Archive::parse(&p, &data)?;

    let mut members_summary = Vec::new();
    for m in &archive.members {
        members_summary.push((m.name.clone(), m.size));
    }

    let mut symbols_summary = Vec::new();
    for (sym, &mem_idx) in &archive.symbol_index {
        let mem_name = archive
            .members
            .get(mem_idx)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| format!("<idx_{}>", mem_idx));
        symbols_summary.push((sym.clone(), mem_name));
    }
    symbols_summary.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(ArchiveInspection {
        path: p,
        total_size: data.len(),
        members_count: archive.members.len(),
        members: members_summary,
        symbols_count: archive.symbol_index.len(),
        symbols: symbols_summary,
    })
}
