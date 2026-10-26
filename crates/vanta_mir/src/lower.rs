//! Lowering from typed AST to Vanta MIR.

use crate::ir::*;
use std::collections::HashMap;
use vanta_parser::ast::*;
use vanta_sema::{SemanticContext, Type};

pub struct MirLowerer<'a> {
    sema: &'a SemanticContext,
    next_val: u32,
    next_slot: u32,
    next_block: u32,
    current_slots: Vec<LocalSlot>,
    current_blocks: Vec<BasicBlock>,
    current_block_idx: usize,
    var_map: Vec<HashMap<String, SlotId>>,
    loop_exit_blocks: Vec<BlockId>,
    loop_cond_blocks: Vec<BlockId>,
    string_literals: Vec<StringLiteral>,
}

impl<'a> MirLowerer<'a> {
    pub fn new(sema: &'a SemanticContext) -> Self {
        Self {
            sema,
            next_val: 1,
            next_slot: 1,
            next_block: 1,
            current_slots: Vec::new(),
            current_blocks: Vec::new(),
            current_block_idx: 0,
            var_map: vec![HashMap::new()],
            loop_exit_blocks: Vec::new(),
            loop_cond_blocks: Vec::new(),
            string_literals: Vec::new(),
        }
    }

    fn intern_string(&mut self, s: &str) -> String {
        let bytes = s.as_bytes().to_vec();
        for lit in &self.string_literals {
            if lit.bytes == bytes {
                return lit.symbol.clone();
            }
        }
        let symbol = format!(".Lstr_{}", self.string_literals.len());
        self.string_literals.push(StringLiteral {
            symbol: symbol.clone(),
            bytes,
        });
        symbol
    }

    fn new_value(&mut self) -> ValueId {
        let v = ValueId(self.next_val);
        self.next_val += 1;
        v
    }

    fn new_slot(&mut self, name: String, ty: Type) -> SlotId {
        let s = SlotId(self.next_slot);
        self.next_slot += 1;
        self.current_slots.push(LocalSlot {
            id: s,
            name: name.clone(),
            ty,
        });
        self.var_map.last_mut().unwrap().insert(name, s);
        s
    }

    fn lookup_slot(&self, name: &str) -> Option<SlotId> {
        for scope in self.var_map.iter().rev() {
            if let Some(&s) = scope.get(name) {
                return Some(s);
            }
        }
        None
    }

    fn new_block(&mut self, name: impl Into<String>) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        self.current_blocks.push(BasicBlock {
            id,
            name: name.into(),
            instructions: Vec::new(),
            terminator: None,
        });
        id
    }

    fn switch_to_block(&mut self, id: BlockId) {
        if let Some(pos) = self.current_blocks.iter().position(|b| b.id == id) {
            self.current_block_idx = pos;
        }
    }

    fn emit(&mut self, inst: Instruction) {
        self.current_blocks[self.current_block_idx]
            .instructions
            .push(inst);
    }

    fn terminate(&mut self, term: Terminator) {
        if self.current_blocks[self.current_block_idx]
            .terminator
            .is_none()
        {
            self.current_blocks[self.current_block_idx].terminator = Some(term);
        }
    }

    fn is_current_terminated(&self) -> bool {
        self.current_blocks[self.current_block_idx]
            .terminator
            .is_some()
    }

    pub fn lower_program(&mut self, program: &Program) -> MirProgram {
        let mut mir_prog = MirProgram::default();
        for item in &program.items {
            if let Item::Function(f) = item {
                mir_prog.functions.push(self.lower_function(f));
            }
        }
        mir_prog.string_literals = std::mem::take(&mut self.string_literals);
        mir_prog
    }

    fn lower_function(&mut self, f: &FnDecl) -> MirFunction {
        self.next_val = 1;
        self.next_slot = 1;
        self.next_block = 1;
        self.current_slots.clear();
        self.current_blocks.clear();
        self.var_map = vec![HashMap::new()];
        self.loop_exit_blocks.clear();
        self.loop_cond_blocks.clear();

        let sig = self.sema.functions.get(&f.name.name).unwrap();
        let mut params = Vec::new();

        let entry_block = self.new_block("entry");
        self.switch_to_block(entry_block);

        for (p_name, p_ty, _) in &sig.params {
            let slot = self.new_slot(p_name.clone(), p_ty.clone());
            params.push((slot, p_ty.clone()));
        }

        let tail_val = self.lower_block(&f.body);

        // Ensure block terminates
        if !self.is_current_terminated() {
            if sig.ret_type == Type::Unit {
                self.terminate(Terminator::Return(None));
            } else if let Some(val) = tail_val {
                self.terminate(Terminator::Return(Some(val)));
            } else {
                self.terminate(Terminator::Unreachable);
            }
        }

        MirFunction {
            name: f.name.name.clone(),
            params,
            ret_type: sig.ret_type.clone(),
            slots: std::mem::take(&mut self.current_slots),
            blocks: std::mem::take(&mut self.current_blocks),
        }
    }

    fn emit_bounds_check(&mut self, idx_v: ValueId, len_v: ValueId) -> BlockId {
        let zero_v = self.new_value();
        self.emit(Instruction::ConstInt {
            dest: zero_v,
            val: 0,
            ty: Type::I64,
        });

        let ge_zero = self.new_value();
        self.emit(Instruction::Binary {
            dest: ge_zero,
            op: MirBinaryOp::GtEq,
            lhs: idx_v,
            rhs: zero_v,
            ty: Type::Bool,
        });

        let lt_len = self.new_value();
        self.emit(Instruction::Binary {
            dest: lt_len,
            op: MirBinaryOp::Lt,
            lhs: idx_v,
            rhs: len_v,
            ty: Type::Bool,
        });

        let in_bounds = self.new_value();
        self.emit(Instruction::Binary {
            dest: in_bounds,
            op: MirBinaryOp::And,
            lhs: ge_zero,
            rhs: lt_len,
            ty: Type::Bool,
        });

        let ok_block = self.new_block("bounds_ok");
        let fail_block = self.new_block("bounds_fail");

        self.terminate(Terminator::CondBranch {
            cond: in_bounds,
            then_block: ok_block,
            else_block: fail_block,
        });

        // Fail block calls runtime bounds check failure and exits 101
        self.switch_to_block(fail_block);
        self.emit(Instruction::Call {
            dest: None,
            callee: "vanta_bounds_check_fail".to_string(),
            args: vec![idx_v, len_v],
            ret_type: Type::Unit,
        });
        self.terminate(Terminator::Unreachable);

        // Resume execution in ok_block
        self.switch_to_block(ok_block);
        ok_block
    }

    fn lower_block(&mut self, block: &Block) -> Option<ValueId> {
        self.var_map.push(HashMap::new());

        for stmt in &block.stmts {
            if self.is_current_terminated() {
                break;
            }
            self.lower_stmt(stmt);
        }

        let result = if !self.is_current_terminated() {
            block.expr.as_ref().map(|tail| self.lower_expr(tail))
        } else {
            None
        };

        self.var_map.pop();
        result
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                ty,
                init,
                span: _,
                ..
            } => {
                let ty = if let Some(t) = ty {
                    let mut diags = vanta_diagnostics::DiagnosticBag::new();
                    self.sema.type_from_ast(t, &mut diags)
                } else if let Some(sym) = self.sema.symbols.values().find(|s| s.span == name.span) {
                    sym.ty.clone()
                } else if let Some(init_expr) = init {
                    if let ExprKind::String(_) = &init_expr.kind {
                        Type::Ref {
                            is_mut: false,
                            inner: Box::new(Type::Str),
                        }
                    } else {
                        Type::I64
                    }
                } else {
                    Type::I64
                };

                let slot = self.new_slot(name.name.clone(), ty.clone());
                if let Some(init_expr) = init {
                    if ty.is_str_slice() {
                        let (ptr_v, len_v) = self.lower_slice_expr(init_expr);
                        self.emit(Instruction::StoreSlice {
                            slot,
                            ptr: ptr_v,
                            len: len_v,
                        });
                    } else if let Type::Array { inner, .. } = &ty {
                        if let ExprKind::ArrayLiteral(elements) = &init_expr.kind {
                            let elem_size = inner.size_in_bytes().max(8);
                            for (idx, elem) in elements.iter().enumerate() {
                                let elem_v = self.lower_expr(elem);
                                let idx_v = self.new_value();
                                self.emit(Instruction::ConstInt {
                                    dest: idx_v,
                                    val: idx as i64,
                                    ty: Type::I64,
                                });
                                let addr_v = self.new_value();
                                self.emit(Instruction::IndexAddr {
                                    dest: addr_v,
                                    slot,
                                    index: idx_v,
                                    elem_size,
                                });
                                self.emit(Instruction::StoreIndirect {
                                    addr: addr_v,
                                    val: elem_v,
                                });
                            }
                        } else {
                            let v = self.lower_expr(init_expr);
                            self.emit(Instruction::Store { slot, val: v });
                        }
                    } else {
                        let v = self.lower_expr(init_expr);
                        self.emit(Instruction::Store { slot, val: v });
                    }
                }
            }
            Stmt::Assign {
                target,
                op,
                value,
                span: _,
            } => {
                if let ExprKind::Ident(name) = &target.kind {
                    if let Some(slot) = self.lookup_slot(name) {
                        let slot_ty = self
                            .current_slots
                            .iter()
                            .find(|s| s.id == slot)
                            .map(|s| s.ty.clone());
                        if let Some(ty) = slot_ty {
                            if ty.is_str_slice() && *op == AssignOp::Eq {
                                let (ptr_v, len_v) = self.lower_slice_expr(value);
                                self.emit(Instruction::StoreSlice {
                                    slot,
                                    ptr: ptr_v,
                                    len: len_v,
                                });
                                return;
                            }
                        }
                    }
                }

                let val_v = self.lower_expr(value);
                let final_val = match op {
                    AssignOp::Eq => val_v,
                    _ => {
                        let current_val = self.lower_expr(target);
                        let bin_op = match op {
                            AssignOp::PlusEq => MirBinaryOp::Add,
                            AssignOp::MinusEq => MirBinaryOp::Sub,
                            AssignOp::StarEq => MirBinaryOp::Mul,
                            AssignOp::SlashEq => MirBinaryOp::Div,
                            AssignOp::PercentEq => MirBinaryOp::Rem,
                            _ => unreachable!(),
                        };
                        let dest = self.new_value();
                        self.emit(Instruction::Binary {
                            dest,
                            op: bin_op,
                            lhs: current_val,
                            rhs: val_v,
                            ty: Type::I64,
                        });
                        dest
                    }
                };

                match &target.kind {
                    ExprKind::Ident(name) => {
                        if let Some(slot) = self.lookup_slot(name) {
                            self.emit(Instruction::Store {
                                slot,
                                val: final_val,
                            });
                        }
                    }
                    ExprKind::Deref(inner) => {
                        let addr = self.lower_expr(inner);
                        self.emit(Instruction::StoreIndirect {
                            addr,
                            val: final_val,
                        });
                    }
                    ExprKind::Index { array, index } => {
                        let idx_v = self.lower_expr(index);

                        let arr_ty = self
                            .sema
                            .expr_types
                            .get(&(array.as_ref() as *const Expr))
                            .cloned()
                            .unwrap_or_else(|| {
                                if let ExprKind::Ident(name) = &array.kind {
                                    if let Some(slot) = self.lookup_slot(name) {
                                        return self
                                            .current_slots
                                            .iter()
                                            .find(|s| s.id == slot)
                                            .map(|s| s.ty.clone())
                                            .unwrap_or(Type::Array {
                                                inner: Box::new(Type::I64),
                                                len: 0,
                                            });
                                    }
                                }
                                Type::Array {
                                    inner: Box::new(Type::I64),
                                    len: 0,
                                }
                            });

                        let (base_ptr, len_v, elem_size) = match &arr_ty {
                            Type::Array { inner, len } => {
                                let len_v = self.new_value();
                                self.emit(Instruction::ConstInt {
                                    dest: len_v,
                                    val: *len as i64,
                                    ty: Type::I64,
                                });
                                let base_ptr = self.lower_expr(array);
                                (base_ptr, len_v, inner.size_in_bytes().max(8))
                            }
                            Type::Ref { inner, .. } => match inner.as_ref() {
                                Type::Array { inner: elem_t, len } => {
                                    let len_v = self.new_value();
                                    self.emit(Instruction::ConstInt {
                                        dest: len_v,
                                        val: *len as i64,
                                        ty: Type::I64,
                                    });
                                    let base_ptr = self.lower_expr(array);
                                    (base_ptr, len_v, elem_t.size_in_bytes().max(8))
                                }
                                _ => {
                                    let dummy = self.new_value();
                                    self.emit(Instruction::ConstInt {
                                        dest: dummy,
                                        val: 0,
                                        ty: Type::I64,
                                    });
                                    (dummy, dummy, 8)
                                }
                            },
                            _ => {
                                let dummy = self.new_value();
                                self.emit(Instruction::ConstInt {
                                    dest: dummy,
                                    val: 0,
                                    ty: Type::I64,
                                });
                                (dummy, dummy, 8)
                            }
                        };

                        // Bounds check must strictly dominate the indexed store
                        self.emit_bounds_check(idx_v, len_v);

                        let addr_v = self.new_value();
                        self.emit(Instruction::IndexPtrAddr {
                            dest: addr_v,
                            base_ptr,
                            index: idx_v,
                            elem_size,
                        });
                        self.emit(Instruction::StoreIndirect {
                            addr: addr_v,
                            val: final_val,
                        });
                    }
                    _ => {}
                }
            }
            Stmt::While {
                cond,
                body,
                span: _,
            } => {
                let cond_block = self.new_block("while.cond");
                let body_block = self.new_block("while.body");
                let exit_block = self.new_block("while.exit");

                self.terminate(Terminator::Branch(cond_block));

                // Cond block
                self.switch_to_block(cond_block);
                let cond_v = self.lower_expr(cond);
                self.terminate(Terminator::CondBranch {
                    cond: cond_v,
                    then_block: body_block,
                    else_block: exit_block,
                });

                // Body block
                self.loop_cond_blocks.push(cond_block);
                self.loop_exit_blocks.push(exit_block);
                self.switch_to_block(body_block);
                self.lower_block(body);
                if !self.is_current_terminated() {
                    self.terminate(Terminator::Branch(cond_block));
                }
                self.loop_cond_blocks.pop();
                self.loop_exit_blocks.pop();

                // Exit block
                self.switch_to_block(exit_block);
            }
            Stmt::Loop { body, span: _ } => {
                let body_block = self.new_block("loop.body");
                let exit_block = self.new_block("loop.exit");

                self.terminate(Terminator::Branch(body_block));

                self.loop_cond_blocks.push(body_block);
                self.loop_exit_blocks.push(exit_block);
                self.switch_to_block(body_block);
                self.lower_block(body);
                if !self.is_current_terminated() {
                    self.terminate(Terminator::Branch(body_block));
                }
                self.loop_cond_blocks.pop();
                self.loop_exit_blocks.pop();

                self.switch_to_block(exit_block);
            }
            Stmt::Return { value, span: _ } => {
                let val_v = value.as_ref().map(|e| self.lower_expr(e));
                self.terminate(Terminator::Return(val_v));
            }
            Stmt::Break { .. } => {
                if let Some(&exit) = self.loop_exit_blocks.last() {
                    self.terminate(Terminator::Branch(exit));
                }
            }
            Stmt::Continue { .. } => {
                if let Some(&cond) = self.loop_cond_blocks.last() {
                    self.terminate(Terminator::Branch(cond));
                }
            }
            Stmt::Expr(expr) => {
                self.lower_expr(expr);
            }
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> ValueId {
        match &expr.kind {
            ExprKind::Int(n) => {
                let dest = self.new_value();
                self.emit(Instruction::ConstInt {
                    dest,
                    val: *n,
                    ty: Type::I64,
                });
                dest
            }
            ExprKind::Bool(b) => {
                let dest = self.new_value();
                self.emit(Instruction::ConstBool { dest, val: *b });
                dest
            }
            ExprKind::Ident(name) => {
                if let Some(slot) = self.lookup_slot(name) {
                    let slot_ty = self
                        .current_slots
                        .iter()
                        .find(|s| s.id == slot)
                        .map(|s| s.ty.clone());
                    let dest = self.new_value();
                    if let Some(Type::Array { .. }) = slot_ty {
                        self.emit(Instruction::SlotAddr { dest, slot });
                    } else {
                        self.emit(Instruction::Load {
                            dest,
                            slot,
                            ty: slot_ty.unwrap_or(Type::I64),
                        });
                    }
                    dest
                } else {
                    let dest = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest,
                        val: 0,
                        ty: Type::I64,
                    });
                    dest
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs_v = self.lower_expr(lhs);
                let rhs_v = self.lower_expr(rhs);
                let mir_op = match op {
                    BinaryOp::Add => MirBinaryOp::Add,
                    BinaryOp::Sub => MirBinaryOp::Sub,
                    BinaryOp::Mul => MirBinaryOp::Mul,
                    BinaryOp::Div => MirBinaryOp::Div,
                    BinaryOp::Rem => MirBinaryOp::Rem,
                    BinaryOp::Eq => MirBinaryOp::Eq,
                    BinaryOp::NotEq => MirBinaryOp::NotEq,
                    BinaryOp::Lt => MirBinaryOp::Lt,
                    BinaryOp::LtEq => MirBinaryOp::LtEq,
                    BinaryOp::Gt => MirBinaryOp::Gt,
                    BinaryOp::GtEq => MirBinaryOp::GtEq,
                    BinaryOp::And => MirBinaryOp::And,
                    BinaryOp::Or => MirBinaryOp::Or,
                    BinaryOp::BitAnd => MirBinaryOp::BitAnd,
                    BinaryOp::BitOr => MirBinaryOp::BitOr,
                    BinaryOp::BitXor => MirBinaryOp::BitXor,
                    BinaryOp::Shl => MirBinaryOp::Shl,
                    BinaryOp::Shr => MirBinaryOp::Shr,
                };
                let dest = self.new_value();
                self.emit(Instruction::Binary {
                    dest,
                    op: mir_op,
                    lhs: lhs_v,
                    rhs: rhs_v,
                    ty: Type::I64,
                });
                dest
            }
            ExprKind::Unary { op, expr: inner } => {
                let inner_v = self.lower_expr(inner);
                let mir_op = match op {
                    UnaryOp::Neg => MirUnaryOp::Neg,
                    UnaryOp::Not => MirUnaryOp::Not,
                };
                let dest = self.new_value();
                self.emit(Instruction::Unary {
                    dest,
                    op: mir_op,
                    operand: inner_v,
                    ty: Type::I64,
                });
                dest
            }
            ExprKind::Ref { expr: inner, .. } => {
                if let ExprKind::Ident(name) = &inner.kind {
                    if let Some(slot) = self.lookup_slot(name) {
                        let dest = self.new_value();
                        self.emit(Instruction::SlotAddr { dest, slot });
                        return dest;
                    }
                }
                let dest = self.new_value();
                self.emit(Instruction::ConstInt {
                    dest,
                    val: 0,
                    ty: Type::I64,
                });
                dest
            }
            ExprKind::Deref(inner) => {
                let addr_v = self.lower_expr(inner);
                let dest = self.new_value();
                self.emit(Instruction::LoadIndirect {
                    dest,
                    addr: addr_v,
                    ty: Type::I64,
                });
                dest
            }
            ExprKind::Call { callee, args } => {
                if callee == "panic" {
                    let (ptr_v, len_v) = if let Some(arg) = args.first() {
                        self.lower_slice_expr(arg)
                    } else {
                        let zero = self.new_value();
                        self.emit(Instruction::ConstInt {
                            dest: zero,
                            val: 0,
                            ty: Type::I64,
                        });
                        (zero, zero)
                    };
                    self.emit(Instruction::Call {
                        dest: None,
                        callee: "vanta_panic".to_string(),
                        args: vec![ptr_v, len_v],
                        ret_type: Type::Unit,
                    });
                    self.terminate(Terminator::Unreachable);
                    let dummy = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest: dummy,
                        val: 0,
                        ty: Type::I64,
                    });
                    return dummy;
                }

                let mut arg_vals = Vec::new();
                for arg in args {
                    let mut is_slice = false;
                    if let ExprKind::String(_) = &arg.kind {
                        is_slice = true;
                    } else if let ExprKind::Ident(name) = &arg.kind {
                        if let Some(slot) = self.lookup_slot(name) {
                            if let Some(s) = self.current_slots.iter().find(|s| s.id == slot) {
                                if s.ty.is_str_slice() {
                                    is_slice = true;
                                }
                            }
                        }
                    }

                    if is_slice {
                        let (ptr_v, len_v) = self.lower_slice_expr(arg);
                        arg_vals.push(ptr_v);
                        arg_vals.push(len_v);
                    } else {
                        arg_vals.push(self.lower_expr(arg));
                    }
                }

                let ret_type = if callee == "print_i64" || callee == "print_str" {
                    Type::Unit
                } else if let Some(sig) = self.sema.functions.get(callee) {
                    sig.ret_type.clone()
                } else {
                    Type::I64
                };

                let dest = if ret_type == Type::Unit {
                    None
                } else {
                    Some(self.new_value())
                };

                self.emit(Instruction::Call {
                    dest,
                    callee: callee.clone(),
                    args: arg_vals,
                    ret_type,
                });

                dest.unwrap_or_else(|| {
                    let dummy = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest: dummy,
                        val: 0,
                        ty: Type::I64,
                    });
                    dummy
                })
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_v = self.lower_expr(cond);
                let then_block = self.new_block("if.then");
                let else_block = self.new_block("if.else");
                let merge_block = self.new_block("if.merge");

                self.terminate(Terminator::CondBranch {
                    cond: cond_v,
                    then_block,
                    else_block,
                });

                // Then
                self.switch_to_block(then_block);
                let then_v = self.lower_block(then_branch);
                if !self.is_current_terminated() {
                    self.terminate(Terminator::Branch(merge_block));
                }

                // Else
                self.switch_to_block(else_block);
                let else_v = match else_branch {
                    Some(ElseBranch::If(else_if)) => Some(self.lower_expr(else_if)),
                    Some(ElseBranch::Block(else_b)) => self.lower_block(else_b),
                    None => None,
                };
                if !self.is_current_terminated() {
                    self.terminate(Terminator::Branch(merge_block));
                }

                self.switch_to_block(merge_block);

                if let (Some(tv), Some(_ev)) = (then_v, else_v) {
                    tv
                } else {
                    let dest = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest,
                        val: 0,
                        ty: Type::I64,
                    });
                    dest
                }
            }
            ExprKind::Block(block) => self.lower_block(block).unwrap_or_else(|| {
                let dest = self.new_value();
                self.emit(Instruction::ConstInt {
                    dest,
                    val: 0,
                    ty: Type::I64,
                });
                dest
            }),
            ExprKind::ArrayLiteral(elements) => {
                let slot = self.new_slot(
                    format!("arr_{}", self.next_slot),
                    Type::Array {
                        inner: Box::new(Type::I64),
                        len: elements.len(),
                    },
                );
                for (idx, elem) in elements.iter().enumerate() {
                    let elem_v = self.lower_expr(elem);
                    let idx_v = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest: idx_v,
                        val: idx as i64,
                        ty: Type::I64,
                    });
                    let addr_v = self.new_value();
                    self.emit(Instruction::IndexAddr {
                        dest: addr_v,
                        slot,
                        index: idx_v,
                        elem_size: 8,
                    });
                    self.emit(Instruction::StoreIndirect {
                        addr: addr_v,
                        val: elem_v,
                    });
                }
                let dest = self.new_value();
                self.emit(Instruction::SlotAddr { dest, slot });
                dest
            }
            ExprKind::Index { array, index } => {
                let idx_v = self.lower_expr(index);

                let arr_ty = self
                    .sema
                    .expr_types
                    .get(&(array.as_ref() as *const Expr))
                    .cloned()
                    .unwrap_or_else(|| {
                        if let ExprKind::Ident(name) = &array.kind {
                            if let Some(slot) = self.lookup_slot(name) {
                                return self
                                    .current_slots
                                    .iter()
                                    .find(|s| s.id == slot)
                                    .map(|s| s.ty.clone())
                                    .unwrap_or(Type::Array {
                                        inner: Box::new(Type::I64),
                                        len: 0,
                                    });
                            }
                        }
                        Type::Array {
                            inner: Box::new(Type::I64),
                            len: 0,
                        }
                    });

                let (base_ptr, len_v, elem_ty, elem_size) = match &arr_ty {
                    Type::Array { inner, len } => {
                        let len_v = self.new_value();
                        self.emit(Instruction::ConstInt {
                            dest: len_v,
                            val: *len as i64,
                            ty: Type::I64,
                        });
                        let base_ptr = self.lower_expr(array);
                        (
                            base_ptr,
                            len_v,
                            inner.as_ref().clone(),
                            inner.size_in_bytes().max(8),
                        )
                    }
                    Type::Ref { inner, .. } => match inner.as_ref() {
                        Type::Array { inner: elem_t, len } => {
                            let len_v = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: len_v,
                                val: *len as i64,
                                ty: Type::I64,
                            });
                            let base_ptr = self.lower_expr(array);
                            (
                                base_ptr,
                                len_v,
                                elem_t.as_ref().clone(),
                                elem_t.size_in_bytes().max(8),
                            )
                        }
                        Type::Str => {
                            let (ptr_v, len_v) = self.lower_slice_expr(array);
                            (ptr_v, len_v, Type::U8, 1)
                        }
                        _ => {
                            let dummy = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: dummy,
                                val: 0,
                                ty: Type::I64,
                            });
                            (dummy, dummy, Type::I64, 8)
                        }
                    },
                    Type::Str => {
                        let (ptr_v, len_v) = self.lower_slice_expr(array);
                        (ptr_v, len_v, Type::U8, 1)
                    }
                    _ => {
                        let dummy = self.new_value();
                        self.emit(Instruction::ConstInt {
                            dest: dummy,
                            val: 0,
                            ty: Type::I64,
                        });
                        (dummy, dummy, Type::I64, 8)
                    }
                };

                // Bounds check strictly dominates indexed load
                self.emit_bounds_check(idx_v, len_v);

                let addr_v = self.new_value();
                self.emit(Instruction::IndexPtrAddr {
                    dest: addr_v,
                    base_ptr,
                    index: idx_v,
                    elem_size,
                });
                let dest = self.new_value();
                self.emit(Instruction::LoadIndirect {
                    dest,
                    addr: addr_v,
                    ty: elem_ty,
                });
                dest
            }
            ExprKind::String(_) => {
                let (ptr_v, _len_v) = self.lower_slice_expr(expr);
                ptr_v
            }
            _ => {
                let dest = self.new_value();
                self.emit(Instruction::ConstInt {
                    dest,
                    val: 0,
                    ty: Type::I64,
                });
                dest
            }
        }
    }

    fn lower_slice_expr(&mut self, expr: &Expr) -> (ValueId, ValueId) {
        match &expr.kind {
            ExprKind::String(s) => {
                let symbol = self.intern_string(s);
                let dest_ptr = self.new_value();
                let dest_len = self.new_value();
                self.emit(Instruction::StrSlice {
                    dest_ptr,
                    dest_len,
                    symbol,
                    len: s.len(),
                });
                (dest_ptr, dest_len)
            }
            ExprKind::Ident(name) => {
                let slot = self
                    .lookup_slot(name)
                    .unwrap_or_else(|| panic!("Undefined slot for slice identifier {}", name));
                let dest_ptr = self.new_value();
                let dest_len = self.new_value();
                self.emit(Instruction::LoadSlice {
                    dest_ptr,
                    dest_len,
                    slot,
                });
                (dest_ptr, dest_len)
            }
            _ => {
                let dest_ptr = self.new_value();
                let dest_len = self.new_value();
                self.emit(Instruction::ConstInt {
                    dest: dest_ptr,
                    val: 0,
                    ty: Type::I64,
                });
                self.emit(Instruction::ConstInt {
                    dest: dest_len,
                    val: 0,
                    ty: Type::I64,
                });
                (dest_ptr, dest_len)
            }
        }
    }
}
