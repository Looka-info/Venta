//! Loop Invariant Code Motion (LICM) and Natural Loop Analysis for Vanta MIR.
//!
//! Detects natural loops using dominator analysis, builds or reuses loop preheaders,
//! identifies pure speculation-safe loop-invariant computations, and hoists them
//! outside loop bodies to eliminate redundant per-iteration work.

use crate::ir::*;
use crate::opt::dom::DominatorTree;
use std::collections::HashSet;

/// Represents a detected natural loop in the control-flow graph.
#[derive(Debug, Clone)]
pub struct NaturalLoop {
    /// The entry basic block of the loop (dominates all loop members).
    pub header: BlockId,
    /// All basic blocks belonging to the loop body.
    pub blocks: HashSet<BlockId>,
    /// Backedge source blocks branching back to `header`.
    pub latches: Vec<BlockId>,
    /// Exit edges leaving the loop: (source_block_in_loop, destination_block_outside_loop).
    pub exits: Vec<(BlockId, BlockId)>,
}

impl NaturalLoop {
    /// Discovers all natural loops in a function using dominator analysis.
    pub fn discover_loops(func: &MirFunction, dom_tree: &DominatorTree) -> Vec<Self> {
        let mut loops = Vec::new();

        for block in &func.blocks {
            let h = block.id;

            // Find backedges targeting `h`: predecessors `l` where `h` dominates `l`
            let preds = match dom_tree.predecessors.get(&h) {
                Some(p) => p,
                None => continue,
            };

            let mut latches = Vec::new();
            for &l in preds {
                if dom_tree.dominates(h, l) {
                    latches.push(l);
                }
            }

            if latches.is_empty() {
                continue;
            }

            // Collect natural loop blocks: h plus all blocks that can reach any latch without going through h
            let mut loop_blocks = HashSet::new();
            loop_blocks.insert(h);

            let mut worklist = Vec::new();
            for &latch in &latches {
                if loop_blocks.insert(latch) {
                    worklist.push(latch);
                }
            }

            while let Some(curr) = worklist.pop() {
                if let Some(preds) = dom_tree.predecessors.get(&curr) {
                    for &p in preds {
                        if loop_blocks.insert(p) {
                            worklist.push(p);
                        }
                    }
                }
            }

            // Verify reducible property: h must dominate all member blocks
            let is_natural = loop_blocks.iter().all(|&b| dom_tree.dominates(h, b));
            if !is_natural {
                continue;
            }

            // Collect exit edges: (b_in_loop, s_outside_loop)
            let mut exits = Vec::new();
            for &b in &loop_blocks {
                if let Some(succs) = dom_tree.successors.get(&b) {
                    for &s in succs {
                        if !loop_blocks.contains(&s) {
                            exits.push((b, s));
                        }
                    }
                }
            }

            loops.push(NaturalLoop {
                header: h,
                blocks: loop_blocks,
                latches,
                exits,
            });
        }

        // Sort loops from innermost (smallest number of blocks) to outermost
        loops.sort_by_key(|l| l.blocks.len());
        loops
    }

    /// Obtains an existing suitable preheader or inserts a dedicated preheader block.
    pub fn get_or_create_preheader(
        func: &mut MirFunction,
        loop_info: &NaturalLoop,
        dom_tree: &DominatorTree,
    ) -> (BlockId, bool) {
        let header_id = loop_info.header;

        // External predecessors entering the header
        let external_preds: Vec<BlockId> = dom_tree
            .predecessors
            .get(&header_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
            .iter()
            .copied()
            .filter(|p| !loop_info.blocks.contains(p))
            .collect();

        // Check if there is already a unique dedicated preheader block
        if external_preds.len() == 1 {
            let ext_id = external_preds[0];
            if let Some(ext_block) = func.blocks.iter().find(|b| b.id == ext_id) {
                // Can reuse if it unconditionally branches only to header
                if ext_block.terminator == Some(Terminator::Branch(header_id))
                    && !loop_info.blocks.contains(&ext_id)
                {
                    return (ext_id, false);
                }
            }
        }

        // Create a new dedicated preheader block
        let next_b_id = BlockId(func.max_block_id() + 1);
        let header_name = func
            .blocks
            .iter()
            .find(|b| b.id == header_id)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| format!("b{}", header_id.0));

        let preheader_block = BasicBlock {
            id: next_b_id,
            name: format!("{}_preheader", header_name),
            instructions: Vec::new(),
            terminator: Some(Terminator::Branch(header_id)),
        };

        // Insert preheader immediately before the loop header in func.blocks
        let h_idx = func.blocks.iter().position(|b| b.id == header_id).unwrap();
        func.blocks.insert(h_idx, preheader_block);

        // Redirect external predecessors to target preheader instead of header
        for block in &mut func.blocks {
            if block.id == next_b_id || loop_info.blocks.contains(&block.id) {
                continue;
            }
            if let Some(ref mut term) = block.terminator {
                match term {
                    Terminator::Branch(target) => {
                        if *target == header_id {
                            *target = next_b_id;
                        }
                    }
                    Terminator::CondBranch {
                        then_block,
                        else_block,
                        ..
                    } => {
                        if *then_block == header_id {
                            *then_block = next_b_id;
                        }
                        if *else_block == header_id {
                            *else_block = next_b_id;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Update Phi nodes in loop header
        let mut next_val_id = func.max_value_id() + 1;
        let mut preheader_phis = Vec::new();

        let header_block = func.blocks.iter_mut().find(|b| b.id == header_id).unwrap();
        for inst in &mut header_block.instructions {
            if let Instruction::Phi { inputs, ty, .. } = inst {
                let mut new_inputs = Vec::new();
                let mut ext_inputs = Vec::new();

                for (pred, val) in inputs.drain(..) {
                    if loop_info.blocks.contains(&pred) {
                        new_inputs.push((pred, val));
                    } else {
                        ext_inputs.push((pred, val));
                    }
                }

                if ext_inputs.len() == 1 {
                    new_inputs.push((next_b_id, ext_inputs[0].1));
                } else if ext_inputs.len() > 1 {
                    let pre_phi_dest = ValueId(next_val_id);
                    next_val_id += 1;
                    preheader_phis.push(Instruction::Phi {
                        dest: pre_phi_dest,
                        inputs: ext_inputs,
                        ty: ty.clone(),
                    });
                    new_inputs.push((next_b_id, pre_phi_dest));
                }

                *inputs = new_inputs;
            }
        }

        if !preheader_phis.is_empty() {
            let preheader = func.blocks.iter_mut().find(|b| b.id == next_b_id).unwrap();
            preheader.instructions.splice(0..0, preheader_phis);
        }

        (next_b_id, true)
    }
}

/// Statistics collected during LICM optimization.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LicmStats {
    pub loops_detected: usize,
    pub loops_transformed: usize,
    pub preheaders_created: usize,
    pub candidates_examined: usize,
    pub instructions_hoisted: usize,
    pub safety_rejections: usize,
}

pub struct Licm;

impl Licm {
    /// Maximum number of LICM iteration rounds per function.
    pub const MAX_LICM_ROUNDS: usize = 6;

    /// Runs LICM across all functions in an MIR program.
    pub fn run_on_program(program: &mut MirProgram) -> (bool, LicmStats) {
        let mut overall_changed = false;
        let mut overall_stats = LicmStats::default();

        for func in &mut program.functions {
            let (changed, stats) = Self::run_on_function(func);
            if changed {
                overall_changed = true;
            }
            overall_stats.loops_detected += stats.loops_detected;
            overall_stats.loops_transformed += stats.loops_transformed;
            overall_stats.preheaders_created += stats.preheaders_created;
            overall_stats.candidates_examined += stats.candidates_examined;
            overall_stats.instructions_hoisted += stats.instructions_hoisted;
            overall_stats.safety_rejections += stats.safety_rejections;
        }

        (overall_changed, overall_stats)
    }

    /// Runs LICM on a single function until fixed point or round limit.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, LicmStats) {
        let mut overall_changed = false;
        let mut overall_stats = LicmStats::default();
        let mut round = 0;

        while round < Self::MAX_LICM_ROUNDS {
            round += 1;

            let dom_tree = match DominatorTree::compute(func) {
                Some(dt) => dt,
                None => break,
            };

            let loops = NaturalLoop::discover_loops(func, &dom_tree);
            if loops.is_empty() {
                break;
            }

            if round == 1 {
                overall_stats.loops_detected = loops.len();
            }

            let mut round_changed = false;

            // Process loops from innermost to outermost
            for loop_info in &loops {
                let (preheader_id, created) =
                    NaturalLoop::get_or_create_preheader(func, loop_info, &dom_tree);
                if created {
                    overall_stats.preheaders_created += 1;
                    round_changed = true;
                }

                let hoisted =
                    Self::hoist_loop_invariants(func, loop_info, preheader_id, &mut overall_stats);
                if hoisted > 0 {
                    overall_stats.loops_transformed += 1;
                    round_changed = true;
                }
            }

            if round_changed {
                overall_changed = true;
            } else {
                break;
            }
        }

        (overall_changed, overall_stats)
    }

    /// Identifies and hoists eligible loop-invariant instructions into the preheader.
    fn hoist_loop_invariants(
        func: &mut MirFunction,
        loop_info: &NaturalLoop,
        preheader_id: BlockId,
        stats: &mut LicmStats,
    ) -> usize {
        // Collect all definitions made inside the loop
        let mut loop_defs = HashSet::new();
        for &b_id in &loop_info.blocks {
            if let Some(block) = func.blocks.iter().find(|b| b.id == b_id) {
                for inst in &block.instructions {
                    if let Some(dest) = Self::extract_dest(inst) {
                        loop_defs.insert(dest);
                    }
                }
            }
        }

        // Values defined outside the loop are invariant
        let mut invariant_values = HashSet::new();
        for block in &func.blocks {
            if !loop_info.blocks.contains(&block.id) {
                for inst in &block.instructions {
                    if let Some(dest) = Self::extract_dest(inst) {
                        invariant_values.insert(dest);
                    }
                }
            }
        }

        // Fixed-point iterative discovery of invariant instructions inside the loop
        let mut hoisted_candidates: Vec<(BlockId, ValueId, Instruction)> = Vec::new();
        let mut progress = true;

        while progress {
            progress = false;

            for &b_id in &loop_info.blocks {
                let block = match func.blocks.iter().find(|b| b.id == b_id) {
                    Some(b) => b,
                    None => continue,
                };

                for inst in &block.instructions {
                    let dest = match Self::extract_dest(inst) {
                        Some(d) => d,
                        None => continue,
                    };

                    if invariant_values.contains(&dest) {
                        continue;
                    }

                    stats.candidates_examined += 1;

                    if !Self::is_speculation_safe_instruction(inst, func, loop_info) {
                        stats.safety_rejections += 1;
                        continue;
                    }

                    let operands = Self::extract_operands(inst);
                    let all_invariant = operands.iter().all(|op| invariant_values.contains(op));

                    if all_invariant {
                        invariant_values.insert(dest);
                        hoisted_candidates.push((b_id, dest, inst.clone()));
                        progress = true;
                    }
                }
            }
        }

        // Prune standalone constants:
        // Bare constants (ConstInt, ConstBool) have zero ALU execution cost. Hoisting standalone constants
        // out of loops provides zero runtime savings, but extends their live ranges across the entire loop body,
        // which drastically increases register pressure and causes linear-scan regalloc to spill hot loop
        // accumulators to stack slots. Therefore, we only hoist constants if they are required as operands
        // to a non-constant instruction being hoisted into the preheader.
        let mut needed_operands = HashSet::new();
        for (_, _, inst) in &hoisted_candidates {
            if !matches!(
                inst,
                Instruction::ConstInt { .. } | Instruction::ConstBool { .. }
            ) {
                for op in Self::extract_operands(inst) {
                    needed_operands.insert(op);
                }
            }
        }

        let mut progress_prop = true;
        while progress_prop {
            progress_prop = false;
            for (_, dest, inst) in &hoisted_candidates {
                if needed_operands.contains(dest) {
                    for op in Self::extract_operands(inst) {
                        if needed_operands.insert(op) {
                            progress_prop = true;
                        }
                    }
                }
            }
        }

        hoisted_candidates.retain(|(_, dest, inst)| {
            if matches!(
                inst,
                Instruction::ConstInt { .. } | Instruction::ConstBool { .. }
            ) {
                needed_operands.contains(dest)
            } else {
                true
            }
        });

        if hoisted_candidates.is_empty() {
            return 0;
        }

        let count = hoisted_candidates.len();
        let hoisted_dests: HashSet<ValueId> =
            hoisted_candidates.iter().map(|(_, d, _)| *d).collect();

        // 1. Remove hoisted instructions from original loop blocks
        for block in &mut func.blocks {
            if loop_info.blocks.contains(&block.id) {
                block.instructions.retain(|inst| {
                    if let Some(dest) = Self::extract_dest(inst) {
                        !hoisted_dests.contains(&dest)
                    } else {
                        true
                    }
                });
            }
        }

        // 2. Append hoisted instructions into preheader
        let preheader = func
            .blocks
            .iter_mut()
            .find(|b| b.id == preheader_id)
            .unwrap();

        for (_, _, inst) in hoisted_candidates {
            preheader.instructions.push(inst);
            stats.instructions_hoisted += 1;
        }

        count
    }

    /// Evaluates if an instruction is completely pure and safe to speculate outside the loop.
    pub fn is_speculation_safe_instruction(
        inst: &Instruction,
        func: &MirFunction,
        loop_info: &NaturalLoop,
    ) -> bool {
        match inst {
            Instruction::ConstInt { .. } | Instruction::ConstBool { .. } => true,
            Instruction::Unary { op, .. } => matches!(op, MirUnaryOp::Neg | MirUnaryOp::Not),
            Instruction::Binary { op, .. } => matches!(
                op,
                MirBinaryOp::Add
                    | MirBinaryOp::Sub
                    | MirBinaryOp::Mul
                    | MirBinaryOp::BitAnd
                    | MirBinaryOp::BitOr
                    | MirBinaryOp::BitXor
                    | MirBinaryOp::Shl
                    | MirBinaryOp::Shr
                    | MirBinaryOp::Eq
                    | MirBinaryOp::NotEq
                    | MirBinaryOp::Lt
                    | MirBinaryOp::LtEq
                    | MirBinaryOp::Gt
                    | MirBinaryOp::GtEq
                    | MirBinaryOp::And
                    | MirBinaryOp::Or
            ),
            Instruction::Copy { .. } => true,
            Instruction::Load { .. } => {
                Self::is_invariant_read_only_slot_load(inst, func, loop_info)
            }
            // Exclude Div, Rem (can trap on zero divisor)
            // Exclude Call, Store*, Index*, Field*, StrSlice, LoadSlice, StoreSlice, Phi
            _ => false,
        }
    }

    /// Checks if a load comes from a verified immutable, unescaped slot (e.g. parameter or pre-loop local).
    fn is_invariant_read_only_slot_load(
        inst: &Instruction,
        func: &MirFunction,
        loop_info: &NaturalLoop,
    ) -> bool {
        if let Instruction::Load { slot, .. } = inst {
            // Check 1: slot address is never taken (unescaped)
            let is_escaped = func.blocks.iter().any(|b| {
                b.instructions.iter().any(|i| match i {
                    Instruction::SlotAddr { slot: s, .. }
                    | Instruction::IndexAddr { slot: s, .. } => s == slot,
                    _ => false,
                })
            });
            if is_escaped {
                return false;
            }

            // Check 2: slot is NEVER stored to inside the loop
            let stored_in_loop = loop_info.blocks.iter().any(|b_id| {
                if let Some(b) = func.blocks.iter().find(|b| b.id == *b_id) {
                    b.instructions.iter().any(|i| match i {
                        Instruction::Store { slot: s, .. }
                        | Instruction::StoreSlice { slot: s, .. } => s == slot,
                        _ => false,
                    })
                } else {
                    false
                }
            });
            if stored_in_loop {
                return false;
            }

            // Check 3: slot is unwritten in the whole function (parameter) or stored only outside loop
            let stores: Vec<BlockId> = func
                .blocks
                .iter()
                .filter(|b| {
                    b.instructions.iter().any(|i| match i {
                        Instruction::Store { slot: s, .. }
                        | Instruction::StoreSlice { slot: s, .. } => s == slot,
                        _ => false,
                    })
                })
                .map(|b| b.id)
                .collect();

            if stores.is_empty() {
                return true; // Read-only parameter slot!
            }

            if stores.len() == 1 && !loop_info.blocks.contains(&stores[0]) {
                return true; // Initialized once before loop!
            }

            false
        } else {
            false
        }
    }

    /// Extracts all operand ValueIds used by an instruction.
    pub fn extract_operands(inst: &Instruction) -> Vec<ValueId> {
        match inst {
            Instruction::ConstInt { .. }
            | Instruction::ConstBool { .. }
            | Instruction::Load { .. } => Vec::new(),
            Instruction::Unary { operand, .. } => vec![*operand],
            Instruction::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
            Instruction::Copy { src, .. } => vec![*src],
            _ => Vec::new(),
        }
    }

    /// Extracts the destination ValueId defined by an instruction, if any.
    pub fn extract_dest(inst: &Instruction) -> Option<ValueId> {
        match inst {
            Instruction::ConstInt { dest, .. }
            | Instruction::ConstBool { dest, .. }
            | Instruction::Unary { dest, .. }
            | Instruction::Binary { dest, .. }
            | Instruction::Copy { dest, .. }
            | Instruction::Load { dest, .. } => Some(*dest),
            _ => None,
        }
    }
}
