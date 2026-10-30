//! Linear scan register allocator for AArch64 backend.
//!
//! Allocates physical AArch64 registers for MIR virtual ValueIds while strictly
//! preserving the Apple Darwin ARM64 ABI:
//! - Uses X9..X15 for caller-saved temporaries.
//! - Uses X19..X28 for callee-saved values across function calls.
//! - Strictly excludes X18 (Apple OS platform reserved).
//! - Reserves X16 and X17 as code emission scratches.

use crate::register::AArch64Reg;
use std::collections::{HashMap, HashSet};
use vanta_mir::{BlockId, Instruction, MirFunction, Terminator, ValueId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmLocation {
    Reg(AArch64Reg),
    Spill(i32),
}

#[derive(Debug, Clone)]
pub struct ArmLiveInterval {
    pub val: ValueId,
    pub start: usize,
    pub end: usize,
    pub does_cross_call: bool,
}

#[derive(Debug)]
pub struct ArmAllocationResult {
    pub assignments: HashMap<ValueId, ArmLocation>,
    pub used_callee_saved: Vec<AArch64Reg>,
    pub num_spills: usize,
}

pub struct ArmRegisterAllocator;

impl ArmRegisterAllocator {
    pub fn allocate(func: &MirFunction) -> ArmAllocationResult {
        let intervals = Self::compute_live_intervals(func);
        Self::linear_scan(intervals)
    }

    pub fn allocate_intervals(intervals: Vec<ArmLiveInterval>) -> ArmAllocationResult {
        Self::linear_scan(intervals)
    }

    pub fn compute_live_intervals(func: &MirFunction) -> Vec<ArmLiveInterval> {
        let mut block_def: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
        let mut block_use: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
        let mut all_vals: HashSet<ValueId> = HashSet::new();

        let mut pred_phi_uses: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::Phi { inputs, .. } = inst {
                    for (pred, val) in inputs {
                        pred_phi_uses.entry(*pred).or_default().insert(*val);
                    }
                }
            }
        }

        let mut call_points: Vec<usize> = Vec::new();
        let mut point = 0usize;
        let mut first_seen: HashMap<ValueId, usize> = HashMap::new();
        let mut last_seen: HashMap<ValueId, usize> = HashMap::new();
        let mut block_ranges: HashMap<BlockId, (usize, usize)> = HashMap::new();

        for block in &func.blocks {
            let b_start = point;
            let mut defs = HashSet::new();
            let mut uses = HashSet::new();

            for inst in &block.instructions {
                point += 2;
                if let Instruction::Call { .. } | Instruction::CallIndirect { .. } = inst {
                    call_points.push(point);
                }

                for u in Self::get_inst_uses(inst) {
                    all_vals.insert(u);
                    first_seen.entry(u).or_insert(point);
                    last_seen.insert(u, point);
                    if !defs.contains(&u) {
                        uses.insert(u);
                    }
                }
                for d in Self::get_inst_defs(inst) {
                    all_vals.insert(d);
                    first_seen.entry(d).or_insert(point);
                    last_seen.insert(d, point);
                    defs.insert(d);
                }
            }

            if let Some(ref term) = block.terminator {
                point += 2;
                for u in Self::get_term_uses(term) {
                    all_vals.insert(u);
                    first_seen.entry(u).or_insert(point);
                    last_seen.insert(u, point);
                    if !defs.contains(&u) {
                        uses.insert(u);
                    }
                }
            }

            if let Some(phi_uses) = pred_phi_uses.get(&block.id) {
                for u in phi_uses {
                    all_vals.insert(*u);
                    first_seen.entry(*u).or_insert(point);
                    last_seen.insert(*u, point);
                    if !defs.contains(u) {
                        uses.insert(*u);
                    }
                }
            }

            block_def.insert(block.id, defs);
            block_use.insert(block.id, uses);
            block_ranges.insert(block.id, (b_start, point));
        }

        // Live-in and live-out dataflow iteration
        let mut live_in: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
        let mut live_out: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
        let mut changed = true;

        while changed {
            changed = false;
            for block in func.blocks.iter().rev() {
                let b_id = block.id;
                let succs = if let Some(ref term) = block.terminator {
                    Self::get_term_successors(term)
                } else {
                    vec![]
                };

                let mut out_set = HashSet::new();
                for s in succs {
                    if let Some(in_s) = live_in.get(&s) {
                        out_set.extend(in_s.iter().copied());
                    }
                }

                let mut in_set = block_use.get(&b_id).cloned().unwrap_or_default();
                for v in &out_set {
                    if !block_def.get(&b_id).map(|d| d.contains(v)).unwrap_or(false) {
                        in_set.insert(*v);
                    }
                }

                if live_out.get(&b_id) != Some(&out_set) {
                    live_out.insert(b_id, out_set);
                    changed = true;
                }
                if live_in.get(&b_id) != Some(&in_set) {
                    live_in.insert(b_id, in_set);
                    changed = true;
                }
            }
        }

        let mut intervals = Vec::with_capacity(all_vals.len());
        for v in all_vals {
            let start = first_seen.get(&v).copied().unwrap_or(0);
            let mut end = last_seen.get(&v).copied().unwrap_or(start);

            for block in &func.blocks {
                if let Some(out_s) = live_out.get(&block.id) {
                    if out_s.contains(&v) {
                        if let Some(&(_, b_end)) = block_ranges.get(&block.id) {
                            if b_end > end {
                                end = b_end;
                            }
                        }
                    }
                }
            }

            let does_cross_call = call_points.iter().any(|&cp| cp > start && cp <= end);

            intervals.push(ArmLiveInterval {
                val: v,
                start,
                end,
                does_cross_call,
            });
        }

        intervals
    }

    fn linear_scan(mut intervals: Vec<ArmLiveInterval>) -> ArmAllocationResult {
        intervals.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.val.0.cmp(&b.val.0)));

        let mut free_callee_saved: Vec<AArch64Reg> = AArch64Reg::CALLEE_SAVED_REGS.to_vec();
        let mut free_caller_saved: Vec<AArch64Reg> = AArch64Reg::CALLER_SAVED_REGS.to_vec();

        #[derive(Debug)]
        struct Active {
            #[allow(dead_code)]
            val: ValueId,
            end: usize,
            reg: AArch64Reg,
            is_callee_saved: bool,
        }

        let mut active: Vec<Active> = Vec::new();
        let mut assignments: HashMap<ValueId, ArmLocation> = HashMap::new();
        let mut used_callee_saved_set: HashSet<AArch64Reg> = HashSet::new();
        let mut next_spill_slot = 0;

        for iv in intervals {
            let mut i = 0;
            while i < active.len() {
                if active[i].end < iv.start {
                    let expired = active.remove(i);
                    if expired.is_callee_saved {
                        free_callee_saved.push(expired.reg);
                    } else {
                        free_caller_saved.push(expired.reg);
                    }
                } else {
                    i += 1;
                }
            }

            let chosen_reg: Option<(AArch64Reg, bool)> = if iv.does_cross_call {
                if let Some(r) = free_callee_saved.pop() {
                    used_callee_saved_set.insert(r);
                    Some((r, true))
                } else {
                    None
                }
            } else if let Some(r) = free_caller_saved.pop() {
                Some((r, false))
            } else if let Some(r) = free_callee_saved.pop() {
                used_callee_saved_set.insert(r);
                Some((r, true))
            } else {
                None
            };

            if let Some((reg, is_callee)) = chosen_reg {
                assignments.insert(iv.val, ArmLocation::Reg(reg));
                active.push(Active {
                    val: iv.val,
                    end: iv.end,
                    reg,
                    is_callee_saved: is_callee,
                });
            } else {
                assignments.insert(iv.val, ArmLocation::Spill(next_spill_slot));
                next_spill_slot += 1;
            }
        }

        let mut used_callee_saved: Vec<AArch64Reg> = used_callee_saved_set.into_iter().collect();
        used_callee_saved.sort_by_key(|r| r.hw_encoding());

        ArmAllocationResult {
            assignments,
            used_callee_saved,
            num_spills: next_spill_slot as usize,
        }
    }

    fn get_inst_defs(inst: &Instruction) -> Vec<ValueId> {
        match inst {
            Instruction::ConstInt { dest, .. }
            | Instruction::ConstBool { dest, .. }
            | Instruction::SlotAddr { dest, .. }
            | Instruction::FieldAddr { dest, .. }
            | Instruction::Load { dest, .. }
            | Instruction::Binary { dest, .. }
            | Instruction::Unary { dest, .. }
            | Instruction::IndexAddr { dest, .. }
            | Instruction::IndexPtrAddr { dest, .. }
            | Instruction::LoadIndirect { dest, .. }
            | Instruction::Phi { dest, .. }
            | Instruction::Copy { dest, .. } => vec![*dest],
            Instruction::StrSlice {
                dest_ptr, dest_len, ..
            }
            | Instruction::LoadSlice {
                dest_ptr, dest_len, ..
            } => vec![*dest_ptr, *dest_len],
            Instruction::Call { dest: Some(d), .. }
            | Instruction::CallIndirect { dest: Some(d), .. } => vec![*d],
            Instruction::VtableAddr { dest, .. } => vec![*dest],
            _ => vec![],
        }
    }

    fn get_inst_uses(inst: &Instruction) -> Vec<ValueId> {
        match inst {
            Instruction::Store { val, .. } => vec![*val],
            Instruction::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
            Instruction::Unary { operand, .. } => vec![*operand],
            Instruction::Call { args, .. } => args.clone(),
            Instruction::CallIndirect {
                callee_ptr, args, ..
            } => {
                let mut u = vec![*callee_ptr];
                u.extend_from_slice(args);
                u
            }
            Instruction::IndexAddr { index, .. } => vec![*index],
            Instruction::IndexPtrAddr {
                base_ptr, index, ..
            } => vec![*base_ptr, *index],
            Instruction::FieldAddr { base_ptr, .. } => vec![*base_ptr],
            Instruction::LoadIndirect { addr, .. } => vec![*addr],
            Instruction::StoreIndirect { addr, val }
            | Instruction::StoreIndirect32 { addr, val } => vec![*addr, *val],
            Instruction::StoreSlice { ptr, len, .. } => vec![*ptr, *len],
            Instruction::Copy { src, .. } => vec![*src],
            _ => vec![],
        }
    }

    fn get_term_uses(term: &Terminator) -> Vec<ValueId> {
        match term {
            Terminator::CondBranch { cond, .. } => vec![*cond],
            Terminator::Return(Some(val)) => vec![*val],
            _ => vec![],
        }
    }

    fn get_term_successors(term: &Terminator) -> Vec<BlockId> {
        match term {
            Terminator::Branch(target) => vec![*target],
            Terminator::CondBranch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            Terminator::Return(_) | Terminator::Unreachable => vec![],
        }
    }
}
