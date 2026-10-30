//! Architecture-independent relocation representation.

#[derive(Debug, Clone)]
pub struct RelocationRecord {
    pub object_id: usize,
    pub section_index: usize,
    pub offset_in_section: usize,
    pub symbol_name: String,
    pub symbol_index: usize,
    pub reloc_type: u32,
    pub addend: i64,
}

impl RelocationRecord {
    pub fn new(
        object_id: usize,
        section_index: usize,
        offset_in_section: usize,
        symbol_name: String,
        symbol_index: usize,
        reloc_type: u32,
        addend: i64,
    ) -> Self {
        Self {
            object_id,
            section_index,
            offset_in_section,
            symbol_name,
            symbol_index,
            reloc_type,
            addend,
        }
    }

    pub fn is_local(&self) -> bool {
        self.symbol_name.is_empty()
    }
}
