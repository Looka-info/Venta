//! Apple Silicon ARM64 Mach-O format reader, relocations, executable writer, and linker.

pub mod linker;
pub mod reader;
pub mod reloc;
pub mod writer;

pub use linker::{link_macho_executable, MachOLinkOutput};
