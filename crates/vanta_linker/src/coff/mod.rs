//! Windows AMD64 PE/COFF format reader, relocations, PE32+ writer, and linker.

pub mod linker;
pub mod pe_writer;
pub mod reader;
pub mod reloc;

pub use linker::{link_coff_executable, CoffLinkOutput};
