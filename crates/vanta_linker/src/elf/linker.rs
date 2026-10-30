//! Linux ELF64 static executable linker coordinator.

use crate::archive::{extract_required_members, Archive, ArchiveMember};
use crate::config::{LinkMode, LinkerConfig};
use crate::elf::reader::read_elf_object;
use crate::elf::reloc::apply_relocation;
use crate::elf::runtime::create_static_runtime_object;
use crate::elf::writer::ElfExecutableWriter;
use crate::error::{LinkResult, LinkerError};
use crate::layout::SegmentLayout;
use crate::map_file::generate_link_map;
use crate::object::ObjectFile;
use crate::resolve::SymbolResolver;
use crate::section::{MergedSection, SectionFlags, SectionKind};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// Detailed outcome of a successful ELF link session.
#[derive(Debug, Clone)]
pub struct ElfLinkOutput {
    pub output_path: PathBuf,
    pub entry_point_va: u64,
    pub total_file_size: usize,
    pub input_objects_count: usize,
    pub extracted_archive_members_count: usize,
    pub map_content: Option<String>,
}

/// Links multiple ELF64 relocatable objects and static archives into a static executable.
pub fn link_elf_executable(config: &LinkerConfig) -> LinkResult<ElfLinkOutput> {
    let mut objects: Vec<ObjectFile> = Vec::new();
    let mut next_obj_id = 0;

    // 1. Load explicit input object files
    for path in &config.input_objects {
        let bytes = fs::read(path).map_err(|e| LinkerError::Io {
            path: path.clone(),
            message: format!("Failed to read input object: {}", e),
        })?;
        let obj = read_elf_object(
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
        let obj = read_elf_object(
            next_obj_id,
            name.clone(),
            PathBuf::from(format!("memory://{}", name)),
            bytes,
        )?;
        next_obj_id += 1;
        objects.push(obj);
    }

    // 3. Load static archives
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

    // 4. In static mode, include self-contained static runtime object
    if config.link_mode == LinkMode::Static {
        let rt_obj = create_static_runtime_object(next_obj_id);
        next_obj_id += 1;
        objects.push(rt_obj);
    }

    // 5. Lazy archive extraction
    let mut extracted_members: Vec<ArchiveMember> = Vec::new();
    if !archives.is_empty() {
        // Collect currently unresolved symbols
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
            let obj = read_elf_object(
                next_obj_id,
                member.name.clone(),
                PathBuf::from(format!("archive://{}", member.name)),
                &member.data,
            )?;
            next_obj_id += 1;
            objects.push(obj);
        }
    }

    // 6. Global symbol resolution
    let mut resolver = SymbolResolver::new();
    resolver.register_definitions(&objects)?;

    // 7. Entry point validation
    let entry_name = config.resolved_entry_symbol();
    let entry_def = resolver
        .find_definition(entry_name, &objects)
        .ok_or_else(|| LinkerError::UndefinedSymbol {
            symbol: entry_name.to_string(),
            referencing_file: PathBuf::from("linker"),
            referencing_section: "entry".into(),
        })?;

    // 8. Verify all required symbols are resolvable
    for obj in &objects {
        for rel in &obj.relocations {
            if rel.is_local() {
                continue;
            }
            if obj
                .symbols
                .iter()
                .any(|s| s.is_local() && s.is_defined() && s.name == rel.symbol_name)
            {
                continue;
            }
            if obj.sections.iter().any(|s| s.name == rel.symbol_name) {
                continue;
            }
            let is_resolved = resolver.strong_symbols.contains_key(&rel.symbol_name)
                || resolver.weak_symbols.contains_key(&rel.symbol_name);
            if !is_resolved {
                return Err(LinkerError::UndefinedSymbol {
                    symbol: rel.symbol_name.clone(),
                    referencing_file: obj.path.clone(),
                    referencing_section: rel.section_index.to_string(),
                });
            }
        }
    }

    // 9. Merge sections
    let mut text_merged =
        MergedSection::new(".text".into(), SectionKind::Code, SectionFlags::code());
    let mut rodata_merged = MergedSection::new(
        ".rodata".into(),
        SectionKind::ReadOnlyData,
        SectionFlags::readonly(),
    );
    let mut data_merged =
        MergedSection::new(".data".into(), SectionKind::Data, SectionFlags::readwrite());
    let mut bss_merged =
        MergedSection::new(".bss".into(), SectionKind::Bss, SectionFlags::readwrite());

    for obj in &objects {
        for sec in &obj.sections {
            match sec.kind {
                SectionKind::Code => {
                    text_merged.append_contribution(sec);
                }
                SectionKind::ReadOnlyData => {
                    rodata_merged.append_contribution(sec);
                }
                SectionKind::Data => {
                    data_merged.append_contribution(sec);
                }
                SectionKind::Bss => {
                    bss_merged.append_contribution(sec);
                }
                _ => {}
            }
        }
    }

    let mut merged_sections: Vec<MergedSection> = Vec::new();
    if !text_merged.contributions.is_empty() {
        merged_sections.push(text_merged);
    }
    if !rodata_merged.contributions.is_empty() {
        merged_sections.push(rodata_merged);
    }
    if !data_merged.contributions.is_empty() {
        merged_sections.push(data_merged);
    }
    if !bss_merged.contributions.is_empty() {
        merged_sections.push(bss_merged);
    }

    // 10. Memory layout calculation
    // Header size: 64 bytes ELF header + (3 or 4) * 56 bytes program headers
    let has_rodata = merged_sections.iter().any(|s| s.name == ".rodata");
    let has_data_or_bss = merged_sections
        .iter()
        .any(|s| s.name == ".data" || s.name == ".bss");
    let num_phdrs = 1 /* PT_LOAD RX */
        + if has_rodata { 1 } else { 0 }
        + if has_data_or_bss { 1 } else { 0 }
        + 1 /* PT_GNU_STACK */;
    let header_bytes = 64 + num_phdrs * 56;

    let mut layout = SegmentLayout::new(0x400000, 4096, 4096);
    layout.compute_layout(&mut merged_sections, header_bytes as u64);

    // 11. Compute symbol virtual addresses
    resolver.compute_symbol_addresses(&objects, &merged_sections);

    let entry_va = resolver
        .resolve_symbol_address(entry_name, entry_def.0, &merged_sections)
        .ok_or_else(|| LinkerError::UndefinedSymbol {
            symbol: entry_name.to_string(),
            referencing_file: PathBuf::from("linker"),
            referencing_section: "entry".into(),
        })?;

    // 12. Apply relocations
    for obj in &objects {
        for rel in &obj.relocations {
            let sym_va = resolver
                .resolve_symbol_address(&rel.symbol_name, obj, &merged_sections)
                .ok_or_else(|| LinkerError::UndefinedSymbol {
                    symbol: rel.symbol_name.clone(),
                    referencing_file: obj.path.clone(),
                    referencing_section: rel.section_index.to_string(),
                })?;

            // Find which merged section contains this contribution
            for merged_sec in merged_sections.iter_mut() {
                if let Some(contrib) = merged_sec
                    .contributions
                    .iter()
                    .find(|c| c.object_id == obj.id && c.source_section_idx == rel.section_index)
                {
                    let patch_offset = contrib.offset_in_merged + rel.offset_in_section;
                    let place_va = merged_sec.virtual_address + patch_offset as u64;

                    apply_relocation(
                        &obj.path,
                        &merged_sec.name,
                        rel,
                        patch_offset,
                        &mut merged_sec.data,
                        sym_va,
                        place_va,
                    )?;
                }
            }
        }
    }

    // 13. Collect global symbols for ELF symbol table
    let mut global_symbols: Vec<(String, u64, u64)> = Vec::new();
    for (name, &va) in &resolver.resolved_addresses {
        let size = resolver
            .find_definition(name, &objects)
            .map(|(_, s)| s.size)
            .unwrap_or(0);
        global_symbols.push((name.clone(), va, size));
    }
    global_symbols.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    // 14. Write ELF executable image
    let binary_bytes =
        ElfExecutableWriter::write_executable(entry_va, &merged_sections, &global_symbols);

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
        message: format!("Failed to write executable output: {}", e),
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&config.output_path)
            .map_err(|e| LinkerError::Io {
                path: config.output_path.clone(),
                message: format!("Failed to get permissions: {}", e),
            })?
            .permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(&config.output_path, perms);
    }

    // 16. Optional link map generation
    let mut map_content = None;
    if let Some(ref map_path) = config.map_file {
        let unused_archives = Vec::new();
        let map_str = generate_link_map(
            &objects,
            &extracted_members,
            &unused_archives,
            &merged_sections,
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

    Ok(ElfLinkOutput {
        output_path: config.output_path.clone(),
        entry_point_va: entry_va,
        total_file_size: binary_bytes.len(),
        input_objects_count: objects.len(),
        extracted_archive_members_count: extracted_members.len(),
        map_content,
    })
}
