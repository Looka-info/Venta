//! Native DWARF 5 debug information emission for source-level debugging in GDB and LLDB.
//!
//! Follows the DWARF 5 specification:
//! - DWARF 5 compilation unit header with `DW_UT_compile` (0x01)
//! - DWARF 5 line table header with directory/file format descriptors
//! - Line number program with `DW_LNE_set_address`, `DW_LNS_advance_pc`, `DW_LNS_advance_line`, `DW_LNS_copy`
//! - Subprogram DIEs with `DW_AT_low_pc`, `DW_AT_high_pc`, `DW_AT_frame_base = [DW_OP_reg6]` (%rbp)
//! - Formal parameter and local variable DIEs with `DW_AT_location = [DW_OP_fbreg, -offset]`
//! - User-defined language code `DW_LANG_Venta = 0x8001` (standard user-defined range 0x8000..0xffff)

use crate::encoding::LineEntry;
use std::collections::HashMap;

// DWARF 5 Tags
pub const DW_TAG_FORMAL_PARAMETER: u16 = 0x05;
pub const DW_TAG_POINTER_TYPE: u16 = 0x0f;
pub const DW_TAG_COMPILE_UNIT: u16 = 0x11;
pub const DW_TAG_BASE_TYPE: u16 = 0x24;
pub const DW_TAG_SUBPROGRAM: u16 = 0x2e;
pub const DW_TAG_VARIABLE: u16 = 0x34;

// Children determination
pub const DW_CHILDREN_NO: u8 = 0x00;
pub const DW_CHILDREN_YES: u8 = 0x01;

// DWARF 5 Attributes
pub const DW_AT_LOCATION: u16 = 0x02;
pub const DW_AT_NAME: u16 = 0x03;
pub const DW_AT_BYTE_SIZE: u16 = 0x0b;
pub const DW_AT_STMT_LIST: u16 = 0x10;
pub const DW_AT_LOW_PC: u16 = 0x11;
pub const DW_AT_HIGH_PC: u16 = 0x12;
pub const DW_AT_LANGUAGE: u16 = 0x13;
pub const DW_AT_COMP_DIR: u16 = 0x1b;
pub const DW_AT_PRODUCER: u16 = 0x25;
pub const DW_AT_DECL_COLUMN: u16 = 0x39;
pub const DW_AT_DECL_FILE: u16 = 0x3a;
pub const DW_AT_DECL_LINE: u16 = 0x3b;
pub const DW_AT_FRAME_BASE: u16 = 0x40;
pub const DW_AT_TYPE: u16 = 0x49;
pub const DW_AT_EXTERNAL: u16 = 0x3f;
pub const DW_AT_ENCODING: u16 = 0x3e;

// DWARF 5 Attribute Forms
pub const DW_FORM_ADDR: u16 = 0x01;
pub const DW_FORM_DATA2: u16 = 0x05;
pub const DW_FORM_DATA4: u16 = 0x06;
pub const DW_FORM_STRING: u16 = 0x08;
pub const DW_FORM_DATA1: u16 = 0x0b;
pub const DW_FORM_UDATA: u16 = 0x0f;
pub const DW_FORM_REF4: u16 = 0x13;
pub const DW_FORM_SEC_OFFSET: u16 = 0x17;
pub const DW_FORM_EXPRLOC: u16 = 0x18;
pub const DW_FORM_FLAG_PRESENT: u16 = 0x19;

// DWARF 5 Unit Types
pub const DW_UT_COMPILE: u8 = 0x01;

// DWARF Base Type Encodings
pub const DW_ATE_BOOLEAN: u8 = 0x02;
pub const DW_ATE_FLOAT: u8 = 0x04;
pub const DW_ATE_SIGNED: u8 = 0x05;
pub const DW_ATE_UNSIGNED: u8 = 0x07;

// DWARF 5 Line Number Content Types
pub const DW_LNCT_PATH: u16 = 0x01;
pub const DW_LNCT_DIRECTORY_INDEX: u16 = 0x02;

// DWARF Line Number Standard Opcodes
pub const DW_LNS_COPY: u8 = 1;
pub const DW_LNS_ADVANCE_PC: u8 = 2;
pub const DW_LNS_ADVANCE_LINE: u8 = 3;
pub const DW_LNS_SET_FILE: u8 = 4;
pub const DW_LNS_SET_COLUMN: u8 = 5;

// DWARF Line Number Extended Opcodes
pub const DW_LNE_END_SEQUENCE: u8 = 1;
pub const DW_LNE_SET_ADDRESS: u8 = 2;

// DWARF 5 Language for Venta (user-defined language code extension in 0x8000..0xffff)
pub const DW_LANG_VENTA: u16 = 0x8001;

// Operations
pub const DW_OP_REG6: u8 = 0x56; // %rbp in x86-64
pub const DW_OP_FBREG: u8 = 0x91;

/// Encode unsigned integer as ULEB128.
pub fn encode_uleb128(mut val: u64, buf: &mut Vec<u8>) {
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if val == 0 {
            break;
        }
    }
}

/// Encode signed integer as SLEB128.
pub fn encode_sleb128(mut val: i64, buf: &mut Vec<u8>) {
    loop {
        let byte = (val & 0x7f) as u8;
        val >>= 7;
        let sign_bit = (byte & 0x40) != 0;
        if (val == 0 && !sign_bit) || (val == -1 && sign_bit) {
            buf.push(byte);
            break;
        } else {
            buf.push(byte | 0x80);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DwarfRelocTarget {
    Text,
    DebugAbbrev,
    DebugLine,
    DebugInfo,
}

#[derive(Debug, Clone)]
pub struct DwarfReloc {
    pub offset: usize,
    pub target: DwarfRelocTarget,
    pub addend: i64,
    pub r_type: u32,
}

/// Information about a formal parameter or local variable for DWARF DIE generation.
#[derive(Debug, Clone)]
pub struct VariableDebugInfo {
    pub name: String,
    pub type_name: String,
    pub file_idx: u32,
    pub line: u32,
    pub stack_offset: i32, // Offset from %rbp (positive magnitude, placed at [rbp - stack_offset])
    pub is_param: bool,
}

/// Information about a function for DWARF subprogram DIE generation.
#[derive(Debug, Clone)]
pub struct FunctionDebugInfo {
    pub name: String,
    pub file_idx: u32,
    pub line: u32,
    pub code_offset: usize,
    pub size: usize,
    pub return_type: Option<String>,
    pub variables: Vec<VariableDebugInfo>,
}

/// Encoder for DWARF 5 `.debug_line` section.
pub struct DwarfLineTable {
    pub section_data: Vec<u8>,
    pub relocations: Vec<DwarfReloc>,
}

impl DwarfLineTable {
    pub fn build(
        comp_dir: &str,
        files: &[String],
        entries: &[LineEntry],
        total_code_size: usize,
    ) -> Self {
        let mut buf = Vec::new();
        let mut relocations = Vec::new();

        // 1. unit_length (u32, placeholder)
        buf.extend_from_slice(&0u32.to_le_bytes());

        // 2. version: 5 (u16)
        buf.extend_from_slice(&5u16.to_le_bytes());

        // 3. address_size: 8 (u8)
        buf.push(8);

        // 4. segment_selector_size: 0 (u8)
        buf.push(0);

        // 5. header_length (u32, placeholder)
        let header_length_offset = buf.len();
        buf.extend_from_slice(&0u32.to_le_bytes());

        let header_start = buf.len();

        // 6. minimum_instruction_length: 1
        buf.push(1);
        // 7. maximum_operations_per_instruction: 1
        buf.push(1);
        // 8. default_is_stmt: 1
        buf.push(1);
        // 9. line_base: -5 (as i8 / 0xfb)
        buf.push((-5i8) as u8);
        // 10. line_range: 14
        buf.push(14);
        // 11. opcode_base: 13
        buf.push(13);
        // 12. standard_opcode_lengths: 12 bytes
        buf.extend_from_slice(&[0, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1]);

        // 13. Directory table
        buf.push(1); // directory_entry_format_count
        encode_uleb128(DW_LNCT_PATH as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);

        // directories_count = 1 (compilation directory at index 0)
        encode_uleb128(1, &mut buf);
        buf.extend_from_slice(comp_dir.as_bytes());
        buf.push(0);

        // 14. File name table
        buf.push(2); // file_name_entry_format_count: path + directory_index
        encode_uleb128(DW_LNCT_PATH as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_LNCT_DIRECTORY_INDEX as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);

        let files_to_emit: Vec<String> = if files.is_empty() {
            vec!["unknown.va".to_string()]
        } else {
            files.to_vec()
        };

        // In DWARF 5, file table has 0-indexed entries
        encode_uleb128(files_to_emit.len() as u64, &mut buf);
        for f in &files_to_emit {
            buf.extend_from_slice(f.as_bytes());
            buf.push(0);
            encode_uleb128(0, &mut buf); // directory index 0
        }

        let header_end = buf.len();
        let header_length = (header_end - header_start) as u32;
        buf[header_length_offset..header_length_offset + 4]
            .copy_from_slice(&header_length.to_le_bytes());

        // 15. Line Number Program
        if !entries.is_empty() {
            let mut current_address = 0usize;
            let mut current_file = u32::MAX;
            let mut current_line = 1u32;
            let mut current_col = 0u32;

            for (i, entry) in entries.iter().enumerate() {
                let target_file = entry.file_idx.saturating_sub(1);
                if i == 0 {
                    // Extended opcode: DW_LNE_set_address
                    buf.push(0); // extended opcode marker
                    encode_uleb128(9, &mut buf); // length: 1 byte opcode + 8 bytes address
                    buf.push(DW_LNE_SET_ADDRESS);
                    let addr_offset = buf.len();
                    relocations.push(DwarfReloc {
                        offset: addr_offset,
                        target: DwarfRelocTarget::Text,
                        addend: entry.code_offset as i64,
                        r_type: 1, // R_X86_64_64
                    });
                    buf.extend_from_slice(&0u64.to_le_bytes());
                    current_address = entry.code_offset;
                } else {
                    let addr_diff = entry.code_offset.saturating_sub(current_address);
                    if addr_diff > 0 {
                        buf.push(DW_LNS_ADVANCE_PC);
                        encode_uleb128(addr_diff as u64, &mut buf);
                        current_address = entry.code_offset;
                    }
                }

                if target_file != current_file {
                    buf.push(DW_LNS_SET_FILE);
                    encode_uleb128(target_file as u64, &mut buf);
                    current_file = target_file;
                }

                if entry.column != current_col && entry.column > 0 {
                    buf.push(DW_LNS_SET_COLUMN);
                    encode_uleb128(entry.column as u64, &mut buf);
                    current_col = entry.column;
                }

                let line_diff = entry.line as i64 - current_line as i64;
                if line_diff != 0 {
                    buf.push(DW_LNS_ADVANCE_LINE);
                    encode_sleb128(line_diff, &mut buf);
                    current_line = entry.line;
                }

                buf.push(DW_LNS_COPY);
            }

            // End of sequence
            let rem_pc = total_code_size.saturating_sub(current_address);
            if rem_pc > 0 {
                buf.push(DW_LNS_ADVANCE_PC);
                encode_uleb128(rem_pc as u64, &mut buf);
            }
            buf.push(0);
            encode_uleb128(1, &mut buf);
            buf.push(DW_LNE_END_SEQUENCE);
        } else if total_code_size > 0 {
            // Minimal empty line program
            buf.push(0);
            encode_uleb128(9, &mut buf);
            buf.push(DW_LNE_SET_ADDRESS);
            let addr_offset = buf.len();
            relocations.push(DwarfReloc {
                offset: addr_offset,
                target: DwarfRelocTarget::Text,
                addend: 0,
                r_type: 1,
            });
            buf.extend_from_slice(&0u64.to_le_bytes());
            buf.push(DW_LNS_ADVANCE_PC);
            encode_uleb128(total_code_size as u64, &mut buf);
            buf.push(0);
            encode_uleb128(1, &mut buf);
            buf.push(DW_LNE_END_SEQUENCE);
        }

        // Patch unit_length = buf.len() - 4
        let unit_length = (buf.len() - 4) as u32;
        buf[0..4].copy_from_slice(&unit_length.to_le_bytes());

        Self {
            section_data: buf,
            relocations,
        }
    }
}

/// Encoder for DWARF 5 `.debug_abbrev` section.
pub struct DwarfAbbrevTable;

impl DwarfAbbrevTable {
    pub fn build() -> Vec<u8> {
        let mut buf = Vec::new();

        // Abbrev 1: DW_TAG_compile_unit (children: yes)
        encode_uleb128(1, &mut buf);
        encode_uleb128(DW_TAG_COMPILE_UNIT as u64, &mut buf);
        buf.push(DW_CHILDREN_YES);
        encode_uleb128(DW_AT_PRODUCER as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_AT_LANGUAGE as u64, &mut buf);
        encode_uleb128(DW_FORM_DATA2 as u64, &mut buf);
        encode_uleb128(DW_AT_NAME as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_AT_COMP_DIR as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_AT_LOW_PC as u64, &mut buf);
        encode_uleb128(DW_FORM_ADDR as u64, &mut buf);
        encode_uleb128(DW_AT_HIGH_PC as u64, &mut buf);
        encode_uleb128(DW_FORM_DATA4 as u64, &mut buf);
        encode_uleb128(DW_AT_STMT_LIST as u64, &mut buf);
        encode_uleb128(DW_FORM_SEC_OFFSET as u64, &mut buf);
        buf.push(0);
        buf.push(0);

        // Abbrev 2: DW_TAG_subprogram with return type (children: yes)
        encode_uleb128(2, &mut buf);
        encode_uleb128(DW_TAG_SUBPROGRAM as u64, &mut buf);
        buf.push(DW_CHILDREN_YES);
        encode_uleb128(DW_AT_EXTERNAL as u64, &mut buf);
        encode_uleb128(DW_FORM_FLAG_PRESENT as u64, &mut buf);
        encode_uleb128(DW_AT_NAME as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_AT_DECL_FILE as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);
        encode_uleb128(DW_AT_DECL_LINE as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);
        encode_uleb128(DW_AT_LOW_PC as u64, &mut buf);
        encode_uleb128(DW_FORM_ADDR as u64, &mut buf);
        encode_uleb128(DW_AT_HIGH_PC as u64, &mut buf);
        encode_uleb128(DW_FORM_DATA4 as u64, &mut buf);
        encode_uleb128(DW_AT_FRAME_BASE as u64, &mut buf);
        encode_uleb128(DW_FORM_EXPRLOC as u64, &mut buf);
        encode_uleb128(DW_AT_TYPE as u64, &mut buf);
        encode_uleb128(DW_FORM_REF4 as u64, &mut buf);
        buf.push(0);
        buf.push(0);

        // Abbrev 3: DW_TAG_subprogram without return type / void (children: yes)
        encode_uleb128(3, &mut buf);
        encode_uleb128(DW_TAG_SUBPROGRAM as u64, &mut buf);
        buf.push(DW_CHILDREN_YES);
        encode_uleb128(DW_AT_EXTERNAL as u64, &mut buf);
        encode_uleb128(DW_FORM_FLAG_PRESENT as u64, &mut buf);
        encode_uleb128(DW_AT_NAME as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_AT_DECL_FILE as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);
        encode_uleb128(DW_AT_DECL_LINE as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);
        encode_uleb128(DW_AT_LOW_PC as u64, &mut buf);
        encode_uleb128(DW_FORM_ADDR as u64, &mut buf);
        encode_uleb128(DW_AT_HIGH_PC as u64, &mut buf);
        encode_uleb128(DW_FORM_DATA4 as u64, &mut buf);
        encode_uleb128(DW_AT_FRAME_BASE as u64, &mut buf);
        encode_uleb128(DW_FORM_EXPRLOC as u64, &mut buf);
        buf.push(0);
        buf.push(0);

        // Abbrev 4: DW_TAG_formal_parameter (children: no)
        encode_uleb128(4, &mut buf);
        encode_uleb128(DW_TAG_FORMAL_PARAMETER as u64, &mut buf);
        buf.push(DW_CHILDREN_NO);
        encode_uleb128(DW_AT_NAME as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_AT_DECL_FILE as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);
        encode_uleb128(DW_AT_DECL_LINE as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);
        encode_uleb128(DW_AT_TYPE as u64, &mut buf);
        encode_uleb128(DW_FORM_REF4 as u64, &mut buf);
        encode_uleb128(DW_AT_LOCATION as u64, &mut buf);
        encode_uleb128(DW_FORM_EXPRLOC as u64, &mut buf);
        buf.push(0);
        buf.push(0);

        // Abbrev 5: DW_TAG_variable (children: no)
        encode_uleb128(5, &mut buf);
        encode_uleb128(DW_TAG_VARIABLE as u64, &mut buf);
        buf.push(DW_CHILDREN_NO);
        encode_uleb128(DW_AT_NAME as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_AT_DECL_FILE as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);
        encode_uleb128(DW_AT_DECL_LINE as u64, &mut buf);
        encode_uleb128(DW_FORM_UDATA as u64, &mut buf);
        encode_uleb128(DW_AT_TYPE as u64, &mut buf);
        encode_uleb128(DW_FORM_REF4 as u64, &mut buf);
        encode_uleb128(DW_AT_LOCATION as u64, &mut buf);
        encode_uleb128(DW_FORM_EXPRLOC as u64, &mut buf);
        buf.push(0);
        buf.push(0);

        // Abbrev 6: DW_TAG_base_type (children: no)
        encode_uleb128(6, &mut buf);
        encode_uleb128(DW_TAG_BASE_TYPE as u64, &mut buf);
        buf.push(DW_CHILDREN_NO);
        encode_uleb128(DW_AT_NAME as u64, &mut buf);
        encode_uleb128(DW_FORM_STRING as u64, &mut buf);
        encode_uleb128(DW_AT_BYTE_SIZE as u64, &mut buf);
        encode_uleb128(DW_FORM_DATA1 as u64, &mut buf);
        encode_uleb128(DW_AT_ENCODING as u64, &mut buf);
        encode_uleb128(DW_FORM_DATA1 as u64, &mut buf);
        buf.push(0);
        buf.push(0);

        // Terminate abbrev table
        buf.push(0);

        buf
    }
}

/// Encoder for DWARF 5 `.debug_info` section.
pub struct DwarfInfoWriter {
    pub section_data: Vec<u8>,
    pub relocations: Vec<DwarfReloc>,
}

impl DwarfInfoWriter {
    pub fn build(
        source_file: &str,
        comp_dir: &str,
        functions: &[FunctionDebugInfo],
        total_code_size: usize,
    ) -> Self {
        let mut buf = Vec::new();
        let mut relocations = Vec::new();

        // --- DWARF 5 Compilation Unit Header ---
        // 1. unit_length (u32, placeholder)
        buf.extend_from_slice(&0u32.to_le_bytes());

        // 2. version: 5 (u16)
        buf.extend_from_slice(&5u16.to_le_bytes());

        // 3. unit_type: DW_UT_compile (1) (u8)
        buf.push(DW_UT_COMPILE);

        // 4. address_size: 8 (u8)
        buf.push(8);

        // 5. debug_abbrev_offset (u32, 0)
        let abbrev_offset = buf.len();
        relocations.push(DwarfReloc {
            offset: abbrev_offset,
            target: DwarfRelocTarget::DebugAbbrev,
            addend: 0,
            r_type: 10, // R_X86_64_32
        });
        buf.extend_from_slice(&0u32.to_le_bytes());

        // --- Compile Unit DIE (Abbrev 1) ---
        encode_uleb128(1, &mut buf);

        // DW_AT_producer
        let producer = "Venta Compiler v1.1.0-alpha.5";
        buf.extend_from_slice(producer.as_bytes());
        buf.push(0);

        // DW_AT_language: DW_LANG_VENTA (0x8001)
        buf.extend_from_slice(&DW_LANG_VENTA.to_le_bytes());

        // DW_AT_name
        buf.extend_from_slice(source_file.as_bytes());
        buf.push(0);

        // DW_AT_comp_dir
        buf.extend_from_slice(comp_dir.as_bytes());
        buf.push(0);

        // DW_AT_low_pc (relocated to .text + 0)
        let low_pc_offset = buf.len();
        relocations.push(DwarfReloc {
            offset: low_pc_offset,
            target: DwarfRelocTarget::Text,
            addend: 0,
            r_type: 1, // R_X86_64_64
        });
        buf.extend_from_slice(&0u64.to_le_bytes());

        // DW_AT_high_pc (size of .text as u32)
        buf.extend_from_slice(&(total_code_size as u32).to_le_bytes());

        // DW_AT_stmt_list (relocated to .debug_line + 0)
        let stmt_list_offset = buf.len();
        relocations.push(DwarfReloc {
            offset: stmt_list_offset,
            target: DwarfRelocTarget::DebugLine,
            addend: 0,
            r_type: 10, // R_X86_64_32
        });
        buf.extend_from_slice(&0u32.to_le_bytes());

        // --- Base Type DIEs ---
        let mut type_offsets: HashMap<String, u32> = HashMap::new();

        let base_types = [
            ("i64", 8u8, DW_ATE_SIGNED),
            ("i32", 4u8, DW_ATE_SIGNED),
            ("i16", 2u8, DW_ATE_SIGNED),
            ("i8", 1u8, DW_ATE_SIGNED),
            ("u64", 8u8, DW_ATE_UNSIGNED),
            ("u32", 4u8, DW_ATE_UNSIGNED),
            ("u16", 2u8, DW_ATE_UNSIGNED),
            ("u8", 1u8, DW_ATE_UNSIGNED),
            ("f64", 8u8, DW_ATE_FLOAT),
            ("f32", 4u8, DW_ATE_FLOAT),
            ("bool", 1u8, DW_ATE_BOOLEAN),
        ];

        for (t_name, t_size, t_enc) in &base_types {
            let offset_in_cu = buf.len() as u32;
            type_offsets.insert(t_name.to_string(), offset_in_cu);

            encode_uleb128(6, &mut buf); // Abbrev 6
            buf.extend_from_slice(t_name.as_bytes());
            buf.push(0);
            buf.push(*t_size);
            buf.push(*t_enc);
        }

        let default_type_offset = *type_offsets.get("i64").unwrap();

        // --- Subprogram DIEs ---
        for func in functions {
            let has_return_type = func.return_type.is_some();
            if has_return_type {
                encode_uleb128(2, &mut buf); // Abbrev 2
            } else {
                encode_uleb128(3, &mut buf); // Abbrev 3
            }

            // DW_AT_name
            buf.extend_from_slice(func.name.as_bytes());
            buf.push(0);

            // DW_AT_decl_file
            encode_uleb128(func.file_idx.saturating_sub(1) as u64, &mut buf);

            // DW_AT_decl_line
            encode_uleb128(func.line as u64, &mut buf);

            // DW_AT_low_pc (relocated to .text + func.code_offset)
            let fn_low_pc_off = buf.len();
            relocations.push(DwarfReloc {
                offset: fn_low_pc_off,
                target: DwarfRelocTarget::Text,
                addend: func.code_offset as i64,
                r_type: 1, // R_X86_64_64
            });
            buf.extend_from_slice(&0u64.to_le_bytes());

            // DW_AT_high_pc (func.size as u32)
            buf.extend_from_slice(&(func.size as u32).to_le_bytes());

            // DW_AT_frame_base: exprloc [DW_OP_REG6] (1 byte length + 0x56)
            encode_uleb128(1, &mut buf);
            buf.push(DW_OP_REG6);

            // DW_AT_type (for Abbrev 2)
            if let Some(ref ret_ty) = func.return_type {
                let ty_off = *type_offsets.get(ret_ty).unwrap_or(&default_type_offset);
                buf.extend_from_slice(&ty_off.to_le_bytes());
            }

            // Parameters and Local Variables (Children of Subprogram)
            for var in &func.variables {
                if var.is_param {
                    encode_uleb128(4, &mut buf); // Abbrev 4 (formal parameter)
                } else {
                    encode_uleb128(5, &mut buf); // Abbrev 5 (variable)
                }

                // DW_AT_name
                buf.extend_from_slice(var.name.as_bytes());
                buf.push(0);

                // DW_AT_decl_file
                encode_uleb128(var.file_idx.saturating_sub(1) as u64, &mut buf);

                // DW_AT_decl_line
                encode_uleb128(var.line as u64, &mut buf);

                // DW_AT_type
                let ty_off = *type_offsets
                    .get(&var.type_name)
                    .unwrap_or(&default_type_offset);
                buf.extend_from_slice(&ty_off.to_le_bytes());

                // DW_AT_location: exprloc [DW_OP_fbreg, -stack_offset]
                let mut expr = Vec::new();
                expr.push(DW_OP_FBREG);
                encode_sleb128(-var.stack_offset as i64, &mut expr);

                encode_uleb128(expr.len() as u64, &mut buf);
                buf.extend_from_slice(&expr);
            }

            // Terminate children of subprogram
            buf.push(0);
        }

        // Terminate children of compilation unit
        buf.push(0);

        // Patch unit_length = buf.len() - 4
        let unit_length = (buf.len() - 4) as u32;
        buf[0..4].copy_from_slice(&unit_length.to_le_bytes());

        Self {
            section_data: buf,
            relocations,
        }
    }
}
