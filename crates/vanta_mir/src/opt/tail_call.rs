//! Safe Tail-Call Optimization (TCO) for Vanta MIR.
//!
//! Provides:
//! 1. Tail-position call recognition and ABI safety verification.
//! 2. Direct self-tail recursion elimination into an O(1) stack loop.
//! 3. Conservative exclusion of non-tail calls, memory returns (sret), and complex types.

use crate::ir::*;
use std::collections::HashMap;
use vanta_sema::{EnumDef, StructDef, Type};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TailCallStats {
    pub calls_analyzed: usize,
    pub self_tail_calls_eliminated: usize,
    pub sibling_tail_calls_eliminated: usize,
}

pub struct TailCall;

impl TailCall {
    /// Runs Safe Tail-Call Optimization on an entire MIR program.
    pub fn run_on_program(
        program: &mut MirProgram,
    ) -> (bool, TailCallStats) {
        let mut stats = TailCallStats::default();
        let mut changed = false;

        for func in &mut program.functions {
            let (f_changed, f_stats) = Self::run_on_function(func, &program.structs, &program.enums);
            if f_changed {
                changed = true;
                stats.calls_analyzed += f_stats.calls_analyzed;
                stats.self_tail_calls_eliminated += f_stats.self_tail_calls_eliminated;
                stats.sibling_tail_calls_eliminated += f_stats.sibling_tail_calls_eliminated;
            }
        }

        (changed, stats)
    }

    /// Runs self-tail call optimization on an MIR function.
    pub fn run_on_function(
        func: &mut MirFunction,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
    ) -> (bool, TailCallStats) {
        let mut stats = TailCallStats::default();

        if func.blocks.is_empty() {
            return (false, stats);
        }

        // Verify return type and parameter types are ABI eligible for TCO:
        // No aggregate memory return (sret)
        if Self::is_memory_type(&func.ret_type, structs, enums) {
            return (false, stats);
        }

        // Reject if any parameter is aggregate memory or complex slice
        for (_, ty) in &func.params {
            if Self::is_memory_type(ty, structs, enums) || ty.is_slice() || ty.is_str_slice() {
                return (false, stats);
            }
        }

        // Find candidate blocks that end in a self-tail call:
        // A block where the last instruction is `Call { callee == func.name, .. }`
        // followed immediately by returning that call's result (or void return).
        let mut self_tail_blocks = Vec::new();

        for block in &func.blocks {
            if let Some(term) = &block.terminator {
                if let Some(inst) = block.instructions.last() {
                    if let Instruction::Call {
                        dest,
                        callee,
                        args,
                        ret_type,
                    } = inst
                    {
                        stats.calls_analyzed += 1;

                        // Check if it is a self call
                        if callee == &func.name {
                            // Verify matching argument count
                            if args.len() != func.params.len() {
                                continue;
                            }

                            // Verify tail position relative to terminator
                            let is_tail = match term {
                                Terminator::Return(Some(ret_val)) => {
                                    dest.as_ref() == Some(ret_val)
                                }
                                Terminator::Return(None) => {
                                    dest.is_none() || *ret_type == Type::Unit
                                }
                                _ => false,
                            };

                            if is_tail {
                                self_tail_blocks.push((block.id, args.clone()));
                            }
                        }
                    }
                }
            }
        }

        if self_tail_blocks.is_empty() {
            return (false, stats);
        }

        // Transform self-tail recursion into a loop:
        // 1. Create a dedicated loop header block `loop_header`.
        // 2. Move original entry block instructions & terminator into `loop_header`.
        // 3. Make entry block (id 0) branch unconditionally to `loop_header`.
        // 4. In each self-tail block:
        //    - Remove the `Call` instruction.
        //    - Append `Store` instructions for each parameter with the call arguments.
        //    - Replace the `Return` terminator with `Branch(loop_header)`.
        let next_block_id = BlockId(func.max_block_id() + 1);

        let orig_entry_instructions = std::mem::take(&mut func.blocks[0].instructions);
        let orig_entry_terminator = func.blocks[0].terminator.take();

        // New entry block now simply branches to the loop header
        func.blocks[0].terminator = Some(Terminator::Branch(next_block_id));

        let loop_header = BasicBlock {
            id: next_block_id,
            name: format!("{}_tail_loop_header", func.name),
            instructions: orig_entry_instructions,
            terminator: orig_entry_terminator,
        };

        // Insert loop_header directly after entry block (at index 1)
        func.blocks.insert(1, loop_header);

        let tail_count = self_tail_blocks.len();

        for (tail_block_id, tail_args) in self_tail_blocks {
            if let Some(block) = func.blocks.iter_mut().find(|b| b.id == tail_block_id) {
                // Remove the trailing Call instruction
                block.instructions.pop();

                // Append parameter stores with the tail call arguments
                for ((param_slot, _), arg_val) in func.params.iter().zip(&tail_args) {
                    block.instructions.push(Instruction::Store {
                        slot: *param_slot,
                        val: *arg_val,
                    });
                }

                // Replace Return with Branch to loop header
                block.terminator = Some(Terminator::Branch(next_block_id));
            }
        }

        stats.self_tail_calls_eliminated += tail_count;
        (true, stats)
    }

    /// Checks if a type is passed / returned in memory according to System V AMD64 ABI.
    fn is_memory_type(
        ty: &Type,
        structs: &HashMap<String, StructDef>,
        _enums: &HashMap<String, EnumDef>,
    ) -> bool {
        match ty {
            Type::Struct(name) => {
                if let Some(def) = structs.get(name) {
                    let mut size = 0;
                    for f in &def.fields {
                        size += f.ty.size_with_structs(structs);
                    }
                    size > 16 || size == 0
                } else {
                    true
                }
            }
            Type::Array { .. } => true,
            _ => false,
        }
    }
}
