//! Global Value Numbering (GVN) and Common Subexpression Elimination (CSE) for Vanta MIR.
//!
//! Eliminates redundant pure computations across dominating basic blocks using
//! dominator-tree preorder scoped value numbering, commutative operand canonicalization,
//! comparison dual normalization, copy propagation, load elimination, and phi simplification.

use crate::ir::*;
use crate::opt::dom::DominatorTree;
use std::collections::{HashMap, HashSet};
use vanta_sema::Type;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GvnStats {
    pub eliminated_subexpressions: usize,
    pub simplified_phis: usize,
    pub simplified_copies: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GvnExpr {
    ConstInt {
        val: i64,
        ty: Type,
    },
    ConstBool {
        val: bool,
    },
    Binary {
        op: MirBinaryOp,
        lhs: ValueId,
        rhs: ValueId,
        ty: Type,
    },
    Unary {
        op: MirUnaryOp,
        operand: ValueId,
        ty: Type,
    },
    Phi {
        block: BlockId,
        inputs: Vec<(BlockId, ValueId)>,
        ty: Type,
    },
    Load {
        slot: SlotId,
    },
    SlotAddr {
        slot: SlotId,
    },
    FieldAddr {
        base_ptr: ValueId,
        offset: usize,
    },
    IndexAddr {
        slot: SlotId,
        index: ValueId,
        elem_size: usize,
    },
    IndexPtrAddr {
        base_ptr: ValueId,
        index: ValueId,
        elem_size: usize,
    },
}

pub struct Gvn;

impl Gvn {
    /// Runs GVN and CSE on an MIR function.
    /// Returns `(changed, stats)`.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, GvnStats) {
        if func.blocks.is_empty() {
            return (false, GvnStats::default());
        }

        let dom_tree = match DominatorTree::compute(func) {
            Some(dt) => dt,
            None => return (false, GvnStats::default()),
        };

        // Identify slots that are written to or have their address taken
        let mut written_slots = HashSet::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Store { slot, .. }
                    | Instruction::StoreSlice { slot, .. }
                    | Instruction::SlotAddr { slot, .. }
                    | Instruction::IndexAddr { slot, .. } => {
                        written_slots.insert(*slot);
                    }
                    _ => {}
                }
            }
        }

        let mut ctx = GvnContext {
            dom_tree: &dom_tree,
            written_slots,
            leader: HashMap::new(),
            available: HashMap::new(),
            stats: GvnStats::default(),
        };

        let orig_order: Vec<BlockId> = func.blocks.iter().map(|b| b.id).collect();
        let mut block_map: HashMap<BlockId, BasicBlock> =
            func.blocks.drain(..).map(|b| (b.id, b)).collect();

        // Traverse the dominator tree in preorder starting from entry
        ctx.traverse_dom_tree(dom_tree.entry, &mut block_map);

        // Reconstruct func.blocks in original order
        func.blocks = orig_order
            .into_iter()
            .filter_map(|id| block_map.remove(&id))
            .collect();

        // Post-pass: substitute all uses of replaced/copied values across the entire function
        let changed = ctx.stats.eliminated_subexpressions > 0
            || ctx.stats.simplified_phis > 0
            || ctx.stats.simplified_copies > 0;

        if changed {
            ctx.substitute_all_uses(func);
        }

        (changed, ctx.stats)
    }
}

struct GvnContext<'a> {
    dom_tree: &'a DominatorTree,
    written_slots: HashSet<SlotId>,
    leader: HashMap<ValueId, ValueId>,
    available: HashMap<GvnExpr, ValueId>,
    stats: GvnStats,
}

impl<'a> GvnContext<'a> {
    fn find_leader(&self, mut v: ValueId) -> ValueId {
        let mut hops = 0;
        let mut visited = HashSet::new();
        while let Some(&next) = self.leader.get(&v) {
            if next == v || !visited.insert(v) || hops > 128 {
                break;
            }
            v = next;
            hops += 1;
        }
        v
    }

    fn set_leader(&mut self, v: ValueId, leader_val: ValueId) {
        let canonical = self.find_leader(leader_val);
        if canonical != v {
            self.leader.insert(v, canonical);
        }
    }

    fn traverse_dom_tree(
        &mut self,
        block_id: BlockId,
        block_map: &mut HashMap<BlockId, BasicBlock>,
    ) {
        let mut rollback_stack: Vec<(GvnExpr, Option<ValueId>)> = Vec::new();

        if let Some(block) = block_map.get_mut(&block_id) {
            for inst in &mut block.instructions {
                // First rewrite input operands using current leaders
                self.substitute_inst_operands(inst);

                match inst {
                    Instruction::ConstInt { dest, val, ty } => {
                        let expr = GvnExpr::ConstInt {
                            val: *val,
                            ty: ty.clone(),
                        };
                        if let Some(&existing) = self.available.get(&expr) {
                            self.set_leader(*dest, existing);
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src: existing,
                                ty: ty.clone(),
                            };
                            self.stats.eliminated_subexpressions += 1;
                        } else {
                            let prev = self.available.insert(expr.clone(), *dest);
                            rollback_stack.push((expr, prev));
                        }
                    }
                    Instruction::ConstBool { dest, val } => {
                        let expr = GvnExpr::ConstBool { val: *val };
                        if let Some(&existing) = self.available.get(&expr) {
                            self.set_leader(*dest, existing);
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src: existing,
                                ty: Type::Bool,
                            };
                            self.stats.eliminated_subexpressions += 1;
                        } else {
                            let prev = self.available.insert(expr.clone(), *dest);
                            rollback_stack.push((expr, prev));
                        }
                    }
                    Instruction::Copy { dest, src, .. } => {
                        let lead_src = self.find_leader(*src);
                        if *dest != lead_src {
                            self.set_leader(*dest, lead_src);
                            self.stats.simplified_copies += 1;
                        }
                    }
                    Instruction::Phi { dest, inputs, ty } => {
                        // Check for trivial Phi: all non-self incoming values identical
                        let non_self: HashSet<ValueId> = inputs
                            .iter()
                            .map(|(_, val)| self.find_leader(*val))
                            .filter(|&v| v != *dest)
                            .collect();

                        if non_self.len() == 1 {
                            let common_val = *non_self.iter().next().unwrap();
                            self.set_leader(*dest, common_val);
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src: common_val,
                                ty: ty.clone(),
                            };
                            self.stats.simplified_phis += 1;
                        } else if non_self.len() > 1 {
                            // Check identical phi in same block
                            let mut sorted_inputs = inputs.clone();
                            sorted_inputs.sort();
                            let expr = GvnExpr::Phi {
                                block: block_id,
                                inputs: sorted_inputs,
                                ty: ty.clone(),
                            };
                            if let Some(&existing) = self.available.get(&expr) {
                                self.set_leader(*dest, existing);
                                *inst = Instruction::Copy {
                                    dest: *dest,
                                    src: existing,
                                    ty: ty.clone(),
                                };
                                self.stats.simplified_phis += 1;
                            } else {
                                let prev = self.available.insert(expr.clone(), *dest);
                                rollback_stack.push((expr, prev));
                            }
                        }
                    }
                    Instruction::Load { dest, slot, ty } => {
                        // Only read-only slots that are never stored to can be treated as pure expressions
                        if !self.written_slots.contains(slot) {
                            let expr = GvnExpr::Load { slot: *slot };
                            if let Some(&existing) = self.available.get(&expr) {
                                self.set_leader(*dest, existing);
                                *inst = Instruction::Copy {
                                    dest: *dest,
                                    src: existing,
                                    ty: ty.clone(),
                                };
                                self.stats.eliminated_subexpressions += 1;
                            } else {
                                let prev = self.available.insert(expr.clone(), *dest);
                                rollback_stack.push((expr, prev));
                            }
                        }
                    }
                    Instruction::Binary {
                        dest,
                        op,
                        lhs,
                        rhs,
                        ty,
                    } => {
                        if Self::is_gvn_eligible_binary(*op) {
                            let (norm_op, norm_lhs, norm_rhs) =
                                self.normalize_binary(*op, *lhs, *rhs);
                            let expr = GvnExpr::Binary {
                                op: norm_op,
                                lhs: norm_lhs,
                                rhs: norm_rhs,
                                ty: ty.clone(),
                            };
                            if let Some(&existing) = self.available.get(&expr) {
                                self.set_leader(*dest, existing);
                                *inst = Instruction::Copy {
                                    dest: *dest,
                                    src: existing,
                                    ty: ty.clone(),
                                };
                                self.stats.eliminated_subexpressions += 1;
                            } else {
                                let prev = self.available.insert(expr.clone(), *dest);
                                rollback_stack.push((expr, prev));
                            }
                        }
                    }
                    Instruction::Unary {
                        dest,
                        op,
                        operand,
                        ty,
                    } => {
                        let norm_operand = self.find_leader(*operand);
                        let expr = GvnExpr::Unary {
                            op: *op,
                            operand: norm_operand,
                            ty: ty.clone(),
                        };
                        if let Some(&existing) = self.available.get(&expr) {
                            self.set_leader(*dest, existing);
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src: existing,
                                ty: ty.clone(),
                            };
                            self.stats.eliminated_subexpressions += 1;
                        } else {
                            let prev = self.available.insert(expr.clone(), *dest);
                            rollback_stack.push((expr, prev));
                        }
                    }
                    Instruction::SlotAddr { dest, slot } => {
                        let expr = GvnExpr::SlotAddr { slot: *slot };
                        if let Some(&existing) = self.available.get(&expr) {
                            self.set_leader(*dest, existing);
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src: existing,
                                ty: Type::I64,
                            };
                            self.stats.eliminated_subexpressions += 1;
                        } else {
                            let prev = self.available.insert(expr.clone(), *dest);
                            rollback_stack.push((expr, prev));
                        }
                    }
                    Instruction::FieldAddr {
                        dest,
                        base_ptr,
                        offset,
                    } => {
                        let norm_base = self.find_leader(*base_ptr);
                        let expr = GvnExpr::FieldAddr {
                            base_ptr: norm_base,
                            offset: *offset,
                        };
                        if let Some(&existing) = self.available.get(&expr) {
                            self.set_leader(*dest, existing);
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src: existing,
                                ty: Type::I64,
                            };
                            self.stats.eliminated_subexpressions += 1;
                        } else {
                            let prev = self.available.insert(expr.clone(), *dest);
                            rollback_stack.push((expr, prev));
                        }
                    }
                    Instruction::IndexAddr {
                        dest,
                        slot,
                        index,
                        elem_size,
                    } => {
                        let norm_index = self.find_leader(*index);
                        let expr = GvnExpr::IndexAddr {
                            slot: *slot,
                            index: norm_index,
                            elem_size: *elem_size,
                        };
                        if let Some(&existing) = self.available.get(&expr) {
                            self.set_leader(*dest, existing);
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src: existing,
                                ty: Type::I64,
                            };
                            self.stats.eliminated_subexpressions += 1;
                        } else {
                            let prev = self.available.insert(expr.clone(), *dest);
                            rollback_stack.push((expr, prev));
                        }
                    }
                    Instruction::IndexPtrAddr {
                        dest,
                        base_ptr,
                        index,
                        elem_size,
                    } => {
                        let norm_base = self.find_leader(*base_ptr);
                        let norm_index = self.find_leader(*index);
                        let expr = GvnExpr::IndexPtrAddr {
                            base_ptr: norm_base,
                            index: norm_index,
                            elem_size: *elem_size,
                        };
                        if let Some(&existing) = self.available.get(&expr) {
                            self.set_leader(*dest, existing);
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src: existing,
                                ty: Type::I64,
                            };
                            self.stats.eliminated_subexpressions += 1;
                        } else {
                            let prev = self.available.insert(expr.clone(), *dest);
                            rollback_stack.push((expr, prev));
                        }
                    }
                    _ => {}
                }
            }

            if let Some(term) = &mut block.terminator {
                self.substitute_term_operands(term);
            }
        }

        // Recursively visit dom tree children in deterministic sorted order
        if let Some(children) = self.dom_tree.dom_children.get(&block_id) {
            let mut sorted_children = children.clone();
            sorted_children.sort();
            for child in sorted_children {
                self.traverse_dom_tree(child, block_map);
            }
        }

        // Pop available expressions defined/updated in this basic block in reverse LIFO order
        for (expr, prev) in rollback_stack.into_iter().rev() {
            if let Some(old_val) = prev {
                self.available.insert(expr, old_val);
            } else {
                self.available.remove(&expr);
            }
        }
    }

    fn normalize_binary(
        &self,
        op: MirBinaryOp,
        lhs: ValueId,
        rhs: ValueId,
    ) -> (MirBinaryOp, ValueId, ValueId) {
        let mut op = op;
        let mut lhs = self.find_leader(lhs);
        let mut rhs = self.find_leader(rhs);

        // Normalize comparison duals
        if op == MirBinaryOp::Gt {
            op = MirBinaryOp::Lt;
            std::mem::swap(&mut lhs, &mut rhs);
        } else if op == MirBinaryOp::GtEq {
            op = MirBinaryOp::LtEq;
            std::mem::swap(&mut lhs, &mut rhs);
        }

        // Commutative operand ordering
        if Self::is_commutative(op) && lhs > rhs {
            std::mem::swap(&mut lhs, &mut rhs);
        }

        (op, lhs, rhs)
    }

    fn is_commutative(op: MirBinaryOp) -> bool {
        matches!(
            op,
            MirBinaryOp::Add
                | MirBinaryOp::Mul
                | MirBinaryOp::BitAnd
                | MirBinaryOp::BitOr
                | MirBinaryOp::BitXor
                | MirBinaryOp::Eq
                | MirBinaryOp::NotEq
                | MirBinaryOp::And
                | MirBinaryOp::Or
        )
    }

    fn is_gvn_eligible_binary(op: MirBinaryOp) -> bool {
        matches!(
            op,
            MirBinaryOp::Add
                | MirBinaryOp::Sub
                | MirBinaryOp::Mul
                | MirBinaryOp::BitAnd
                | MirBinaryOp::BitOr
                | MirBinaryOp::BitXor
                | MirBinaryOp::Eq
                | MirBinaryOp::NotEq
                | MirBinaryOp::Lt
                | MirBinaryOp::LtEq
                | MirBinaryOp::Gt
                | MirBinaryOp::GtEq
                | MirBinaryOp::Shl
                | MirBinaryOp::Shr
                | MirBinaryOp::And
                | MirBinaryOp::Or
        )
    }

    fn substitute_inst_operands(&self, inst: &mut Instruction) {
        match inst {
            Instruction::Store { val, .. } => {
                *val = self.find_leader(*val);
            }
            Instruction::Binary { lhs, rhs, .. } => {
                *lhs = self.find_leader(*lhs);
                *rhs = self.find_leader(*rhs);
            }
            Instruction::Unary { operand, .. } => {
                *operand = self.find_leader(*operand);
            }
            Instruction::Call { args, .. } => {
                for arg in args {
                    *arg = self.find_leader(*arg);
                }
            }
            Instruction::CallIndirect {
                callee_ptr, args, ..
            } => {
                *callee_ptr = self.find_leader(*callee_ptr);
                for arg in args {
                    *arg = self.find_leader(*arg);
                }
            }
            Instruction::IndexAddr { index, .. } => {
                *index = self.find_leader(*index);
            }
            Instruction::IndexPtrAddr {
                base_ptr, index, ..
            } => {
                *base_ptr = self.find_leader(*base_ptr);
                *index = self.find_leader(*index);
            }
            Instruction::FieldAddr { base_ptr, .. } => {
                *base_ptr = self.find_leader(*base_ptr);
            }
            Instruction::LoadIndirect { addr, .. } => {
                *addr = self.find_leader(*addr);
            }
            Instruction::StoreIndirect { addr, val }
            | Instruction::StoreIndirect32 { addr, val } => {
                *addr = self.find_leader(*addr);
                *val = self.find_leader(*val);
            }
            Instruction::StoreSlice { ptr, len, .. } => {
                *ptr = self.find_leader(*ptr);
                *len = self.find_leader(*len);
            }
            Instruction::Copy { src, .. } => {
                *src = self.find_leader(*src);
            }
            Instruction::Phi { inputs, .. } => {
                for (_, val) in inputs {
                    *val = self.find_leader(*val);
                }
            }
            Instruction::ConstInt { .. }
            | Instruction::ConstBool { .. }
            | Instruction::SlotAddr { .. }
            | Instruction::Load { .. }
            | Instruction::StrSlice { .. }
            | Instruction::LoadSlice { .. }
            | Instruction::VtableAddr { .. } => {}
        }
    }

    fn substitute_term_operands(&self, term: &mut Terminator) {
        match term {
            Terminator::CondBranch { cond, .. } => {
                *cond = self.find_leader(*cond);
            }
            Terminator::Return(Some(val)) => {
                *val = self.find_leader(*val);
            }
            Terminator::Return(None) | Terminator::Branch(_) | Terminator::Unreachable => {}
        }
    }

    fn substitute_all_uses(&self, func: &mut MirFunction) {
        for block in &mut func.blocks {
            for inst in &mut block.instructions {
                self.substitute_inst_operands(inst);
            }
            if let Some(term) = &mut block.terminator {
                self.substitute_term_operands(term);
            }
        }
    }
}
