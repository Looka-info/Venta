//! SIMD Vector Register Allocator for x86-64 backend.
//!
//! Provides deterministic allocation of virtual 256-bit SIMD values (`SimdValueId`)
//! to physical vector registers (`ymm0`..`ymm15`) and handles 32-byte aligned vector spills.
//!
//! In the System V AMD64 ABI, all vector registers are caller-saved (volatile).
//! Any SIMD value that is live across a function call instruction is deterministically
//! spilled to a 32-byte aligned stack frame slot to prevent callee clobbering.

use crate::abi::classify_type;
use crate::register::{SimdReg, VectorWidth};
use std::collections::{HashMap, HashSet};
use std::fmt;
use vanta_mir::{Instruction, MirFunction, ValueId};
use vanta_sema::{EnumDef, StructDef};

/// Strongly typed identifier for a virtual SIMD vector value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SimdValueId(pub u32);

impl fmt::Display for SimdValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vsimd_{}", self.0)
    }
}

/// Identifies all MIR ValueIds that represent virtual SIMD vector values along with their width.
pub fn identify_simd_values_with_width(
    func: &MirFunction,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> (
    HashMap<ValueId, SimdValueId>,
    HashMap<SimdValueId, VectorWidth>,
) {
    let mut val_map = HashMap::new();
    let mut width_map = HashMap::new();
    let mut next_id = 0u32;

    for block in &func.blocks {
        for inst in &block.instructions {
            match inst {
                Instruction::Call {
                    dest: Some(d),
                    callee,
                    ret_type,
                    ..
                } => {
                    let is_intrinsic_512 = callee == "venta_simd_vec_load512"
                        || callee == "simd_vec_load512"
                        || callee == "venta_simd_vec_add512"
                        || callee == "simd_vec_add512"
                        || callee == "venta_simd_vec_fma512"
                        || callee == "simd_vec_fma512"
                        || callee == "venta_simd_reg_load512"
                        || callee == "simd_reg_load512"
                        || callee == "venta_simd_reg_add512"
                        || callee == "simd_reg_add512"
                        || callee == "venta_simd_reg_fma512"
                        || callee == "simd_reg_fma512";
                    let is_intrinsic_256 = callee == "venta_simd_reg_load256"
                        || callee == "simd_reg_load256"
                        || callee == "venta_simd_reg_add256"
                        || callee == "simd_reg_add256"
                        || callee == "venta_simd_reg_fma256"
                        || callee == "simd_reg_fma256"
                        || callee == "venta_simd_vec_load256"
                        || callee == "simd_vec_load256"
                        || callee == "venta_simd_vec_add256"
                        || callee == "simd_vec_add256"
                        || callee == "venta_simd_vec_fma256"
                        || callee == "simd_vec_fma256";
                    let ret_cls = classify_type(ret_type, structs, enums);
                    let is_512 = is_intrinsic_512 || ret_cls.is_vector512();
                    let is_256 = is_intrinsic_256 || ret_cls.is_vector256();
                    if is_512 || is_256 {
                        let sid = SimdValueId(next_id);
                        val_map.insert(*d, sid);
                        width_map.insert(
                            sid,
                            if is_512 {
                                VectorWidth::V512
                            } else {
                                VectorWidth::V256
                            },
                        );
                        next_id += 1;
                    }
                }
                Instruction::Load { dest, ty, .. } => {
                    let cls = classify_type(ty, structs, enums);
                    if cls.is_vector() {
                        let sid = SimdValueId(next_id);
                        val_map.insert(*dest, sid);
                        width_map.insert(
                            sid,
                            if cls.is_vector512() {
                                VectorWidth::V512
                            } else {
                                VectorWidth::V256
                            },
                        );
                        next_id += 1;
                    }
                }
                _ => {}
            }
        }
    }
    (val_map, width_map)
}

/// Identifies all MIR ValueIds that represent virtual SIMD vector values.
pub fn identify_simd_values(
    func: &MirFunction,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> HashMap<ValueId, SimdValueId> {
    identify_simd_values_with_width(func, structs, enums).0
}

/// Physical location of a virtual SIMD value after allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimdLocation {
    /// Allocated to a physical SIMD register (e.g. YMM0..YMM13 or ZMM0..ZMM29).
    Reg(SimdReg),
    /// Spilled to an aligned stack frame slot (0-based index).
    Spill(u32),
}

/// Live interval representing the lifetime of a virtual SIMD value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimdLiveInterval {
    pub val: SimdValueId,
    pub width: VectorWidth,
    pub start: usize,
    pub end: usize,
    pub does_cross_call: bool,
}

/// Result of SIMD register allocation for a function.
#[derive(Debug, Clone, Default)]
pub struct SimdAllocResult {
    /// Mapping from virtual SIMD value to assigned physical register or spill slot.
    pub assignments: HashMap<SimdValueId, SimdLocation>,
    /// Total number of spill slots required.
    pub num_spills: usize,
    /// Mapping from spill slot index to required vector width.
    pub spill_widths: HashMap<u32, VectorWidth>,
    /// Physical vector registers utilized by the allocation.
    pub used_registers: Vec<SimdReg>,
    /// Set of values that were spilled specifically due to crossing a function call.
    pub call_crossing_spills: HashSet<SimdValueId>,
}

pub struct SimdRegisterAllocator;

impl SimdRegisterAllocator {
    /// Allocatable physical YMM registers: YMM0 through YMM13 (14 registers).
    ///
    /// YMM14 and YMM15 are reserved as dedicated scratch buffers for
    /// emitting 256-bit memory reloads and stores around spilled operands.
    pub const ALLOCATABLE_YMM: [SimdReg; 14] = [
        SimdReg(0),
        SimdReg(1),
        SimdReg(2),
        SimdReg(3),
        SimdReg(4),
        SimdReg(5),
        SimdReg(6),
        SimdReg(7),
        SimdReg(8),
        SimdReg(9),
        SimdReg(10),
        SimdReg(11),
        SimdReg(12),
        SimdReg(13),
    ];

    /// Dedicated scratch register A for reloading/storing spilled operand 1 (AVX/AVX2).
    pub const SCRATCH_A: SimdReg = SimdReg(14);
    /// Dedicated scratch register B for reloading/storing spilled operand 2 (AVX/AVX2).
    pub const SCRATCH_B: SimdReg = SimdReg(15);

    /// Allocatable physical ZMM registers: ZMM0 through ZMM29 (30 registers).
    ///
    /// ZMM30 and ZMM31 are reserved as dedicated scratch buffers for
    /// emitting 512-bit memory reloads and stores around spilled operands.
    pub const ALLOCATABLE_ZMM: [SimdReg; 30] = [
        SimdReg(0),
        SimdReg(1),
        SimdReg(2),
        SimdReg(3),
        SimdReg(4),
        SimdReg(5),
        SimdReg(6),
        SimdReg(7),
        SimdReg(8),
        SimdReg(9),
        SimdReg(10),
        SimdReg(11),
        SimdReg(12),
        SimdReg(13),
        SimdReg(14),
        SimdReg(15),
        SimdReg(16),
        SimdReg(17),
        SimdReg(18),
        SimdReg(19),
        SimdReg(20),
        SimdReg(21),
        SimdReg(22),
        SimdReg(23),
        SimdReg(24),
        SimdReg(25),
        SimdReg(26),
        SimdReg(27),
        SimdReg(28),
        SimdReg(29),
    ];

    /// Dedicated scratch register A for reloading/storing spilled operand 1 (AVX-512).
    pub const SCRATCH_A_512: SimdReg = SimdReg(30);
    /// Dedicated scratch register B for reloading/storing spilled operand 2 (AVX-512).
    pub const SCRATCH_B_512: SimdReg = SimdReg(31);

    pub fn scratch_a(is_512: bool) -> SimdReg {
        if is_512 {
            Self::SCRATCH_A_512
        } else {
            Self::SCRATCH_A
        }
    }

    pub fn scratch_b(is_512: bool) -> SimdReg {
        if is_512 {
            Self::SCRATCH_B_512
        } else {
            Self::SCRATCH_B
        }
    }

    /// Allocates physical vector registers using AVX2 defaults.
    pub fn allocate(intervals: Vec<SimdLiveInterval>) -> SimdAllocResult {
        Self::allocate_with_target(intervals, false)
    }

    /// Allocates physical vector registers with target-feature awareness:
    /// - When `is_avx512` is true: uses 30 allocatable ZMM registers (`zmm0`..`zmm29`).
    /// - Otherwise: uses 14 allocatable YMM registers (`ymm0`..`ymm13`).
    pub fn allocate_with_target(
        mut intervals: Vec<SimdLiveInterval>,
        is_avx512: bool,
    ) -> SimdAllocResult {
        let mut result = SimdAllocResult::default();

        if intervals.is_empty() {
            return result;
        }

        // Sort intervals by start point ascending, breaking ties deterministically by SimdValueId
        intervals.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.val.0.cmp(&b.val.0)));

        // Free pool initialized in reverse order so pop() yields register 0, register 1, ...
        let mut free_regs: Vec<SimdReg> = if is_avx512 {
            Self::ALLOCATABLE_ZMM.iter().copied().rev().collect()
        } else {
            Self::ALLOCATABLE_YMM.iter().copied().rev().collect()
        };

        #[derive(Debug)]
        struct Active {
            val: SimdValueId,
            end: usize,
            reg: SimdReg,
        }

        let mut active: Vec<Active> = Vec::new();
        let mut next_spill_slot = 0u32;
        let mut used_regs_set: HashSet<SimdReg> = HashSet::new();

        for iv in intervals {
            // 1. Expire active intervals whose live range ended before iv.start
            let mut i = 0;
            while i < active.len() {
                if active[i].end < iv.start {
                    let expired = active.remove(i);
                    free_regs.push(expired.reg);
                } else {
                    i += 1;
                }
            }

            // 2. In System V AMD64 ABI, all vector registers are volatile (caller-saved).
            // Any SIMD value that is live across a function call must be spilled to memory.
            if iv.does_cross_call {
                result
                    .assignments
                    .insert(iv.val, SimdLocation::Spill(next_spill_slot));
                result.spill_widths.insert(next_spill_slot, iv.width);
                result.call_crossing_spills.insert(iv.val);
                next_spill_slot += 1;
                continue;
            }

            // 3. Allocate from free pool if available
            if let Some(reg) = free_regs.pop() {
                result.assignments.insert(iv.val, SimdLocation::Reg(reg));
                used_regs_set.insert(reg);
                active.push(Active {
                    val: iv.val,
                    end: iv.end,
                    reg,
                });
            } else {
                // Register pressure: all allocatable vector registers are currently occupied.
                // Find active interval with the furthest end point to evict.
                let mut latest_idx = None;
                let mut max_end = iv.end;

                for (idx, a) in active.iter().enumerate() {
                    if a.end > max_end {
                        max_end = a.end;
                        latest_idx = Some(idx);
                    }
                }

                if let Some(evict_idx) = latest_idx {
                    // Evict the interval that ends furthest in the future
                    let evicted = active.remove(evict_idx);
                    result
                        .assignments
                        .insert(evicted.val, SimdLocation::Spill(next_spill_slot));
                    result.spill_widths.insert(next_spill_slot, iv.width);
                    next_spill_slot += 1;

                    // Assign the evicted physical register to the current interval
                    result
                        .assignments
                        .insert(iv.val, SimdLocation::Reg(evicted.reg));
                    used_regs_set.insert(evicted.reg);
                    active.push(Active {
                        val: iv.val,
                        end: iv.end,
                        reg: evicted.reg,
                    });
                } else {
                    // Current interval ends later than all active intervals; spill it
                    result
                        .assignments
                        .insert(iv.val, SimdLocation::Spill(next_spill_slot));
                    result.spill_widths.insert(next_spill_slot, iv.width);
                    next_spill_slot += 1;
                }
            }
        }

        result.num_spills = next_spill_slot as usize;
        let mut used_vec: Vec<SimdReg> = used_regs_set.into_iter().collect();
        used_vec.sort_by_key(|r| r.0);
        result.used_registers = used_vec;

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_intervals() {
        let res = SimdRegisterAllocator::allocate(vec![]);
        assert_eq!(res.num_spills, 0);
        assert!(res.assignments.is_empty());
    }

    #[test]
    fn test_small_expression_fits_in_registers() {
        let intervals = vec![
            SimdLiveInterval {
                val: SimdValueId(1),
                width: VectorWidth::V256,
                start: 0,
                end: 10,
                does_cross_call: false,
            },
            SimdLiveInterval {
                val: SimdValueId(2),
                width: VectorWidth::V256,
                start: 2,
                end: 8,
                does_cross_call: false,
            },
            SimdLiveInterval {
                val: SimdValueId(3),
                width: VectorWidth::V256,
                start: 4,
                end: 12,
                does_cross_call: false,
            },
        ];

        let res = SimdRegisterAllocator::allocate(intervals);
        assert_eq!(res.num_spills, 0);
        assert_eq!(res.assignments.len(), 3);

        // All 3 overlap at point 4..8, so they must receive distinct physical registers
        let r1 = match res.assignments[&SimdValueId(1)] {
            SimdLocation::Reg(r) => r,
            _ => panic!("Expected register for v1"),
        };
        let r2 = match res.assignments[&SimdValueId(2)] {
            SimdLocation::Reg(r) => r,
            _ => panic!("Expected register for v2"),
        };
        let r3 = match res.assignments[&SimdValueId(3)] {
            SimdLocation::Reg(r) => r,
            _ => panic!("Expected register for v3"),
        };

        assert_ne!(r1, r2);
        assert_ne!(r1, r3);
        assert_ne!(r2, r3);
    }

    #[test]
    fn test_non_overlapping_intervals_reuse_register() {
        let intervals = vec![
            SimdLiveInterval {
                val: SimdValueId(1),
                width: VectorWidth::V256,
                start: 0,
                end: 4,
                does_cross_call: false,
            },
            SimdLiveInterval {
                val: SimdValueId(2),
                width: VectorWidth::V256,
                start: 6,
                end: 10,
                does_cross_call: false,
            },
        ];

        let res = SimdRegisterAllocator::allocate(intervals);
        assert_eq!(res.num_spills, 0);

        let r1 = match res.assignments[&SimdValueId(1)] {
            SimdLocation::Reg(r) => r,
            _ => panic!("Expected register"),
        };
        let r2 = match res.assignments[&SimdValueId(2)] {
            SimdLocation::Reg(r) => r,
            _ => panic!("Expected register"),
        };

        // Interval 1 expired before interval 2 started, so its register was recycled
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_call_crossing_forces_spill() {
        let intervals = vec![
            SimdLiveInterval {
                val: SimdValueId(1),
                width: VectorWidth::V256,
                start: 0,
                end: 10,
                does_cross_call: true, // Live across a call!
            },
            SimdLiveInterval {
                val: SimdValueId(2),
                width: VectorWidth::V256,
                start: 2,
                end: 6,
                does_cross_call: false,
            },
        ];

        let res = SimdRegisterAllocator::allocate(intervals);
        assert_eq!(res.num_spills, 1);
        assert!(matches!(
            res.assignments[&SimdValueId(1)],
            SimdLocation::Spill(0)
        ));
        assert!(matches!(
            res.assignments[&SimdValueId(2)],
            SimdLocation::Reg(_)
        ));
    }

    #[test]
    fn test_high_register_pressure_causes_furthest_end_spill() {
        // Create 16 simultaneous live intervals (capacity is 14)
        let mut intervals = Vec::new();
        for i in 0..16 {
            intervals.push(SimdLiveInterval {
                val: SimdValueId(i),
                width: VectorWidth::V256,
                start: 0,
                // End points: v0..v15 have increasing lifetimes
                end: 10 + (i as usize * 2),
                does_cross_call: false,
            });
        }

        let res = SimdRegisterAllocator::allocate(intervals);
        // Exactly 2 intervals must be spilled because 16 - 14 = 2
        assert_eq!(res.num_spills, 2);

        // The furthest end point intervals (v14 and v15) should be spilled
        assert!(matches!(
            res.assignments[&SimdValueId(14)],
            SimdLocation::Spill(_)
        ));
        assert!(matches!(
            res.assignments[&SimdValueId(15)],
            SimdLocation::Spill(_)
        ));

        // Earlier intervals (v0..v13) should have received physical registers
        for i in 0..14 {
            assert!(matches!(
                res.assignments[&SimdValueId(i)],
                SimdLocation::Reg(_)
            ));
        }
    }

    #[test]
    fn test_deterministic_allocation() {
        let make_intervals = || {
            vec![
                SimdLiveInterval {
                    val: SimdValueId(1),
                    width: VectorWidth::V256,
                    start: 0,
                    end: 10,
                    does_cross_call: false,
                },
                SimdLiveInterval {
                    val: SimdValueId(2),
                    width: VectorWidth::V256,
                    start: 0,
                    end: 12,
                    does_cross_call: false,
                },
                SimdLiveInterval {
                    val: SimdValueId(3),
                    width: VectorWidth::V256,
                    start: 5,
                    end: 15,
                    does_cross_call: false,
                },
            ]
        };

        let res1 = SimdRegisterAllocator::allocate(make_intervals());
        let res2 = SimdRegisterAllocator::allocate(make_intervals());

        assert_eq!(res1.num_spills, res2.num_spills);
        for id in [SimdValueId(1), SimdValueId(2), SimdValueId(3)] {
            assert_eq!(res1.assignments[&id], res2.assignments[&id]);
        }
    }

    #[test]
    fn test_avx512_allocates_up_to_30_zmm_registers() {
        // Create 30 simultaneous live intervals (all overlapping)
        let mut intervals = Vec::new();
        for i in 0..30 {
            intervals.push(SimdLiveInterval {
                val: SimdValueId(i),
                width: VectorWidth::V512,
                start: 0,
                end: 20 + (i as usize),
                does_cross_call: false,
            });
        }

        // In AVX-512 mode, all 30 allocatable ZMM registers (zmm0..zmm29) are available
        let res = SimdRegisterAllocator::allocate_with_target(intervals, true);
        assert_eq!(res.num_spills, 0);
        assert_eq!(res.assignments.len(), 30);
        assert_eq!(res.used_registers.len(), 30);

        for i in 0..30 {
            assert!(matches!(
                res.assignments[&SimdValueId(i)],
                SimdLocation::Reg(_)
            ));
        }
    }

    #[test]
    fn test_avx512_pressure_above_30_spills() {
        // Create 32 simultaneous live intervals (capacity is 30)
        let mut intervals = Vec::new();
        for i in 0..32 {
            intervals.push(SimdLiveInterval {
                val: SimdValueId(i),
                width: VectorWidth::V512,
                start: 0,
                end: 10 + (i as usize * 2),
                does_cross_call: false,
            });
        }

        let res = SimdRegisterAllocator::allocate_with_target(intervals, true);
        // 32 - 30 = 2 spills
        assert_eq!(res.num_spills, 2);

        // Furthest end points (v30 and v31) must be spilled
        assert!(matches!(
            res.assignments[&SimdValueId(30)],
            SimdLocation::Spill(_)
        ));
        assert!(matches!(
            res.assignments[&SimdValueId(31)],
            SimdLocation::Spill(_)
        ));

        // Spill widths must be recorded as 512-bit
        assert_eq!(res.spill_widths.get(&0), Some(&VectorWidth::V512));
        assert_eq!(res.spill_widths.get(&1), Some(&VectorWidth::V512));
    }

    #[test]
    fn test_avx512_call_crossing_spills_with_512_width() {
        let intervals = vec![
            SimdLiveInterval {
                val: SimdValueId(1),
                width: VectorWidth::V512,
                start: 0,
                end: 10,
                does_cross_call: true,
            },
            SimdLiveInterval {
                val: SimdValueId(2),
                width: VectorWidth::V512,
                start: 2,
                end: 6,
                does_cross_call: false,
            },
        ];

        let res = SimdRegisterAllocator::allocate_with_target(intervals, true);
        assert_eq!(res.num_spills, 1);
        assert!(matches!(
            res.assignments[&SimdValueId(1)],
            SimdLocation::Spill(0)
        ));
        assert_eq!(res.spill_widths.get(&0), Some(&VectorWidth::V512));
        assert!(res.call_crossing_spills.contains(&SimdValueId(1)));
        assert!(matches!(
            res.assignments[&SimdValueId(2)],
            SimdLocation::Reg(_)
        ));
    }
}
