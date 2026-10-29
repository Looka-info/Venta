//! Dependency-Aware Instruction Scheduling for Vanta MIR.
//!
//! Reorders independent instructions within each basic block to minimize critical path
//! execution latency and hide memory and arithmetic latencies before physical register allocation,
//! while strictly preserving RAW, WAR, and WAW data and memory dependencies.

use crate::ir::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstructionScheduleStats {
    pub blocks_scheduled: usize,
    pub instructions_scheduled: usize,
    pub instructions_reordered: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemEffect {
    None,
    ReadSlot(SlotId),
    WriteSlot(SlotId),
    ReadIndirect,
    WriteIndirect,
    Barrier, // Unknown side effects / function call
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SchedCandidate {
    node_idx: usize,
    height: u32,
    latency: u32,
    num_successors: usize,
    original_idx: usize,
}

impl Ord for SchedCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // Highest height first (critical path)
        self.height
            .cmp(&other.height)
            // Highest latency first
            .then_with(|| self.latency.cmp(&other.latency))
            // Most successors first
            .then_with(|| self.num_successors.cmp(&other.num_successors))
            // Lowest original index first (deterministic tie-breaker)
            .then_with(|| other.original_idx.cmp(&self.original_idx))
    }
}

impl PartialOrd for SchedCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct InstructionScheduler;

impl InstructionScheduler {
    /// Runs instruction scheduling on a single MIR function.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, InstructionScheduleStats) {
        let mut overall_changed = false;
        let mut stats = InstructionScheduleStats::default();

        for block in &mut func.blocks {
            if block.instructions.len() <= 1 {
                continue;
            }

            // Separate leading Phi instructions from non-Phi instructions
            let phi_count = block
                .instructions
                .iter()
                .take_while(|i| matches!(i, Instruction::Phi { .. }))
                .count();
            if block.instructions.len() - phi_count <= 1 {
                continue;
            }

            let non_phis: Vec<Instruction> = block.instructions.drain(phi_count..).collect();
            let n = non_phis.len();
            stats.instructions_scheduled += n;
            stats.blocks_scheduled += 1;

            // 1. Analyze defs, uses, and memory effects for each non-Phi instruction
            let mut defs: Vec<Vec<ValueId>> = Vec::with_capacity(n);
            let mut uses: Vec<Vec<ValueId>> = Vec::with_capacity(n);
            let mut mem_effects: Vec<MemEffect> = Vec::with_capacity(n);
            let mut latencies: Vec<u32> = Vec::with_capacity(n);
            let mut can_trap: Vec<bool> = Vec::with_capacity(n);

            for inst in &non_phis {
                let (d, u) = Self::get_defs_and_uses(inst);
                let mem = Self::get_mem_effect(inst);
                let lat = Self::get_latency(inst);
                let trap = Self::is_potentially_trapping(inst);

                defs.push(d);
                uses.push(u);
                mem_effects.push(mem);
                latencies.push(lat);
                can_trap.push(trap);
            }

            // 2. Build Dependency DAG: edges[u] contains all v where u must execute before v
            let mut edges: Vec<Vec<usize>> = vec![Vec::new(); n];
            let mut in_degrees: Vec<usize> = vec![0; n];

            // Map each defined ValueId to the instruction index that defines it
            let mut val_def_map: HashMap<ValueId, usize> = HashMap::new();
            for (i, d_list) in defs.iter().enumerate() {
                for &d in d_list {
                    val_def_map.insert(d, i);
                }
            }

            for v in 0..n {
                let mut pred_set = HashSet::new();

                // RAW Data Dependency
                for &u_val in &uses[v] {
                    if let Some(&u) = val_def_map.get(&u_val) {
                        if u < v {
                            pred_set.insert(u);
                        }
                    }
                }

                // Memory and Side-Effect Dependencies with all preceding instructions u < v
                for u in 0..v {
                    if pred_set.contains(&u) {
                        continue;
                    }

                    let u_mem = &mem_effects[u];
                    let v_mem = &mem_effects[v];

                    let mut has_mem_dep = false;

                    // Barrier (Call) constraints: Calls cannot reorder with any memory op or Call
                    if *u_mem == MemEffect::Barrier {
                        if *v_mem != MemEffect::None {
                            has_mem_dep = true;
                        }
                    } else if *v_mem == MemEffect::Barrier {
                        if *u_mem != MemEffect::None {
                            has_mem_dep = true;
                        }
                    } else {
                        match (u_mem, v_mem) {
                            (MemEffect::WriteSlot(s1), MemEffect::ReadSlot(s2))
                            | (MemEffect::ReadSlot(s1), MemEffect::WriteSlot(s2))
                            | (MemEffect::WriteSlot(s1), MemEffect::WriteSlot(s2)) => {
                                if s1 == s2 {
                                    has_mem_dep = true;
                                }
                            }
                            (MemEffect::WriteIndirect, MemEffect::ReadIndirect)
                            | (MemEffect::ReadIndirect, MemEffect::WriteIndirect)
                            | (MemEffect::WriteIndirect, MemEffect::WriteIndirect) => {
                                has_mem_dep = true;
                            }
                            (MemEffect::WriteIndirect, MemEffect::ReadSlot(_))
                            | (MemEffect::WriteIndirect, MemEffect::WriteSlot(_))
                            | (MemEffect::ReadSlot(_), MemEffect::WriteIndirect)
                            | (MemEffect::WriteSlot(_), MemEffect::WriteIndirect)
                            | (MemEffect::WriteSlot(_), MemEffect::ReadIndirect)
                            | (MemEffect::ReadIndirect, MemEffect::WriteSlot(_)) => {
                                // Conservative aliasing between stack and indirect memory
                                has_mem_dep = true;
                            }
                            _ => {}
                        }
                    }

                    // Trapping constraint: potentially trapping instructions must not reorder across side effects
                    if can_trap[v] && *u_mem != MemEffect::None {
                        has_mem_dep = true;
                    }
                    if can_trap[u] && *v_mem != MemEffect::None {
                        has_mem_dep = true;
                    }

                    if has_mem_dep {
                        pred_set.insert(u);
                    }
                }

                for &u in &pred_set {
                    edges[u].push(v);
                    in_degrees[v] += 1;
                }
            }

            // 3. Compute Critical Path Height for each instruction
            let mut heights: Vec<u32> = vec![0; n];
            // Compute heights in reverse topological order (bottom-up from n-1 to 0)
            for u in (0..n).rev() {
                let max_succ_height = edges[u].iter().map(|&v| heights[v]).max().unwrap_or(0);
                heights[u] = latencies[u] + max_succ_height;
            }

            // 4. List Scheduling with Priority Queue
            let mut ready_heap = BinaryHeap::new();
            for i in 0..n {
                if in_degrees[i] == 0 {
                    ready_heap.push(SchedCandidate {
                        node_idx: i,
                        height: heights[i],
                        latency: latencies[i],
                        num_successors: edges[i].len(),
                        original_idx: i,
                    });
                }
            }

            let mut scheduled_order: Vec<usize> = Vec::with_capacity(n);

            while let Some(cand) = ready_heap.pop() {
                let u = cand.node_idx;
                scheduled_order.push(u);

                for &v in &edges[u] {
                    in_degrees[v] -= 1;
                    if in_degrees[v] == 0 {
                        ready_heap.push(SchedCandidate {
                            node_idx: v,
                            height: heights[v],
                            latency: latencies[v],
                            num_successors: edges[v].len(),
                            original_idx: v,
                        });
                    }
                }
            }

            // In case of cycles (should not happen in DAG), fallback to appending any remaining
            if scheduled_order.len() < n {
                for i in 0..n {
                    if !scheduled_order.contains(&i) {
                        scheduled_order.push(i);
                    }
                }
            }

            // 5. Check if reordering occurred
            let mut reordered = false;
            for (idx, &orig_idx) in scheduled_order.iter().enumerate() {
                if idx != orig_idx {
                    reordered = true;
                    break;
                }
            }

            if reordered {
                overall_changed = true;
                stats.instructions_reordered += n;
            }

            // Assemble final instructions
            let mut final_instructions = Vec::with_capacity(phi_count + n);
            // Append phi instructions unchanged
            for _ in 0..phi_count {
                final_instructions.push(block.instructions.remove(0));
            }
            // Append scheduled non-phi instructions
            for &idx in &scheduled_order {
                final_instructions.push(non_phis[idx].clone());
            }

            block.instructions = final_instructions;
        }

        (overall_changed, stats)
    }

    /// Identifies all ValueIds defined and used by an instruction.
    fn get_defs_and_uses(inst: &Instruction) -> (Vec<ValueId>, Vec<ValueId>) {
        let mut defs = Vec::new();
        let mut uses = Vec::new();

        match inst {
            Instruction::ConstInt { dest, .. } | Instruction::ConstBool { dest, .. } => {
                defs.push(*dest);
            }
            Instruction::SlotAddr { dest, .. } => {
                defs.push(*dest);
            }
            Instruction::Load { dest, .. } => {
                defs.push(*dest);
            }
            Instruction::Store { val, .. } => {
                uses.push(*val);
            }
            Instruction::Binary { dest, lhs, rhs, .. } => {
                defs.push(*dest);
                uses.push(*lhs);
                uses.push(*rhs);
            }
            Instruction::Unary { dest, operand, .. } => {
                defs.push(*dest);
                uses.push(*operand);
            }
            Instruction::Call { dest, args, .. } => {
                if let Some(d) = dest {
                    defs.push(*d);
                }
                for a in args {
                    uses.push(*a);
                }
            }
            Instruction::IndexAddr { dest, index, .. } => {
                defs.push(*dest);
                uses.push(*index);
            }
            Instruction::IndexPtrAddr {
                dest,
                base_ptr,
                index,
                ..
            } => {
                defs.push(*dest);
                uses.push(*base_ptr);
                uses.push(*index);
            }
            Instruction::FieldAddr { dest, base_ptr, .. } => {
                defs.push(*dest);
                uses.push(*base_ptr);
            }
            Instruction::LoadIndirect { dest, addr, .. } => {
                defs.push(*dest);
                uses.push(*addr);
            }
            Instruction::StoreIndirect { addr, val }
            | Instruction::StoreIndirect32 { addr, val } => {
                uses.push(*addr);
                uses.push(*val);
            }
            Instruction::StrSlice {
                dest_ptr, dest_len, ..
            } => {
                defs.push(*dest_ptr);
                defs.push(*dest_len);
            }
            Instruction::StoreSlice { ptr, len, .. } => {
                uses.push(*ptr);
                uses.push(*len);
            }
            Instruction::LoadSlice {
                dest_ptr, dest_len, ..
            } => {
                defs.push(*dest_ptr);
                defs.push(*dest_len);
            }
            Instruction::Phi { dest, inputs, .. } => {
                defs.push(*dest);
                for (_, v) in inputs {
                    uses.push(*v);
                }
            }
            Instruction::Copy { dest, src, .. } => {
                defs.push(*dest);
                uses.push(*src);
            }
        }

        (defs, uses)
    }

    /// Identifies the memory effect classification of an instruction.
    fn get_mem_effect(inst: &Instruction) -> MemEffect {
        match inst {
            Instruction::Load { slot, .. } => MemEffect::ReadSlot(*slot),
            Instruction::Store { slot, .. } => MemEffect::WriteSlot(*slot),
            Instruction::LoadSlice { slot, .. } => MemEffect::ReadSlot(*slot),
            Instruction::StoreSlice { slot, .. } => MemEffect::WriteSlot(*slot),
            Instruction::LoadIndirect { .. } => MemEffect::ReadIndirect,
            Instruction::StoreIndirect { .. } | Instruction::StoreIndirect32 { .. } => {
                MemEffect::WriteIndirect
            }
            Instruction::Call { .. } => MemEffect::Barrier,
            _ => MemEffect::None,
        }
    }

    /// Estimated x86-64 execution latency for an instruction.
    fn get_latency(inst: &Instruction) -> u32 {
        match inst {
            Instruction::Binary { op, .. } => match op {
                MirBinaryOp::Div | MirBinaryOp::Rem => 20,
                MirBinaryOp::Mul => 3,
                _ => 1,
            },
            Instruction::Call { .. } => 15,
            Instruction::LoadIndirect { .. }
            | Instruction::LoadSlice { .. }
            | Instruction::Load { .. } => 4,
            _ => 1,
        }
    }

    /// Whether an instruction can potentially trap.
    fn is_potentially_trapping(inst: &Instruction) -> bool {
        match inst {
            Instruction::Binary { op, .. } => matches!(op, MirBinaryOp::Div | MirBinaryOp::Rem),
            Instruction::Call { .. } => true,
            _ => false,
        }
    }
}
