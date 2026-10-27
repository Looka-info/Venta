//! Direct Call Graph analysis and conservative function summaries for Vanta MIR.
//!
//! Analyzes direct call relationships, detects self and mutual recursion,
//! computes caller/callee sets, and evaluates conservative function summaries
//! (code size, leaf property, return types) for inlining and interprocedural analysis.

use crate::ir::*;
use std::collections::{HashMap, HashSet};
use vanta_sema::Type;

#[derive(Debug, Clone)]
pub struct CallEdge {
    pub caller: String,
    pub callee: String,
    pub block_id: BlockId,
    pub inst_idx: usize,
    pub is_constant_call: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionSummary {
    pub name: String,
    pub instruction_count: usize,
    pub block_count: usize,
    pub is_recursive: bool,
    pub is_leaf: bool,
    pub call_site_count: usize,
    pub has_memory_return: bool,
    pub has_aggregate_types: bool,
    pub ret_type: Type,
    pub param_types: Vec<Type>,
}

#[derive(Debug, Clone, Default)]
pub struct CallGraph {
    pub summaries: HashMap<String, FunctionSummary>,
    pub callers_of: HashMap<String, Vec<String>>,
    pub callees_of: HashMap<String, Vec<String>>,
    pub edges: Vec<CallEdge>,
}

impl CallGraph {
    /// Builds the call graph from a MIR program.
    pub fn build(program: &MirProgram) -> Self {
        let mut summaries = HashMap::new();
        let mut callers_of: HashMap<String, Vec<String>> = HashMap::new();
        let mut callees_of: HashMap<String, Vec<String>> = HashMap::new();
        let mut edges = Vec::new();
        let mut call_counts: HashMap<String, usize> = HashMap::new();

        // Register all functions
        for func in &program.functions {
            callers_of.entry(func.name.clone()).or_default();
            callees_of.entry(func.name.clone()).or_default();
            call_counts.entry(func.name.clone()).or_default();
        }

        // Scan call sites
        for func in &program.functions {
            let mut inst_count = 0;
            for block in &func.blocks {
                for (idx, inst) in block.instructions.iter().enumerate() {
                    inst_count += 1;
                    if let Instruction::Call { callee, args, .. } = inst {
                        // Record outgoing edge
                        callees_of
                            .entry(func.name.clone())
                            .or_default()
                            .push(callee.clone());
                        callers_of
                            .entry(callee.clone())
                            .or_default()
                            .push(func.name.clone());
                        *call_counts.entry(callee.clone()).or_default() += 1;

                        // Check if all arguments are known constants in the block
                        let mut const_args = true;
                        for arg in args {
                            let is_const = block.instructions.iter().any(|i| match i {
                                Instruction::ConstInt { dest, .. }
                                | Instruction::ConstBool { dest, .. } => dest == arg,
                                _ => false,
                            });
                            if !is_const {
                                const_args = false;
                                break;
                            }
                        }

                        edges.push(CallEdge {
                            caller: func.name.clone(),
                            callee: callee.clone(),
                            block_id: block.id,
                            inst_idx: idx,
                            is_constant_call: const_args,
                        });
                    }
                }
            }

            let param_types: Vec<Type> = func.params.iter().map(|(_, ty)| ty.clone()).collect();
            let is_agg = |ty: &Type| {
                ty.is_str_slice()
                    || ty.is_slice()
                    || matches!(
                        ty,
                        Type::Struct(_)
                            | Type::Enum(_)
                            | Type::Array { .. }
                            | Type::Slice(_)
                            | Type::Str
                    )
            };
            let has_memory_return = is_agg(&func.ret_type);
            let has_aggregate_types = has_memory_return || param_types.iter().any(is_agg);

            summaries.insert(
                func.name.clone(),
                FunctionSummary {
                    name: func.name.clone(),
                    instruction_count: inst_count,
                    block_count: func.blocks.len(),
                    is_recursive: false, // will be computed next
                    is_leaf: true,       // will be updated next
                    call_site_count: 0,
                    has_memory_return,
                    has_aggregate_types,
                    ret_type: func.ret_type.clone(),
                    param_types,
                },
            );
        }

        // Update call site counts and leaf status
        for (name, summary) in &mut summaries {
            summary.call_site_count = call_counts.get(name).copied().unwrap_or(0);
            let callee_list = callees_of.get(name).map(|v| v.as_slice()).unwrap_or(&[]);
            summary.is_leaf = callee_list.is_empty();
        }

        // Detect recursion (direct self-recursion and mutual recursion cycles)
        for name in summaries.keys().cloned().collect::<Vec<_>>() {
            let is_rec = Self::detect_cycle(&callees_of, &name);
            if let Some(s) = summaries.get_mut(&name) {
                s.is_recursive = is_rec;
            }
        }

        Self {
            summaries,
            callers_of,
            callees_of,
            edges,
        }
    }

    /// Returns true if `target` can reach itself via a call cycle of length >= 1.
    fn detect_cycle(callees_of: &HashMap<String, Vec<String>>, target: &str) -> bool {
        let mut visited = HashSet::new();
        let mut stack = Vec::new();

        if let Some(first_callees) = callees_of.get(target) {
            for c in first_callees {
                if c == target {
                    return true;
                }
                stack.push(c.as_str());
            }
        }

        while let Some(curr) = stack.pop() {
            if curr == target {
                return true;
            }
            if visited.insert(curr) {
                if let Some(next_callees) = callees_of.get(curr) {
                    for n in next_callees {
                        stack.push(n.as_str());
                    }
                }
            }
        }

        false
    }

    /// Checks if a callee is eligible for inlining under conservative policy.
    pub fn is_inlining_candidate(&self, callee_name: &str) -> (bool, &'static str) {
        let summary = match self.summaries.get(callee_name) {
            Some(s) => s,
            None => return (false, "external or runtime function without MIR body"),
        };

        if summary.is_recursive {
            return (false, "recursive or mutually recursive function");
        }

        if summary.has_aggregate_types {
            return (false, "callee uses aggregate or memory-class types");
        }

        // Exclude large functions
        const MAX_LEAF_INLINE_INSTRUCTIONS: usize = 60;
        const MAX_NON_LEAF_INLINE_INSTRUCTIONS: usize = 35;

        let max_size = if summary.is_leaf {
            MAX_LEAF_INLINE_INSTRUCTIONS
        } else {
            MAX_NON_LEAF_INLINE_INSTRUCTIONS
        };

        if summary.instruction_count > max_size {
            return (false, "callee exceeds instruction count size budget");
        }

        (true, "eligible")
    }
}
