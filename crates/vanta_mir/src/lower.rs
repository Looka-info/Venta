//! Lowering from typed AST to Vanta MIR.

use std::collections::HashMap;
use crate::ir::*;
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
        }
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

        self.lower_block(&f.body);

        // Ensure block terminates
        if !self.is_current_terminated() {
            if sig.ret_type == Type::Unit {
                self.terminate(Terminator::Return(None));
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

    fn lower_block(&mut self, block: &Block) -> Option<ValueId> {
        self.var_map.push(HashMap::new());

        for stmt in &block.stmts {
            if self.is_current_terminated() {
                break;
            }
            self.lower_stmt(stmt);
        }

        let result = if !self.is_current_terminated() {
            if let Some(tail) = &block.expr {
                Some(self.lower_expr(tail))
            } else {
                None
            }
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
                ty: _,
                init,
                span: _,
                ..
            } => {
                let init_val = init.as_ref().map(|e| self.lower_expr(e));
                let ty = if let Some(_v) = init_val {
                    Type::I64
                } else {
                    Type::I64
                };
                let slot = self.new_slot(name.name.clone(), ty);
                if let Some(v) = init_val {
                    self.emit(Instruction::Store { slot, val: v });
                }
            }
            Stmt::Assign {
                target,
                op,
                value,
                span: _,
            } => {
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
                        if let ExprKind::Ident(name) = &array.kind {
                            if let Some(slot) = self.lookup_slot(name) {
                                let idx_v = self.lower_expr(index);
                                let addr_v = self.new_value();
                                self.emit(Instruction::IndexAddr {
                                    dest: addr_v,
                                    slot,
                                    index: idx_v,
                                    elem_size: 8,
                                });
                                self.emit(Instruction::StoreIndirect {
                                    addr: addr_v,
                                    val: final_val,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Stmt::While { cond, body, span: _ } => {
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
                    let dest = self.new_value();
                    self.emit(Instruction::Load {
                        dest,
                        slot,
                        ty: Type::I64,
                    });
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
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.lower_expr(arg));
                }
                let dest = self.new_value();
                self.emit(Instruction::Call {
                    dest: Some(dest),
                    callee: callee.clone(),
                    args: arg_vals,
                    ret_type: Type::I64,
                });
                dest
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
                if let ExprKind::Ident(name) = &array.kind {
                    if let Some(slot) = self.lookup_slot(name) {
                        let idx_v = self.lower_expr(index);
                        let addr_v = self.new_value();
                        self.emit(Instruction::IndexAddr {
                            dest: addr_v,
                            slot,
                            index: idx_v,
                            elem_size: 8,
                        });
                        let dest = self.new_value();
                        self.emit(Instruction::LoadIndirect {
                            dest,
                            addr: addr_v,
                            ty: Type::I64,
                        });
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
}
