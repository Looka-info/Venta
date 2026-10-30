//! Section representation and merging model.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    Code,
    ReadOnlyData,
    Data,
    Bss,
    UnwindPdata,
    UnwindXdata,
    DebugInfo,
    DebugAbbrev,
    DebugLine,
    Other,
}

impl SectionKind {
    pub fn is_allocatable(&self) -> bool {
        match self {
            SectionKind::Code
            | SectionKind::ReadOnlyData
            | SectionKind::Data
            | SectionKind::Bss
            | SectionKind::UnwindPdata
            | SectionKind::UnwindXdata => true,
            SectionKind::DebugInfo
            | SectionKind::DebugAbbrev
            | SectionKind::DebugLine
            | SectionKind::Other => false,
        }
    }

    pub fn is_executable(&self) -> bool {
        matches!(self, SectionKind::Code)
    }

    pub fn is_writable(&self) -> bool {
        matches!(self, SectionKind::Data | SectionKind::Bss)
    }

    pub fn is_zero_init(&self) -> bool {
        matches!(self, SectionKind::Bss)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionFlags {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub alloc: bool,
}

impl SectionFlags {
    pub fn code() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
            alloc: true,
        }
    }

    pub fn readonly() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
            alloc: true,
        }
    }

    pub fn readwrite() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
            alloc: true,
        }
    }

    pub fn debug() -> Self {
        Self {
            read: true,
            write: false,
            execute: false,
            alloc: false,
        }
    }
}

/// A section originating from an input relocatable object file.
#[derive(Debug, Clone)]
pub struct InputSection {
    pub object_id: usize,
    pub source_index: usize,
    pub name: String,
    pub kind: SectionKind,
    pub flags: SectionFlags,
    pub align: usize,
    pub data: Vec<u8>,
    pub size: usize, // can be > data.len() for BSS
}

/// A merged output section combining contributions from multiple input objects.
#[derive(Debug, Clone)]
pub struct MergedSection {
    pub name: String,
    pub kind: SectionKind,
    pub flags: SectionFlags,
    pub align: usize,
    pub virtual_address: u64,
    pub file_offset: u64,
    pub size: usize,
    pub data: Vec<u8>,
    pub contributions: Vec<SectionContribution>,
}

#[derive(Debug, Clone)]
pub struct SectionContribution {
    pub object_id: usize,
    pub source_section_idx: usize,
    pub source_section_name: String,
    pub offset_in_merged: usize,
    pub size: usize,
    pub align: usize,
}

impl MergedSection {
    pub fn new(name: String, kind: SectionKind, flags: SectionFlags) -> Self {
        Self {
            name,
            kind,
            flags,
            align: 16,
            virtual_address: 0,
            file_offset: 0,
            size: 0,
            data: Vec::new(),
            contributions: Vec::new(),
        }
    }

    /// Appends an input section contribution with alignment padding.
    pub fn append_contribution(&mut self, input: &InputSection) -> usize {
        let align = input.align.max(1);
        self.align = self.align.max(align);

        // Align current data
        let padding = (align - (self.data.len() % align)) % align;
        if padding > 0 && !input.kind.is_zero_init() {
            self.data.resize(self.data.len() + padding, 0);
        }

        let offset = self.data.len();
        if input.kind.is_zero_init() {
            self.size += padding + input.size;
        } else {
            self.data.extend_from_slice(&input.data);
            self.size = self.data.len();
        }

        self.contributions.push(SectionContribution {
            object_id: input.object_id,
            source_section_idx: input.source_index,
            source_section_name: input.name.clone(),
            offset_in_merged: offset,
            size: input.size,
            align,
        });

        offset
    }
}
