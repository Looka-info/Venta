//! Segment mapping and memory layout calculator.

use crate::section::{MergedSection, SectionKind};

#[derive(Debug, Clone)]
pub struct SegmentLayout {
    pub base_virtual_address: u64,
    pub page_size: u64,
    pub file_alignment: u64,
    pub total_memory_size: u64,
    pub total_file_size: u64,
}

impl Default for SegmentLayout {
    fn default() -> Self {
        Self {
            base_virtual_address: 0x400000,
            page_size: 4096,
            file_alignment: 4096,
            total_memory_size: 0,
            total_file_size: 0,
        }
    }
}

impl SegmentLayout {
    pub fn new(base_va: u64, page_size: u64, file_alignment: u64) -> Self {
        Self {
            base_virtual_address: base_va,
            page_size,
            file_alignment,
            total_memory_size: 0,
            total_file_size: 0,
        }
    }

    /// Computes virtual addresses and file offsets for each merged section.
    pub fn compute_layout(
        &mut self,
        merged_sections: &mut [MergedSection],
        initial_file_offset: u64,
    ) {
        let mut curr_va = self.base_virtual_address + (initial_file_offset % self.page_size);
        let mut curr_file_offset = initial_file_offset;
        let mut last_kind = None;

        for section in merged_sections.iter_mut() {
            let align = (section.align as u64).max(1);

            // Separate code, read-only data, and writable data onto page boundaries
            if let Some(prev) = last_kind {
                if should_page_align(prev, section.kind) {
                    curr_va = (curr_va + self.page_size - 1) & !(self.page_size - 1);
                    curr_file_offset =
                        (curr_file_offset + self.file_alignment - 1) & !(self.file_alignment - 1);
                }
            }
            last_kind = Some(section.kind);

            // Apply section intra-page alignment
            curr_va = (curr_va + align - 1) & !(align - 1);
            if !section.kind.is_zero_init() {
                curr_file_offset = (curr_file_offset + align - 1) & !(align - 1);
            }

            section.virtual_address = curr_va;
            section.file_offset = if section.kind.is_zero_init() {
                0
            } else {
                curr_file_offset
            };

            let sec_size = section.size as u64;
            curr_va += sec_size;
            if !section.kind.is_zero_init() {
                curr_file_offset += sec_size;
            }
        }

        self.total_memory_size = curr_va - self.base_virtual_address;
        self.total_file_size = curr_file_offset;
    }
}

fn should_page_align(a: SectionKind, b: SectionKind) -> bool {
    let perm_a = (a.is_executable(), a.is_writable());
    let perm_b = (b.is_executable(), b.is_writable());
    perm_a != perm_b
}
