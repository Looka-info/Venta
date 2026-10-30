//! Link map generation for debugging and binary size analysis.

use crate::archive::{Archive, ArchiveMember};
use crate::object::ObjectFile;
use crate::section::MergedSection;
use std::collections::HashMap;
use std::fmt::Write;

pub struct LinkMap;

impl LinkMap {
    pub fn generate(
        target: &str,
        objects: &[ObjectFile],
        archives: &[Archive],
        extracted_members: &[String],
        merged_sections: &[MergedSection],
        symbol_addresses: &[(String, u64, String)], // (name, address, object_name)
        entry_point: (&str, u64),
    ) -> String {
        let mut out = String::new();

        writeln!(
            &mut out,
            "=============================================================================="
        )
        .unwrap();
        writeln!(
            &mut out,
            "                      VENTA NATIVE LINKER MAP FILE"
        )
        .unwrap();
        writeln!(
            &mut out,
            "=============================================================================="
        )
        .unwrap();
        writeln!(&mut out, "Target: {}\n", target).unwrap();

        // 1. Input Objects
        writeln!(&mut out, "Input Objects:").unwrap();
        for obj in objects {
            writeln!(&mut out, "  [{}] {}", obj.id, obj.path.display()).unwrap();
        }
        writeln!(&mut out).unwrap();

        // 2. Archive Members
        if !archives.is_empty() {
            writeln!(&mut out, "Archive Member Extraction:").unwrap();
            for arch in archives {
                writeln!(&mut out, "  Archive: {}", arch.path.display()).unwrap();
                for member in &arch.members {
                    let was_extracted = extracted_members.iter().any(|m| m == &member.name);
                    let status = if was_extracted { "EXTRACTED" } else { "unused" };
                    writeln!(
                        &mut out,
                        "    - {:<30} [{}] ({} bytes)",
                        member.name, status, member.size
                    )
                    .unwrap();
                }
            }
            writeln!(&mut out).unwrap();
        }

        // 3. Section Layout
        writeln!(&mut out, "Output Sections:").unwrap();
        writeln!(
            &mut out,
            "  {:<16} {:<18} {:<12} {:<12} {:<6}",
            "Section", "VMA", "File Off", "Size", "Align"
        )
        .unwrap();
        writeln!(
            &mut out,
            "  ----------------------------------------------------------------------"
        )
        .unwrap();
        for sec in merged_sections {
            writeln!(
                &mut out,
                "  {:<16} 0x{:016x} 0x{:08x}   0x{:08x}   {}",
                sec.name, sec.virtual_address, sec.file_offset, sec.size, sec.align
            )
            .unwrap();
            for contrib in &sec.contributions {
                let obj_name = objects
                    .iter()
                    .find(|o| o.id == contrib.object_id)
                    .map(|o| o.name.as_str())
                    .unwrap_or("unknown");
                writeln!(
                    &mut out,
                    "    +-- {:<20} {:<24} +0x{:06x} (0x{:x} bytes)",
                    contrib.source_section_name, obj_name, contrib.offset_in_merged, contrib.size
                )
                .unwrap();
            }
        }
        writeln!(&mut out).unwrap();

        // 4. Global Symbols
        writeln!(&mut out, "Symbol Table:").unwrap();
        writeln!(
            &mut out,
            "  {:<18} {:<36} {}",
            "Address", "Symbol", "Defined In"
        )
        .unwrap();
        writeln!(
            &mut out,
            "  ----------------------------------------------------------------------"
        )
        .unwrap();
        let mut sorted_symbols = symbol_addresses.to_vec();
        sorted_symbols.sort_by_key(|s| s.1);
        for (name, addr, obj) in sorted_symbols {
            writeln!(&mut out, "  0x{:016x} {:<36} {}", addr, name, obj).unwrap();
        }
        writeln!(&mut out).unwrap();

        // 5. Entry point
        writeln!(
            &mut out,
            "Entry Point: '{}' at 0x{:016x}",
            entry_point.0, entry_point.1
        )
        .unwrap();

        out
    }
}

/// Convenience helper to produce a formatted link map.
pub fn generate_link_map(
    objects: &[ObjectFile],
    extracted_members: &[ArchiveMember],
    _unused_archives: &[Archive],
    merged_sections: &[MergedSection],
    resolved_symbols: &HashMap<String, u64>,
    entry_point_va: u64,
    entry_point_name: &str,
) -> String {
    let mut symbol_triples = Vec::new();
    for (sym_name, &va) in resolved_symbols {
        let defining_obj = objects
            .iter()
            .find(|o| {
                o.symbols
                    .iter()
                    .any(|s| s.name == *sym_name && s.is_defined())
            })
            .map(|o| o.name.clone())
            .unwrap_or_else(|| "linker".into());
        symbol_triples.push((sym_name.clone(), va, defining_obj));
    }

    let member_names: Vec<String> = extracted_members.iter().map(|m| m.name.clone()).collect();
    LinkMap::generate(
        "Venta Native Executable",
        objects,
        &[],
        &member_names,
        merged_sections,
        &symbol_triples,
        (entry_point_name, entry_point_va),
    )
}
