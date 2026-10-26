//! Vanta MIR Optimization Module.

pub mod const_prop;
pub mod dce;

pub use const_prop::{ConstValue, ConstantPropagation};
pub use dce::DeadCodeElimination;

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
        if level == OptLevel::None {
            return;
        }

        // Verify pre-conditions
        MirVerifier::verify_program(program, diags);
        if diags.has_errors() {
            return;
        }

        for func in &mut program.functions {
            let mut iteration = 0;
            const MAX_PASS_ITERATIONS: usize = 10;

            loop {
                iteration += 1;
                let mut changed = false;

                // Pass 1: Constant Propagation and Folding
                changed |= ConstantPropagation::run_on_function(func);

                // Pass 2: Dead-Code and Unreachable Block Elimination
                changed |= DeadCodeElimination::run_on_function(func);

                if !changed || iteration >= MAX_PASS_ITERATIONS {
                    break;
                }
            }
        }

        // Verify post-conditions
        MirVerifier::verify_program(program, diags);
    }
}
