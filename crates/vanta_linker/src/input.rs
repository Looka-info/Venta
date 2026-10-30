//! Input file inspection and format identification.

use crate::error::{LinkResult, LinkerError};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Elf64,
    CoffAmd64,
    MachO64,
    ArchiveUnix,
    ArchiveCoff,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct InputSource {
    pub name: String,
    pub path: PathBuf,
    pub data: Vec<u8>,
    pub format: InputFormat,
}

impl InputSource {
    pub fn from_file<P: AsRef<Path>>(path: P) -> LinkResult<Self> {
        let p = path.as_ref().to_path_buf();
        let data = fs::read(&p).map_err(|e| LinkerError::IoError {
            path: p.clone(),
            message: e.to_string(),
        })?;
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("input")
            .to_string();
        let format = detect_format(&data);
        Ok(Self {
            name,
            path: p,
            data,
            format,
        })
    }

    pub fn from_bytes(name: String, data: Vec<u8>) -> Self {
        let format = detect_format(&data);
        Self {
            name: name.clone(),
            path: PathBuf::from(&name),
            data,
            format,
        }
    }
}

pub fn detect_format(data: &[u8]) -> InputFormat {
    if data.len() < 4 {
        return InputFormat::Unknown;
    }

    if data.starts_with(b"!<arch>\n") {
        // Distinguish Windows COFF archive from Unix ar:
        // In COFF archives, the first member is usually named "/" (linker member)
        if data.len() >= 68 && &data[8..10] == b"/\x20" {
            return InputFormat::ArchiveCoff;
        }
        return InputFormat::ArchiveUnix;
    }

    if data.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return InputFormat::Elf64;
    }

    // Mach-O 64-bit Little Endian (0xFEEDFACF)
    if data.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) {
        return InputFormat::MachO64;
    }

    // PE/COFF AMD64 machine (0x8664)
    if data.len() >= 20 && data[0] == 0x64 && data[1] == 0x86 {
        return InputFormat::CoffAmd64;
    }

    InputFormat::Unknown
}
