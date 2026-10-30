//! Windows AMD64 PE/COFF to PE32+ executable linker coordinator.

use crate::archive::{extract_required_members, Archive, ArchiveMember};
use crate::coff::pe_writer::{
    PeSectionDesc, PeWriter, IMAGE_REL_BASED_DIR64, IMAGE_SCN_CNT_CODE,
    IMAGE_SCN_CNT_INITIALIZED_DATA, IMAGE_SCN_MEM_DISCARDABLE, IMAGE_SCN_MEM_EXECUTE,
    IMAGE_SCN_MEM_READ, IMAGE_SCN_MEM_WRITE,
};
use crate::coff::reader::read_coff_object;
use crate::coff::reloc::apply_coff_relocation;
use crate::config::LinkerConfig;
use crate::error::{LinkResult, LinkerError};
use crate::map_file::generate_link_map;
use crate::object::ObjectFile;
use crate::resolve::SymbolResolver;
use crate::section::{MergedSection, SectionFlags, SectionKind};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

/// Outcome of a successful PE32+ link session.
#[derive(Debug, Clone)]
pub struct CoffLinkOutput {
    pub output_path: PathBuf,
    pub entry_point_rva: u32,
    pub total_file_size: usize,
    pub input_objects_count: usize,
    pub extracted_archive_members_count: usize,
    pub map_content: Option<String>,
}

/// Links multiple AMD64 COFF relocatable objects (.obj) and archives (.lib) into a PE32+ executable.
pub fn link_coff_executable(config: &LinkerConfig) -> LinkResult<CoffLinkOutput> {
    let mut objects: Vec<ObjectFile> = Vec::new();
    let mut next_obj_id = 0;

    // 1. Load explicit input object files
    for path in &config.input_objects {
        let bytes = fs::read(path).map_err(|e| LinkerError::Io {
            path: path.clone(),
            message: format!("Failed to read input object: {}", e),
        })?;
        let obj = read_coff_object(
            next_obj_id,
            path.display().to_string(),
            path.clone(),
            &bytes,
        )?;
        next_obj_id += 1;
        objects.push(obj);
    }

    // 2. Load explicit in-memory input buffers
    for (name, bytes) in &config.input_buffers {
        let obj = read_coff_object(
            next_obj_id,
            name.clone(),
            PathBuf::from(format!("memory://{}", name)),
            bytes,
        )?;
        next_obj_id += 1;
        objects.push(obj);
    }

    // 3. Load static archives (.lib)
    let mut archives: Vec<Archive> = Vec::new();
    for path in &config.archives {
        let bytes = fs::read(path).map_err(|e| LinkerError::Io {
            path: path.clone(),
            message: format!("Failed to read archive: {}", e),
        })?;
        let ar = Archive::parse(path, &bytes)?;
        archives.push(ar);
    }

    for (name, bytes) in &config.archive_buffers {
        let p = PathBuf::from(format!("memory://{}", name));
        let ar = Archive::parse(&p, bytes)?;
        archives.push(ar);
    }

    // 4. Lazy archive extraction
    let mut extracted_members: Vec<ArchiveMember> = Vec::new();
    if !archives.is_empty() {
        let mut defined_symbols: HashSet<String> = HashSet::new();
        let mut referenced_symbols: HashSet<String> = HashSet::new();

        for obj in &objects {
            for sym in &obj.symbols {
                if sym.is_defined() && !sym.is_local() {
                    defined_symbols.insert(sym.name.clone());
                } else if sym.is_undefined() {
                    referenced_symbols.insert(sym.name.clone());
                }
            }
            for rel in &obj.relocations {
                if !defined_symbols.contains(&rel.symbol_name) {
                    referenced_symbols.insert(rel.symbol_name.clone());
                }
            }
        }

        let mut unresolved: HashSet<String> = referenced_symbols
            .difference(&defined_symbols)
            .cloned()
            .collect();

        extract_required_members(&archives, &mut unresolved, &mut extracted_members);

        for member in &extracted_members {
            let obj = read_coff_object(
                next_obj_id,
                member.name.clone(),
                PathBuf::from(format!("archive://{}", member.name)),
                &member.data,
            )?;
            next_obj_id += 1;
            objects.push(obj);
        }
    }

    // 5. Global symbol resolution
    let mut resolver = SymbolResolver::new();
    resolver.register_definitions(&objects)?;

    // 6. Section merging
    let mut text_merged =
        MergedSection::new(".text".into(), SectionKind::Code, SectionFlags::code());
    let mut rdata_merged = MergedSection::new(
        ".rdata".into(),
        SectionKind::ReadOnlyData,
        SectionFlags::readonly(),
    );
    let mut data_merged =
        MergedSection::new(".data".into(), SectionKind::Data, SectionFlags::readwrite());
    let mut bss_merged =
        MergedSection::new(".bss".into(), SectionKind::Bss, SectionFlags::readwrite());
    let mut pdata_merged = MergedSection::new(
        ".pdata".into(),
        SectionKind::UnwindPdata,
        SectionFlags::readonly(),
    );
    let mut xdata_merged = MergedSection::new(
        ".xdata".into(),
        SectionKind::UnwindXdata,
        SectionFlags::readonly(),
    );

    for obj in &objects {
        for sec in &obj.sections {
            match sec.kind {
                SectionKind::Code => {
                    text_merged.append_contribution(sec);
                }
                SectionKind::ReadOnlyData => {
                    rdata_merged.append_contribution(sec);
                }
                SectionKind::Data => {
                    data_merged.append_contribution(sec);
                }
                SectionKind::Bss => {
                    bss_merged.append_contribution(sec);
                }
                SectionKind::UnwindPdata => {
                    pdata_merged.append_contribution(sec);
                }
                SectionKind::UnwindXdata => {
                    xdata_merged.append_contribution(sec);
                }
                _ => {}
            }
        }
    }

    let mut merged_sections: Vec<MergedSection> = Vec::new();
    if !text_merged.contributions.is_empty() {
        merged_sections.push(text_merged);
    }
    if !rdata_merged.contributions.is_empty() {
        merged_sections.push(rdata_merged);
    }
    if !data_merged.contributions.is_empty() {
        merged_sections.push(data_merged);
    }
    if !bss_merged.contributions.is_empty() {
        merged_sections.push(bss_merged);
    }
    if !pdata_merged.contributions.is_empty() {
        merged_sections.push(pdata_merged);
    }
    if !xdata_merged.contributions.is_empty() {
        merged_sections.push(xdata_merged);
    }

    // 7. Calculate RVAs and Virtual Addresses for each section
    // SectionAlignment = 4096, FileAlignment = 512
    let image_base = PeWriter::DEFAULT_IMAGE_BASE;
    let mut current_rva = 0x1000u32; // First section starts at RVA 0x1000

    for sec in &mut merged_sections {
        sec.virtual_address = image_base + current_rva as u64;
        sec.file_offset = current_rva as u64; // will be aligned by writer
        let sec_len = if sec.kind.is_zero_init() {
            sec.size
        } else {
            sec.data.len()
        };
        current_rva = (current_rva + sec_len as u32 + 0xFFF) & !0xFFF;
    }

    // 8. Symbol virtual address assignment
    resolver.compute_symbol_addresses(&objects, &merged_sections);

    // 9. Entry point lookup
    let entry_name = config.resolved_entry_symbol();
    let entry_va = if let Some(def) = resolver.find_definition(entry_name, &objects) {
        resolver.resolve_symbol_address(entry_name, def.0, &merged_sections)
    } else if let Some(def) = resolver.find_definition("main", &objects) {
        // Fall back to "main" if "_start" not explicitly provided
        resolver.resolve_symbol_address("main", def.0, &merged_sections)
    } else {
        None
    };

    let entry_va = entry_va.ok_or_else(|| LinkerError::UndefinedSymbol {
        symbol: entry_name.to_string(),
        referencing_file: PathBuf::from("linker"),
        referencing_section: "entry".into(),
    })?;

    let entry_rva = (entry_va.saturating_sub(image_base)) as u32;

    // 10. Apply COFF Relocations
    let mut base_reloc_rvas: Vec<u32> = Vec::new();

    for obj in &objects {
        for rel in &obj.relocations {
            let sym_va = resolver
                .resolve_symbol_address(&rel.symbol_name, obj, &merged_sections)
                .ok_or_else(|| LinkerError::UndefinedSymbol {
                    symbol: rel.symbol_name.clone(),
                    referencing_file: obj.path.clone(),
                    referencing_section: rel.section_index.to_string(),
                })?;

            for merged_sec in merged_sections.iter_mut() {
                if let Some(contrib) = merged_sec
                    .contributions
                    .iter()
                    .find(|c| c.object_id == obj.id && c.source_section_idx == rel.section_index)
                {
                    let patch_offset = contrib.offset_in_merged + rel.offset_in_section;
                    let place_va = merged_sec.virtual_address + patch_offset as u64;

                    apply_coff_relocation(
                        &obj.path,
                        &merged_sec.name,
                        rel,
                        patch_offset,
                        &mut merged_sec.data,
                        sym_va,
                        place_va,
                        image_base,
                    )?;

                    // If 64-bit absolute address, record for .reloc base relocations
                    if rel.reloc_type == 1 {
                        let rva = (place_va.saturating_sub(image_base)) as u32;
                        base_reloc_rvas.push(rva);
                    }
                }
            }
        }
    }

    // 11. Synthesize .idata section (Import Table for KERNEL32.dll)
    let (idata_bytes, idata_rva, idata_size, iat_rva, iat_size) = build_idata_section(current_rva);
    current_rva = (current_rva + idata_bytes.len() as u32 + 0xFFF) & !0xFFF;

    // 12. Synthesize .reloc section (Base Relocations)
    let (reloc_bytes, reloc_rva, reloc_size) = build_reloc_section(current_rva, &base_reloc_rvas);

    // 13. Build final PeSectionDesc list
    let mut pe_sections = Vec::new();
    let mut pdata_rva_and_size = None;

    for sec in merged_sections {
        let rva = (sec.virtual_address.saturating_sub(image_base)) as u32;
        let v_size = if sec.kind.is_zero_init() {
            sec.size as u32
        } else {
            sec.data.len() as u32
        };

        let chars = match sec.kind {
            SectionKind::Code => IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_READ,
            SectionKind::ReadOnlyData | SectionKind::UnwindPdata | SectionKind::UnwindXdata => {
                IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ
            }
            SectionKind::Data => {
                IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE
            }
            SectionKind::Bss => {
                IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ | IMAGE_SCN_MEM_WRITE
            }
            _ => IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ,
        };

        if sec.name == ".pdata" {
            pdata_rva_and_size = Some((rva, v_size));
        }

        pe_sections.push(PeSectionDesc {
            name: sec.name,
            virtual_size: v_size,
            virtual_address: rva,
            data: sec.data,
            characteristics: chars,
        });
    }

    // Add .idata section
    if !idata_bytes.is_empty() {
        pe_sections.push(PeSectionDesc {
            name: ".idata".into(),
            virtual_size: idata_size,
            virtual_address: idata_rva,
            data: idata_bytes,
            characteristics: IMAGE_SCN_CNT_INITIALIZED_DATA
                | IMAGE_SCN_MEM_READ
                | IMAGE_SCN_MEM_WRITE,
        });
    }

    // Add .reloc section
    if !reloc_bytes.is_empty() {
        pe_sections.push(PeSectionDesc {
            name: ".reloc".into(),
            virtual_size: reloc_size,
            virtual_address: reloc_rva,
            data: reloc_bytes,
            characteristics: IMAGE_SCN_CNT_INITIALIZED_DATA
                | IMAGE_SCN_MEM_READ
                | IMAGE_SCN_MEM_DISCARDABLE,
        });
    }

    // 14. Emit PE32+ image
    let binary_bytes = PeWriter::write_image(
        entry_rva,
        pe_sections,
        pdata_rva_and_size,
        Some((idata_rva, idata_size)),
        Some((iat_rva, iat_size)),
        Some((reloc_rva, reloc_size)),
    );

    // 15. Write to output file
    if let Some(parent) = config.output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| LinkerError::Io {
                path: parent.to_path_buf(),
                message: format!("Failed to create output directory: {}", e),
            })?;
        }
    }

    fs::write(&config.output_path, &binary_bytes).map_err(|e| LinkerError::Io {
        path: config.output_path.clone(),
        message: format!("Failed to write PE output: {}", e),
    })?;

    // 16. Optional link map
    let mut map_content = None;
    if let Some(ref map_path) = config.map_file {
        let dummy_merged = Vec::new();
        let map_str = generate_link_map(
            &objects,
            &extracted_members,
            &[],
            &dummy_merged,
            &resolver.resolved_addresses,
            entry_va,
            entry_name,
        );
        fs::write(map_path, &map_str).map_err(|e| LinkerError::Io {
            path: map_path.clone(),
            message: format!("Failed to write map file: {}", e),
        })?;
        map_content = Some(map_str);
    }

    Ok(CoffLinkOutput {
        output_path: config.output_path.clone(),
        entry_point_rva: entry_rva,
        total_file_size: binary_bytes.len(),
        input_objects_count: objects.len(),
        extracted_archive_members_count: extracted_members.len(),
        map_content,
    })
}

/// Builds `.idata` section containing imports for `KERNEL32.dll`:
/// - `ExitProcess`
/// - `GetStdHandle`
/// - `WriteFile`
fn build_idata_section(start_rva: u32) -> (Vec<u8>, u32, u32, u32, u32) {
    let mut idata = Vec::new();

    // 1. Import Directory Table (20 bytes per DLL, terminated by 20 zero bytes)
    // Entry 0: KERNEL32.dll
    // Entry 1: Null terminator
    let dir_table_size = 40;
    idata.resize(dir_table_size, 0);

    // 2. Import Lookup Table (ILT) - 4 entries (3 functions + 1 null entry), 8 bytes each = 32 bytes
    let ilt_offset = idata.len();
    let ilt_rva = start_rva + ilt_offset as u32;
    idata.resize(ilt_offset + 32, 0);

    // 3. Import Address Table (IAT) - duplicate of ILT, 32 bytes
    let iat_offset = idata.len();
    let iat_rva = start_rva + iat_offset as u32;
    idata.resize(iat_offset + 32, 0);

    // 4. DLL Name string
    let dll_name_offset = idata.len();
    let dll_name_rva = start_rva + dll_name_offset as u32;
    idata.extend_from_slice(b"KERNEL32.dll\0");
    if idata.len() % 2 != 0 {
        idata.push(0);
    }

    // 5. Hint/Name table for functions
    let funcs = ["ExitProcess", "GetStdHandle", "WriteFile"];
    let mut hint_name_rvas = Vec::new();

    for func in &funcs {
        let hn_offset = idata.len();
        hint_name_rvas.push(start_rva + hn_offset as u32);
        idata.extend_from_slice(&0u16.to_le_bytes()); // Hint = 0
        idata.extend_from_slice(func.as_bytes());
        idata.push(0); // NUL terminator
        if idata.len() % 2 != 0 {
            idata.push(0); // 2-byte align
        }
    }

    // Fill ILT and IAT entries
    for (i, &hn_rva) in hint_name_rvas.iter().enumerate() {
        let entry_val = hn_rva as u64; // bit 63 = 0 -> import by name
        let ilt_dest = ilt_offset + i * 8;
        idata[ilt_dest..ilt_dest + 8].copy_from_slice(&entry_val.to_le_bytes());

        let iat_dest = iat_offset + i * 8;
        idata[iat_dest..iat_dest + 8].copy_from_slice(&entry_val.to_le_bytes());
    }

    // Fill Directory Table entry 0
    let dir = &mut idata[0..20];
    dir[0..4].copy_from_slice(&ilt_rva.to_le_bytes()); // OriginalFirstThunk (ILT)
    dir[4..8].copy_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
    dir[8..12].copy_from_slice(&0u32.to_le_bytes()); // ForwarderChain
    dir[12..16].copy_from_slice(&dll_name_rva.to_le_bytes()); // Name RVA
    dir[16..20].copy_from_slice(&iat_rva.to_le_bytes()); // FirstThunk (IAT)

    let total_size = idata.len() as u32;
    let iat_size = 32u32;

    (idata, start_rva, total_size, iat_rva, iat_size)
}

/// Builds `.reloc` section with base relocation blocks.
fn build_reloc_section(start_rva: u32, rvas: &[u32]) -> (Vec<u8>, u32, u32) {
    if rvas.is_empty() {
        // Dummy 8-byte empty block
        let mut empty = Vec::new();
        empty.extend_from_slice(&start_rva.to_le_bytes()); // Page RVA
        empty.extend_from_slice(&8u32.to_le_bytes()); // Block Size
        let sz = empty.len() as u32;
        return (empty, start_rva, sz);
    }

    // Group RVAs by 4KB page (RVA & !0xFFF)
    let mut pages: HashMap<u32, Vec<u16>> = HashMap::new();
    for &rva in rvas {
        let page = rva & !0xFFF;
        let offset = (rva & 0xFFF) as u16;
        pages.entry(page).or_default().push(offset);
    }

    let mut sorted_pages: Vec<u32> = pages.keys().copied().collect();
    sorted_pages.sort_unstable();

    let mut out = Vec::new();
    for page in sorted_pages {
        let mut offsets = pages.remove(&page).unwrap();
        offsets.sort_unstable();

        // Each entry is 2 bytes: type (4 bits) | offset (12 bits)
        // IMAGE_REL_BASED_DIR64 = 10 (0xA)
        let block_entries_len = offsets.len();
        let pad = block_entries_len % 2 != 0; // Each block must be 4-byte aligned
        let block_size = 8 + (block_entries_len + if pad { 1 } else { 0 }) * 2;

        out.extend_from_slice(&page.to_le_bytes());
        out.extend_from_slice(&(block_size as u32).to_le_bytes());

        for off in offsets {
            let entry = (IMAGE_REL_BASED_DIR64 << 12) | (off & 0x0FFF);
            out.extend_from_slice(&entry.to_le_bytes());
        }

        if pad {
            out.extend_from_slice(&0u16.to_le_bytes()); // IMAGE_REL_BASED_ABSOLUTE padding
        }
    }

    let total_size = out.len() as u32;
    (out, start_rva, total_size)
}
