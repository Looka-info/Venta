//! Symbol definitions, visibility, and global symbol table.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolVisibility {
    Local,
    Global,
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Data,
    Object,
    Section,
    Notype,
    Other,
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub visibility: SymbolVisibility,
    pub kind: SymbolKind,
    pub section_index: Option<usize>, // None if undefined
    pub value: u64,                   // offset in input section
    pub size: u64,
    pub object_id: usize,
    pub final_virtual_address: Option<u64>,
}

impl Symbol {
    pub fn is_defined(&self) -> bool {
        self.section_index.is_some()
    }

    pub fn is_undefined(&self) -> bool {
        self.section_index.is_none()
    }

    pub fn is_local(&self) -> bool {
        self.visibility == SymbolVisibility::Local
    }

    pub fn is_global(&self) -> bool {
        self.visibility == SymbolVisibility::Global
    }

    pub fn is_weak(&self) -> bool {
        self.visibility == SymbolVisibility::Weak
    }
}
