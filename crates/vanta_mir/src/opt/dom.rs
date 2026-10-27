//! Dominator Tree and Control Flow Graph Analysis for Vanta MIR.
//!
//! Provides dominance analysis, immediate dominators (idom), dominator tree
//! construction, and dominance frontier calculation.

use crate::ir::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct DominatorTree {
    pub entry: BlockId,
    pub reachable_blocks: Vec<BlockId>,
    pub predecessors: HashMap<BlockId, Vec<BlockId>>,
    pub successors: HashMap<BlockId, Vec<BlockId>>,
    pub dom_sets: HashMap<BlockId, HashSet<BlockId>>,
    pub idom: HashMap<BlockId, BlockId>,
    pub dom_children: HashMap<BlockId, Vec<BlockId>>,
}

impl DominatorTree {
    /// Computes the CFG and dominator tree for a function.
    pub fn compute(func: &MirFunction) -> Option<Self> {
        if func.blocks.is_empty() {
            return None;
        }

        let entry = func.blocks[0].id;
        let (reachable_blocks, predecessors, successors) = Self::compute_cfg(func, entry);
        if reachable_blocks.is_empty() {
            return None;
        }

        let dom_sets = Self::compute_dominators(&reachable_blocks, entry, &predecessors);
        let idom = Self::compute_immediate_dominators(&reachable_blocks, entry, &dom_sets);
        let dom_children = Self::compute_dom_children(&reachable_blocks, entry, &idom);

        Some(Self {
            entry,
            reachable_blocks,
            predecessors,
            successors,
            dom_sets,
            idom,
            dom_children,
        })
    }

    /// Returns true if basic block `a` dominates basic block `b`.
    pub fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        if let Some(b_dom) = self.dom_sets.get(&b) {
            b_dom.contains(&a)
        } else {
            false
        }
    }

    /// Returns true if basic block `a` strictly dominates basic block `b`.
    pub fn strictly_dominates(&self, a: BlockId, b: BlockId) -> bool {
        a != b && self.dominates(a, b)
    }

    /// Returns the immediate dominator of `b`, if any.
    pub fn idom_of(&self, b: BlockId) -> Option<BlockId> {
        self.idom.get(&b).copied()
    }

    /// Returns the children of `b` in the dominator tree.
    pub fn children_of(&self, b: BlockId) -> &[BlockId] {
        self.dom_children
            .get(&b)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Computes Dominance Frontiers for all reachable blocks.
    pub fn compute_dominance_frontiers(&self) -> HashMap<BlockId, HashSet<BlockId>> {
        let mut df: HashMap<BlockId, HashSet<BlockId>> = HashMap::new();
        for &b in &self.reachable_blocks {
            df.entry(b).or_default();
        }

        // For each node B with multiple predecessors:
        for &b in &self.reachable_blocks {
            let p_list = match self.predecessors.get(&b) {
                Some(p) => p,
                None => continue,
            };
            if p_list.len() >= 2 {
                let b_idom = self.idom.get(&b).copied();
                for &p in p_list {
                    let mut runner = p;
                    while Some(runner) != b_idom {
                        df.entry(runner).or_default().insert(b);
                        if let Some(&next_runner) = self.idom.get(&runner) {
                            runner = next_runner;
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        df
    }

    /// Computes reachable blocks and predecessor/successor maps.
    fn compute_cfg(
        func: &MirFunction,
        entry: BlockId,
    ) -> (
        Vec<BlockId>,
        HashMap<BlockId, Vec<BlockId>>,
        HashMap<BlockId, Vec<BlockId>>,
    ) {
        let mut reachable = HashSet::new();
        let mut worklist = vec![entry];
        reachable.insert(entry);

        let block_map: HashMap<BlockId, &BasicBlock> =
            func.blocks.iter().map(|b| (b.id, b)).collect();

        while let Some(b_id) = worklist.pop() {
            if let Some(b) = block_map.get(&b_id) {
                if let Some(ref term) = b.terminator {
                    match term {
                        Terminator::Branch(t) => {
                            if reachable.insert(*t) {
                                worklist.push(*t);
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            if reachable.insert(*then_block) {
                                worklist.push(*then_block);
                            }
                            if reachable.insert(*else_block) {
                                worklist.push(*else_block);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut predecessors: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
        let mut successors: HashMap<BlockId, Vec<BlockId>> = HashMap::new();

        for b_id in &reachable {
            predecessors.entry(*b_id).or_default();
            successors.entry(*b_id).or_default();
        }

        for &b_id in &reachable {
            if let Some(b) = block_map.get(&b_id) {
                if let Some(ref term) = b.terminator {
                    match term {
                        Terminator::Branch(t) => {
                            if reachable.contains(t) {
                                successors.get_mut(&b_id).unwrap().push(*t);
                                predecessors.get_mut(t).unwrap().push(b_id);
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            if reachable.contains(then_block) {
                                successors.get_mut(&b_id).unwrap().push(*then_block);
                                predecessors.get_mut(then_block).unwrap().push(b_id);
                            }
                            if reachable.contains(else_block) {
                                successors.get_mut(&b_id).unwrap().push(*else_block);
                                predecessors.get_mut(else_block).unwrap().push(b_id);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let reachable_blocks: Vec<BlockId> = func
            .blocks
            .iter()
            .map(|b| b.id)
            .filter(|id| reachable.contains(id))
            .collect();

        (reachable_blocks, predecessors, successors)
    }

    /// Computes the set of dominators for each reachable block using fixed-point iteration.
    fn compute_dominators(
        reachable_blocks: &[BlockId],
        entry: BlockId,
        preds: &HashMap<BlockId, Vec<BlockId>>,
    ) -> HashMap<BlockId, HashSet<BlockId>> {
        let all_blocks: HashSet<BlockId> = reachable_blocks.iter().copied().collect();
        let mut dom: HashMap<BlockId, HashSet<BlockId>> = HashMap::new();

        for &b in reachable_blocks {
            if b == entry {
                let mut s = HashSet::new();
                s.insert(entry);
                dom.insert(b, s);
            } else {
                dom.insert(b, all_blocks.clone());
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for &b in reachable_blocks {
                if b == entry {
                    continue;
                }
                let p_list = match preds.get(&b) {
                    Some(p) => p,
                    None => continue,
                };
                if p_list.is_empty() {
                    continue;
                }

                let mut new_dom = dom.get(&p_list[0]).cloned().unwrap_or_default();
                for p in &p_list[1..] {
                    if let Some(p_dom) = dom.get(p) {
                        new_dom.retain(|item| p_dom.contains(item));
                    }
                }
                new_dom.insert(b);

                if dom.get(&b) != Some(&new_dom) {
                    dom.insert(b, new_dom);
                    changed = true;
                }
            }
        }

        dom
    }

    /// Computes the immediate dominator (idom) for each reachable block.
    fn compute_immediate_dominators(
        reachable_blocks: &[BlockId],
        entry: BlockId,
        dom: &HashMap<BlockId, HashSet<BlockId>>,
    ) -> HashMap<BlockId, BlockId> {
        let mut idom = HashMap::new();

        for &b in reachable_blocks {
            if b == entry {
                continue;
            }
            if let Some(b_dom) = dom.get(&b) {
                // Strict dominators: Dom(b) \ {b}
                let mut strict_doms: Vec<BlockId> =
                    b_dom.iter().copied().filter(|&d| d != b).collect();

                // idom(b) is the unique strict dominator that is dominated by all other strict dominators
                strict_doms.sort_by_key(|d| dom.get(d).map(|s| s.len()).unwrap_or(0));
                if let Some(&imm_dom) = strict_doms.last() {
                    idom.insert(b, imm_dom);
                }
            }
        }

        idom
    }

    /// Computes children in the dominator tree.
    fn compute_dom_children(
        reachable_blocks: &[BlockId],
        entry: BlockId,
        idom: &HashMap<BlockId, BlockId>,
    ) -> HashMap<BlockId, Vec<BlockId>> {
        let mut dom_children: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
        for &b in reachable_blocks {
            dom_children.entry(b).or_default();
        }
        for (&b, &parent) in idom {
            if b != entry {
                dom_children.entry(parent).or_default().push(b);
            }
        }
        dom_children
    }
}
