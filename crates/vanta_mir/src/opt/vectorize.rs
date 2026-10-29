//! Loop Auto-Vectorization & Strip-Mining pass for Venta MIR.
//!
//! Recognizes eligible counted scalar loops operating independently over
//! contiguous arrays or slices, strip-mines them into a 4-wide 256-bit SIMD vector loop
//! using YMM operations (`vpaddq` for 64-bit integer addition, `vfmadd231pd` for double-precision FMA)
//! and preserves a correct scalar remainder loop handling leftover iterations.

use crate::ir::*;
use crate::opt::dom::DominatorTree;
use crate::opt::licm::NaturalLoop;
use std::collections::{HashMap, HashSet};
use vanta_sema::Type;

/// Target CPU SIMD capabilities used to gate auto-vectorization passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VectorizeTargetConfig {
    /// 256-bit floating point and unaligned vector memory moves (`vmovups`).
    pub has_avx: bool,
    /// 256-bit integer SIMD (`vpaddq`).
    pub has_avx2: bool,
    /// Fused Multiply-Add (`vfmadd231pd`).
    pub has_fma: bool,
}

/// Statistics and optimization diagnostics collected by the loop vectorizer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VectorizeStats {
    pub loops_analyzed: usize,
    pub loops_vectorized: usize,
    pub rejected_unsafe: usize,
    pub rejected_unsupported: usize,
    pub rejected_target_features: usize,
    pub rejected_profitability: usize,
}

/// Base location for array accesses (either pointer-backed or stack-slot-backed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseLocation {
    Ptr(ValueId),
    Slot(SlotId),
}

/// Recognized vectorizable element-wise operation pattern.
#[derive(Debug, Clone)]
enum VectorPattern {
    /// 4-lane 64-bit packed integer addition: out[i] = a[i] + b[i]
    IntegerAdd256 {
        base_out: BaseLocation,
        base_a: BaseLocation,
        base_b: BaseLocation,
    },
    /// 4-lane double-precision packed FMA: out[i] = a[i] * b[i] + c[i]
    FloatFma256 {
        base_out: BaseLocation,
        base_a: BaseLocation,
        base_b: BaseLocation,
        base_c: BaseLocation,
    },
}

pub struct LoopVectorizer;

impl LoopVectorizer {
    /// Runs loop auto-vectorization on a single MIR function.
    pub fn run_on_function(
        func: &mut MirFunction,
        target_config: &VectorizeTargetConfig,
    ) -> (bool, VectorizeStats) {
        let mut overall_changed = false;
        let mut stats = VectorizeStats::default();

        if func.blocks.is_empty() {
            return (false, stats);
        }

        let dom_tree = match DominatorTree::compute(func) {
            Some(dt) => dt,
            None => return (false, stats),
        };

        let loops = NaturalLoop::discover_loops(func, &dom_tree);
        if loops.is_empty() {
            return (false, stats);
        }

        for loop_info in &loops {
            stats.loops_analyzed += 1;

            // 1. Must be single-latch
            if loop_info.latches.len() != 1 {
                stats.rejected_unsupported += 1;
                continue;
            }
            let latch_id = loop_info.latches[0];

            // 2. Discover header terminator: CondBranch { cond, then_block, else_block }
            let header_block = match func.blocks.iter().find(|b| b.id == loop_info.header) {
                Some(b) => b,
                None => {
                    stats.rejected_unsupported += 1;
                    continue;
                }
            };

            let (cond_val, body_entry_id, exit_block_id) = match &header_block.terminator {
                Some(Terminator::CondBranch {
                    cond,
                    then_block,
                    else_block,
                }) => {
                    if loop_info.blocks.contains(then_block)
                        && !loop_info.blocks.contains(else_block)
                    {
                        (*cond, *then_block, *else_block)
                    } else {
                        stats.rejected_unsupported += 1;
                        continue;
                    }
                }
                _ => {
                    stats.rejected_unsupported += 1;
                    continue;
                }
            };

            // 3. Obtain dedicated preheader block
            let (preheader_id, _) =
                NaturalLoop::get_or_create_preheader(func, loop_info, &dom_tree);

            // 4. Identify basic induction variable in loop header
            let header_block = func
                .blocks
                .iter()
                .find(|b| b.id == loop_info.header)
                .unwrap();
            let mut phis = Vec::new();
            for inst in &header_block.instructions {
                if let Instruction::Phi { dest, inputs, ty } = inst {
                    let mut init_val = None;
                    let mut latch_val = None;
                    for (pred, val) in inputs {
                        if *pred == preheader_id {
                            init_val = Some(*val);
                        } else if *pred == latch_id {
                            latch_val = Some(*val);
                        }
                    }
                    if let (Some(iv), Some(lv)) = (init_val, latch_val) {
                        phis.push((*dest, iv, lv, ty.clone()));
                    }
                }
            }

            // Reject if there are multiple Phis representing loop-carried dependencies (e.g. reductions)
            if phis.is_empty() {
                stats.rejected_unsupported += 1;
                continue;
            }
            if phis.len() > 1 {
                stats.rejected_unsafe += 1;
                continue;
            }

            let (iv_dest, init_val, latch_val, iv_ty) = phis[0].clone();
            if iv_ty != Type::I64 {
                stats.rejected_unsupported += 1;
                continue;
            }

            // 5. Verify step is +1 in latch block
            let latch_block = match func.blocks.iter().find(|b| b.id == latch_id) {
                Some(b) => b,
                None => {
                    stats.rejected_unsupported += 1;
                    continue;
                }
            };

            let mut is_step_plus_one = false;
            for inst in &latch_block.instructions {
                if let Instruction::Binary {
                    dest,
                    op: MirBinaryOp::Add,
                    lhs,
                    rhs,
                    ..
                } = inst
                {
                    if *dest == latch_val {
                        // Check if one operand is iv_dest and the other is constant 1
                        let other = if *lhs == iv_dest {
                            Some(*rhs)
                        } else if *rhs == iv_dest {
                            Some(*lhs)
                        } else {
                            None
                        };

                        if let Some(other_val) = other {
                            if Self::find_const_int(func, other_val) == Some(1) {
                                is_step_plus_one = true;
                                break;
                            }
                        }
                    }
                }
            }

            if !is_step_plus_one {
                stats.rejected_unsupported += 1;
                continue;
            }

            // 6. Inspect loop condition `cond_val`: iv_dest < limit_val or iv_dest <= limit_val
            let (is_lte, limit_val) =
                match Self::find_loop_bound(func, loop_info.header, cond_val, iv_dest) {
                    Some(b) => b,
                    None => {
                        stats.rejected_unsupported += 1;
                        continue;
                    }
                };

            // 7. Check for escaping definitions from the loop body to outside
            if Self::has_escaping_definitions(func, loop_info, iv_dest) {
                stats.rejected_unsafe += 1;
                continue;
            }

            // 8. Analyze loop body instructions for supported SIMD patterns
            let pattern = match Self::analyze_loop_body(
                func,
                loop_info,
                iv_dest,
                target_config,
                &mut stats,
            ) {
                Some(p) => p,
                None => {
                    continue;
                }
            };

            // 9. Profitability check: if static trip count is known to be < 4, reject
            let init_const = Self::find_const_int(func, init_val);
            let limit_const = Self::find_const_int(func, limit_val);
            if let (Some(init_c), Some(limit_c)) = (init_const, limit_const) {
                let total = if is_lte {
                    limit_c - init_c + 1
                } else {
                    limit_c - init_c
                };
                if total < 4 {
                    stats.rejected_profitability += 1;
                    continue;
                }
            }

            // 10. Perform Strip-Mining & Vector Loop Emission
            let changed = Self::apply_strip_mining(
                func,
                loop_info,
                preheader_id,
                body_entry_id,
                exit_block_id,
                iv_dest,
                init_val,
                limit_val,
                is_lte,
                latch_id,
                latch_val,
                pattern,
            );

            if changed {
                overall_changed = true;
                stats.loops_vectorized += 1;
                break; // One transformation per pass run
            }
        }

        (overall_changed, stats)
    }

    /// Finds a constant integer definition for a ValueId in the function.
    fn find_const_int(func: &MirFunction, val: ValueId) -> Option<i64> {
        for b in &func.blocks {
            for inst in &b.instructions {
                if let Instruction::ConstInt { dest, val: c, .. } = inst {
                    if *dest == val {
                        return Some(*c);
                    }
                }
            }
        }
        None
    }

    /// Finds the loop bound tested by `cond_val`. Returns (is_lte, limit_value).
    fn find_loop_bound(
        func: &MirFunction,
        header_id: BlockId,
        cond_val: ValueId,
        iv_dest: ValueId,
    ) -> Option<(bool, ValueId)> {
        let header = func.blocks.iter().find(|b| b.id == header_id)?;
        for inst in &header.instructions {
            if let Instruction::Binary {
                dest, op, lhs, rhs, ..
            } = inst
            {
                if *dest == cond_val && *lhs == iv_dest {
                    match op {
                        MirBinaryOp::Lt => return Some((false, *rhs)),
                        MirBinaryOp::LtEq => return Some((true, *rhs)),
                        _ => return None,
                    }
                }
            }
        }
        None
    }

    /// Checks if any SSA value defined inside the loop body is used outside the loop.
    fn has_escaping_definitions(
        func: &MirFunction,
        loop_info: &NaturalLoop,
        iv_dest: ValueId,
    ) -> bool {
        // Collect side-exit blocks reached from the loop body that terminate in Unreachable
        let mut side_exits = HashSet::new();
        for &b_id in &loop_info.blocks {
            if let Some(b) = func.blocks.iter().find(|blk| blk.id == b_id) {
                if let Some(term) = &b.terminator {
                    match term {
                        Terminator::Branch(t) => {
                            if !loop_info.blocks.contains(t) {
                                side_exits.insert(*t);
                            }
                        }
                        Terminator::CondBranch {
                            then_block,
                            else_block,
                            ..
                        } => {
                            if !loop_info.blocks.contains(then_block) {
                                side_exits.insert(*then_block);
                            }
                            if !loop_info.blocks.contains(else_block) {
                                side_exits.insert(*else_block);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut loop_defs = HashSet::new();
        for &b_id in &loop_info.blocks {
            if let Some(b) = func.blocks.iter().find(|blk| blk.id == b_id) {
                for inst in &b.instructions {
                    for d in inst.defs() {
                        if d != iv_dest {
                            loop_defs.insert(d);
                        }
                    }
                }
            }
        }

        for b in &func.blocks {
            if !loop_info.blocks.contains(&b.id) {
                // Ignore side-exit blocks that terminate in Unreachable (e.g. bounds_fail panic blocks)
                if side_exits.contains(&b.id) && b.terminator == Some(Terminator::Unreachable) {
                    continue;
                }

                for inst in &b.instructions {
                    for u in inst.uses() {
                        if loop_defs.contains(&u) {
                            return true;
                        }
                    }
                }
                if let Some(term) = &b.terminator {
                    match term {
                        Terminator::CondBranch { cond, .. } => {
                            if loop_defs.contains(cond) {
                                return true;
                            }
                        }
                        Terminator::Return(Some(val)) => {
                            if loop_defs.contains(val) {
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        false
    }

    /// Analyzes the loop body instructions to match a supported vector pattern.
    fn analyze_loop_body(
        func: &MirFunction,
        loop_info: &NaturalLoop,
        iv_dest: ValueId,
        target_config: &VectorizeTargetConfig,
        stats: &mut VectorizeStats,
    ) -> Option<VectorPattern> {
        let mut stores = Vec::new();
        let mut has_unsupported_calls = false;

        // Collect stores and check calls
        for &b_id in &loop_info.blocks {
            let b = func.blocks.iter().find(|blk| blk.id == b_id)?;
            for inst in &b.instructions {
                match inst {
                    Instruction::Call { callee, .. } => {
                        // Allow bounds check fail calls
                        if callee != "vanta_bounds_check_fail" {
                            has_unsupported_calls = true;
                        }
                    }
                    Instruction::StoreIndirect { addr, val } => {
                        stores.push((*addr, *val));
                    }
                    Instruction::Store { .. } => {
                        // Reject non-indirect scalar slot stores inside loop body
                        stats.rejected_unsafe += 1;
                        return None;
                    }
                    _ => {}
                }
            }
        }

        if has_unsupported_calls {
            stats.rejected_unsupported += 1;
            return None;
        }

        // Exactly one array store is supported for simple vector kernels
        if stores.len() != 1 {
            stats.rejected_unsupported += 1;
            return None;
        }

        let (out_addr, val_dest) = stores[0];

        // Find how out_addr is computed: must be index with iv_dest and elem_size 8
        let base_out = Self::find_array_base(func, loop_info, out_addr, iv_dest)?;

        // Build def map for instructions inside the loop
        let mut inst_map: HashMap<ValueId, Instruction> = HashMap::new();
        for &b_id in &loop_info.blocks {
            let b = func.blocks.iter().find(|blk| blk.id == b_id)?;
            for inst in &b.instructions {
                for d in inst.defs() {
                    inst_map.insert(d, inst.clone());
                }
            }
        }

        let val_inst = inst_map.get(&val_dest)?;

        // Match Pattern A: 64-bit Integer Addition (`vpaddq`)
        if let Instruction::Binary {
            op: MirBinaryOp::Add,
            lhs,
            rhs,
            ty: Type::I64,
            ..
        } = val_inst
        {
            let base_a =
                Self::match_array_load(func, loop_info, &inst_map, *lhs, iv_dest, Type::I64);
            let base_b =
                Self::match_array_load(func, loop_info, &inst_map, *rhs, iv_dest, Type::I64);

            if let (Some(a), Some(b)) = (base_a, base_b) {
                if !target_config.has_avx2 {
                    stats.rejected_target_features += 1;
                    return None;
                }
                return Some(VectorPattern::IntegerAdd256 {
                    base_out,
                    base_a: a,
                    base_b: b,
                });
            }
        }

        // Match Pattern B: Double-Precision FMA (`vfmadd231pd`)
        // val = (a * b) + c or c + (a * b)
        if let Instruction::Binary {
            op: MirBinaryOp::Add,
            lhs,
            rhs,
            ty: Type::F64,
            ..
        } = val_inst
        {
            let (mul_val, c_val) = match (inst_map.get(lhs), inst_map.get(rhs)) {
                (
                    Some(Instruction::Binary {
                        op: MirBinaryOp::Mul,
                        ..
                    }),
                    _,
                ) => (*lhs, *rhs),
                (
                    _,
                    Some(Instruction::Binary {
                        op: MirBinaryOp::Mul,
                        ..
                    }),
                ) => (*rhs, *lhs),
                _ => {
                    stats.rejected_unsupported += 1;
                    return None;
                }
            };

            if let Some(Instruction::Binary {
                op: MirBinaryOp::Mul,
                lhs: mul_a,
                rhs: mul_b,
                ty: Type::F64,
                ..
            }) = inst_map.get(&mul_val)
            {
                let base_a =
                    Self::match_array_load(func, loop_info, &inst_map, *mul_a, iv_dest, Type::F64);
                let base_b =
                    Self::match_array_load(func, loop_info, &inst_map, *mul_b, iv_dest, Type::F64);
                let base_c =
                    Self::match_array_load(func, loop_info, &inst_map, c_val, iv_dest, Type::F64);

                if let (Some(a), Some(b), Some(c)) = (base_a, base_b, base_c) {
                    if !target_config.has_avx || !target_config.has_fma {
                        stats.rejected_target_features += 1;
                        return None;
                    }
                    return Some(VectorPattern::FloatFma256 {
                        base_out,
                        base_a: a,
                        base_b: b,
                        base_c: c,
                    });
                }
            }
        }

        stats.rejected_unsupported += 1;
        None
    }

    /// Matches a LoadIndirect of an indexed array element.
    fn match_array_load(
        func: &MirFunction,
        loop_info: &NaturalLoop,
        inst_map: &HashMap<ValueId, Instruction>,
        load_val: ValueId,
        iv_dest: ValueId,
        expected_ty: Type,
    ) -> Option<BaseLocation> {
        let inst = inst_map.get(&load_val)?;
        if let Instruction::LoadIndirect { addr, ty, .. } = inst {
            if *ty == expected_ty {
                return Self::find_array_base(func, loop_info, *addr, iv_dest);
            }
        }
        None
    }

    /// Finds the base pointer or slot from an address value indexed by `iv_dest`.
    fn find_array_base(
        func: &MirFunction,
        loop_info: &NaturalLoop,
        addr_val: ValueId,
        iv_dest: ValueId,
    ) -> Option<BaseLocation> {
        for &b_id in &loop_info.blocks {
            let b = func.blocks.iter().find(|blk| blk.id == b_id)?;
            for inst in &b.instructions {
                match inst {
                    Instruction::IndexPtrAddr {
                        dest,
                        base_ptr,
                        index,
                        elem_size,
                    } => {
                        if *dest == addr_val && *index == iv_dest && *elem_size == 8 {
                            return Some(BaseLocation::Ptr(*base_ptr));
                        }
                    }
                    Instruction::IndexAddr {
                        dest,
                        slot,
                        index,
                        elem_size,
                    } => {
                        if *dest == addr_val && *index == iv_dest && *elem_size == 8 {
                            return Some(BaseLocation::Slot(*slot));
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    /// Ensures that an SSA value is available before entering the vector loop by hoisting
    /// read-only slot loads or constants into check_block if defined inside the loop.
    fn ensure_available<F>(
        val: ValueId,
        func: &MirFunction,
        loop_info: &NaturalLoop,
        insts: &mut Vec<Instruction>,
        alloc_val: &mut F,
    ) -> Option<ValueId>
    where
        F: FnMut() -> ValueId,
    {
        let mut def_block = None;
        let mut def_inst = None;

        for b in &func.blocks {
            for inst in &b.instructions {
                for d in inst.defs() {
                    if d == val {
                        def_block = Some(b.id);
                        def_inst = Some(inst.clone());
                        break;
                    }
                }
            }
        }

        let def_b_id = def_block?;
        if !loop_info.blocks.contains(&def_b_id) {
            // Defined outside the loop: already available!
            return Some(val);
        }

        // Defined inside the loop: hoist read-only slot loads, consts, or slot addresses
        match def_inst? {
            Instruction::Load { slot, ty, .. } => {
                let is_mutated = func.blocks.iter().any(|b| {
                    loop_info.blocks.contains(&b.id)
                        && b.instructions.iter().any(|i| match i {
                            Instruction::Store { slot: s, .. } => *s == slot,
                            _ => false,
                        })
                });

                if is_mutated {
                    return None;
                }

                let fresh_v = alloc_val();
                insts.push(Instruction::Load {
                    dest: fresh_v,
                    slot,
                    ty,
                });
                Some(fresh_v)
            }
            Instruction::ConstInt { val: c, ty, .. } => {
                let fresh_v = alloc_val();
                insts.push(Instruction::ConstInt {
                    dest: fresh_v,
                    val: c,
                    ty,
                });
                Some(fresh_v)
            }
            Instruction::SlotAddr { slot, .. } => {
                let fresh_v = alloc_val();
                insts.push(Instruction::SlotAddr {
                    dest: fresh_v,
                    slot,
                });
                Some(fresh_v)
            }
            _ => None,
        }
    }

    /// Ensures that a BaseLocation is available in check_block.
    fn ensure_base_available<F>(
        base: BaseLocation,
        func: &MirFunction,
        loop_info: &NaturalLoop,
        insts: &mut Vec<Instruction>,
        alloc_val: &mut F,
    ) -> Option<BaseLocation>
    where
        F: FnMut() -> ValueId,
    {
        match base {
            BaseLocation::Slot(s) => Some(BaseLocation::Slot(s)),
            BaseLocation::Ptr(p) => {
                let hoisted_p = Self::ensure_available(p, func, loop_info, insts, alloc_val)?;
                Some(BaseLocation::Ptr(hoisted_p))
            }
        }
    }

    /// Applies strip-mining to the recognized loop:
    /// Constructs `check_block`, `vector_header`, `vector_body`, and re-wires `scalar_header`.
    fn apply_strip_mining(
        func: &mut MirFunction,
        loop_info: &NaturalLoop,
        preheader_id: BlockId,
        _body_entry_id: BlockId,
        _exit_block_id: BlockId,
        iv_dest: ValueId,
        init_val: ValueId,
        limit_val: ValueId,
        is_lte: bool,
        latch_id: BlockId,
        latch_val: ValueId,
        pattern: VectorPattern,
    ) -> bool {
        let scalar_header_id = loop_info.header;

        // Allocate fresh identifiers
        let mut next_b_id = func.max_block_id() + 1;
        let check_block_id = BlockId(next_b_id);
        next_b_id += 1;
        let vec_header_id = BlockId(next_b_id);
        next_b_id += 1;
        let vec_body_id = BlockId(next_b_id);

        let mut next_v_id = func.max_value_id() + 1;
        let mut alloc_val = || {
            let id = ValueId(next_v_id);
            next_v_id += 1;
            id
        };

        // 1. Build check_block
        let mut check_insts = Vec::new();

        // Ensure all loop inputs and bases are hoisted and available in check_block
        let init_val = match Self::ensure_available(
            init_val,
            func,
            loop_info,
            &mut check_insts,
            &mut alloc_val,
        ) {
            Some(v) => v,
            None => return false,
        };
        let limit_val = match Self::ensure_available(
            limit_val,
            func,
            loop_info,
            &mut check_insts,
            &mut alloc_val,
        ) {
            Some(v) => v,
            None => return false,
        };
        let pattern = match pattern {
            VectorPattern::IntegerAdd256 {
                base_out,
                base_a,
                base_b,
            } => {
                let out = match Self::ensure_base_available(
                    base_out,
                    func,
                    loop_info,
                    &mut check_insts,
                    &mut alloc_val,
                ) {
                    Some(v) => v,
                    None => return false,
                };
                let a = match Self::ensure_base_available(
                    base_a,
                    func,
                    loop_info,
                    &mut check_insts,
                    &mut alloc_val,
                ) {
                    Some(v) => v,
                    None => return false,
                };
                let b = match Self::ensure_base_available(
                    base_b,
                    func,
                    loop_info,
                    &mut check_insts,
                    &mut alloc_val,
                ) {
                    Some(v) => v,
                    None => return false,
                };
                VectorPattern::IntegerAdd256 {
                    base_out: out,
                    base_a: a,
                    base_b: b,
                }
            }
            VectorPattern::FloatFma256 {
                base_out,
                base_a,
                base_b,
                base_c,
            } => {
                let out = match Self::ensure_base_available(
                    base_out,
                    func,
                    loop_info,
                    &mut check_insts,
                    &mut alloc_val,
                ) {
                    Some(v) => v,
                    None => return false,
                };
                let a = match Self::ensure_base_available(
                    base_a,
                    func,
                    loop_info,
                    &mut check_insts,
                    &mut alloc_val,
                ) {
                    Some(v) => v,
                    None => return false,
                };
                let b = match Self::ensure_base_available(
                    base_b,
                    func,
                    loop_info,
                    &mut check_insts,
                    &mut alloc_val,
                ) {
                    Some(v) => v,
                    None => return false,
                };
                let c = match Self::ensure_base_available(
                    base_c,
                    func,
                    loop_info,
                    &mut check_insts,
                    &mut alloc_val,
                ) {
                    Some(v) => v,
                    None => return false,
                };
                VectorPattern::FloatFma256 {
                    base_out: out,
                    base_a: a,
                    base_b: b,
                    base_c: c,
                }
            }
        };

        // count = limit - init (or limit - init + 1 if <=)
        let raw_diff = alloc_val();
        check_insts.push(Instruction::Binary {
            dest: raw_diff,
            op: MirBinaryOp::Sub,
            lhs: limit_val,
            rhs: init_val,
            ty: Type::I64,
        });

        let count_val = if is_lte {
            let const_one = alloc_val();
            check_insts.push(Instruction::ConstInt {
                dest: const_one,
                val: 1,
                ty: Type::I64,
            });
            let diff_plus_1 = alloc_val();
            check_insts.push(Instruction::Binary {
                dest: diff_plus_1,
                op: MirBinaryOp::Add,
                lhs: raw_diff,
                rhs: const_one,
                ty: Type::I64,
            });
            diff_plus_1
        } else {
            raw_diff
        };

        // can_vectorize: count >= 4
        let const_4 = alloc_val();
        check_insts.push(Instruction::ConstInt {
            dest: const_4,
            val: 4,
            ty: Type::I64,
        });

        let count_ge_4 = alloc_val();
        check_insts.push(Instruction::Binary {
            dest: count_ge_4,
            op: MirBinaryOp::LtEq,
            lhs: const_4,
            rhs: count_val,
            ty: Type::Bool,
        });

        // Compute vector_end = init + (count & -4)
        let mask_val = alloc_val();
        check_insts.push(Instruction::ConstInt {
            dest: mask_val,
            val: -4,
            ty: Type::I64,
        });

        let vec_count = alloc_val();
        check_insts.push(Instruction::Binary {
            dest: vec_count,
            op: MirBinaryOp::BitAnd,
            lhs: count_val,
            rhs: mask_val,
            ty: Type::I64,
        });

        let vector_end_val = alloc_val();
        check_insts.push(Instruction::Binary {
            dest: vector_end_val,
            op: MirBinaryOp::Add,
            lhs: init_val,
            rhs: vec_count,
            ty: Type::I64,
        });

        // Runtime alias check if destination and sources are distinct pointers
        let can_vectorize_val = Self::emit_alias_check(
            &mut check_insts,
            &pattern,
            count_val,
            count_ge_4,
            &mut alloc_val,
        );

        let check_block = BasicBlock {
            id: check_block_id,
            name: "vector.check".to_string(),
            instructions: check_insts,
            terminator: Some(Terminator::CondBranch {
                cond: can_vectorize_val,
                then_block: vec_header_id,
                else_block: scalar_header_id,
            }),
        };

        // 2. Build vector_header
        let i_vec_v = alloc_val();
        let i_vec_next_v = alloc_val();

        let vec_phi = Instruction::Phi {
            dest: i_vec_v,
            inputs: vec![(check_block_id, init_val), (vec_body_id, i_vec_next_v)],
            ty: Type::I64,
        };

        let vec_cond_v = alloc_val();
        let vec_cmp = Instruction::Binary {
            dest: vec_cond_v,
            op: MirBinaryOp::Lt,
            lhs: i_vec_v,
            rhs: vector_end_val,
            ty: Type::Bool,
        };

        let vec_header = BasicBlock {
            id: vec_header_id,
            name: "vector.header".to_string(),
            instructions: vec![vec_phi, vec_cmp],
            terminator: Some(Terminator::CondBranch {
                cond: vec_cond_v,
                then_block: vec_body_id,
                else_block: scalar_header_id,
            }),
        };

        // 3. Build vector_body
        let mut vec_body_insts = Vec::new();

        match pattern {
            VectorPattern::IntegerAdd256 {
                base_out,
                base_a,
                base_b,
            } => {
                let addr_out = alloc_val();
                let addr_a = alloc_val();
                let addr_b = alloc_val();

                vec_body_insts.push(Self::make_index_addr_inst(addr_a, base_a, i_vec_v));
                vec_body_insts.push(Self::make_index_addr_inst(addr_b, base_b, i_vec_v));
                vec_body_insts.push(Self::make_index_addr_inst(addr_out, base_out, i_vec_v));

                vec_body_insts.push(Instruction::Call {
                    dest: None,
                    callee: "venta_simd_vpaddq256".to_string(),
                    args: vec![addr_out, addr_a, addr_b],
                    ret_type: Type::Unit,
                });
            }
            VectorPattern::FloatFma256 {
                base_out,
                base_a,
                base_b,
                base_c,
            } => {
                let addr_out = alloc_val();
                let addr_a = alloc_val();
                let addr_b = alloc_val();
                let addr_c = alloc_val();

                vec_body_insts.push(Self::make_index_addr_inst(addr_a, base_a, i_vec_v));
                vec_body_insts.push(Self::make_index_addr_inst(addr_b, base_b, i_vec_v));
                vec_body_insts.push(Self::make_index_addr_inst(addr_c, base_c, i_vec_v));
                vec_body_insts.push(Self::make_index_addr_inst(addr_out, base_out, i_vec_v));

                vec_body_insts.push(Instruction::Call {
                    dest: None,
                    callee: "venta_simd_vfmadd231pd256".to_string(),
                    args: vec![addr_out, addr_a, addr_b, addr_c],
                    ret_type: Type::Unit,
                });
            }
        }

        // i_vec_next = i_vec + 4
        let const_4_step = alloc_val();
        vec_body_insts.push(Instruction::ConstInt {
            dest: const_4_step,
            val: 4,
            ty: Type::I64,
        });
        vec_body_insts.push(Instruction::Binary {
            dest: i_vec_next_v,
            op: MirBinaryOp::Add,
            lhs: i_vec_v,
            rhs: const_4_step,
            ty: Type::I64,
        });

        let vec_body = BasicBlock {
            id: vec_body_id,
            name: "vector.body".to_string(),
            instructions: vec_body_insts,
            terminator: Some(Terminator::Branch(vec_header_id)),
        };

        // 4. Redirect preheader to check_block
        let p_block = func
            .blocks
            .iter_mut()
            .find(|b| b.id == preheader_id)
            .unwrap();
        p_block.terminator = Some(Terminator::Branch(check_block_id));

        // 5. Update Phi in scalar_header to accept input from check_block, vec_header, and latch
        let s_block = func
            .blocks
            .iter_mut()
            .find(|b| b.id == scalar_header_id)
            .unwrap();
        for inst in &mut s_block.instructions {
            if let Instruction::Phi { dest, inputs, .. } = inst {
                if *dest == iv_dest {
                    *inputs = vec![
                        (check_block_id, init_val),
                        (vec_header_id, i_vec_v),
                        (latch_id, latch_val),
                    ];
                    break;
                }
            }
        }

        // 6. Insert new basic blocks immediately before scalar_header in func.blocks
        let s_idx = func
            .blocks
            .iter()
            .position(|b| b.id == scalar_header_id)
            .unwrap();
        func.blocks.insert(s_idx, check_block);
        func.blocks.insert(s_idx + 1, vec_header);
        func.blocks.insert(s_idx + 2, vec_body);

        true
    }

    /// Emits a runtime alias check to prove disjointness of non-identical memory regions.
    fn emit_alias_check<F>(
        insts: &mut Vec<Instruction>,
        pattern: &VectorPattern,
        count_val: ValueId,
        count_ge_4_val: ValueId,
        alloc_val: &mut F,
    ) -> ValueId
    where
        F: FnMut() -> ValueId,
    {
        let (base_out, bases_in) = match pattern {
            VectorPattern::IntegerAdd256 {
                base_out,
                base_a,
                base_b,
            } => (*base_out, vec![*base_a, *base_b]),
            VectorPattern::FloatFma256 {
                base_out,
                base_a,
                base_b,
                base_c,
            } => (*base_out, vec![*base_a, *base_b, *base_c]),
        };

        // If base_out is a stack slot, stack slots cannot alias other allocations
        if let BaseLocation::Slot(_) = base_out {
            return count_ge_4_val;
        }

        let out_ptr = match base_out {
            BaseLocation::Ptr(p) => p,
            _ => return count_ge_4_val,
        };

        // Compute bytes = count * 8
        let const_8 = alloc_val();
        insts.push(Instruction::ConstInt {
            dest: const_8,
            val: 8,
            ty: Type::I64,
        });

        let bytes_len = alloc_val();
        insts.push(Instruction::Binary {
            dest: bytes_len,
            op: MirBinaryOp::Mul,
            lhs: count_val,
            rhs: const_8,
            ty: Type::I64,
        });

        let out_end = alloc_val();
        insts.push(Instruction::Binary {
            dest: out_end,
            op: MirBinaryOp::Add,
            lhs: out_ptr,
            rhs: bytes_len,
            ty: Type::I64,
        });

        let mut curr_ok = count_ge_4_val;

        for base_in in bases_in {
            match base_in {
                BaseLocation::Slot(_) => {
                    // Stack slot doesn't alias pointer
                    continue;
                }
                BaseLocation::Ptr(in_ptr) => {
                    if in_ptr == out_ptr {
                        // Identical base pointers (in-place operation): completely safe!
                        continue;
                    }

                    // in_end = in_ptr + bytes
                    let in_end = alloc_val();
                    insts.push(Instruction::Binary {
                        dest: in_end,
                        op: MirBinaryOp::Add,
                        lhs: in_ptr,
                        rhs: bytes_len,
                        ty: Type::I64,
                    });

                    // out_end <= in_ptr
                    let cond1 = alloc_val();
                    insts.push(Instruction::Binary {
                        dest: cond1,
                        op: MirBinaryOp::LtEq,
                        lhs: out_end,
                        rhs: in_ptr,
                        ty: Type::Bool,
                    });

                    // in_end <= out_ptr
                    let cond2 = alloc_val();
                    insts.push(Instruction::Binary {
                        dest: cond2,
                        op: MirBinaryOp::LtEq,
                        lhs: in_end,
                        rhs: out_ptr,
                        ty: Type::Bool,
                    });

                    // no_alias = cond1 || cond2
                    let no_alias = alloc_val();
                    insts.push(Instruction::Binary {
                        dest: no_alias,
                        op: MirBinaryOp::Or,
                        lhs: cond1,
                        rhs: cond2,
                        ty: Type::Bool,
                    });

                    // curr_ok = curr_ok && no_alias
                    let new_ok = alloc_val();
                    insts.push(Instruction::Binary {
                        dest: new_ok,
                        op: MirBinaryOp::And,
                        lhs: curr_ok,
                        rhs: no_alias,
                        ty: Type::Bool,
                    });

                    curr_ok = new_ok;
                }
            }
        }

        curr_ok
    }

    /// Helper to emit IndexPtrAddr or IndexAddr instruction for indexing.
    fn make_index_addr_inst(dest: ValueId, base: BaseLocation, index: ValueId) -> Instruction {
        match base {
            BaseLocation::Ptr(base_ptr) => Instruction::IndexPtrAddr {
                dest,
                base_ptr,
                index,
                elem_size: 8,
            },
            BaseLocation::Slot(slot) => Instruction::IndexAddr {
                dest,
                slot,
                index,
                elem_size: 8,
            },
        }
    }
}
