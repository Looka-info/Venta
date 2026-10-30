//! Linker errors and structured diagnostics.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkerError {
    IoError {
        path: PathBuf,
        message: String,
    },
    Io {
        path: PathBuf,
        message: String,
    },
    MalformedInput {
        path: PathBuf,
        offset: usize,
        message: String,
    },
    IncompatibleArchitecture {
        path: PathBuf,
        expected: String,
        found: String,
    },
    DuplicateSymbol {
        symbol: String,
        first_file: PathBuf,
        second_file: PathBuf,
    },
    UndefinedSymbol {
        symbol: String,
        referencing_file: PathBuf,
        referencing_section: String,
    },
    UnsupportedRelocation {
        path: PathBuf,
        section: String,
        reloc_type: u32,
        reloc_name: String,
    },
    RelocationOverflow {
        path: PathBuf,
        section: String,
        offset: usize,
        symbol: String,
        value: i64,
        bits: u32,
    },
    MissingEntryPoint {
        symbol: String,
    },
    SectionAlignmentError {
        section: String,
        alignment: usize,
    },
    UnsupportedFeature {
        feature: String,
        details: String,
    },
    General(String),
}

impl fmt::Display for LinkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkerError::IoError { path, message } | LinkerError::Io { path, message } => {
                write!(f, "I/O error reading '{}': {}", path.display(), message)
            }
            LinkerError::MalformedInput {
                path,
                offset,
                message,
            } => {
                write!(
                    f,
                    "Malformed input file '{}' at offset 0x{:x}: {}",
                    path.display(),
                    offset,
                    message
                )
            }
            LinkerError::IncompatibleArchitecture {
                path,
                expected,
                found,
            } => {
                write!(
                    f,
                    "Incompatible object architecture in '{}': expected {}, found {}",
                    path.display(),
                    expected,
                    found
                )
            }
            LinkerError::DuplicateSymbol {
                symbol,
                first_file,
                second_file,
            } => {
                write!(
                    f,
                    "Duplicate strong symbol '{}' defined in both '{}' and '{}'",
                    symbol,
                    first_file.display(),
                    second_file.display()
                )
            }
            LinkerError::UndefinedSymbol {
                symbol,
                referencing_file,
                referencing_section,
            } => {
                write!(
                    f,
                    "Undefined symbol reference '{}' in section '{}' of '{}'",
                    symbol,
                    referencing_section,
                    referencing_file.display()
                )
            }
            LinkerError::UnsupportedRelocation {
                path,
                section,
                reloc_type,
                reloc_name,
            } => {
                write!(
                    f,
                    "Unsupported relocation type {} ({}) in section '{}' of '{}'",
                    reloc_type,
                    reloc_name,
                    section,
                    path.display()
                )
            }
            LinkerError::RelocationOverflow {
                path,
                section,
                offset,
                symbol,
                value,
                bits,
            } => {
                write!(
                    f,
                    "Relocation overflow at offset 0x{:x} in section '{}' of '{}': value 0x{:x} does not fit in {} bits for symbol '{}'",
                    offset,
                    section,
                    path.display(),
                    value,
                    bits,
                    symbol
                )
            }
            LinkerError::MissingEntryPoint { symbol } => {
                write!(
                    f,
                    "Entry point symbol '{}' not found in any input object",
                    symbol
                )
            }
            LinkerError::SectionAlignmentError { section, alignment } => {
                write!(
                    f,
                    "Invalid section alignment {} for section '{}'",
                    alignment, section
                )
            }
            LinkerError::UnsupportedFeature { feature, details } => {
                write!(f, "Unsupported linker feature '{}': {}", feature, details)
            }
            LinkerError::General(msg) => write!(f, "Linker error: {}", msg),
        }
    }
}

impl std::error::Error for LinkerError {}

pub type LinkResult<T> = Result<T, LinkerError>;
