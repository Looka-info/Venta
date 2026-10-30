//! Internal parsed object file representation.

use crate::input::InputFormat;
use crate::relocation::RelocationRecord;
use crate::section::InputSection;
use crate::symbol::Symbol;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ObjectFile {
    pub id: usize,
    pub name: String,
    pub path: PathBuf,
    pub format: InputFormat,
    pub sections: Vec<InputSection>,
    pub symbols: Vec<Symbol>,
    pub relocations: Vec<RelocationRecord>,
}

impl ObjectFile {
    pub fn new(id: usize, name: String, path: PathBuf, format: InputFormat) -> Self {
        Self {
            id,
            name,
            path,
            format,
            sections: Vec::new(),
            symbols: Vec::new(),
            relocations: Vec::new(),
        }
    }

    pub fn get_section(&self, index: usize) -> Option<&InputSection> {
        self.sections.iter().find(|s| s.source_index == index)
    }

    pub fn get_symbol(&self, index: usize) -> Option<&Symbol> {
        self.symbols.get(index)
    }
}
