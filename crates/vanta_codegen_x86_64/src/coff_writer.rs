//! Pure-Rust Microsoft PE/COFF 64-bit Relocatable Object File (.obj) Writer.
//!
//! Generates valid COFF relocatable object files (`IMAGE_FILE_MACHINE_AMD64 = 0x8664`)
//! conforming to the Microsoft Portable Executable and Common Object File Format (PE/COFF)
//! specification, including:
//! - COFF File Header (`IMAGE_FILE_HEADER`)
//! - Section Headers for `.text`, `.rdata`, `.pdata`, and `.xdata`
//! - Unwind information records (`IMAGE_RUNTIME_FUNCTION_ENTRY` and `UNWIND_INFO`)
//! - Relocations (`IMAGE_REL_AMD64_REL32`, `IMAGE_REL_AMD64_ADDR32NB`, `IMAGE_REL_AMD64_ADDR64`)
//! - 18-byte COFF symbol table entries with long-name string table support

use crate::elf_writer::{FunctionSymbol, RodataSymbol};
use crate::encoding::Relocation;
use std::collections::HashMap;

pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;

// Section characteristics flags
pub const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
pub const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
pub const IMAGE_SCN_ALIGN_4BYTES: u32 = 0x0030_0000;
pub const IMAGE_SCN_ALIGN_16BYTES: u32 = 0x0050_0000;

// Symbol classes and types
pub const IMAGE_SYM_CLASS_EXTERNAL: u8 = 2;
pub const IMAGE_SYM_CLASS_STATIC: u8 = 3;
pub const IMAGE_SYM_DTYPE_FUNCTION: u16 = 0x20;

// AMD64 Relocation types
pub const IMAGE_REL_AMD64_ADDR64: u16 = 1;
pub const IMAGE_REL_AMD64_ADDR32NB: u16 = 3;
pub const IMAGE_REL_AMD64_REL32: u16 = 4;

#[derive(Debug, Clone)]
pub struct CoffRelocation {
    pub virtual_address: u32,
    pub symbol_table_index: u32,
    pub reloc_type: u16,
}

#[derive(Debug, Clone)]
pub struct CoffSymbol {
    pub name: String,
    pub value: u32,
    pub section_number: i16,
    pub symbol_type: u16,
    pub storage_class: u8,
}

pub struct CoffWriter {
    text: Vec<u8>,
    rdata: Vec<u8>,
    text_relocations: Vec<Relocation>,
    rdata_relocations: Vec<Relocation>,
    functions: Vec<FunctionSymbol>,
    rdata_symbols: Vec<RodataSymbol>,
}

impl CoffWriter {
    pub fn new(
        text: Vec<u8>,
        rdata: Vec<u8>,
        text_relocations: Vec<Relocation>,
        functions: Vec<FunctionSymbol>,
        rdata_symbols: Vec<RodataSymbol>,
    ) -> Self {
        Self {
            text,
            rdata,
            text_relocations,
            rdata_relocations: Vec::new(),
            functions,
            rdata_symbols,
        }
    }

    pub fn with_rdata_relocations(mut self, relocs: Vec<Relocation>) -> Self {
        self.rdata_relocations = relocs;
        self
    }

    /// Emits a valid COFF 64-bit object file (.obj) byte buffer.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();

        // 1. Build .pdata and .xdata unwind sections
        let (pdata_bytes, pdata_relocs, xdata_bytes) = self.build_unwind_tables();

        // 2. Build symbols and map each symbol to its 0-based index in the COFF symbol table
        let mut symbols: Vec<CoffSymbol> = Vec::new();
        let mut symbol_indices: HashMap<String, u32> = HashMap::new();

        // Section indices in COFF are 1-based:
        // 1: .text
        // 2: .rdata
        // 3: .pdata
        // 4: .xdata
        let sec_text_idx = 1i16;
        let sec_rdata_idx = 2i16;
        let sec_pdata_idx = 3i16;
        let sec_xdata_idx = 4i16;

        // Push section symbols first
        symbols.push(CoffSymbol {
            name: ".text".into(),
            value: 0,
            section_number: sec_text_idx,
            symbol_type: 0,
            storage_class: IMAGE_SYM_CLASS_STATIC,
        });
        symbol_indices.insert(".text".into(), 0);

        symbols.push(CoffSymbol {
            name: ".rdata".into(),
            value: 0,
            section_number: sec_rdata_idx,
            symbol_type: 0,
            storage_class: IMAGE_SYM_CLASS_STATIC,
        });
        symbol_indices.insert(".rdata".into(), 1);

        symbols.push(CoffSymbol {
            name: ".pdata".into(),
            value: 0,
            section_number: sec_pdata_idx,
            symbol_type: 0,
            storage_class: IMAGE_SYM_CLASS_STATIC,
        });
        symbol_indices.insert(".pdata".into(), 2);

        symbols.push(CoffSymbol {
            name: ".xdata".into(),
            value: 0,
            section_number: sec_xdata_idx,
            symbol_type: 0,
            storage_class: IMAGE_SYM_CLASS_STATIC,
        });
        symbol_indices.insert(".xdata".into(), 3);

        // Defined functions in .text
        for func in &self.functions {
            let idx = symbols.len() as u32;
            symbols.push(CoffSymbol {
                name: func.name.clone(),
                value: func.offset as u32,
                section_number: sec_text_idx,
                symbol_type: IMAGE_SYM_DTYPE_FUNCTION,
                storage_class: IMAGE_SYM_CLASS_EXTERNAL,
            });
            symbol_indices.insert(func.name.clone(), idx);
        }

        // Defined rdata symbols
        for rsym in &self.rdata_symbols {
            let idx = symbols.len() as u32;
            symbols.push(CoffSymbol {
                name: rsym.name.clone(),
                value: rsym.offset as u32,
                section_number: sec_rdata_idx,
                symbol_type: 0,
                storage_class: IMAGE_SYM_CLASS_STATIC,
            });
            symbol_indices.insert(rsym.name.clone(), idx);
        }

        // Undefined / external symbols referenced by relocations
        for r in self.text_relocations.iter().chain(&self.rdata_relocations) {
            if !symbol_indices.contains_key(&r.symbol) {
                let idx = symbols.len() as u32;
                symbols.push(CoffSymbol {
                    name: r.symbol.clone(),
                    value: 0,
                    section_number: 0, // IMAGE_SYM_UNDEFINED
                    symbol_type: IMAGE_SYM_DTYPE_FUNCTION,
                    storage_class: IMAGE_SYM_CLASS_EXTERNAL,
                });
                symbol_indices.insert(r.symbol.clone(), idx);
            }
        }

        // 3. Convert text relocations to COFF format
        let mut coff_text_relocs: Vec<CoffRelocation> = Vec::new();
        for r in &self.text_relocations {
            let sym_idx = *symbol_indices
                .get(&r.symbol)
                .expect("all referenced symbols mapped");
            let r_type = match r.r_type {
                1 => IMAGE_REL_AMD64_ADDR64,
                2 | 4 => IMAGE_REL_AMD64_REL32,
                _ => IMAGE_REL_AMD64_REL32,
            };
            coff_text_relocs.push(CoffRelocation {
                virtual_address: r.offset as u32,
                symbol_table_index: sym_idx,
                reloc_type: r_type,
            });
        }

        // 4. Convert rdata relocations to COFF format
        let mut coff_rdata_relocs: Vec<CoffRelocation> = Vec::new();
        for r in &self.rdata_relocations {
            let sym_idx = *symbol_indices
                .get(&r.symbol)
                .expect("all referenced symbols mapped");
            let r_type = match r.r_type {
                1 => IMAGE_REL_AMD64_ADDR64,
                _ => IMAGE_REL_AMD64_ADDR64,
            };
            coff_rdata_relocs.push(CoffRelocation {
                virtual_address: r.offset as u32,
                symbol_table_index: sym_idx,
                reloc_type: r_type,
            });
        }

        // 5. Layout file headers and sections
        let num_sections = 4u16;
        let header_size = 20usize;
        let section_headers_size = 40usize * (num_sections as usize);
        let raw_data_start = header_size + section_headers_size;

        let text_raw_size = (self.text.len() + 15) & !15;
        let rdata_raw_size = (self.rdata.len() + 15) & !15;
        let pdata_raw_size = (pdata_bytes.len() + 15) & !15;
        let xdata_raw_size = (xdata_bytes.len() + 15) & !15;

        let text_data_ptr = raw_data_start as u32;
        let rdata_data_ptr = text_data_ptr + text_raw_size as u32;
        let pdata_data_ptr = rdata_data_ptr + rdata_raw_size as u32;
        let xdata_data_ptr = pdata_data_ptr + pdata_raw_size as u32;

        let text_relocs_ptr = xdata_data_ptr + xdata_raw_size as u32;
        let text_relocs_size = (coff_text_relocs.len() * 10) as u32;

        let rdata_relocs_ptr = text_relocs_ptr + text_relocs_size;
        let rdata_relocs_size = (coff_rdata_relocs.len() * 10) as u32;

        let pdata_relocs_ptr = rdata_relocs_ptr + rdata_relocs_size;
        let pdata_relocs_size = (pdata_relocs.len() * 10) as u32;

        let sym_table_ptr = pdata_relocs_ptr + pdata_relocs_size;

        // 6. Build String Table
        let mut string_table = Vec::new();
        string_table.extend_from_slice(&0u32.to_le_bytes()); // placeholder for 4-byte size

        // 7. Write COFF File Header (20 bytes)
        out.extend_from_slice(&IMAGE_FILE_MACHINE_AMD64.to_le_bytes()); // Machine
        out.extend_from_slice(&num_sections.to_le_bytes()); // NumberOfSections
        out.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
        out.extend_from_slice(&sym_table_ptr.to_le_bytes()); // PointerToSymbolTable
        out.extend_from_slice(&(symbols.len() as u32).to_le_bytes()); // NumberOfSymbols
        out.extend_from_slice(&0u16.to_le_bytes()); // SizeOfOptionalHeader
        out.extend_from_slice(&0u16.to_le_bytes()); // Characteristics

        // 8. Write Section Headers (40 bytes each)
        // Section 1: .text
        Self::write_section_header(
            &mut out,
            b".text\0\0\0",
            text_raw_size as u32,
            text_data_ptr,
            if coff_text_relocs.is_empty() {
                0
            } else {
                text_relocs_ptr
            },
            coff_text_relocs.len() as u16,
            IMAGE_SCN_CNT_CODE
                | IMAGE_SCN_MEM_EXECUTE
                | IMAGE_SCN_MEM_READ
                | IMAGE_SCN_ALIGN_16BYTES,
        );

        // Section 2: .rdata
        Self::write_section_header(
            &mut out,
            b".rdata\0\0",
            rdata_raw_size as u32,
            rdata_data_ptr,
            if coff_rdata_relocs.is_empty() {
                0
            } else {
                rdata_relocs_ptr
            },
            coff_rdata_relocs.len() as u16,
            IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_ALIGN_16BYTES,
        );

        // Section 3: .pdata
        Self::write_section_header(
            &mut out,
            b".pdata\0\0",
            pdata_raw_size as u32,
            pdata_data_ptr,
            if pdata_relocs.is_empty() {
                0
            } else {
                pdata_relocs_ptr
            },
            pdata_relocs.len() as u16,
            IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_ALIGN_4BYTES,
        );

        // Section 4: .xdata
        Self::write_section_header(
            &mut out,
            b".xdata\0\0",
            xdata_raw_size as u32,
            xdata_data_ptr,
            0,
            0,
            IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_ALIGN_4BYTES,
        );

        // 9. Write Section Raw Data
        // .text
        out.extend_from_slice(&self.text);
        while out.len() < (text_data_ptr as usize + text_raw_size) {
            out.push(0x90); // nop padding
        }

        // .rdata
        out.extend_from_slice(&self.rdata);
        while out.len() < (rdata_data_ptr as usize + rdata_raw_size) {
            out.push(0);
        }

        // .pdata
        out.extend_from_slice(&pdata_bytes);
        while out.len() < (pdata_data_ptr as usize + pdata_raw_size) {
            out.push(0);
        }

        // .xdata
        out.extend_from_slice(&xdata_bytes);
        while out.len() < (xdata_data_ptr as usize + xdata_raw_size) {
            out.push(0);
        }

        // 10. Write Relocations
        // .text relocations
        for r in &coff_text_relocs {
            out.extend_from_slice(&r.virtual_address.to_le_bytes());
            out.extend_from_slice(&r.symbol_table_index.to_le_bytes());
            out.extend_from_slice(&r.reloc_type.to_le_bytes());
        }

        // .rdata relocations
        for r in &coff_rdata_relocs {
            out.extend_from_slice(&r.virtual_address.to_le_bytes());
            out.extend_from_slice(&r.symbol_table_index.to_le_bytes());
            out.extend_from_slice(&r.reloc_type.to_le_bytes());
        }

        // .pdata relocations
        for r in &pdata_relocs {
            out.extend_from_slice(&r.virtual_address.to_le_bytes());
            out.extend_from_slice(&r.symbol_table_index.to_le_bytes());
            out.extend_from_slice(&r.reloc_type.to_le_bytes());
        }

        // 11. Write Symbol Table (18 bytes per symbol)
        for sym in &symbols {
            let name_bytes = sym.name.as_bytes();
            if name_bytes.len() <= 8 {
                let mut name_field = [0u8; 8];
                name_field[..name_bytes.len()].copy_from_slice(name_bytes);
                out.extend_from_slice(&name_field);
            } else {
                let str_offset = string_table.len() as u32;
                string_table.extend_from_slice(name_bytes);
                string_table.push(0); // NUL terminator
                out.extend_from_slice(&0u32.to_le_bytes()); // zeroes
                out.extend_from_slice(&str_offset.to_le_bytes());
            }

            out.extend_from_slice(&sym.value.to_le_bytes());
            out.extend_from_slice(&sym.section_number.to_le_bytes());
            out.extend_from_slice(&sym.symbol_type.to_le_bytes());
            out.push(sym.storage_class);
            out.push(0); // NumberOfAuxSymbols
        }

        // 12. Write String Table
        let str_table_size = string_table.len() as u32;
        string_table[0..4].copy_from_slice(&str_table_size.to_le_bytes());
        out.extend_from_slice(&string_table);

        out
    }

    fn write_section_header(
        out: &mut Vec<u8>,
        name: &[u8; 8],
        raw_size: u32,
        raw_ptr: u32,
        reloc_ptr: u32,
        num_relocs: u16,
        characteristics: u32,
    ) {
        out.extend_from_slice(name); // Name (8 bytes)
        out.extend_from_slice(&0u32.to_le_bytes()); // VirtualSize
        out.extend_from_slice(&0u32.to_le_bytes()); // VirtualAddress
        out.extend_from_slice(&raw_size.to_le_bytes()); // SizeOfRawData
        out.extend_from_slice(&raw_ptr.to_le_bytes()); // PointerToRawData
        out.extend_from_slice(&reloc_ptr.to_le_bytes()); // PointerToRelocations
        out.extend_from_slice(&0u32.to_le_bytes()); // PointerToLinenumbers
        out.extend_from_slice(&num_relocs.to_le_bytes()); // NumberOfRelocations
        out.extend_from_slice(&0u16.to_le_bytes()); // NumberOfLinenumbers
        out.extend_from_slice(&characteristics.to_le_bytes()); // Characteristics
    }

    /// Builds the .pdata and .xdata sections containing 64-bit exception handling and unwind tables.
    fn build_unwind_tables(&self) -> (Vec<u8>, Vec<CoffRelocation>, Vec<u8>) {
        let mut pdata_bytes = Vec::new();
        let mut pdata_relocs = Vec::new();
        let mut xdata_bytes = Vec::new();

        let sym_text_idx = 0u32; // index of .text section symbol in symbol table
        let sym_xdata_idx = 3u32; // index of .xdata section symbol in symbol table

        for func in &self.functions {
            let xdata_offset = xdata_bytes.len() as u32;

            // UNWIND_INFO (Version 1, no exception handler):
            // byte 0: Version (3 bits = 1) | Flags (5 bits = 0) = 0x01
            // byte 1: SizeOfProlog = 8 bytes (push rbp [1], mov rbp, rsp [3], sub rsp, ... [4])
            // byte 2: CountOfCodes = 3
            // byte 3: FrameRegister = 5 (RBP), FrameOffset = 0 -> 0x05
            xdata_bytes.push(0x01);
            xdata_bytes.push(0x08);
            xdata_bytes.push(0x03);
            xdata_bytes.push(0x05);

            // Unwind code slots in reverse order of execution:
            // Slot 1: mov rbp, rsp -> UWOP_SET_FPREG (3), offset 4
            // code_offset = 4, op = 3, info = 0
            xdata_bytes.extend_from_slice(&[0x04, 0x03]);

            // Slot 2: push rbp -> UWOP_PUSH_NONVOL (0), reg = 5 (RBP), offset 1
            // code_offset = 1, op = 0, info = 5 (0x50)
            xdata_bytes.extend_from_slice(&[0x01, 0x50]);

            // Slot 3: sub rsp -> UWOP_ALLOC_SMALL (2), info = 0
            xdata_bytes.extend_from_slice(&[0x08, 0x02]);

            // Align UNWIND_INFO record to 4 bytes:
            // Header (4) + 3 codes * 2 = 10 bytes -> pad 2 bytes to 12
            xdata_bytes.push(0);
            xdata_bytes.push(0);

            // IMAGE_RUNTIME_FUNCTION_ENTRY in .pdata (12 bytes):
            let pdata_entry_offset = pdata_bytes.len() as u32;

            // BeginAddress (offset in .text)
            pdata_relocs.push(CoffRelocation {
                virtual_address: pdata_entry_offset,
                symbol_table_index: sym_text_idx,
                reloc_type: IMAGE_REL_AMD64_ADDR32NB,
            });
            pdata_bytes.extend_from_slice(&(func.offset as u32).to_le_bytes());

            // EndAddress (offset + size in .text)
            pdata_relocs.push(CoffRelocation {
                virtual_address: pdata_entry_offset + 4,
                symbol_table_index: sym_text_idx,
                reloc_type: IMAGE_REL_AMD64_ADDR32NB,
            });
            pdata_bytes.extend_from_slice(&((func.offset + func.size) as u32).to_le_bytes());

            // UnwindInfoAddress (offset in .xdata)
            pdata_relocs.push(CoffRelocation {
                virtual_address: pdata_entry_offset + 8,
                symbol_table_index: sym_xdata_idx,
                reloc_type: IMAGE_REL_AMD64_ADDR32NB,
            });
            pdata_bytes.extend_from_slice(&xdata_offset.to_le_bytes());
        }

        (pdata_bytes, pdata_relocs, xdata_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coff_writer_magic_and_structure() {
        let text = vec![0x90, 0xC3]; // nop, ret
        let rdata = b"hello\0".to_vec();
        let funcs = vec![FunctionSymbol {
            name: "test_fn".into(),
            offset: 0,
            size: 2,
            is_global: true,
        }];
        let rsyms = vec![RodataSymbol {
            name: ".str0".into(),
            offset: 0,
            size: 6,
        }];

        let writer = CoffWriter::new(text, rdata, vec![], funcs, rsyms);
        let bytes = writer.write();

        // 1. Machine AMD64 (0x8664)
        assert!(bytes.len() >= 20);
        assert_eq!(
            u16::from_le_bytes([bytes[0], bytes[1]]),
            IMAGE_FILE_MACHINE_AMD64
        );

        // 2. 4 sections (.text, .rdata, .pdata, .xdata)
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 4);

        // 3. Pointer to symbol table > 0
        let sym_ptr = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        assert!(sym_ptr > 20 + 4 * 40);

        // 4. Section 1 is .text
        assert_eq!(&bytes[20..26], b".text\0");

        // 5. Section 2 is .rdata
        assert_eq!(&bytes[60..67], b".rdata\0");

        // 6. Section 3 is .pdata
        assert_eq!(&bytes[100..107], b".pdata\0");

        // 7. Section 4 is .xdata
        assert_eq!(&bytes[140..147], b".xdata\0");
    }
}
