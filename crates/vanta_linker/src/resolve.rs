//! Global symbol resolution and collision detection.

use crate::error::{LinkResult, LinkerError};
use crate::object::ObjectFile;
use crate::section::MergedSection;
use crate::symbol::{Symbol, SymbolVisibility};
use std::collections::HashMap;

/// Global symbol resolution registry.
#[derive(Debug, Default)]
pub struct SymbolResolver {
    /// Global strong symbols: name -> (object_id, symbol_index)
    pub strong_symbols: HashMap<String, (usize, usize)>,
    /// Global weak symbols: name -> (object_id, symbol_index)
    pub weak_symbols: HashMap<String, (usize, usize)>,
    /// Resolved virtual address of each global symbol: name -> virtual_address
    pub resolved_addresses: HashMap<String, u64>,
}

impl SymbolResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers all global and weak definitions across input objects.
    /// Emits `LinkerError::DuplicateSymbol` if two strong definitions collide.
    pub fn register_definitions(&mut self, objects: &[ObjectFile]) -> LinkResult<()> {
        for obj in objects {
            for (sym_idx, sym) in obj.symbols.iter().enumerate() {
                if !sym.is_defined() || sym.is_local() {
                    continue;
                }

                match sym.visibility {
                    SymbolVisibility::Global => {
                        if let Some(&(first_obj_id, _)) = self.strong_symbols.get(&sym.name) {
                            let first_path = objects
                                .iter()
                                .find(|o| o.id == first_obj_id)
                                .map(|o| o.path.clone())
                                .unwrap_or_else(|| obj.path.clone());
                            return Err(LinkerError::DuplicateSymbol {
                                symbol: sym.name.clone(),
                                first_file: first_path,
                                second_file: obj.path.clone(),
                            });
                        }
                        self.strong_symbols
                            .insert(sym.name.clone(), (obj.id, sym_idx));
                    }
                    SymbolVisibility::Weak => {
                        // Only record weak definition if no strong or prior weak exists
                        if !self.strong_symbols.contains_key(&sym.name)
                            && !self.weak_symbols.contains_key(&sym.name)
                        {
                            self.weak_symbols
                                .insert(sym.name.clone(), (obj.id, sym_idx));
                        }
                    }
                    SymbolVisibility::Local => unreachable!(),
                }
            }
        }
        Ok(())
    }

    /// Finds the defining object and symbol for a global symbol name.
    pub fn find_definition<'a>(
        &self,
        name: &str,
        objects: &'a [ObjectFile],
    ) -> Option<(&'a ObjectFile, &'a Symbol)> {
        if let Some(&(obj_id, sym_idx)) = self.strong_symbols.get(name) {
            let obj = objects.iter().find(|o| o.id == obj_id)?;
            let sym = obj.symbols.get(sym_idx)?;
            return Some((obj, sym));
        }
        if let Some(&(obj_id, sym_idx)) = self.weak_symbols.get(name) {
            let obj = objects.iter().find(|o| o.id == obj_id)?;
            let sym = obj.symbols.get(sym_idx)?;
            return Some((obj, sym));
        }
        None
    }

    /// Assigns final virtual addresses to all resolved symbols based on merged section layouts.
    pub fn compute_symbol_addresses(
        &mut self,
        objects: &[ObjectFile],
        merged_sections: &[MergedSection],
    ) {
        for merged in merged_sections {
            for contrib in &merged.contributions {
                if let Some(obj) = objects.iter().find(|o| o.id == contrib.object_id) {
                    for sym in &obj.symbols {
                        if sym.section_index == Some(contrib.source_section_idx) {
                            let sym_va = merged.virtual_address
                                + contrib.offset_in_merged as u64
                                + sym.value;
                            if sym.is_global() || sym.is_weak() {
                                self.resolved_addresses.insert(sym.name.clone(), sym_va);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Resolves the virtual address of a symbol referenced from a specific object.
    pub fn resolve_symbol_address(
        &self,
        sym_name: &str,
        referencing_object: &ObjectFile,
        merged_sections: &[MergedSection],
    ) -> Option<u64> {
        // 1. Check if it's a local symbol within the referencing object
        for sym in &referencing_object.symbols {
            if sym.is_local() && sym.name == sym_name {
                if let Some(sec_idx) = sym.section_index {
                    for merged in merged_sections {
                        for contrib in &merged.contributions {
                            if contrib.object_id == referencing_object.id
                                && contrib.source_section_idx == sec_idx
                            {
                                return Some(
                                    merged.virtual_address
                                        + contrib.offset_in_merged as u64
                                        + sym.value,
                                );
                            }
                        }
                    }
                }
            }
        }

        // 1b. Check if sym_name refers to an input section name of referencing_object
        for merged in merged_sections {
            for contrib in &merged.contributions {
                if contrib.object_id == referencing_object.id
                    && contrib.source_section_name == sym_name
                {
                    return Some(merged.virtual_address + contrib.offset_in_merged as u64);
                }
            }
        }

        // 2. Otherwise look up in global resolved addresses
        self.resolved_addresses.get(sym_name).copied()
    }
}
