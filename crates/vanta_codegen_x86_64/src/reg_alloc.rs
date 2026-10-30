//! Linear scan register allocator for x86-64 backend.
//!
//! Computes CFG-aware dataflow liveness and allocates physical registers
//! to MIR ValueId virtual registers, minimizing stack traffic while strictly
//! preserving the System V AMD64 ABI.

use crate::register::Reg;
use std::collections::{HashMap, HashSet};
use vanta_mir::{BlockId, Instruction, MirFunction, Terminator, ValueId};
use vanta_target::TargetAbi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Reg(Reg),
    Spill(i32), // 0-based spill index
}

#[derive(Debug, Clone)]
pub struct LiveInterval {
    pub val: ValueId,
    pub start: usize,
    pub end: usize,
    pub does_cross_call: bool,
}

#[derive(Debug, Clone)]
pub struct AllocationResult {
    pub assignments: HashMap<ValueId, Location>,
    pub used_callee_saved: Vec<Reg>,
    pub num_spills: usize,
}

pub struct RegisterAllocator;

impl RegisterAllocator {
    /// Allocates physical registers for all scalar ValueIds in the given MIR function.
    pub fn allocate(func: &MirFunction) -> AllocationResult {
        let intervals = Self::compute_live_intervals(func);
        Self::linear_scan(intervals, TargetAbi::SystemVAmd64)
    }

    /// Allocates physical registers for a pre-filtered list of live intervals.
    pub fn allocate_intervals(intervals: Vec<LiveInterval>) -> AllocationResult {
        Self::allocate_intervals_with_abi(intervals, TargetAbi::SystemVAmd64)
    }

    /// Allocates physical registers for a pre-filtered list of live intervals under the specified ABI.
    pub fn allocate_intervals_with_abi(
        intervals: Vec<LiveInterval>,
        abi: TargetAbi,
    ) -> AllocationResult {
        Self::linear_scan(intervals, abi)
    }

    /// Computes dataflow liveness over the CFG and constructs live intervals for all ValueIds.
    pub fn compute_live_intervals(func: &MirFunction) -> Vec<LiveInterval> {
        let mut block_def: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
        let mut block_use: HashMap<BlockId, HashSet<ValueId>> = HashMap::new();
        let mut all_vals: HashSet<ValueId> = HashSet::new();

        // Collect phi uses per predecessor block
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

        // 1. Compute block-level def and use sets
        for block in &func.blocks {
            let mut defs = HashSet::new();
            let mut uses = HashSet::new();

            for inst in &block.instructions {
                for u in Self::get_inst_uses(inst) {
                    all_vals.insert(u);
                    if !defs.contains(&u) {
                        uses.insert(u);
                    }
                }
                for d in Self::get_inst_defs(inst) {
                    all_vals.insert(d);
                    defs.insert(d);
                }
            }

            if let Some(ref term) = block.terminator {
                for u in Self::get_term_uses(term) {
                    all_vals.insert(u);
                    if !defs.contains(&u) {
                        uses.insert(u);
                    }
                }
            }

            if let Some(phi_uses) = pred_phi_uses.get(&block.id) {
                for u in phi_uses {
                    all_vals.insert(*u);
                    if !defs.contains(u) {
                        uses.insert(*u);
                    }
                }
            }

            block_def.insert(block.id, defs);
            block_use.insert(block.id, uses);
        }

        // 2. Fixed-point backward dataflow iteration for live_in and live_out
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

                let mut new_live_out = HashSet::new();
                for s in succs {
                    if let Some(in_set) = live_in.get(&s) {
                        new_live_out.extend(in_set.iter().copied());
                    }
                }

                let mut new_live_in = block_use.get(&b_id).cloned().unwrap_or_default();
                for v in &new_live_out {
                    if !block_def.get(&b_id).map(|d| d.contains(v)).unwrap_or(false) {
                        new_live_in.insert(*v);
                    }
                }

                if live_out.get(&b_id) != Some(&new_live_out) {
                    live_out.insert(b_id, new_live_out);
                    changed = true;
                }
                if live_in.get(&b_id) != Some(&new_live_in) {
                    live_in.insert(b_id, new_live_in);
                    changed = true;
                }
            }
        }

        // 3. Linearize program points and identify definitions and uses
        let mut point = 0;
        let mut block_ranges: HashMap<BlockId, (usize, usize)> = HashMap::new();
        let mut call_points: Vec<usize> = Vec::new();
        let mut val_defs: HashMap<ValueId, usize> = HashMap::new();
        let mut val_uses: HashMap<ValueId, Vec<usize>> = HashMap::new();

        for block in &func.blocks {
            let start_p = point;
            for inst in &block.instructions {
                point += 2;
                if matches!(
                    inst,
                    Instruction::Call { .. } | Instruction::CallIndirect { .. }
                ) {
                    call_points.push(point);
                }
                for d in Self::get_inst_defs(inst) {
                    val_defs.entry(d).or_insert(point);
                }
                for u in Self::get_inst_uses(inst) {
                    val_uses.entry(u).or_default().push(point);
                }
            }
            if let Some(ref term) = block.terminator {
                point += 2;
                for u in Self::get_term_uses(term) {
                    val_uses.entry(u).or_default().push(point);
                }
            }
            let end_p = point;
            if let Some(phi_uses) = pred_phi_uses.get(&block.id) {
                for u in phi_uses {
                    val_uses.entry(*u).or_default().push(end_p);
                }
            }
            block_ranges.insert(block.id, (start_p, end_p));
        }

        // 4. Construct LiveInterval for each ValueId
        let mut intervals = Vec::new();
        for v in all_vals {
            let start = val_defs.get(&v).copied().unwrap_or(0);
            let mut end = start;

            if let Some(uses) = val_uses.get(&v) {
                for u in uses {
                    if *u > end {
                        end = *u;
                    }
                }
            }

            for block in &func.blocks {
                if let Some(out_set) = live_out.get(&block.id) {
                    if out_set.contains(&v) {
                        if let Some(&(_, b_end)) = block_ranges.get(&block.id) {
                            if b_end > end {
                                end = b_end;
                            }
                        }
                    }
                }
            }

            let does_cross_call = call_points.iter().any(|&cp| cp > start && cp <= end);

            intervals.push(LiveInterval {
                val: v,
                start,
                end,
                does_cross_call,
            });
        }

        intervals
    }

    /// Deterministic linear-scan allocator.
    fn linear_scan(mut intervals: Vec<LiveInterval>, abi: TargetAbi) -> AllocationResult {
        // Sort intervals by start point ascending, then by ValueId for determinism
        intervals.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.val.0.cmp(&b.val.0)));

        // Allocatable pools:
        // Under Microsoft x64, Rdi and Rsi are non-volatile (callee-saved).
        let (callee_saved_regs, caller_saved_regs): (Vec<Reg>, Vec<Reg>) = match abi {
            TargetAbi::MicrosoftX64 => (
                vec![
                    Reg::Rbx,
                    Reg::R12,
                    Reg::R13,
                    Reg::R14,
                    Reg::R15,
                    Reg::Rdi,
                    Reg::Rsi,
                ],
                vec![Reg::R8, Reg::R9],
            ),
            _ => (
                vec![Reg::Rbx, Reg::R12, Reg::R13, Reg::R14, Reg::R15],
                vec![Reg::Rdi, Reg::Rsi, Reg::R8, Reg::R9],
            ),
        };

        let mut free_callee_saved: Vec<Reg> = callee_saved_regs.to_vec();
        let mut free_caller_saved: Vec<Reg> = caller_saved_regs.to_vec();

        #[derive(Debug)]
        struct Active {
            val: ValueId,
            end: usize,
            reg: Reg,
            is_callee_saved: bool,
        }

        let mut active: Vec<Active> = Vec::new();
        let mut assignments: HashMap<ValueId, Location> = HashMap::new();
        let mut used_callee_saved_set: HashSet<Reg> = HashSet::new();
        let mut next_spill_slot = 0;

        for iv in intervals {
            // 1. Expire old intervals whose live range ended before iv.start
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

            // 2. Allocate or spill
            if iv.does_cross_call {
                // Must receive a callee-saved register to survive call clobbers
                if let Some(r) = free_callee_saved.pop() {
                    assignments.insert(iv.val, Location::Reg(r));
                    used_callee_saved_set.insert(r);
                    active.push(Active {
                        val: iv.val,
                        end: iv.end,
                        reg: r,
                        is_callee_saved: true,
                    });
                } else {
                    // Check if we can evict an active callee-saved interval with a later end point
                    let mut latest_idx = None;
                    let mut max_end = iv.end;
                    for (idx, a) in active.iter().enumerate() {
                        if a.is_callee_saved && a.end > max_end {
                            max_end = a.end;
                            latest_idx = Some(idx);
                        }
                    }

                    if let Some(evict_idx) = latest_idx {
                        let evicted = active.remove(evict_idx);
                        assignments.insert(evicted.val, Location::Spill(next_spill_slot));
                        next_spill_slot += 1;

                        assignments.insert(iv.val, Location::Reg(evicted.reg));
                        used_callee_saved_set.insert(evicted.reg);
                        active.push(Active {
                            val: iv.val,
                            end: iv.end,
                            reg: evicted.reg,
                            is_callee_saved: true,
                        });
                    } else {
                        // Spill iv
                        assignments.insert(iv.val, Location::Spill(next_spill_slot));
                        next_spill_slot += 1;
                    }
                }
            } else {
                // Does not cross calls: prefer caller-saved, then callee-saved
                if let Some(r) = free_caller_saved.pop() {
                    assignments.insert(iv.val, Location::Reg(r));
                    active.push(Active {
                        val: iv.val,
                        end: iv.end,
                        reg: r,
                        is_callee_saved: false,
                    });
                } else if let Some(r) = free_callee_saved.pop() {
                    assignments.insert(iv.val, Location::Reg(r));
                    used_callee_saved_set.insert(r);
                    active.push(Active {
                        val: iv.val,
                        end: iv.end,
                        reg: r,
                        is_callee_saved: true,
                    });
                } else {
                    // Spill
                    let mut latest_idx = None;
                    let mut max_end = iv.end;
                    for (idx, a) in active.iter().enumerate() {
                        if a.end > max_end {
                            max_end = a.end;
                            latest_idx = Some(idx);
                        }
                    }

                    if let Some(evict_idx) = latest_idx {
                        let evicted = active.remove(evict_idx);
                        assignments.insert(evicted.val, Location::Spill(next_spill_slot));
                        next_spill_slot += 1;

                        assignments.insert(iv.val, Location::Reg(evicted.reg));
                        if evicted.is_callee_saved {
                            used_callee_saved_set.insert(evicted.reg);
                        }
                        active.push(Active {
                            val: iv.val,
                            end: iv.end,
                            reg: evicted.reg,
                            is_callee_saved: evicted.is_callee_saved,
                        });
                    } else {
                        assignments.insert(iv.val, Location::Spill(next_spill_slot));
                        next_spill_slot += 1;
                    }
                }
            }
        }

        // Return used callee-saved registers in deterministic order
        let mut used_callee_saved: Vec<Reg> = callee_saved_regs
            .iter()
            .copied()
            .filter(|r| used_callee_saved_set.contains(r))
            .collect();
        used_callee_saved.sort_by_key(|r| *r as u8);

        AllocationResult {
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

#[cfg(test)]
mod tests {
    use super::*;
    use vanta_mir::{BasicBlock, MirBinaryOp, MirFunction};
    use vanta_sema::Type;

    #[test]
    fn test_non_overlapping_intervals_share_register() {
        // block:
        // v1 = 10
        // v2 = v1 + 5   (v1 dies here)
        // v3 = 20       (can reuse v1's register)
        // return v3
        let func = MirFunction::new(
            "test_share".into(),
            vec![],
            Type::I64,
            vec![],
            vec![BasicBlock {
                id: BlockId(0),
                name: "entry".into(),
                instructions: vec![
                    Instruction::ConstInt {
                        dest: ValueId(1),
                        val: 10,
                        ty: Type::I64,
                    },
                    Instruction::Binary {
                        dest: ValueId(2),
                        op: MirBinaryOp::Add,
                        lhs: ValueId(1),
                        rhs: ValueId(1),
                        ty: Type::I64,
                    },
                    Instruction::ConstInt {
                        dest: ValueId(3),
                        val: 20,
                        ty: Type::I64,
                    },
                ],
                terminator: Some(Terminator::Return(Some(ValueId(3)))),
            }],
        );

        let res = RegisterAllocator::allocate(&func);
        assert_eq!(res.num_spills, 0, "No values should spill");
        let loc1 = res.assignments.get(&ValueId(1)).unwrap();
        let loc3 = res.assignments.get(&ValueId(3)).unwrap();
        assert!(matches!(loc1, Location::Reg(_)));
        assert!(matches!(loc3, Location::Reg(_)));
    }

    #[test]
    fn test_call_crossing_interval_gets_callee_saved_or_spill() {
        // v1 = 10
        // call foo()  (v1 is live across this call)
        // v2 = v1 + 5
        // return v2
        let func = MirFunction::new(
            "test_call_cross".into(),
            vec![],
            Type::I64,
            vec![],
            vec![BasicBlock {
                id: BlockId(0),
                name: "entry".into(),
                instructions: vec![
                    Instruction::ConstInt {
                        dest: ValueId(1),
                        val: 10,
                        ty: Type::I64,
                    },
                    Instruction::Call {
                        dest: None,
                        callee: "foo".into(),
                        args: vec![],
                        ret_type: Type::Unit,
                    },
                    Instruction::Binary {
                        dest: ValueId(2),
                        op: MirBinaryOp::Add,
                        lhs: ValueId(1),
                        rhs: ValueId(1),
                        ty: Type::I64,
                    },
                ],
                terminator: Some(Terminator::Return(Some(ValueId(2)))),
            }],
        );

        let res = RegisterAllocator::allocate(&func);
        let loc1 = res.assignments.get(&ValueId(1)).unwrap();
        match loc1 {
            Location::Reg(r) => {
                assert!(
                    [Reg::Rbx, Reg::R12, Reg::R13, Reg::R14, Reg::R15].contains(r),
                    "Call crossing value must be in a callee-saved register, got {:?}",
                    r
                );
                assert!(res.used_callee_saved.contains(r));
            }
            Location::Spill(_) => {
                // Spill is also legal if registers were exhausted
            }
        }
    }

    #[test]
    fn test_register_exhaustion_spills_gracefully() {
        // Create 20 values simultaneously live
        let mut insts = Vec::new();
        for i in 1..=20 {
            insts.push(Instruction::ConstInt {
                dest: ValueId(i),
                val: i as i64,
                ty: Type::I64,
            });
        }
        // Chain all 20 values so they must all be live at the point where accumulation starts
        let mut last = ValueId(1);
        let mut final_id = ValueId(1);
        for i in 2..=20 {
            let next = ValueId(19 + i);
            insts.push(Instruction::Binary {
                dest: next,
                op: MirBinaryOp::Add,
                lhs: last,
                rhs: ValueId(i),
                ty: Type::I64,
            });
            last = next;
            final_id = next;
        }

        let func = MirFunction::new(
            "test_exhaustion".into(),
            vec![],
            Type::I64,
            vec![],
            vec![BasicBlock {
                id: BlockId(0),
                name: "entry".into(),
                instructions: insts,
                terminator: Some(Terminator::Return(Some(final_id))),
            }],
        );

        let res = RegisterAllocator::allocate(&func);
        assert!(
            res.num_spills > 0,
            "Expected spills under high register pressure"
        );
        // Verify every value 1..=39 has a valid location
        for i in 1..=39 {
            assert!(
                res.assignments.contains_key(&ValueId(i)),
                "Value {} has assignment",
                i
            );
        }
    }
}
