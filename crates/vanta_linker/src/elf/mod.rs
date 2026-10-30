//! Linux x86-64 ELF64 format reader, relocations, runtime, and static linker.

pub mod linker;
pub mod reader;
pub mod reloc;
pub mod runtime;
pub mod writer;

pub use linker::{link_elf_executable, ElfLinkOutput};
