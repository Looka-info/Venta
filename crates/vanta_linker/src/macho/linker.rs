//! Apple Silicon ARM64 Mach-O executable linker coordinator.

use crate::archive::{extract_required_members, Archive, ArchiveMember};
use crate::config::LinkerConfig;
use crate::error::{LinkResult, LinkerError};
use crate::macho::reader::read_macho_object;
use crate::macho::reloc::apply_macho_relocation;
use crate::macho::writer::{MachOExecutableWriter, MachOSectionDesc};
use crate::map_file::generate_link_map;
use crate::object::ObjectFile;
use crate::resolve::SymbolResolver;
use crate::section::{MergedSection, SectionFlags, SectionKind};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// Outcome of a successful Mach-O link session.
#[derive(Debug, Clone)]
pub struct MachOLinkOutput {
    pub output_path: PathBuf,
    pub entry_point_offset: u64,
    pub total_file_size: usize,
    pub input_objects_count: usize,
    pub extracted_archive_members_count: usize,
    pub map_content: Option<String>,
}

/// Links multiple ARM64 Mach-O objects (.o) and archives (.a) into a dyld-compatible `MH_EXECUTE` binary.
pub fn link_macho_executable(config: &LinkerConfig) -> LinkResult<MachOLinkOutput> {
    let mut objects: Vec<ObjectFile> = Vec::new();
    let mut next_obj_id = 0;

    // 1. Load explicit input object files
    for path in &config.input_objects {
        let bytes = fs::read(path).map_err(|e| LinkerError::Io {
            path: path.clone(),
            message: format!("Failed to read input object: {}", e),
        })?;
        let obj = read_macho_object(
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
        let obj = read_macho_object(
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
            let obj = read_macho_object(
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
    let mut text_merged = MergedSection::new(
        "__TEXT.__text".into(),
        SectionKind::Code,
        SectionFlags::code(),
    );
    let mut const_merged = MergedSection::new(
        "__TEXT.__const".into(),
        SectionKind::ReadOnlyData,
        SectionFlags::readonly(),
    );
    let mut data_merged = MergedSection::new(
        "__DATA.__data".into(),
        SectionKind::Data,
        SectionFlags::readwrite(),
    );
    let mut bss_merged = MergedSection::new(
        "__DATA.__bss".into(),
        SectionKind::Bss,
        SectionFlags::readwrite(),
    );

    for obj in &objects {
        for sec in &obj.sections {
            match sec.kind {
                SectionKind::Code => {
                    text_merged.append_contribution(sec);
                }
                SectionKind::ReadOnlyData => {
                    const_merged.append_contribution(sec);
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
    if !text_merged.contributions.is_empty() && !text_merged.data.is_empty() {
        merged_sections.push(text_merged);
    }
    if !const_merged.contributions.is_empty() && !const_merged.data.is_empty() {
        merged_sections.push(const_merged);
    }
    if !data_merged.contributions.is_empty()
        && (!data_merged.data.is_empty() || data_merged.size > 0)
    {
        merged_sections.push(data_merged);
    }
    if !bss_merged.contributions.is_empty() && bss_merged.size > 0 {
        merged_sections.push(bss_merged);
    }

    // 7. Base Virtual Address Layout
    // __TEXT base: 0x100000000
    let base_text_va = MachOExecutableWriter::BASE_TEXT_ADDR;
    let mut current_va = base_text_va + 0x4000; // Header and load commands take up to 16KB page

    for sec in &mut merged_sections {
        sec.virtual_address = current_va;
        let sec_len = if sec.kind.is_zero_init() {
            sec.size
        } else {
            sec.data.len()
        };
        current_va = (current_va + sec_len as u64 + 15) & !15;
    }

    // 8. Symbol address computation
    resolver.compute_symbol_addresses(&objects, &merged_sections);

    // 9. Entry point lookup (check "_main" or "main" or entry_name)
    let entry_name = config.resolved_entry_symbol();
    let entry_va = if let Some(def) = resolver.find_definition(entry_name, &objects) {
        resolver.resolve_symbol_address(entry_name, def.0, &merged_sections)
    } else if let Some(def) = resolver.find_definition("_main", &objects) {
        resolver.resolve_symbol_address("_main", def.0, &merged_sections)
    } else if let Some(def) = resolver.find_definition("main", &objects) {
        resolver.resolve_symbol_address("main", def.0, &merged_sections)
    } else {
        None
    };

    let entry_va = entry_va.ok_or_else(|| LinkerError::UndefinedSymbol {
        symbol: entry_name.to_string(),
        referencing_file: PathBuf::from("linker"),
        referencing_section: "entry".into(),
    })?;

    let entry_offset = entry_va.saturating_sub(base_text_va);

    // 10. Apply Mach-O relocations
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

                    apply_macho_relocation(
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

    // 11. Separate into text and data section descriptors
    let mut text_sections = Vec::new();
    let mut data_sections = Vec::new();

    for sec in &merged_sections {
        let (segname, sectname) = if sec.name.starts_with("__TEXT") {
            (
                "__TEXT",
                sec.name
                    .trim_start_matches("__TEXT.")
                    .trim_start_matches("__TEXT"),
            )
        } else {
            (
                "__DATA",
                sec.name
                    .trim_start_matches("__DATA.")
                    .trim_start_matches("__DATA"),
            )
        };

        let desc = MachOSectionDesc {
            segname: segname.to_string(),
            sectname: sectname.to_string(),
            addr: sec.virtual_address,
            size: if sec.kind.is_zero_init() {
                sec.size as u64
            } else {
                sec.data.len() as u64
            },
            data: sec.data.clone(),
            is_zerofill: sec.kind.is_zero_init(),
        };

        if segname == "__TEXT" {
            text_sections.push(desc);
        } else {
            data_sections.push(desc);
        }
    }

    // 12. Collect symbols for LC_SYMTAB
    let mut symbols_list: Vec<(String, u64)> = Vec::new();
    for (name, &va) in &resolver.resolved_addresses {
        symbols_list.push((name.clone(), va));
    }
    symbols_list.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    // 13. Write Mach-O binary
    let binary_bytes = MachOExecutableWriter::write_executable(
        entry_offset,
        &text_sections,
        &data_sections,
        &symbols_list,
    );

    // 14. Write to disk
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
        message: format!("Failed to write Mach-O output: {}", e),
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

    #[cfg(target_os = "macos")]
    {
        // On macOS Apple Silicon (ARM64), executables must be signed (at least ad-hoc) to execute.
        let _ = std::process::Command::new("codesign")
            .arg("-s")
            .arg("-")
            .arg(&config.output_path)
            .output();
    }

    // 15. Optional link map
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

    Ok(MachOLinkOutput {
        output_path: config.output_path.clone(),
        entry_point_offset: entry_offset,
        total_file_size: binary_bytes.len(),
        input_objects_count: objects.len(),
        extracted_archive_members_count: extracted_members.len(),
        map_content,
    })
}
