//! Vanta MIR Optimization Module.

pub mod call_graph;
pub mod const_prop;
pub mod dce;
pub mod dom;
pub mod global_dce;
pub mod gvn;
pub mod induction_var;
pub mod inlining;
pub mod instruction_schedule;
pub mod ipa_constprop;
pub mod licm;
pub mod loop_peel;
pub mod loop_unroll;
pub mod mem2reg;
pub mod sroa;
pub mod strength_reduction;
pub mod tail_call;
pub mod vectorize;

pub use call_graph::{CallGraph, FunctionSummary};
pub use const_prop::{ConstValue, ConstantPropagation};
pub use dce::DeadCodeElimination;
pub use dom::DominatorTree;
pub use global_dce::{GlobalDce, GlobalDceStats};
pub use gvn::{Gvn, GvnExpr, GvnStats};
pub use induction_var::{BasicIndVar, InductionVarOpt, IvoStats};
pub use inlining::{Inliner, InlinerStats};
pub use instruction_schedule::{InstructionScheduleStats, InstructionScheduler};
pub use ipa_constprop::{IpaConstProp, IpaConstPropStats};
pub use licm::{Licm, LicmStats, NaturalLoop};
pub use loop_peel::{LoopPeelStats, LoopPeeling};
pub use loop_unroll::{LoopUnrollStats, LoopUnrolling};
pub use mem2reg::{Mem2Reg, Mem2RegStats};
pub use sroa::{Sroa, SroaStats};
pub use strength_reduction::{LoopStrengthReduction, LsrStats};
pub use tail_call::{TailCall, TailCallStats};
pub use vectorize::{LoopVectorizer, VectorizeStats, VectorizeTargetConfig};

use crate::ir::MirProgram;
use crate::verify::MirVerifier;
use vanta_diagnostics::DiagnosticBag;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptLevel {
    None,
    #[default]
    O1,
}

pub struct OptimizationPipeline;

impl OptimizationPipeline {
    /// Executes the optimization pipeline on a MIR program according to the chosen optimization level.
    /// Runs verification before and after all optimization passes.
    pub fn optimize_program(program: &mut MirProgram, level: OptLevel, diags: &mut DiagnosticBag) {
        Self::optimize_program_with_options(
            program, level, true, true, true, true, true, true, true, true, true, true, true, diags,
        );
    }

    /// Executes the optimization pipeline with explicit control over each optimization pass.
    pub fn optimize_program_with_options(
        program: &mut MirProgram,
        level: OptLevel,
        enable_mem2reg: bool,
        enable_gvn: bool,
        enable_inline: bool,
        enable_ipa_constprop: bool,
        enable_licm: bool,
        enable_indvar: bool,
        enable_strength_reduction: bool,
        enable_sroa: bool,
        enable_loop_peel: bool,
        enable_loop_unroll: bool,
        enable_instruction_schedule: bool,
        diags: &mut DiagnosticBag,
    ) {
        Self::optimize_program_full(
            program,
            level,
            enable_mem2reg,
            enable_gvn,
            enable_inline,
            enable_ipa_constprop,
            enable_licm,
            enable_indvar,
            enable_strength_reduction,
            enable_sroa,
            enable_loop_peel,
            enable_loop_unroll,
            enable_instruction_schedule,
            true,  // enable_global_dce
            true,  // enable_tail_calls
            false, // is_executable (library/test mode; preserves all functions)
            diags,
        );
    }

    /// Executes the full optimization pipeline with fine-grained control over all passes,
    /// including Global DCE, Safe Tail-Call Optimization, and execution target mode.
    pub fn optimize_program_full(
        program: &mut MirProgram,
        level: OptLevel,
        enable_mem2reg: bool,
        enable_gvn: bool,
        enable_inline: bool,
        enable_ipa_constprop: bool,
        enable_licm: bool,
        enable_indvar: bool,
        enable_strength_reduction: bool,
        enable_sroa: bool,
        enable_loop_peel: bool,
        enable_loop_unroll: bool,
        enable_instruction_schedule: bool,
        enable_global_dce: bool,
        enable_tail_calls: bool,
        is_executable: bool,
        diags: &mut DiagnosticBag,
    ) {
        Self::optimize_program_full_with_target(
            program,
            level,
            enable_mem2reg,
            enable_gvn,
            enable_inline,
            enable_ipa_constprop,
            enable_licm,
            enable_indvar,
            enable_strength_reduction,
            enable_sroa,
            enable_loop_peel,
            enable_loop_unroll,
            true, // enable_vectorize
            VectorizeTargetConfig::default(),
            enable_instruction_schedule,
            enable_global_dce,
            enable_tail_calls,
            is_executable,
            diags,
        );
    }

    /// Executes the full optimization pipeline with explicit target SIMD capability configuration
    /// for automatic vectorization.
    pub fn optimize_program_full_with_target(
        program: &mut MirProgram,
        level: OptLevel,
        enable_mem2reg: bool,
        enable_gvn: bool,
        enable_inline: bool,
        enable_ipa_constprop: bool,
        enable_licm: bool,
        enable_indvar: bool,
        enable_strength_reduction: bool,
        enable_sroa: bool,
        enable_loop_peel: bool,
        enable_loop_unroll: bool,
        enable_vectorize: bool,
        target_config: VectorizeTargetConfig,
        enable_instruction_schedule: bool,
        enable_global_dce: bool,
        enable_tail_calls: bool,
        is_executable: bool,
        diags: &mut DiagnosticBag,
    ) {
        if level == OptLevel::None {
            return;
        }

        // Verify pre-conditions
        MirVerifier::verify_program(program, diags);
        if diags.has_errors() {
            return;
        }

        // Phase 0: Safe Tail-Call Optimization (converts self-tail recursion into loop)
        if enable_tail_calls {
            TailCall::run_on_program(program);
        }

        // Phase 1: Pre-inlining Global DCE, SROA, and early Mem2Reg
        if enable_global_dce {
            GlobalDce::run_on_program(program, is_executable);
        }

        if enable_sroa {
            Sroa::run_on_program(program);
        }

        for func in &mut program.functions {
            DeadCodeElimination::run_on_function(func);
            if enable_mem2reg {
                Mem2Reg::run_on_function(func);
            }
        }

        // Phase 2: Interprocedural Constant Propagation
        if enable_ipa_constprop {
            IpaConstProp::run_on_program(program);
        }

        // Phase 3: Conservative Function Inlining
        if enable_inline {
            Inliner::run_on_program(program);
        }

        // Phase 4: Fixed-Point Intraprocedural Optimization on all functions
        for func in &mut program.functions {
            if enable_mem2reg {
                Mem2Reg::run_on_function(func);
            }
            DeadCodeElimination::run_on_function(func);

            if enable_sroa {
                let (sroa_changed, _) =
                    Sroa::run_on_function(func, &program.structs, &program.enums);
                if sroa_changed && enable_mem2reg {
                    Mem2Reg::run_on_function(func);
                    DeadCodeElimination::run_on_function(func);
                }
            }

            if enable_loop_peel {
                let (peeled, _) = LoopPeeling::run_on_function(func);
                if peeled {
                    ConstantPropagation::run_on_function(func);
                    DeadCodeElimination::run_on_function(func);
                }
            }

            // Loop Auto-Vectorization & Strip-Mining (runs before scalar loop unrolling)
            if enable_vectorize {
                let (vectorized, _) = LoopVectorizer::run_on_function(func, &target_config);
                if vectorized {
                    ConstantPropagation::run_on_function(func);
                    DeadCodeElimination::run_on_function(func);
                }
            }

            if enable_loop_unroll {
                let (unrolled, _) = LoopUnrolling::run_on_function(func);
                if unrolled {
                    ConstantPropagation::run_on_function(func);
                    DeadCodeElimination::run_on_function(func);
                }
            }

            let mut iteration = 0;
            const MAX_PASS_ITERATIONS: usize = 10;

            loop {
                iteration += 1;
                let mut changed = false;

                // Pass 1: Constant Propagation and Folding
                changed |= ConstantPropagation::run_on_function(func);

                // Pass 2: Induction Variable Optimization (IVO)
                if enable_indvar {
                    changed |= InductionVarOpt::run_on_function(func).0;
                }

                // Pass 3: Loop Strength Reduction (LSR)
                if enable_strength_reduction {
                    changed |= LoopStrengthReduction::run_on_function(func).0;
                }

                // Pass 4: Loop Invariant Code Motion (LICM)
                if enable_licm {
                    changed |= Licm::run_on_function(func).0;
                }

                // Pass 5: Global Value Numbering & Common Subexpression Elimination
                if enable_gvn {
                    changed |= Gvn::run_on_function(func).0;
                }

                // Pass 6: Dead-Code and Unreachable Block Elimination
                changed |= DeadCodeElimination::run_on_function(func);

                if !changed || iteration >= MAX_PASS_ITERATIONS {
                    break;
                }
            }

            // Pass 7: Instruction Scheduling (prior to physical register allocation)
            if enable_instruction_schedule {
                InstructionScheduler::run_on_function(func);
            }
        }

        // Phase 5: Post-Optimization Global DCE (removes functions made dead by inlining or constant folding)
        if enable_global_dce {
            GlobalDce::run_on_program(program, is_executable);
        }

        // Verify post-conditions
        MirVerifier::verify_program(program, diags);
    }
}
