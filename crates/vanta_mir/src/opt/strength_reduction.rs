//! Loop Strength Reduction (LSR) Optimization Pass for Vanta MIR.
//!
//! Replaces expensive operations (specifically multiplications of an induction
//! variable by a loop-invariant stride) inside natural loops with cheap
//! recurrence additions updated along loop backedges.

use crate::ir::*;
use crate::opt::dom::DominatorTree;
use crate::opt::induction_var::{BasicIndVar, InductionVarOpt};
use crate::opt::licm::NaturalLoop;
use std::collections::{HashMap, HashSet};
use vanta_sema::Type;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LsrStats {
    pub loops_analyzed: usize,
    pub candidates_examined: usize,
    pub recurrences_created: usize,
    pub multiplications_reduced: usize,
    pub candidates_rejected: usize,
}

pub struct LoopStrengthReduction;

impl LoopStrengthReduction {
    /// Maximum number of LSR transformation rounds per function.
    pub const MAX_ROUNDS: usize = 4;

    /// Runs Loop Strength Reduction across an MIR function.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, LsrStats) {
        let mut overall_changed = false;
        let mut overall_stats = LsrStats::default();

        if func.blocks.is_empty() {
            return (false, overall_stats);
        }

        let mut round = 0;
        loop {
            round += 1;
            if round > Self::MAX_ROUNDS {
                break;
            }

            let dom_tree = match DominatorTree::compute(func) {
                Some(dt) => dt,
                None => break,
            };

            let mut loops = NaturalLoop::discover_loops(func, &dom_tree);
            if loops.is_empty() {
                break;
            }

            // Process innermost loops first
            loops.sort_by_key(|l| l.blocks.len());

            let mut round_changed = false;

            for loop_info in &loops {
                overall_stats.loops_analyzed += 1;

                let (changed, stats) = Self::reduce_loop(func, loop_info, &dom_tree);
                if changed {
                    round_changed = true;
                    overall_changed = true;
                }
                overall_stats.candidates_examined += stats.candidates_examined;
                overall_stats.recurrences_created += stats.recurrences_created;
                overall_stats.multiplications_reduced += stats.multiplications_reduced;
                overall_stats.candidates_rejected += stats.candidates_rejected;

                if round_changed {
                    break;
                }
            }

            if !round_changed {
                break;
            }
        }

        (overall_changed, overall_stats)
    }

    fn reduce_loop(
        func: &mut MirFunction,
        loop_info: &NaturalLoop,
        dom_tree: &DominatorTree,
    ) -> (bool, LsrStats) {
        let mut stats = LsrStats::default();

        // 1. Collect all constants defined in the function
        let mut const_map: HashMap<ValueId, i64> = HashMap::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::ConstInt { dest, val, .. } = inst {
                    const_map.insert(*dest, *val);
                }
            }
        }

        // 2. Discover basic induction variables in this loop
        let ivs = InductionVarOpt::find_basic_induction_vars(func, loop_info);
        if ivs.is_empty() {
            return (false, stats);
        }

        // Map phi_dest to &BasicIndVar
        let iv_map: HashMap<ValueId, &BasicIndVar> =
            ivs.iter().map(|iv| (iv.phi_dest, iv)).collect();

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

        // Also, constants inside the loop are invariant
        for &val_id in const_map.keys() {
            invariant_values.insert(val_id);
        }

        // 3. Find candidate multiplications: iv * stride where stride is invariant
        struct Candidate {
            mul_dest: ValueId,
            iv_phi: ValueId,
            stride_val: ValueId,
            stride_const: Option<i64>,
            _mul_block: BlockId,
            ty: Type,
        }

        let mut candidates = Vec::new();

        for &b_id in &loop_info.blocks {
            if let Some(block) = func.blocks.iter().find(|b| b.id == b_id) {
                for inst in &block.instructions {
                    if let Instruction::Binary {
                        dest,
                        op: MirBinaryOp::Mul,
                        lhs,
                        rhs,
                        ty,
                    } = inst
                    {
                        stats.candidates_examined += 1;

                        if !matches!(ty, Type::I64 | Type::I32 | Type::U64 | Type::U32) {
                            stats.candidates_rejected += 1;
                            continue;
                        }

                        let match_candidate =
                            if iv_map.contains_key(lhs) && invariant_values.contains(rhs) {
                                Some((*lhs, *rhs))
                            } else if iv_map.contains_key(rhs) && invariant_values.contains(lhs) {
                                Some((*rhs, *lhs))
                            } else {
                                None
                            };

                        if let Some((iv_phi, stride_val)) = match_candidate {
                            let stride_const = const_map.get(&stride_val).copied();
                            candidates.push(Candidate {
                                mul_dest: *dest,
                                iv_phi,
                                stride_val,
                                stride_const,
                                _mul_block: b_id,
                                ty: ty.clone(),
                            });
                        } else {
                            stats.candidates_rejected += 1;
                        }
                    }
                }
            }
        }

        if candidates.is_empty() {
            return (false, stats);
        }

        // 4. Ensure loop preheader exists
        let (preheader_id, _) = NaturalLoop::get_or_create_preheader(func, loop_info, dom_tree);

        // Ensure single latch block
        if loop_info.latches.len() != 1 {
            stats.candidates_rejected += candidates.len();
            return (false, stats);
        }
        let latch_id = loop_info.latches[0];

        let mut next_val_id = func.max_value_id() + 1;
        let mut subst_map: HashMap<ValueId, ValueId> = HashMap::new();
        let mut muls_to_remove = HashSet::new();

        // 5. Transform each candidate into a recurrence
        for cand in candidates {
            let iv = iv_map[&cand.iv_phi];

            // In preheader: compute r_init = iv.init_val * stride_val
            let r_init = ValueId(next_val_id);
            next_val_id += 1;

            // Make sure stride_val is available in preheader (if it was defined in loop as const)
            let eff_stride_val = if let Some(c) = cand.stride_const {
                let const_val = ValueId(next_val_id);
                next_val_id += 1;
                let preheader = func
                    .blocks
                    .iter_mut()
                    .find(|b| b.id == preheader_id)
                    .unwrap();
                preheader.instructions.push(Instruction::ConstInt {
                    dest: const_val,
                    val: c,
                    ty: cand.ty.clone(),
                });
                const_val
            } else {
                cand.stride_val
            };

            let preheader = func
                .blocks
                .iter_mut()
                .find(|b| b.id == preheader_id)
                .unwrap();
            preheader.instructions.push(Instruction::Binary {
                dest: r_init,
                op: MirBinaryOp::Mul,
                lhs: iv.init_val,
                rhs: eff_stride_val,
                ty: cand.ty.clone(),
            });

            // In preheader: compute delta = iv.step * stride_val
            let delta_val = ValueId(next_val_id);
            next_val_id += 1;

            if let Some(c) = cand.stride_const {
                let delta_imm = iv.step.wrapping_mul(c);
                let preheader = func
                    .blocks
                    .iter_mut()
                    .find(|b| b.id == preheader_id)
                    .unwrap();
                preheader.instructions.push(Instruction::ConstInt {
                    dest: delta_val,
                    val: delta_imm,
                    ty: cand.ty.clone(),
                });
            } else {
                let step_imm_val = ValueId(next_val_id);
                next_val_id += 1;
                let preheader = func
                    .blocks
                    .iter_mut()
                    .find(|b| b.id == preheader_id)
                    .unwrap();
                preheader.instructions.push(Instruction::ConstInt {
                    dest: step_imm_val,
                    val: iv.step,
                    ty: cand.ty.clone(),
                });
                preheader.instructions.push(Instruction::Binary {
                    dest: delta_val,
                    op: MirBinaryOp::Mul,
                    lhs: step_imm_val,
                    rhs: eff_stride_val,
                    ty: cand.ty.clone(),
                });
            }

            // In loop header: insert Phi node
            let r_phi = ValueId(next_val_id);
            next_val_id += 1;
            let r_next = ValueId(next_val_id);
            next_val_id += 1;

            let header = func
                .blocks
                .iter_mut()
                .find(|b| b.id == loop_info.header)
                .unwrap();
            // Insert after existing phis
            let insert_pos = header
                .instructions
                .iter()
                .position(|inst| !matches!(inst, Instruction::Phi { .. }))
                .unwrap_or(header.instructions.len());

            header.instructions.insert(
                insert_pos,
                Instruction::Phi {
                    dest: r_phi,
                    inputs: vec![(preheader_id, r_init), (latch_id, r_next)],
                    ty: cand.ty.clone(),
                },
            );

            // In latch block: insert r_next = r_phi + delta_val
            let latch = func.blocks.iter_mut().find(|b| b.id == latch_id).unwrap();
            latch.instructions.push(Instruction::Binary {
                dest: r_next,
                op: MirBinaryOp::Add,
                lhs: r_phi,
                rhs: delta_val,
                ty: cand.ty.clone(),
            });

            // Map candidate mul_dest to r_phi
            subst_map.insert(cand.mul_dest, r_phi);
            muls_to_remove.insert(cand.mul_dest);

            stats.recurrences_created += 1;
            stats.multiplications_reduced += 1;
        }

        // Replace uses of mul_dest with r_phi
        Self::apply_substitutions(func, &subst_map);

        // Remove the original multiplications
        for &b_id in &loop_info.blocks {
            if let Some(block) = func.blocks.iter_mut().find(|b| b.id == b_id) {
                block.instructions.retain(|inst| {
                    if let Some(dest) = Self::extract_dest(inst) {
                        !muls_to_remove.contains(&dest)
                    } else {
                        true
                    }
                });
            }
        }

        (true, stats)
    }

    fn extract_dest(inst: &Instruction) -> Option<ValueId> {
        match inst {
            Instruction::ConstInt { dest, .. }
            | Instruction::ConstBool { dest, .. }
            | Instruction::SlotAddr { dest, .. }
            | Instruction::Load { dest, .. }
            | Instruction::Binary { dest, .. }
            | Instruction::Unary { dest, .. }
            | Instruction::IndexAddr { dest, .. }
            | Instruction::IndexPtrAddr { dest, .. }
            | Instruction::FieldAddr { dest, .. }
            | Instruction::LoadIndirect { dest, .. }
            | Instruction::Copy { dest, .. }
            | Instruction::Phi { dest, .. } => Some(*dest),
            Instruction::Call { dest, .. } => *dest,
            _ => None,
        }
    }

    fn apply_substitutions(func: &mut MirFunction, subst: &HashMap<ValueId, ValueId>) {
        for block in &mut func.blocks {
            for inst in &mut block.instructions {
                match inst {
                    Instruction::Binary { lhs, rhs, .. } => {
                        if let Some(&new_lhs) = subst.get(lhs) {
                            *lhs = new_lhs;
                        }
                        if let Some(&new_rhs) = subst.get(rhs) {
                            *rhs = new_rhs;
                        }
                    }
                    Instruction::Unary { operand, .. } => {
                        if let Some(&new_op) = subst.get(operand) {
                            *operand = new_op;
                        }
                    }
                    Instruction::Store { val, .. } => {
                        if let Some(&new_val) = subst.get(val) {
                            *val = new_val;
                        }
                    }
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
                        if let Some(&new_addr) = subst.get(addr) {
                            *addr = new_addr;
                        }
                        if let Some(&new_val) = subst.get(val) {
                            *val = new_val;
                        }
                    }
                    Instruction::LoadIndirect { addr, .. } => {
                        if let Some(&new_addr) = subst.get(addr) {
                            *addr = new_addr;
                        }
                    }
                    Instruction::IndexAddr { index, .. } => {
                        if let Some(&new_idx) = subst.get(index) {
                            *index = new_idx;
                        }
                    }
                    Instruction::IndexPtrAddr {
                        base_ptr, index, ..
                    } => {
                        if let Some(&new_base) = subst.get(base_ptr) {
                            *base_ptr = new_base;
                        }
                        if let Some(&new_idx) = subst.get(index) {
                            *index = new_idx;
                        }
                    }
                    Instruction::FieldAddr { base_ptr, .. } => {
                        if let Some(&new_base) = subst.get(base_ptr) {
                            *base_ptr = new_base;
                        }
                    }
                    Instruction::Call { args, .. } => {
                        for a in args {
                            if let Some(&new_a) = subst.get(a) {
                                *a = new_a;
                            }
                        }
                    }
                    Instruction::Copy { src, .. } => {
                        if let Some(&new_src) = subst.get(src) {
                            *src = new_src;
                        }
                    }
                    Instruction::Phi { inputs, .. } => {
                        for (_, val) in inputs {
                            if let Some(&new_val) = subst.get(val) {
                                *val = new_val;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ref mut term) = block.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        if let Some(&new_cond) = subst.get(cond) {
                            *cond = new_cond;
                        }
                    }
                    Terminator::Return(Some(val)) => {
                        if let Some(&new_val) = subst.get(val) {
                            *val = new_val;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
