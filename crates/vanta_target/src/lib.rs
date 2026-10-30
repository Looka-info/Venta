//! Target triple and architecture specification model for the Vanta compiler.
//!
//! Provides explicit target modeling distinguishing host and target:
//! - Architecture (`x86_64`, `aarch64`)
//! - Operating System (`linux`, `windows`, `macos`)
//! - Object format (`elf64`, `coff`, `macho64`)
//! - Calling convention / ABI (`system_v_amd64`, `microsoft_x64`, `apple_aarch64`)

use std::fmt;
use std::str::FromStr;

/// Target CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    X86_64,
    AArch64,
}

impl Arch {
    pub const fn name(&self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::AArch64 => "aarch64",
        }
    }
}

/// Operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    Linux,
    Windows,
    MacOS,
}

impl Os {
    pub const fn name(&self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Windows => "windows",
            Os::MacOS => "macos",
        }
    }
}

/// Relocatable object file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectFormat {
    Elf64,
    Coff,
    MachO64,
}

impl ObjectFormat {
    pub const fn extension(&self) -> &'static str {
        match self {
            ObjectFormat::Elf64 => "o",
            ObjectFormat::Coff => "obj",
            ObjectFormat::MachO64 => "o",
        }
    }
}

/// Platform calling convention / ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetAbi {
    SystemVAmd64,
    MicrosoftX64,
    AppleAArch64,
}

/// Full specification for a compilation target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetSpec {
    pub triple: TargetTriple,
    pub arch: Arch,
    pub os: Os,
    pub object_format: ObjectFormat,
    pub abi: TargetAbi,
    pub pointer_width: u8,
    pub is_little_endian: bool,
    pub symbol_prefix: &'static str,
    pub obj_extension: &'static str,
    pub exe_extension: &'static str,
}

/// Officially supported target triples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetTriple {
    /// Linux x86-64 with System V AMD64 ABI and ELF64 objects (default).
    X86_64UnknownLinuxGnu,
    /// Windows x86-64 with Microsoft x64 ABI and PE/COFF objects.
    X86_64PcWindowsMsvc,
    /// Apple Silicon macOS with Apple ARM64 ABI and Mach-O 64 objects.
    Aarch64AppleDarwin,
}

impl Default for TargetTriple {
    fn default() -> Self {
        TargetTriple::X86_64UnknownLinuxGnu
    }
}

impl TargetTriple {
    /// Return the canonical triple string.
    pub const fn as_str(&self) -> &'static str {
        match self {
            TargetTriple::X86_64UnknownLinuxGnu => "x86_64-unknown-linux-gnu",
            TargetTriple::X86_64PcWindowsMsvc => "x86_64-pc-windows-msvc",
            TargetTriple::Aarch64AppleDarwin => "aarch64-apple-darwin",
        }
    }

    /// Return the full target specification.
    pub const fn spec(&self) -> TargetSpec {
        match self {
            TargetTriple::X86_64UnknownLinuxGnu => TargetSpec {
                triple: *self,
                arch: Arch::X86_64,
                os: Os::Linux,
                object_format: ObjectFormat::Elf64,
                abi: TargetAbi::SystemVAmd64,
                pointer_width: 64,
                is_little_endian: true,
                symbol_prefix: "",
                obj_extension: "o",
                exe_extension: "",
            },
            TargetTriple::X86_64PcWindowsMsvc => TargetSpec {
                triple: *self,
                arch: Arch::X86_64,
                os: Os::Windows,
                object_format: ObjectFormat::Coff,
                abi: TargetAbi::MicrosoftX64,
                pointer_width: 64,
                is_little_endian: true,
                symbol_prefix: "",
                obj_extension: "obj",
                exe_extension: "exe",
            },
            TargetTriple::Aarch64AppleDarwin => TargetSpec {
                triple: *self,
                arch: Arch::AArch64,
                os: Os::MacOS,
                object_format: ObjectFormat::MachO64,
                abi: TargetAbi::AppleAArch64,
                pointer_width: 64,
                is_little_endian: true,
                symbol_prefix: "_",
                obj_extension: "o",
                exe_extension: "",
            },
        }
    }

    /// List all supported target triple strings.
    pub const fn supported_triples() -> &'static [&'static str] {
        &[
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-apple-darwin",
        ]
    }
}

impl fmt::Display for TargetTriple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetParseError {
    pub input: String,
}

impl fmt::Display for TargetParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Unsupported target triple '{}'. Supported targets: {}",
            self.input,
            TargetTriple::supported_triples().join(", ")
        )
    }
}

impl std::error::Error for TargetParseError {}

impl FromStr for TargetTriple {
    type Err = TargetParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "x86_64-unknown-linux-gnu" | "x86_64-linux-gnu" | "linux" => {
                Ok(TargetTriple::X86_64UnknownLinuxGnu)
            }
            "x86_64-pc-windows-msvc" | "x86_64-windows-msvc" | "windows" | "win64" => {
                Ok(TargetTriple::X86_64PcWindowsMsvc)
            }
            "aarch64-apple-darwin"
            | "arm64-apple-darwin"
            | "macos"
            | "darwin"
            | "apple-silicon" => Ok(TargetTriple::Aarch64AppleDarwin),
            other => Err(TargetParseError {
                input: other.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_parsing_valid() {
        assert_eq!(
            "x86_64-unknown-linux-gnu".parse::<TargetTriple>().unwrap(),
            TargetTriple::X86_64UnknownLinuxGnu
        );
        assert_eq!(
            "x86_64-pc-windows-msvc".parse::<TargetTriple>().unwrap(),
            TargetTriple::X86_64PcWindowsMsvc
        );
        assert_eq!(
            "aarch64-apple-darwin".parse::<TargetTriple>().unwrap(),
            TargetTriple::Aarch64AppleDarwin
        );
    }

    #[test]
    fn test_target_parsing_invalid() {
        assert!("x86_64-pc-windows-gnu".parse::<TargetTriple>().is_err());
        assert!("armv7-unknown-linux-gnueabihf"
            .parse::<TargetTriple>()
            .is_err());
    }

    #[test]
    fn test_target_specs() {
        let linux = TargetTriple::X86_64UnknownLinuxGnu.spec();
        assert_eq!(linux.arch, Arch::X86_64);
        assert_eq!(linux.object_format, ObjectFormat::Elf64);
        assert_eq!(linux.abi, TargetAbi::SystemVAmd64);

        let win = TargetTriple::X86_64PcWindowsMsvc.spec();
        assert_eq!(win.arch, Arch::X86_64);
        assert_eq!(win.object_format, ObjectFormat::Coff);
        assert_eq!(win.abi, TargetAbi::MicrosoftX64);
        assert_eq!(win.obj_extension, "obj");
        assert_eq!(win.exe_extension, "exe");

        let mac = TargetTriple::Aarch64AppleDarwin.spec();
        assert_eq!(mac.arch, Arch::AArch64);
        assert_eq!(mac.object_format, ObjectFormat::MachO64);
        assert_eq!(mac.abi, TargetAbi::AppleAArch64);
        assert_eq!(mac.symbol_prefix, "_");
    }
}
