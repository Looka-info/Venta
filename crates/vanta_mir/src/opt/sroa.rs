//! Scalar Replacement of Aggregates (SROA) for Vanta MIR.
//!
//! Replaces eligible local, non-escaping, scalar-only aggregate (struct) slots
//! with independent scalar local slots. Field loads and stores are rewritten
//! into direct local scalar loads and stores, enabling subsequent Mem2Reg
//! passes to promote aggregate fields directly into SSA registers.

use crate::ir::*;
use std::collections::{HashMap, HashSet};
use vanta_sema::{EnumDef, StructDef, Type};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SroaStats {
    pub aggregates_examined: usize,
    pub aggregates_promoted: usize,
    pub fields_promoted: usize,
    pub loads_replaced: usize,
    pub stores_replaced: usize,
    pub aggregates_rejected: usize,
}

pub struct Sroa;

impl Sroa {
    /// Runs SROA across all functions in an MIR program.
    pub fn run_on_program(
        program: &mut MirProgram,
    ) -> (bool, SroaStats) {
        let mut overall_changed = false;
        let mut overall_stats = SroaStats::default();

        for func in &mut program.functions {
            let (changed, stats) =
                Self::run_on_function(func, &program.structs, &program.enums);
            if changed {
                overall_changed = true;
            }
            overall_stats.aggregates_examined += stats.aggregates_examined;
            overall_stats.aggregates_promoted += stats.aggregates_promoted;
            overall_stats.fields_promoted += stats.fields_promoted;
            overall_stats.loads_replaced += stats.loads_replaced;
            overall_stats.stores_replaced += stats.stores_replaced;
            overall_stats.aggregates_rejected += stats.aggregates_rejected;
        }

        (overall_changed, overall_stats)
    }

    /// Runs SROA on a single MIR function.
    pub fn run_on_function(
        func: &mut MirFunction,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
    ) -> (bool, SroaStats) {
        let mut stats = SroaStats::default();
        if func.slots.is_empty() || func.blocks.is_empty() {
            return (false, stats);
        }

        let param_slots: HashSet<SlotId> = func.params.iter().map(|(s, _)| *s).collect();

        // 1. Identify candidate struct slots
        let mut candidate_slots: Vec<SlotId> = Vec::new();
        for slot in &func.slots {
            if param_slots.contains(&slot.id) {
                continue;
            }
            if let Type::Struct(ref s_name) = slot.ty {
                stats.aggregates_examined += 1;
                if let Some(s_def) = structs.get(s_name) {
                    if Self::is_struct_eligible(s_def, structs, enums) {
                        candidate_slots.push(slot.id);
                    } else {
                        stats.aggregates_rejected += 1;
                    }
                } else {
                    stats.aggregates_rejected += 1;
                }
            }
        }

        if candidate_slots.is_empty() {
            return (false, stats);
        }

        // 2. Perform escape analysis for each candidate slot
        let mut promoted_slots = Vec::new();
        for &slot_id in &candidate_slots {
            let slot = func.slots.iter().find(|s| s.id == slot_id).unwrap();
            let struct_name = match &slot.ty {
                Type::Struct(name) => name,
                _ => unreachable!(),
            };
            let s_def = &structs[struct_name];

            if Self::is_slot_non_escaping(func, slot_id, s_def) {
                promoted_slots.push(slot_id);
            } else {
                stats.aggregates_rejected += 1;
            }
        }

        if promoted_slots.is_empty() {
            return (false, stats);
        }

        // 3. Promote eligible struct slots to scalar slots
        let mut overall_changed = false;
        let mut next_slot_id = func.slots.iter().map(|s| s.id.0).max().unwrap_or(0) + 1;

        for &slot_id in &promoted_slots {
            let slot_idx = func.slots.iter().position(|s| s.id == slot_id).unwrap();
            let slot = func.slots[slot_idx].clone();
            let struct_name = match &slot.ty {
                Type::Struct(name) => name,
                _ => unreachable!(),
            };
            let s_def = structs[struct_name].clone();

            // Create field slots
            let mut offset_to_slot: HashMap<usize, SlotId> = HashMap::new();
            let mut new_field_slots = Vec::new();

            for f in &s_def.fields {
                let f_slot_id = SlotId(next_slot_id);
                next_slot_id += 1;
                offset_to_slot.insert(f.offset, f_slot_id);

                new_field_slots.push(LocalSlot {
                    id: f_slot_id,
                    name: format!("{}_{}", slot.name, f.name),
                    ty: f.ty.clone(),
                    size: f.ty.size_with_context(structs, enums),
                    align: f.ty.alignment(),
                });
            }

            // Find all base_ptr values created from SlotAddr for this slot
            let mut base_ptrs = HashSet::new();
            for block in &func.blocks {
                for inst in &block.instructions {
                    if let Instruction::SlotAddr { dest, slot } = inst {
                        if *slot == slot_id {
                            base_ptrs.insert(*dest);
                        }
                    }
                }
            }

            // Map field_ptr values to their corresponding scalar slot
            let mut field_ptr_to_slot: HashMap<ValueId, SlotId> = HashMap::new();
            for block in &func.blocks {
                for inst in &block.instructions {
                    if let Instruction::FieldAddr {
                        dest,
                        base_ptr,
                        offset,
                    } = inst
                    {
                        if base_ptrs.contains(base_ptr) {
                            if let Some(&f_slot) = offset_to_slot.get(offset) {
                                field_ptr_to_slot.insert(*dest, f_slot);
                            }
                        }
                    }
                }
            }

            // Rewrite instructions:
            // LoadIndirect -> Load from field slot
            // StoreIndirect / StoreIndirect32 -> Store to field slot
            let mut dead_dests = HashSet::new();
            dead_dests.extend(base_ptrs.iter().copied());
            dead_dests.extend(field_ptr_to_slot.keys().copied());

            for block in &mut func.blocks {
                for inst in &mut block.instructions {
                    match inst {
                        Instruction::LoadIndirect { dest, addr, ty } => {
                            if let Some(&f_slot) = field_ptr_to_slot.get(addr) {
                                *inst = Instruction::Load {
                                    dest: *dest,
                                    slot: f_slot,
                                    ty: ty.clone(),
                                };
                                stats.loads_replaced += 1;
                            }
                        }
                        Instruction::StoreIndirect { addr, val }
                        | Instruction::StoreIndirect32 { addr, val } => {
                            if let Some(&f_slot) = field_ptr_to_slot.get(addr) {
                                *inst = Instruction::Store {
                                    slot: f_slot,
                                    val: *val,
                                };
                                stats.stores_replaced += 1;
                            }
                        }
                        _ => {}
                    }
                }

                // Remove now-dead FieldAddr and SlotAddr instructions
                block.instructions.retain(|inst| match inst {
                    Instruction::SlotAddr { slot, .. } if *slot == slot_id => false,
                    Instruction::FieldAddr { dest, .. } if dead_dests.contains(dest) => false,
                    _ => true,
                });
            }

            // Remove the aggregate slot from func.slots and append the new scalar slots
            func.slots.retain(|s| s.id != slot_id);
            func.slots.extend(new_field_slots);

            stats.aggregates_promoted += 1;
            stats.fields_promoted += s_def.fields.len();
            overall_changed = true;
        }

        (overall_changed, stats)
    }

    /// Checks if a struct is eligible for SROA (pure scalar fields, no destructors/drop).
    fn is_struct_eligible(
        s_def: &StructDef,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
    ) -> bool {
        if s_def.fields.is_empty() {
            return false;
        }

        // 256-bit vector types are native SIMD registers and must not be decomposed into scalars
        if s_def.name == "v256"
            || s_def.name == "Simd256"
            || s_def.name.ends_with("__v256")
            || s_def.name.ends_with("__Simd256")
        {
            return false;
        }

        for f in &s_def.fields {
            // Must not need drop
            if f.ty.needs_drop(structs, enums) {
                return false;
            }
            // Must be primitive scalar type
            if !matches!(
                f.ty,
                Type::I64
                    | Type::I32
                    | Type::U64
                    | Type::U32
                    | Type::I16
                    | Type::U16
                    | Type::I8
                    | Type::U8
                    | Type::Bool
            ) {
                return false;
            }
        }

        true
    }

    /// Verifies that the aggregate slot address never escapes.
    fn is_slot_non_escaping(
        func: &MirFunction,
        slot_id: SlotId,
        s_def: &StructDef,
    ) -> bool {
        // Direct loads or stores to the slot mean the aggregate is accessed as a whole
        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Store { slot, .. } | Instruction::Load { slot, .. } => {
                        if *slot == slot_id {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut base_ptrs = HashSet::new();

        // 1. Find all base pointers produced for this slot
        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::SlotAddr { dest, slot } = inst {
                    if *slot == slot_id {
                        base_ptrs.insert(*dest);
                    }
                }
            }
        }

        if base_ptrs.is_empty() {
            // Never addressed; safe
            return true;
        }

        // 2. Check all uses of base_ptrs: must only be used in FieldAddr
        let mut field_ptrs = HashSet::new();
        let valid_offsets: HashSet<usize> = s_def.fields.iter().map(|f| f.offset).collect();

        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::FieldAddr {
                        dest,
                        base_ptr,
                        offset,
                    } => {
                        if base_ptrs.contains(base_ptr) {
                            if !valid_offsets.contains(offset) {
                                return false; // invalid offset
                            }
                            field_ptrs.insert(*dest);
                        }
                    }
                    Instruction::Call { args, .. } => {
                        for a in args {
                            if base_ptrs.contains(a) {
                                return false; // escapes to call
                            }
                        }
                    }
                    Instruction::Store { val, .. } => {
                        if base_ptrs.contains(val) {
                            return false;
                        }
                    }
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
                        if base_ptrs.contains(addr) || base_ptrs.contains(val) {
                            return false;
                        }
                    }
                    Instruction::Copy { src, .. } => {
                        if base_ptrs.contains(src) {
                            return false;
                        }
                    }
                    Instruction::Phi { inputs, .. } => {
                        for (_, v) in inputs {
                            if base_ptrs.contains(v) {
                                return false;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ref term) = block.terminator {
                match term {
                    Terminator::Return(Some(val)) if base_ptrs.contains(val) => {
                        return false; // escapes via return
                    }
                    Terminator::CondBranch { cond, .. } if base_ptrs.contains(cond) => {
                        return false;
                    }
                    _ => {}
                }
            }
        }

        // 3. Check all uses of field_ptrs: must ONLY be addr in LoadIndirect or StoreIndirect
        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::LoadIndirect { addr, .. } => {
                        // Valid use as addr
                        let _ = addr;
                    }
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
                        if field_ptrs.contains(val) {
                            return false; // field pointer stored as data -> escapes
                        }
                        // Valid use as addr
                        let _ = addr;
                    }
                    Instruction::Call { args, .. } => {
                        for a in args {
                            if field_ptrs.contains(a) {
                                return false; // escapes to call
                            }
                        }
                    }
                    Instruction::Copy { src, .. } => {
                        if field_ptrs.contains(src) {
                            return false;
                        }
                    }
                    Instruction::Phi { inputs, .. } => {
                        for (_, v) in inputs {
                            if field_ptrs.contains(v) {
                                return false;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ref term) = block.terminator {
                match term {
                    Terminator::Return(Some(val)) if field_ptrs.contains(val) => {
                        return false;
                    }
                    _ => {}
                }
            }
        }

        true
    }
}
