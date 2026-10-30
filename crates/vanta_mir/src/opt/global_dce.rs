//! Program-level Global Dead Code Elimination (Global DCE) for Vanta MIR.
//!
//! Provides:
//! 1. Inter-procedural function reachability analysis from compiler roots (`main`, `vanta_rt_*`).
//! 2. Intra-procedural unreachable basic block elimination and phi node pruning/simplification.
//! 3. Conservative effect-aware unused pure instruction elimination (fixed-point).

use crate::ir::*;
use crate::opt::call_graph::CallGraph;
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GlobalDceStats {
    pub functions_eliminated: usize,
    pub blocks_eliminated: usize,
    pub instructions_eliminated: usize,
}

pub struct GlobalDce;

impl GlobalDce {
    /// Runs Global Dead Code Elimination on an entire MIR program.
    ///
    /// When `is_executable` is true, eliminates functions that are not reachable
    /// from the program entry point (`main`) or runtime-required roots (`vanta_rt_*`).
    /// When `is_executable` is false (library/object mode), all functions are retained as roots.
    ///
    /// Also eliminates unreachable basic blocks and dead pure computations within all functions.
    pub fn run_on_program(program: &mut MirProgram, is_executable: bool) -> (bool, GlobalDceStats) {
        let mut stats = GlobalDceStats::default();
        let mut changed = false;

        // 1. Inter-procedural Function Reachability (for executables with a main entry point)
        let has_main = program.functions.iter().any(|f| f.name == "main");
        if is_executable && has_main {
            let initial_funcs = program.functions.len();
            let call_graph = CallGraph::build(program);
            let mut reachable = HashSet::new();
            let mut queue = VecDeque::new();

            // Seed root functions: `main`, `vanta_rt_*`, foreign ABI hooks
            for func in &program.functions {
                if func.name == "main"
                    || func.name.starts_with("vanta_rt_")
                    || func.name.starts_with("__")
                {
                    if reachable.insert(func.name.clone()) {
                        queue.push_back(func.name.clone());
                    }
                }
            }

            // Conservatively retain any function whose name appears in program vtables
            // (for dynamic dispatch)
            for vtable in &program.vtables {
                for method in &vtable.methods {
                    if reachable.insert(method.clone()) {
                        queue.push_back(method.clone());
                    }
                }
            }

            // Conservatively retain any function whose name appears in string literals
            // (in case of dynamic dispatch or reflection)
            for lit in &program.string_literals {
                if let Ok(s) = std::str::from_utf8(&lit.bytes) {
                    for func in &program.functions {
                        if s == func.name && reachable.insert(func.name.clone()) {
                            queue.push_back(func.name.clone());
                        }
                    }
                }
            }

            // Breadth-first traversal of direct call graph
            while let Some(caller) = queue.pop_front() {
                if let Some(callees) = call_graph.callees_of.get(&caller) {
                    for callee in callees {
                        if reachable.insert(callee.clone()) {
                            queue.push_back(callee.clone());
                        }
                    }
                }
            }

            program.functions.retain(|f| reachable.contains(&f.name));
            let funcs_removed = initial_funcs - program.functions.len();
            if funcs_removed > 0 {
                changed = true;
                stats.functions_eliminated += funcs_removed;
            }
        }

        // 2. Intra-procedural Unreachable Block & Effect-Aware Instruction Elimination
        for func in &mut program.functions {
            let (func_changed, blocks_rem, insts_rem) = Self::run_on_function(func);
            if func_changed {
                changed = true;
                stats.blocks_eliminated += blocks_rem;
                stats.instructions_eliminated += insts_rem;
            }
        }

        (changed, stats)
    }

    /// Runs intra-procedural unreachable block elimination and effect-aware
    /// dead instruction pruning on a single function.
    pub fn run_on_function(func: &mut MirFunction) -> (bool, usize, usize) {
        let mut changed = false;
        let mut total_blocks_removed = 0;
        let mut total_insts_removed = 0;

        // 1. Unreachable basic block elimination
        let (blocks_changed, blocks_removed) = Self::eliminate_unreachable_blocks(func);
        if blocks_changed {
            changed = true;
            total_blocks_removed += blocks_removed;
        }

        // 2. Effect-aware unused instruction elimination (fixed-point loop)
        loop {
            let (insts_changed, count) = Self::eliminate_unused_instructions(func);
            if insts_changed {
                changed = true;
                total_insts_removed += count;
            } else {
                break;
            }
        }

        (changed, total_blocks_removed, total_insts_removed)
    }

    /// Eliminates basic blocks that cannot be reached from the function entry block,
    /// repairs predecessor edges, and cleans up / simplifies affected phi nodes.
    fn eliminate_unreachable_blocks(func: &mut MirFunction) -> (bool, usize) {
        if func.blocks.is_empty() {
            return (false, 0);
        }

        let entry_id = func.blocks[0].id;
        let mut reachable = HashSet::new();
        let mut stack = vec![entry_id];
        reachable.insert(entry_id);

        let block_map: std::collections::HashMap<BlockId, &BasicBlock> =
            func.blocks.iter().map(|b| (b.id, b)).collect();

        while let Some(current_id) = stack.pop() {
            if let Some(block) = block_map.get(&current_id) {
                if let Some(term) = &block.terminator {
                    match term {
                        Terminator::Branch(target) => {
                            if reachable.insert(*target) {
                                stack.push(*target);
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            if reachable.insert(*then_block) {
                                stack.push(*then_block);
                            }
                            if reachable.insert(*else_block) {
                                stack.push(*else_block);
                            }
                        }
                        Terminator::Return(_) | Terminator::Unreachable => {}
                    }
                }
            }
        }

        let initial_count = func.blocks.len();
        func.blocks.retain(|b| reachable.contains(&b.id));
        let blocks_removed = initial_count - func.blocks.len();

        // Clean up and simplify phis that referenced eliminated blocks
        if blocks_removed > 0 {
            for block in &mut func.blocks {
                for inst in &mut block.instructions {
                    if let Instruction::Phi { dest, inputs, ty } = inst {
                        inputs.retain(|(pred, _)| reachable.contains(pred));
                        if inputs.len() == 1 {
                            let src = inputs[0].1;
                            *inst = Instruction::Copy {
                                dest: *dest,
                                src,
                                ty: ty.clone(),
                            };
                        } else if !inputs.is_empty() {
                            let first_src = inputs[0].1;
                            if inputs.iter().all(|(_, v)| *v == first_src) {
                                *inst = Instruction::Copy {
                                    dest: *dest,
                                    src: first_src,
                                    ty: ty.clone(),
                                };
                            }
                        }
                    }
                }
            }
        }

        (blocks_removed > 0, blocks_removed)
    }

    /// Eliminates instructions whose results are never used and that produce no side effects.
    /// Distinguishes pure computations from memory writes, trapping operations, and calls.
    fn eliminate_unused_instructions(func: &mut MirFunction) -> (bool, usize) {
        let mut used_values = HashSet::new();

        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Store { val, .. } => {
                        used_values.insert(*val);
                    }
                    Instruction::StoreIndirect { addr, val }
                    | Instruction::StoreIndirect32 { addr, val } => {
                        used_values.insert(*addr);
                        used_values.insert(*val);
                    }
                    Instruction::LoadIndirect { addr, .. } => {
                        used_values.insert(*addr);
                    }
                    Instruction::Call { args, .. } => {
                        for arg in args {
                            used_values.insert(*arg);
                        }
                    }
                    Instruction::CallIndirect {
                        callee_ptr, args, ..
                    } => {
                        used_values.insert(*callee_ptr);
                        for arg in args {
                            used_values.insert(*arg);
                        }
                    }
                    Instruction::Binary { lhs, rhs, .. } => {
                        used_values.insert(*lhs);
                        used_values.insert(*rhs);
                    }
                    Instruction::Unary { operand, .. } => {
                        used_values.insert(*operand);
                    }
                    Instruction::IndexAddr { index, .. } => {
                        used_values.insert(*index);
                    }
                    Instruction::IndexPtrAddr {
                        base_ptr, index, ..
                    } => {
                        used_values.insert(*base_ptr);
                        used_values.insert(*index);
                    }
                    Instruction::FieldAddr { base_ptr, .. } => {
                        used_values.insert(*base_ptr);
                    }
                    Instruction::StoreSlice { ptr, len, .. } => {
                        used_values.insert(*ptr);
                        used_values.insert(*len);
                    }
                    Instruction::Copy { src, .. } => {
                        used_values.insert(*src);
                    }
                    Instruction::Phi { inputs, .. } => {
                        for (_pred, val) in inputs {
                            used_values.insert(*val);
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

            if let Some(term) = &block.terminator {
                match term {
                    Terminator::CondBranch { cond, .. } => {
                        used_values.insert(*cond);
                    }
                    Terminator::Return(Some(val)) => {
                        used_values.insert(*val);
                    }
                    Terminator::Return(None) | Terminator::Branch(_) | Terminator::Unreachable => {}
                }
            }
        }

        let mut changed = false;
        let mut insts_removed = 0;

        for block in &mut func.blocks {
            let initial_len = block.instructions.len();
            block.instructions.retain(|inst| {
                match inst {
                    Instruction::ConstInt { dest, .. }
                    | Instruction::ConstBool { dest, .. }
                    | Instruction::SlotAddr { dest, .. }
                    | Instruction::Load { dest, .. } => used_values.contains(dest),
                    Instruction::Unary { dest, .. } => used_values.contains(dest),
                    Instruction::Binary { dest, op, .. } => {
                        // Preserves Div/Rem if they could potentially trap at runtime
                        match op {
                            MirBinaryOp::Div | MirBinaryOp::Rem => true,
                            _ => used_values.contains(dest),
                        }
                    }
                    Instruction::IndexAddr { dest, .. }
                    | Instruction::IndexPtrAddr { dest, .. }
                    | Instruction::FieldAddr { dest, .. } => used_values.contains(dest),
                    Instruction::Copy { dest, .. } | Instruction::Phi { dest, .. } => {
                        used_values.contains(dest)
                    }
                    Instruction::VtableAddr { dest, .. } => used_values.contains(dest),
                    Instruction::StrSlice {
                        dest_ptr, dest_len, ..
                    } => used_values.contains(dest_ptr) || used_values.contains(dest_len),
                    Instruction::LoadSlice {
                        dest_ptr, dest_len, ..
                    } => used_values.contains(dest_ptr) || used_values.contains(dest_len),
                    // NEVER remove instructions with potential side effects:
                    // Stores write memory; Calls may perform I/O or loop; Indirect loads read memory
                    Instruction::Store { .. }
                    | Instruction::StoreSlice { .. }
                    | Instruction::StoreIndirect { .. }
                    | Instruction::StoreIndirect32 { .. }
                    | Instruction::LoadIndirect { .. }
                    | Instruction::Call { .. }
                    | Instruction::CallIndirect { .. } => true,
                }
            });

            let removed_in_block = initial_len - block.instructions.len();
            if removed_in_block > 0 {
                changed = true;
                insts_removed += removed_in_block;
            }
        }

        (changed, insts_removed)
    }
}
