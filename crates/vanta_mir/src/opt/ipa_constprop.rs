//! Interprocedural Constant Propagation (IPA-ConstProp) for Vanta MIR.
//!
//! Analyzes direct call sites with statically known scalar arguments, evaluates
//! pure callee functions symbolically, exposes constant return values, and folds
//! known calls into immediate constants without requiring full function inlining.

use crate::ir::*;
use crate::opt::const_prop::{ConstValue, ConstantPropagation};
use crate::opt::dce::DeadCodeElimination;
use crate::opt::mem2reg::Mem2Reg;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IpaConstPropStats {
    pub call_sites_analyzed: usize,
    pub known_scalar_arguments: usize,
    pub constant_results_exposed: usize,
    pub unsupported_cases: usize,
}

pub struct IpaConstProp;

impl IpaConstProp {
    /// Maximum number of instructions in a callee to evaluate symbolically.
    pub const MAX_EVAL_CALLEE_SIZE: usize = 50;

    /// Runs interprocedural constant propagation across the program.
    pub fn run_on_program(program: &mut MirProgram) -> (bool, IpaConstPropStats) {
        let mut stats = IpaConstPropStats::default();
        let mut changed = false;

        let func_map: HashMap<String, MirFunction> = program
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.clone()))
            .collect();

        for func in &mut program.functions {
            // Collect known constants in this caller function
            let mut known_consts: HashMap<ValueId, ConstValue> = HashMap::new();
            for block in &func.blocks {
                for inst in &block.instructions {
                    match inst {
                        Instruction::ConstInt { dest, val, .. } => {
                            known_consts.insert(*dest, ConstValue::Int(*val));
                        }
                        Instruction::ConstBool { dest, val } => {
                            known_consts.insert(*dest, ConstValue::Bool(*val));
                        }
                        _ => {}
                    }
                }
            }

            for block in &mut func.blocks {
                for inst in &mut block.instructions {
                    let (dest_val, callee, args, ret_type) = match inst {
                        Instruction::Call {
                            dest: Some(dest_val),
                            callee,
                            args,
                            ret_type,
                        } => (*dest_val, callee.clone(), args.clone(), ret_type.clone()),
                        _ => continue,
                    };

                    stats.call_sites_analyzed += 1;

                    let callee_func = match func_map.get(&callee) {
                        Some(f) => f,
                        None => {
                            stats.unsupported_cases += 1;
                            continue;
                        }
                    };

                    // Check purity of callee (no I/O, no runtime calls, no panics, no stores to non-local memory)
                    if !Self::is_pure_callee(callee_func) {
                        stats.unsupported_cases += 1;
                        continue;
                    }

                    let inst_count: usize = callee_func
                        .blocks
                        .iter()
                        .map(|b| b.instructions.len())
                        .sum();
                    if inst_count > Self::MAX_EVAL_CALLEE_SIZE {
                        stats.unsupported_cases += 1;
                        continue;
                    }

                    // Check if all arguments are known constants
                    let mut const_args = Vec::new();
                    let mut all_const = true;
                    for arg in &args {
                        if let Some(&c) = known_consts.get(arg) {
                            stats.known_scalar_arguments += 1;
                            const_args.push(c);
                        } else {
                            all_const = false;
                            break;
                        }
                    }

                    if !all_const {
                        continue;
                    }

                    // Try symbolic evaluation of callee with constant arguments
                    if let Some(folded_const) = Self::try_evaluate_callee(callee_func, &const_args)
                    {
                        match folded_const {
                            ConstValue::Int(v) => {
                                *inst = Instruction::ConstInt {
                                    dest: dest_val,
                                    val: v,
                                    ty: ret_type,
                                };
                                known_consts.insert(dest_val, ConstValue::Int(v));
                                stats.constant_results_exposed += 1;
                                changed = true;
                            }
                            ConstValue::Bool(b) => {
                                *inst = Instruction::ConstBool {
                                    dest: dest_val,
                                    val: b,
                                };
                                known_consts.insert(dest_val, ConstValue::Bool(b));
                                stats.constant_results_exposed += 1;
                                changed = true;
                            }
                        }
                    }
                }
            }
        }

        (changed, stats)
    }

    /// Evaluates if a function is pure and safe for symbolic evaluation.
    fn is_pure_callee(func: &MirFunction) -> bool {
        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Call { .. }
                    | Instruction::LoadIndirect { .. }
                    | Instruction::StoreIndirect { .. }
                    | Instruction::StoreIndirect32 { .. }
                    | Instruction::StoreSlice { .. }
                    | Instruction::LoadSlice { .. }
                    | Instruction::StrSlice { .. } => return false,
                    _ => {}
                }
            }
        }
        true
    }

    /// Clones the callee with constant parameters, runs ConstantPropagation and DCE,
    /// and checks if it reduces to a compile-time constant return.
    fn try_evaluate_callee(callee: &MirFunction, args: &[ConstValue]) -> Option<ConstValue> {
        let mut cloned = callee.clone();
        if cloned.blocks.is_empty() {
            return None;
        }

        // Inject constant assignments for each parameter in the entry block
        let mut next_val_id = cloned.max_value_id() + 1;
        let mut prefix_insts = Vec::new();

        for ((slot, ty), &const_val) in cloned.params.iter().zip(args.iter()) {
            let val_id = ValueId(next_val_id);
            next_val_id += 1;
            match const_val {
                ConstValue::Int(v) => {
                    prefix_insts.push(Instruction::ConstInt {
                        dest: val_id,
                        val: v,
                        ty: ty.clone(),
                    });
                }
                ConstValue::Bool(b) => {
                    prefix_insts.push(Instruction::ConstBool {
                        dest: val_id,
                        val: b,
                    });
                }
            }
            prefix_insts.push(Instruction::Store {
                slot: *slot,
                val: val_id,
            });
        }

        prefix_insts.append(&mut cloned.blocks[0].instructions);
        cloned.blocks[0].instructions = prefix_insts;

        // Run Mem2Reg to promote constant parameter stores into SSA values
        Mem2Reg::run_on_function(&mut cloned);

        // Run ConstantPropagation and DCE
        for _ in 0..6 {
            let mut changed = false;
            changed |= ConstantPropagation::run_on_function(&mut cloned);
            changed |= DeadCodeElimination::run_on_function(&mut cloned);
            if !changed {
                break;
            }
        }

        // Check if the entry block now returns a known constant directly
        if cloned.blocks.len() == 1 {
            let block = &cloned.blocks[0];
            if let Some(Terminator::Return(Some(ret_v))) = &block.terminator {
                for inst in &block.instructions {
                    match inst {
                        Instruction::ConstInt { dest, val, .. } if dest == ret_v => {
                            return Some(ConstValue::Int(*val));
                        }
                        Instruction::ConstBool { dest, val } if dest == ret_v => {
                            return Some(ConstValue::Bool(*val));
                        }
                        _ => {}
                    }
                }
            }
        }

        None
    }
}
