//! Lowering from typed AST to Vanta MIR.

use crate::ir::*;
use std::collections::HashMap;
use vanta_parser::ast::*;
use vanta_sema::{ModuleId, ModuleNode, ModuleTree, SemanticContext, Type};
use vanta_source::Span;

#[derive(Clone, Debug)]
pub struct DropCandidate {
    pub slot: SlotId,
    pub ty: Type,
    pub name: String,
    pub is_moved: bool,
}

pub struct MirLowerer<'a> {
    sema: &'a SemanticContext,
    next_val: u32,
    next_slot: u32,
    next_block: u32,
    current_slots: Vec<LocalSlot>,
    current_blocks: Vec<BasicBlock>,
    current_block_idx: usize,
    var_map: Vec<HashMap<String, SlotId>>,
    drop_scopes: Vec<Vec<DropCandidate>>,
    loop_exit_blocks: Vec<BlockId>,
    loop_cond_blocks: Vec<BlockId>,
    loop_scope_depths: Vec<usize>,
    string_literals: Vec<StringLiteral>,
    current_fn_ret_type: Type,
    current_fn_span: Span,
    slot_spans: HashMap<SlotId, Span>,
    val_spans: HashMap<ValueId, Span>,
    terminator_spans: HashMap<BlockId, Span>,
    current_span: Span,
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
            drop_scopes: vec![Vec::new()],
            loop_exit_blocks: Vec::new(),
            loop_cond_blocks: Vec::new(),
            loop_scope_depths: Vec::new(),
            string_literals: Vec::new(),
            current_fn_ret_type: Type::Unit,
            current_fn_span: Span::dummy(),
            slot_spans: HashMap::new(),
            val_spans: HashMap::new(),
            terminator_spans: HashMap::new(),
            current_span: Span::dummy(),
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
        if !self.current_span.is_dummy() {
            self.val_spans.insert(v, self.current_span);
        }
        v
    }

    fn new_slot(&mut self, name: String, ty: Type) -> SlotId {
        let s = SlotId(self.next_slot);
        self.next_slot += 1;
        let size = ty.size_with_context(&self.sema.structs, &self.sema.enums);
        let align = ty.alignment_with_context(&self.sema.structs, &self.sema.enums);
        self.current_slots.push(LocalSlot {
            id: s,
            name: name.clone(),
            ty,
            size,
            align,
        });
        self.var_map.last_mut().unwrap().insert(name, s);
        if !self.current_span.is_dummy() {
            self.slot_spans.insert(s, self.current_span);
        }
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
            let b_id = self.current_blocks[self.current_block_idx].id;
            if !self.current_span.is_dummy() {
                self.terminator_spans.insert(b_id, self.current_span);
            }
            self.current_blocks[self.current_block_idx].terminator = Some(term);
        }
    }

    fn is_current_terminated(&self) -> bool {
        self.current_blocks[self.current_block_idx]
            .terminator
            .is_some()
    }

    pub fn lower_program(&mut self, program: &Program) -> MirProgram {
        let root_id = ModuleId(0);
        let modules = vec![ModuleNode {
            id: root_id,
            name: "main".into(),
            path: Vec::new(),
            file_path: std::path::PathBuf::from("main.va"),
            file_id: vanta_source::FileId(1),
            parent: None,
            children: HashMap::new(),
            is_pub: true,
            items: program.items.clone(),
        }];
        let mut path_to_module = HashMap::new();
        path_to_module.insert(Vec::new(), root_id);

        let tree = ModuleTree {
            root: root_id,
            modules,
            path_to_module,
        };

        self.lower_module_tree(&tree)
    }

    pub fn lower_module_tree(&mut self, tree: &ModuleTree) -> MirProgram {
        let mut mir_prog = MirProgram {
            structs: self.sema.structs.clone(),
            enums: self.sema.enums.clone(),
            ..Default::default()
        };

        for mod_node in &tree.modules {
            let is_root = mod_node.id == tree.root;
            let scope = self.sema.module_scopes.get(&mod_node.id).cloned();

            for item in &mod_node.items {
                match item {
                    Item::Function(f) => {
                        if f.type_params.is_empty() {
                            let mangled_name = if let Some(ref s) = scope {
                                s.functions.get(&f.name.name).cloned().unwrap_or_else(|| {
                                    if is_root && f.name.name == "main" {
                                        "main".to_string()
                                    } else {
                                        f.name.name.clone()
                                    }
                                })
                            } else if is_root && f.name.name == "main" {
                                "main".to_string()
                            } else {
                                f.name.name.clone()
                            };
                            mir_prog
                                .functions
                                .push(self.lower_function_with_name(f, &mangled_name));
                        }
                    }
                    Item::Impl(impl_block) => {
                        if !impl_block.type_params.is_empty() {
                            continue;
                        }
                        let target_canonical = if let Some(ref s) = scope {
                            s.structs
                                .get(&impl_block.target.name)
                                .cloned()
                                .or_else(|| s.enums.get(&impl_block.target.name).cloned())
                                .unwrap_or_else(|| impl_block.target.name.clone())
                        } else {
                            impl_block.target.name.clone()
                        };

                        if let Some(ref tr) = impl_block.trait_name {
                            let canon_trait = if let Some(ref s) = scope {
                                s.traits
                                    .get(&tr.name)
                                    .cloned()
                                    .unwrap_or_else(|| tr.name.clone())
                            } else {
                                tr.name.clone()
                            };
                            for method in &impl_block.methods {
                                let mangled_name = format!(
                                    "{}__{}_{}",
                                    target_canonical,
                                    canon_trait.replace("::", "_"),
                                    method.name.name
                                );
                                mir_prog
                                    .functions
                                    .push(self.lower_function_with_name(method, &mangled_name));
                            }
                        } else {
                            for method in &impl_block.methods {
                                let mangled_name =
                                    format!("{}_{}", target_canonical, method.name.name);
                                mir_prog
                                    .functions
                                    .push(self.lower_function_with_name(method, &mangled_name));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        for specialized_f in &self.sema.monomorphized_functions {
            mir_prog.functions.push(self.lower_function(specialized_f));
        }

        let iter_structs: Vec<String> = self
            .sema
            .structs
            .keys()
            .filter(|k| k.starts_with("Iter__"))
            .cloned()
            .collect();

        for iter_name in iter_structs {
            let mangled_fn = format!("{}_std_iter_Iterator_next", iter_name);
            if let Some(sig) = self.sema.functions.get(&mangled_fn).cloned() {
                let opt_enum_name = match &sig.ret_type {
                    Type::Enum(ename) => ename.clone(),
                    _ => continue,
                };
                let elem_ty = if let Some(impl_def) = self
                    .sema
                    .trait_impls
                    .get(&("Iterator".to_string(), iter_name.clone()))
                {
                    match impl_def.type_args.first() {
                        Some(Type::Ref { inner, .. }) => (**inner).clone(),
                        _ => Type::I64,
                    }
                } else {
                    Type::I64
                };
                mir_prog.functions.push(self.generate_iter_next_fn(
                    &mangled_fn,
                    &iter_name,
                    &elem_ty,
                    &opt_enum_name,
                ));
            }
        }

        mir_prog.string_literals = std::mem::take(&mut self.string_literals);
        mir_prog
    }

    fn generate_iter_next_fn(
        &mut self,
        mangled_name: &str,
        iter_struct_name: &str,
        elem_ty: &Type,
        opt_enum_name: &str,
    ) -> MirFunction {
        self.next_val = 1;
        self.next_slot = 1;
        self.next_block = 1;
        self.current_slots.clear();
        self.current_blocks.clear();
        self.var_map = vec![HashMap::new()];
        self.drop_scopes = vec![Vec::new()];
        self.loop_exit_blocks.clear();
        self.loop_cond_blocks.clear();
        self.loop_scope_depths.clear();

        let elem_size = elem_ty.size_with_structs(&self.sema.structs).max(8);
        let opt_ty = Type::Enum(opt_enum_name.to_string());
        self.current_fn_ret_type = opt_ty.clone();

        let self_slot = self.new_slot(
            "self".to_string(),
            Type::Ref {
                is_mut: true,
                inner: Box::new(Type::Struct(iter_struct_name.to_string())),
            },
        );
        let params = vec![(
            self_slot,
            Type::Ref {
                is_mut: true,
                inner: Box::new(Type::Struct(iter_struct_name.to_string())),
            },
        )];

        let entry_block = self.new_block("entry");
        let some_block = self.new_block("some");
        let none_block = self.new_block("none");

        self.switch_to_block(entry_block);

        let self_ptr = self.new_value();
        self.emit(Instruction::Load {
            dest: self_ptr,
            slot: self_slot,
            ty: Type::I64,
        });

        // Field 0: vec_ptr
        let vec_field_addr = self.new_value();
        self.emit(Instruction::FieldAddr {
            dest: vec_field_addr,
            base_ptr: self_ptr,
            offset: 0,
        });
        let vec_ptr = self.new_value();
        self.emit(Instruction::LoadIndirect {
            dest: vec_ptr,
            addr: vec_field_addr,
            ty: Type::I64,
        });

        // Field 1: index
        let index_field_addr = self.new_value();
        self.emit(Instruction::FieldAddr {
            dest: index_field_addr,
            base_ptr: self_ptr,
            offset: 8,
        });
        let index_val = self.new_value();
        self.emit(Instruction::LoadIndirect {
            dest: index_val,
            addr: index_field_addr,
            ty: Type::I64,
        });

        let len_val = self.new_value();
        self.emit(Instruction::Call {
            dest: Some(len_val),
            callee: "vanta_vec_len".to_string(),
            args: vec![vec_ptr],
            ret_type: Type::I64,
        });

        let cond = self.new_value();
        self.emit(Instruction::Binary {
            dest: cond,
            op: MirBinaryOp::Lt,
            lhs: index_val,
            rhs: len_val,
            ty: Type::Bool,
        });

        self.terminate(Terminator::CondBranch {
            cond,
            then_block: some_block,
            else_block: none_block,
        });

        // --- some_block ---
        self.switch_to_block(some_block);

        let size_val = self.new_value();
        self.emit(Instruction::ConstInt {
            dest: size_val,
            val: elem_size as i64,
            ty: Type::I64,
        });
        let elem_ptr = self.new_value();
        self.emit(Instruction::Call {
            dest: Some(elem_ptr),
            callee: "vanta_vec_index".to_string(),
            args: vec![vec_ptr, index_val, size_val],
            ret_type: Type::I64,
        });

        let one_val = self.new_value();
        self.emit(Instruction::ConstInt {
            dest: one_val,
            val: 1,
            ty: Type::I64,
        });
        let next_index = self.new_value();
        self.emit(Instruction::Binary {
            dest: next_index,
            op: MirBinaryOp::Add,
            lhs: index_val,
            rhs: one_val,
            ty: Type::I64,
        });
        self.emit(Instruction::StoreIndirect {
            addr: index_field_addr,
            val: next_index,
        });

        let res_slot_some = self.new_slot("_res_some".to_string(), opt_ty.clone());
        let res_ptr_some = self.new_value();
        self.emit(Instruction::SlotAddr {
            dest: res_ptr_some,
            slot: res_slot_some,
        });
        let tag_some = self.new_value();
        self.emit(Instruction::ConstInt {
            dest: tag_some,
            val: 1,
            ty: Type::I32,
        });
        self.emit(Instruction::StoreIndirect {
            addr: res_ptr_some,
            val: tag_some,
        });
        let payload_addr_some = self.new_value();
        self.emit(Instruction::FieldAddr {
            dest: payload_addr_some,
            base_ptr: res_ptr_some,
            offset: 8,
        });
        self.emit(Instruction::StoreIndirect {
            addr: payload_addr_some,
            val: elem_ptr,
        });
        self.terminate(Terminator::Return(Some(res_ptr_some)));

        // --- none_block ---
        self.switch_to_block(none_block);

        let res_slot_none = self.new_slot("_res_none".to_string(), opt_ty.clone());
        let res_ptr_none = self.new_value();
        self.emit(Instruction::SlotAddr {
            dest: res_ptr_none,
            slot: res_slot_none,
        });
        let tag_none = self.new_value();
        self.emit(Instruction::ConstInt {
            dest: tag_none,
            val: 0,
            ty: Type::I32,
        });
        self.emit(Instruction::StoreIndirect {
            addr: res_ptr_none,
            val: tag_none,
        });
        self.terminate(Terminator::Return(Some(res_ptr_none)));

        MirFunction {
            name: mangled_name.to_string(),
            params,
            ret_type: opt_ty,
            slots: std::mem::take(&mut self.current_slots),
            blocks: std::mem::take(&mut self.current_blocks),
            span: Span::dummy(),
            slot_spans: HashMap::new(),
            val_spans: HashMap::new(),
            terminator_spans: HashMap::new(),
        }
    }

    fn lower_function_with_name(&mut self, f: &FnDecl, mangled_name: &str) -> MirFunction {
        self.next_val = 1;
        self.next_slot = 1;
        self.next_block = 1;
        self.current_slots.clear();
        self.current_blocks.clear();
        self.var_map = vec![HashMap::new()];
        self.drop_scopes = vec![Vec::new()];
        self.loop_exit_blocks.clear();
        self.loop_cond_blocks.clear();
        self.loop_scope_depths.clear();
        self.slot_spans.clear();
        self.val_spans.clear();
        self.terminator_spans.clear();
        self.current_fn_span = f.span;
        self.current_span = f.span;

        let sig = self.sema.functions.get(mangled_name).unwrap();
        self.current_fn_ret_type = sig.ret_type.clone();
        let mut params = Vec::new();

        let entry_block = self.new_block("entry");
        self.switch_to_block(entry_block);

        for (i, (p_name, p_ty, _)) in sig.params.iter().enumerate() {
            let p_span = f.params.get(i).map(|p| p.span).unwrap_or(f.span);
            let prev_span = self.current_span;
            self.current_span = p_span;
            let slot = self.new_slot(p_name.clone(), p_ty.clone());
            self.slot_spans.insert(slot, p_span);
            self.current_span = prev_span;
            params.push((slot, p_ty.clone()));
            if p_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                if let Some(scope) = self.drop_scopes.first_mut() {
                    scope.push(DropCandidate {
                        slot,
                        ty: p_ty.clone(),
                        name: p_name.clone(),
                        is_moved: false,
                    });
                }
            }
        }

        let tail_val = self.lower_block(&f.body);

        // Ensure block terminates
        if !self.is_current_terminated() {
            let outer = self.drop_scopes.pop().unwrap_or_default();
            for candidate in outer.iter().rev() {
                if !candidate.is_moved {
                    let ptr = self.new_value();
                    self.emit(Instruction::SlotAddr {
                        dest: ptr,
                        slot: candidate.slot,
                    });
                    self.emit_drop_for_place(ptr, &candidate.ty);
                }
            }
            if sig.ret_type == Type::Unit {
                self.terminate(Terminator::Return(None));
            } else if let Some(val) = tail_val {
                self.terminate(Terminator::Return(Some(val)));
            } else {
                self.terminate(Terminator::Unreachable);
            }
        }

        MirFunction {
            name: mangled_name.to_string(),
            params,
            ret_type: sig.ret_type.clone(),
            slots: std::mem::take(&mut self.current_slots),
            blocks: std::mem::take(&mut self.current_blocks),
            span: self.current_fn_span,
            slot_spans: std::mem::take(&mut self.slot_spans),
            val_spans: std::mem::take(&mut self.val_spans),
            terminator_spans: std::mem::take(&mut self.terminator_spans),
        }
    }

    fn lower_function(&mut self, f: &FnDecl) -> MirFunction {
        self.lower_function_with_name(f, &f.name.name)
    }

    fn mark_moved(&mut self, name: &str) {
        for scope in self.drop_scopes.iter_mut().rev() {
            if let Some(c) = scope.iter_mut().rev().find(|c| c.name == name) {
                c.is_moved = true;
                break;
            }
        }
    }

    fn mark_unmoved(&mut self, name: &str) {
        for scope in self.drop_scopes.iter_mut().rev() {
            if let Some(c) = scope.iter_mut().rev().find(|c| c.name == name) {
                c.is_moved = false;
                break;
            }
        }
    }

    fn emit_drop_for_place(&mut self, ptr: ValueId, ty: &Type) {
        if ty.is_vec() || matches!(ty, Type::Struct(s) if s == "Vec" || s.starts_with("Vec__")) {
            let callee = if ty.vec_elem_type().is_some_and(|e| e.is_string()) {
                "vanta_vec_free_strings"
            } else {
                "vanta_vec_free"
            };
            self.emit(Instruction::Call {
                dest: None,
                callee: callee.to_string(),
                args: vec![ptr],
                ret_type: Type::Unit,
            });
        } else if ty.is_string() {
            self.emit(Instruction::Call {
                dest: None,
                callee: "vanta_str_free".to_string(),
                args: vec![ptr],
                ret_type: Type::Unit,
            });
        } else if let Type::Struct(s_name) = ty {
            if let Some(s_def) = self.sema.structs.get(s_name).cloned() {
                for f in &s_def.fields {
                    if f.ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                        let field_ptr = self.new_value();
                        self.emit(Instruction::FieldAddr {
                            dest: field_ptr,
                            base_ptr: ptr,
                            offset: f.offset,
                        });
                        self.emit_drop_for_place(field_ptr, &f.ty);
                    }
                }
            }
        } else if let Type::Enum(e_name) = ty {
            if let Some(e_def) = self.sema.enums.get(e_name).cloned() {
                if ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                    let tag_ptr = self.new_value();
                    self.emit(Instruction::FieldAddr {
                        dest: tag_ptr,
                        base_ptr: ptr,
                        offset: 0,
                    });
                    let tag_val = self.new_value();
                    self.emit(Instruction::LoadIndirect {
                        dest: tag_val,
                        addr: tag_ptr,
                        ty: Type::I32,
                    });

                    let drop_variants: Vec<_> = e_def
                        .variants
                        .iter()
                        .filter(|v| {
                            v.payload
                                .iter()
                                .any(|p| p.needs_drop(&self.sema.structs, &self.sema.enums))
                        })
                        .cloned()
                        .collect();

                    if !drop_variants.is_empty() {
                        let done_block = self.new_block("enum_drop.done");
                        for (i, v) in drop_variants.iter().enumerate() {
                            let is_last = i == drop_variants.len() - 1;
                            let v_drop_block = self.new_block(format!("enum_drop.{}.body", v.name));
                            let next_test_block = if is_last {
                                done_block
                            } else {
                                self.new_block(format!("enum_drop.{}.next", v.name))
                            };

                            let expected_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: expected_tag,
                                val: v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: expected_tag,
                                ty: Type::Bool,
                            });
                            self.terminate(Terminator::CondBranch {
                                cond,
                                then_block: v_drop_block,
                                else_block: next_test_block,
                            });

                            self.switch_to_block(v_drop_block);
                            for (f_idx, f_ty) in v.payload.iter().enumerate() {
                                if f_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                                    let f_offset = v.payload_offsets[f_idx];
                                    let field_ptr = self.new_value();
                                    self.emit(Instruction::FieldAddr {
                                        dest: field_ptr,
                                        base_ptr: ptr,
                                        offset: f_offset,
                                    });
                                    self.emit_drop_for_place(field_ptr, f_ty);
                                }
                            }
                            self.terminate(Terminator::Branch(done_block));

                            if !is_last {
                                self.switch_to_block(next_test_block);
                            }
                        }
                        self.switch_to_block(done_block);
                    }
                }
            }
        }
    }

    fn lower_index_base_and_len(&mut self, array: &Expr) -> (ValueId, ValueId, Type, usize) {
        let arr_ty = self
            .sema
            .expr_types
            .get(&(array as *const Expr))
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

        if arr_ty.is_vec() || matches!(&arr_ty, Type::Ref { inner, .. } if inner.is_vec()) {
            let elem_ty = arr_ty
                .vec_elem_type()
                .or_else(|| match &arr_ty {
                    Type::Ref { inner, .. } => inner.vec_elem_type(),
                    _ => None,
                })
                .unwrap_or(Type::I64);
            let elem_size = elem_ty.size_with_structs(&self.sema.structs).max(8);
            let (vec_ptr, _) = self.lower_place_addr(array);
            // Load data_ptr from vec_ptr (offset 0)
            let data_ptr = self.new_value();
            self.emit(Instruction::LoadIndirect {
                dest: data_ptr,
                addr: vec_ptr,
                ty: Type::I64,
            });
            // Load len from vec_ptr (offset 8)
            let len_ptr = self.new_value();
            self.emit(Instruction::FieldAddr {
                dest: len_ptr,
                base_ptr: vec_ptr,
                offset: 8,
            });
            let len_v = self.new_value();
            self.emit(Instruction::LoadIndirect {
                dest: len_v,
                addr: len_ptr,
                ty: Type::I64,
            });
            return (data_ptr, len_v, elem_ty, elem_size);
        }

        match &arr_ty {
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
                Type::Slice(elem_t) => {
                    let (ptr_v, len_v) = self.lower_slice_expr(array);
                    let elem_size = elem_t.size_with_structs(&self.sema.structs).max(8);
                    (ptr_v, len_v, elem_t.as_ref().clone(), elem_size)
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

    fn lower_place_addr(&mut self, expr: &Expr) -> (ValueId, Type) {
        match &expr.kind {
            ExprKind::Ident(name) => {
                let slot = self
                    .lookup_slot(name)
                    .unwrap_or_else(|| panic!("Undefined slot for variable '{}'", name));
                let slot_ty = self
                    .current_slots
                    .iter()
                    .find(|s| s.id == slot)
                    .unwrap()
                    .ty
                    .clone();

                if let Type::Ref { is_mut: _, inner } = &slot_ty {
                    let ptr = self.new_value();
                    self.emit(Instruction::Load {
                        dest: ptr,
                        slot,
                        ty: slot_ty.clone(),
                    });
                    (ptr, (**inner).clone())
                } else {
                    let ptr = self.new_value();
                    self.emit(Instruction::SlotAddr { dest: ptr, slot });
                    (ptr, slot_ty)
                }
            }
            ExprKind::FieldAccess { target, field } => {
                let (base_ptr, base_ty) = self.lower_place_addr(target);
                let struct_name = match &base_ty {
                    Type::Struct(s) => s.clone(),
                    Type::Ref { inner, .. } => {
                        if let Type::Struct(s) = &**inner {
                            s.clone()
                        } else {
                            panic!("Field access on non-struct type: {:?}", base_ty);
                        }
                    }
                    _ => panic!("Field access on non-struct type: {:?}", base_ty),
                };
                let s_def = self.sema.structs[&struct_name].clone();
                let f_def = s_def.find_field(&field.name).unwrap_or_else(|| {
                    panic!("Field '{}' not found in '{}'", field.name, struct_name)
                });
                let field_ptr = self.new_value();
                self.emit(Instruction::FieldAddr {
                    dest: field_ptr,
                    base_ptr,
                    offset: f_def.offset,
                });
                (field_ptr, f_def.ty.clone())
            }
            ExprKind::Deref(inner) => {
                let addr = self.lower_expr(inner);
                let inner_ty = self
                    .sema
                    .expr_types
                    .get(&(inner.as_ref() as *const Expr))
                    .cloned()
                    .unwrap_or(Type::I64);
                let target_ty = match inner_ty {
                    Type::Ref { inner: t, .. } => *t,
                    _ => Type::I64,
                };
                (addr, target_ty)
            }
            ExprKind::Index { array, index } => {
                let idx_v = self.lower_expr(index);
                let (base_ptr, len_v, elem_ty, elem_size) = self.lower_index_base_and_len(array);
                self.emit_bounds_check(idx_v, len_v);
                let addr_v = self.new_value();
                self.emit(Instruction::IndexPtrAddr {
                    dest: addr_v,
                    base_ptr,
                    index: idx_v,
                    elem_size,
                });
                (addr_v, elem_ty)
            }
            _ => {
                let val = self.lower_expr(expr);
                (val, Type::I64)
            }
        }
    }

    fn lower_block(&mut self, block: &Block) -> Option<ValueId> {
        self.var_map.push(HashMap::new());
        self.drop_scopes.push(Vec::new());

        for stmt in &block.stmts {
            if self.is_current_terminated() {
                break;
            }
            self.lower_stmt(stmt);
        }

        let result = if !self.is_current_terminated() {
            if let Some(tail) = &block.expr {
                if let ExprKind::Ident(tail_id) = &tail.kind {
                    let ty = self.lookup_slot(tail_id).and_then(|s| {
                        self.current_slots
                            .iter()
                            .find(|slot| slot.id == s)
                            .map(|slot| slot.ty.clone())
                    });
                    if let Some(t) = ty {
                        if t.needs_drop(&self.sema.structs, &self.sema.enums) {
                            self.mark_moved(tail_id);
                        }
                    }
                }
                Some(self.lower_expr(tail))
            } else {
                None
            }
        } else {
            None
        };

        let scope_candidates = self.drop_scopes.pop().unwrap_or_default();
        if !self.is_current_terminated() {
            for candidate in scope_candidates.iter().rev() {
                if !candidate.is_moved {
                    let ptr = self.new_value();
                    self.emit(Instruction::SlotAddr {
                        dest: ptr,
                        slot: candidate.slot,
                    });
                    self.emit_drop_for_place(ptr, &candidate.ty);
                }
            }
        }

        self.var_map.pop();
        result
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        let s_span = match stmt {
            Stmt::Let { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Loop { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::Break { span }
            | Stmt::Continue { span } => *span,
            Stmt::Expr(e) => e.span,
        };
        if !s_span.is_dummy() {
            self.current_span = s_span;
        }
        match stmt {
            Stmt::Let {
                name,
                ty: _,
                init,
                span: _,
                ..
            } => {
                let ty = if let Some(sym) = self.sema.symbols.values().find(|s| s.span == name.span)
                {
                    sym.ty.clone()
                } else if let Some(init_expr) = init {
                    if let Some(inferred) = self.sema.expr_types.get(&(init_expr as *const Expr)) {
                        inferred.clone()
                    } else if let Some(sym) =
                        self.sema.symbols.values().find(|s| s.name == name.name)
                    {
                        sym.ty.clone()
                    } else if let ExprKind::String(_) = &init_expr.kind {
                        Type::Ref {
                            is_mut: false,
                            inner: Box::new(Type::Str),
                        }
                    } else {
                        Type::I64
                    }
                } else if let Some(sym) = self.sema.symbols.values().find(|s| s.name == name.name) {
                    sym.ty.clone()
                } else {
                    Type::I64
                };

                let slot = self.new_slot(name.name.clone(), ty.clone());
                if ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                    if let Some(scope) = self.drop_scopes.last_mut() {
                        scope.push(DropCandidate {
                            slot,
                            ty: ty.clone(),
                            name: name.name.clone(),
                            is_moved: false,
                        });
                    }
                }

                if let Some(init_expr) = init {
                    // Check if initializing from another variable (move)
                    if ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                        if let ExprKind::Ident(src_name) = &init_expr.kind {
                            self.mark_moved(src_name);
                        }
                    }

                    let is_vec_new = match &init_expr.kind {
                        ExprKind::Call { callee, .. } => {
                            callee == "Vec_new" || callee == "Vec::new"
                        }
                        ExprKind::VariantConstructor {
                            enum_name, variant, ..
                        } => {
                            enum_name.as_ref().map(|id| id.name.as_str()) == Some("Vec")
                                && variant.name == "new"
                        }
                        _ => false,
                    };
                    let is_str_new = match &init_expr.kind {
                        ExprKind::Call { callee, .. } => {
                            callee == "String_new"
                                || callee == "String::new"
                                || callee == "std::string::new"
                        }
                        _ => false,
                    };
                    let is_str_from_str = match &init_expr.kind {
                        ExprKind::Call { callee, .. } => {
                            callee == "String_from_str"
                                || callee == "String::from_str"
                                || callee == "std::string::from_str"
                        }
                        _ => false,
                    };

                    if is_vec_new {
                        let base_ptr = self.new_value();
                        self.emit(Instruction::SlotAddr {
                            dest: base_ptr,
                            slot,
                        });
                        self.emit(Instruction::Call {
                            dest: None,
                            callee: "vanta_vec_init".to_string(),
                            args: vec![base_ptr],
                            ret_type: Type::Unit,
                        });
                    } else if is_str_new {
                        let base_ptr = self.new_value();
                        self.emit(Instruction::SlotAddr {
                            dest: base_ptr,
                            slot,
                        });
                        self.emit(Instruction::Call {
                            dest: None,
                            callee: "vanta_str_init".to_string(),
                            args: vec![base_ptr],
                            ret_type: Type::Unit,
                        });
                    } else if is_str_from_str {
                        let base_ptr = self.new_value();
                        self.emit(Instruction::SlotAddr {
                            dest: base_ptr,
                            slot,
                        });
                        let ExprKind::Call { args, .. } = &init_expr.kind else {
                            unreachable!()
                        };
                        let (arg_ptr, arg_len) = self.lower_slice_expr(&args[0]);
                        self.emit(Instruction::Call {
                            dest: None,
                            callee: "vanta_str_from_str".to_string(),
                            args: vec![base_ptr, arg_ptr, arg_len],
                            ret_type: Type::Unit,
                        });
                    } else if ty.is_str_slice() || ty.is_slice() {
                        let (ptr_v, len_v) = self.lower_slice_expr(init_expr);
                        self.emit(Instruction::StoreSlice {
                            slot,
                            ptr: ptr_v,
                            len: len_v,
                        });
                    } else if let Type::Struct(sname) = &ty {
                        let is_vec = sname == "v256"
                            || sname == "Simd256"
                            || sname.ends_with("__v256")
                            || sname.ends_with("__Simd256");
                        if is_vec {
                            if let ExprKind::StructLiteral { fields, .. } = &init_expr.kind {
                                let base_ptr = self.new_value();
                                self.emit(Instruction::SlotAddr {
                                    dest: base_ptr,
                                    slot,
                                });
                                self.lower_init_struct_fields(base_ptr, sname, fields);
                            } else {
                                let val = self.lower_expr(init_expr);
                                self.emit(Instruction::Store { slot, val });
                            }
                        } else {
                            let base_ptr = self.new_value();
                            self.emit(Instruction::SlotAddr {
                                dest: base_ptr,
                                slot,
                            });
                            if let ExprKind::StructLiteral { fields, .. } = &init_expr.kind {
                                self.lower_init_struct_fields(base_ptr, sname, fields);
                            } else if let ExprKind::MethodCall {
                                target: m_target,
                                method,
                                ..
                            } = &init_expr.kind
                            {
                                if method.name == "clone" && ty.is_string() {
                                    let (src_ptr, _) = self.lower_place_addr(m_target);
                                    self.emit(Instruction::Call {
                                        dest: None,
                                        callee: "vanta_str_clone".to_string(),
                                        args: vec![base_ptr, src_ptr],
                                        ret_type: Type::Unit,
                                    });
                                } else {
                                    let (src_ptr, _) = self.lower_place_addr(init_expr);
                                    self.copy_struct_bytes(src_ptr, base_ptr, sname);
                                }
                            } else {
                                let (src_ptr, _) = self.lower_place_addr(init_expr);
                                self.copy_struct_bytes(src_ptr, base_ptr, sname);
                            }
                        }
                    } else if let Type::Enum(ename) = &ty {
                        let base_ptr = self.new_value();
                        self.emit(Instruction::SlotAddr {
                            dest: base_ptr,
                            slot,
                        });
                        if let ExprKind::VariantConstructor {
                            variant: v_ident,
                            args: v_args,
                            ..
                        } = &init_expr.kind
                        {
                            self.lower_init_variant(base_ptr, ename, &v_ident.name, v_args);
                        } else {
                            let (src_ptr, _) = self.lower_place_addr(init_expr);
                            self.copy_enum_bytes(src_ptr, base_ptr, ename);
                        }
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
                            if (ty.is_str_slice() || ty.is_slice()) && *op == AssignOp::Eq {
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

                if *op == AssignOp::Eq {
                    // If target is an ident whose type needs drop:
                    if let ExprKind::Ident(target_ident) = &target.kind {
                        if let Some(target_slot) = self.lookup_slot(target_ident) {
                            let target_ty = self
                                .current_slots
                                .iter()
                                .find(|s| s.id == target_slot)
                                .map(|s| s.ty.clone());
                            if let Some(t_ty) = target_ty {
                                if t_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                                    let is_target_moved = self.drop_scopes.iter().any(|s| {
                                        s.iter().any(|c| c.name == *target_ident && c.is_moved)
                                    });
                                    if !is_target_moved {
                                        let old_ptr = self.new_value();
                                        self.emit(Instruction::SlotAddr {
                                            dest: old_ptr,
                                            slot: target_slot,
                                        });
                                        self.emit_drop_for_place(old_ptr, &t_ty);
                                    }
                                    self.mark_unmoved(target_ident);
                                }
                            }
                        }
                    }

                    // If value is an ident whose type needs drop, it is moved!
                    if let ExprKind::Ident(val_ident) = &value.kind {
                        if let Some(src_slot) = self.lookup_slot(val_ident) {
                            let src_ty = self
                                .current_slots
                                .iter()
                                .find(|s| s.id == src_slot)
                                .map(|s| s.ty.clone());
                            if let Some(s_ty) = src_ty {
                                if s_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                                    self.mark_moved(val_ident);
                                }
                            }
                        }
                    }

                    let (dst_ptr, target_ty) = self.lower_place_addr(target);
                    if let Type::Struct(sname) = &target_ty {
                        if let ExprKind::StructLiteral { fields, .. } = &value.kind {
                            self.lower_init_struct_fields(dst_ptr, sname, fields);
                        } else if let ExprKind::Call { callee, args, .. } = &value.kind {
                            let callee = self.sema.call_callees.get(&value.span).unwrap_or(callee);
                            if callee == "String_new"
                                || callee == "String::new"
                                || callee == "std::string::new"
                            {
                                self.emit(Instruction::Call {
                                    dest: None,
                                    callee: "vanta_str_init".to_string(),
                                    args: vec![dst_ptr],
                                    ret_type: Type::Unit,
                                });
                            } else if callee == "String_from_str"
                                || callee == "String::from_str"
                                || callee == "std::string::from_str"
                            {
                                let (arg_ptr, arg_len) = self.lower_slice_expr(&args[0]);
                                self.emit(Instruction::Call {
                                    dest: None,
                                    callee: "vanta_str_from_str".to_string(),
                                    args: vec![dst_ptr, arg_ptr, arg_len],
                                    ret_type: Type::Unit,
                                });
                            } else if sname == "v256"
                                || sname == "Simd256"
                                || sname.ends_with("__v256")
                                || sname.ends_with("__Simd256")
                            {
                                if let ExprKind::Ident(target_name) = &target.kind {
                                    if let Some(target_slot) = self.lookup_slot(target_name) {
                                        let val = self.lower_expr(value);
                                        self.emit(Instruction::Store {
                                            slot: target_slot,
                                            val,
                                        });
                                    } else {
                                        let val = self.lower_expr(value);
                                        self.emit(Instruction::StoreIndirect {
                                            addr: dst_ptr,
                                            val,
                                        });
                                    }
                                } else {
                                    let val = self.lower_expr(value);
                                    self.emit(Instruction::StoreIndirect { addr: dst_ptr, val });
                                }
                            } else {
                                let (src_ptr, _) = self.lower_place_addr(value);
                                self.copy_struct_bytes(src_ptr, dst_ptr, sname);
                            }
                        } else if let ExprKind::MethodCall {
                            target: m_target,
                            method,
                            ..
                        } = &value.kind
                        {
                            if method.name == "clone" && target_ty.is_string() {
                                let (src_ptr, _) = self.lower_place_addr(m_target);
                                self.emit(Instruction::Call {
                                    dest: None,
                                    callee: "vanta_str_clone".to_string(),
                                    args: vec![dst_ptr, src_ptr],
                                    ret_type: Type::Unit,
                                });
                            } else {
                                let (src_ptr, _) = self.lower_place_addr(value);
                                self.copy_struct_bytes(src_ptr, dst_ptr, sname);
                            }
                        } else if sname == "v256"
                            || sname == "Simd256"
                            || sname.ends_with("__v256")
                            || sname.ends_with("__Simd256")
                        {
                            if let ExprKind::Ident(target_name) = &target.kind {
                                if let Some(target_slot) = self.lookup_slot(target_name) {
                                    let val = self.lower_expr(value);
                                    self.emit(Instruction::Store {
                                        slot: target_slot,
                                        val,
                                    });
                                } else {
                                    let val = self.lower_expr(value);
                                    self.emit(Instruction::StoreIndirect { addr: dst_ptr, val });
                                }
                            } else {
                                let val = self.lower_expr(value);
                                self.emit(Instruction::StoreIndirect { addr: dst_ptr, val });
                            }
                        } else {
                            let (src_ptr, _) = self.lower_place_addr(value);
                            self.copy_struct_bytes(src_ptr, dst_ptr, sname);
                        }
                        return;
                    } else if let Type::Enum(ename) = &target_ty {
                        if let ExprKind::VariantConstructor {
                            variant: v_ident,
                            args: v_args,
                            ..
                        } = &value.kind
                        {
                            self.lower_init_variant(dst_ptr, ename, &v_ident.name, v_args);
                        } else {
                            let (src_ptr, _) = self.lower_place_addr(value);
                            self.copy_enum_bytes(src_ptr, dst_ptr, ename);
                        }
                        return;
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
                    ExprKind::FieldAccess { .. } => {
                        let (field_ptr, _) = self.lower_place_addr(target);
                        self.emit(Instruction::StoreIndirect {
                            addr: field_ptr,
                            val: final_val,
                        });
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
                        let (base_ptr, len_v, _elem_ty, elem_size) =
                            self.lower_index_base_and_len(array);

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
                self.loop_scope_depths.push(self.drop_scopes.len());
                self.loop_cond_blocks.push(cond_block);
                self.loop_exit_blocks.push(exit_block);
                self.switch_to_block(body_block);
                self.lower_block(body);
                if !self.is_current_terminated() {
                    self.terminate(Terminator::Branch(cond_block));
                }
                self.loop_scope_depths.pop();
                self.loop_cond_blocks.pop();
                self.loop_exit_blocks.pop();

                // Exit block
                self.switch_to_block(exit_block);
            }
            Stmt::Loop { body, span: _ } => {
                let body_block = self.new_block("loop.body");
                let exit_block = self.new_block("loop.exit");

                self.terminate(Terminator::Branch(body_block));

                self.loop_scope_depths.push(self.drop_scopes.len());
                self.loop_cond_blocks.push(body_block);
                self.loop_exit_blocks.push(exit_block);
                self.switch_to_block(body_block);
                self.lower_block(body);
                if !self.is_current_terminated() {
                    self.terminate(Terminator::Branch(body_block));
                }
                self.loop_scope_depths.pop();
                self.loop_cond_blocks.pop();
                self.loop_exit_blocks.pop();

                self.switch_to_block(exit_block);
            }
            Stmt::For {
                var,
                iter,
                body,
                span: _,
            } => {
                self.lower_for_stmt(var, iter, body);
            }
            Stmt::Return { value, span: _ } => {
                let val_v = if let Some(e) = value {
                    if let ExprKind::Ident(ret_id) = &e.kind {
                        let ty = self.lookup_slot(ret_id).and_then(|s| {
                            self.current_slots
                                .iter()
                                .find(|slot| slot.id == s)
                                .map(|slot| slot.ty.clone())
                        });
                        if let Some(t) = ty {
                            if t.needs_drop(&self.sema.structs, &self.sema.enums) {
                                self.mark_moved(ret_id);
                            }
                        }
                    }
                    Some(self.lower_expr(e))
                } else {
                    None
                };

                let mut candidates_to_drop = Vec::new();
                for scope in self.drop_scopes.iter().rev() {
                    for candidate in scope.iter().rev() {
                        if !candidate.is_moved {
                            candidates_to_drop.push(candidate.clone());
                        }
                    }
                }
                for candidate in candidates_to_drop {
                    let ptr = self.new_value();
                    self.emit(Instruction::SlotAddr {
                        dest: ptr,
                        slot: candidate.slot,
                    });
                    self.emit_drop_for_place(ptr, &candidate.ty);
                }

                self.terminate(Terminator::Return(val_v));
            }
            Stmt::Break { .. } => {
                if let Some(&depth) = self.loop_scope_depths.last() {
                    let mut candidates_to_drop = Vec::new();
                    for scope in self.drop_scopes[depth..].iter().rev() {
                        for candidate in scope.iter().rev() {
                            if !candidate.is_moved {
                                candidates_to_drop.push(candidate.clone());
                            }
                        }
                    }
                    for candidate in candidates_to_drop {
                        let ptr = self.new_value();
                        self.emit(Instruction::SlotAddr {
                            dest: ptr,
                            slot: candidate.slot,
                        });
                        self.emit_drop_for_place(ptr, &candidate.ty);
                    }
                }
                if let Some(&exit) = self.loop_exit_blocks.last() {
                    self.terminate(Terminator::Branch(exit));
                }
            }
            Stmt::Continue { .. } => {
                if let Some(&depth) = self.loop_scope_depths.last() {
                    let mut candidates_to_drop = Vec::new();
                    for scope in self.drop_scopes[depth..].iter().rev() {
                        for candidate in scope.iter().rev() {
                            if !candidate.is_moved {
                                candidates_to_drop.push(candidate.clone());
                            }
                        }
                    }
                    for candidate in candidates_to_drop {
                        let ptr = self.new_value();
                        self.emit(Instruction::SlotAddr {
                            dest: ptr,
                            slot: candidate.slot,
                        });
                        self.emit_drop_for_place(ptr, &candidate.ty);
                    }
                }
                if let Some(&cond) = self.loop_cond_blocks.last() {
                    self.terminate(Terminator::Branch(cond));
                }
            }
            Stmt::Expr(expr) => {
                self.lower_expr(expr);
            }
        }
    }

    fn lower_for_stmt(&mut self, var: &Ident, iter: &Expr, body: &Block) {
        let iter_ty = self
            .sema
            .expr_types
            .get(&(iter as *const Expr))
            .cloned()
            .unwrap_or(Type::I64);
        let target_name = match &iter_ty {
            Type::Struct(s) => s.clone(),
            Type::Enum(e) => e.clone(),
            _ => panic!("Expected iterable struct or enum, got {:?}", iter_ty),
        };

        // Find Iterator trait impl and mangled method
        let (trait_canon, impl_def) = self
            .sema
            .trait_impls
            .iter()
            .find(|((t_canon, tgt), _)| {
                (tgt == &target_name
                    || tgt.ends_with(&format!("::{}", target_name))
                    || target_name.ends_with(&format!("::{}", tgt)))
                    && (t_canon == "Iterator"
                        || t_canon.ends_with("::Iterator")
                        || t_canon == "std__iter__Iterator")
            })
            .map(|((t, _), def)| (t.clone(), def.clone()))
            .expect("Iterator implementation must exist");

        let next_mangled = self
            .sema
            .trait_methods
            .get(&(target_name.clone(), trait_canon.clone(), "next".to_string()))
            .cloned()
            .or_else(|| {
                self.sema
                    .trait_methods
                    .iter()
                    .find(|((tgt, trt, m), _)| {
                        (tgt == &target_name
                            || tgt.ends_with(&format!("::{}", target_name))
                            || target_name.ends_with(&format!("::{}", tgt)))
                            && (trt == "Iterator"
                                || trt.ends_with("::Iterator")
                                || trt == "std__iter__Iterator")
                            && m == "next"
                    })
                    .map(|(_, name)| name.clone())
            })
            .or_else(|| impl_def.methods.get("next").cloned())
            .expect("next method must exist");

        let elem_ty = impl_def.type_args.first().cloned().unwrap_or(Type::I64);

        // 1. Allocate iter_slot and evaluate iter expression once
        let iter_slot = self.new_slot(format!("_for_iter_{}", self.next_slot), iter_ty.clone());
        let iter_ptr = self.new_value();
        self.emit(Instruction::SlotAddr {
            dest: iter_ptr,
            slot: iter_slot,
        });

        // Mark src moved if iter was a named local variable
        if iter_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
            if let ExprKind::Ident(src_name) = &iter.kind {
                self.mark_moved(src_name);
            }
            if let Some(scope) = self.drop_scopes.last_mut() {
                scope.push(DropCandidate {
                    slot: iter_slot,
                    ty: iter_ty.clone(),
                    name: format!("_for_iter_{}", iter_slot.0),
                    is_moved: false,
                });
            }
        }

        // Evaluate iter into iter_ptr
        if let Type::Struct(sname) = &iter_ty {
            if let ExprKind::StructLiteral { fields, .. } = &iter.kind {
                self.lower_init_struct_fields(iter_ptr, sname, fields);
            } else {
                let (src_ptr, _) = self.lower_place_addr(iter);
                self.copy_struct_bytes(src_ptr, iter_ptr, sname);
            }
        } else if let Type::Enum(ename) = &iter_ty {
            if let ExprKind::VariantConstructor {
                variant: v_ident,
                args: v_args,
                ..
            } = &iter.kind
            {
                self.lower_init_variant(iter_ptr, ename, &v_ident.name, v_args);
            } else {
                let (src_ptr, _) = self.lower_place_addr(iter);
                self.copy_enum_bytes(src_ptr, iter_ptr, ename);
            }
        }

        // 2. Blocks
        let cond_block = self.new_block("for.cond");
        let body_block = self.new_block("for.body");
        let exit_block = self.new_block("for.exit");

        self.terminate(Terminator::Branch(cond_block));

        // 3. Condition block: call next(&mut iter)
        self.switch_to_block(cond_block);
        let iter_ref = self.new_value();
        self.emit(Instruction::SlotAddr {
            dest: iter_ref,
            slot: iter_slot,
        });

        let next_sig = self.sema.functions.get(&next_mangled).unwrap().clone();
        let opt_res = self.new_value();
        self.emit(Instruction::Call {
            dest: Some(opt_res),
            callee: next_mangled,
            args: vec![iter_ref],
            ret_type: next_sig.ret_type.clone(),
        });

        // Check Option tag (0 = None, 1 = Some)
        let opt_ename = match &next_sig.ret_type {
            Type::Enum(e) => e.clone(),
            _ => panic!("Expected Option enum return type"),
        };
        let opt_edef = self.sema.enums.get(&opt_ename).unwrap().clone();
        let some_v = opt_edef.find_variant("Some").unwrap().clone();

        let tag_val = self.new_value();
        self.emit(Instruction::LoadIndirect {
            dest: tag_val,
            addr: opt_res,
            ty: Type::I32,
        });

        let some_tag = self.new_value();
        self.emit(Instruction::ConstInt {
            dest: some_tag,
            val: some_v.tag as i64,
            ty: Type::I32,
        });

        let cond_val = self.new_value();
        self.emit(Instruction::Binary {
            dest: cond_val,
            op: MirBinaryOp::Eq,
            lhs: tag_val,
            rhs: some_tag,
            ty: Type::Bool,
        });

        self.terminate(Terminator::CondBranch {
            cond: cond_val,
            then_block: body_block,
            else_block: exit_block,
        });

        // 4. Body block
        self.switch_to_block(body_block);
        self.var_map.push(HashMap::new());
        self.drop_scopes.push(Vec::new());

        // Unpack Some payload into loop variable `var`
        let var_slot = self.new_slot(var.name.clone(), elem_ty.clone());
        self.var_map
            .last_mut()
            .unwrap()
            .insert(var.name.clone(), var_slot);
        if elem_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
            self.drop_scopes.last_mut().unwrap().push(DropCandidate {
                slot: var_slot,
                ty: elem_ty.clone(),
                name: var.name.clone(),
                is_moved: false,
            });
        }

        let payload_ptr = self.new_value();
        self.emit(Instruction::FieldAddr {
            dest: payload_ptr,
            base_ptr: opt_res,
            offset: some_v.payload_offsets[0],
        });

        if let Type::Struct(sname) = &elem_ty {
            let var_ptr = self.new_value();
            self.emit(Instruction::SlotAddr {
                dest: var_ptr,
                slot: var_slot,
            });
            self.copy_struct_bytes(payload_ptr, var_ptr, sname);
        } else if let Type::Enum(ename) = &elem_ty {
            let var_ptr = self.new_value();
            self.emit(Instruction::SlotAddr {
                dest: var_ptr,
                slot: var_slot,
            });
            self.copy_enum_bytes(payload_ptr, var_ptr, ename);
        } else if elem_ty.is_str_slice() || elem_ty.is_slice() {
            let ptr_v = self.new_value();
            self.emit(Instruction::LoadIndirect {
                dest: ptr_v,
                addr: payload_ptr,
                ty: Type::I64,
            });
            let len_ptr = self.new_value();
            self.emit(Instruction::FieldAddr {
                dest: len_ptr,
                base_ptr: payload_ptr,
                offset: 8,
            });
            let len_v = self.new_value();
            self.emit(Instruction::LoadIndirect {
                dest: len_v,
                addr: len_ptr,
                ty: Type::I64,
            });
            self.emit(Instruction::StoreSlice {
                slot: var_slot,
                ptr: ptr_v,
                len: len_v,
            });
        } else {
            let elem_val = self.new_value();
            self.emit(Instruction::LoadIndirect {
                dest: elem_val,
                addr: payload_ptr,
                ty: elem_ty.clone(),
            });
            self.emit(Instruction::Store {
                slot: var_slot,
                val: elem_val,
            });
        }

        self.loop_scope_depths.push(self.drop_scopes.len());
        self.loop_cond_blocks.push(cond_block);
        self.loop_exit_blocks.push(exit_block);

        for stmt in &body.stmts {
            if self.is_current_terminated() {
                break;
            }
            self.lower_stmt(stmt);
        }

        if let Some(tail) = &body.expr {
            if !self.is_current_terminated() {
                self.lower_expr(tail);
            }
        }

        self.loop_scope_depths.pop();
        self.loop_cond_blocks.pop();
        self.loop_exit_blocks.pop();

        // Clean up loop body locals at the end of iteration
        let body_candidates = self.drop_scopes.pop().unwrap_or_default();
        self.var_map.pop();

        if !self.is_current_terminated() {
            for candidate in body_candidates.iter().rev() {
                if !candidate.is_moved {
                    let ptr = self.new_value();
                    self.emit(Instruction::SlotAddr {
                        dest: ptr,
                        slot: candidate.slot,
                    });
                    self.emit_drop_for_place(ptr, &candidate.ty);
                }
            }
            self.terminate(Terminator::Branch(cond_block));
        }

        // 5. Exit block
        self.switch_to_block(exit_block);
    }

    fn lower_expr(&mut self, expr: &Expr) -> ValueId {
        let prev_span = self.current_span;
        if !expr.span.is_dummy() {
            self.current_span = expr.span;
        }
        let res = match &expr.kind {
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
                    if let Some(Type::Array { .. } | Type::Struct(_) | Type::Enum(_)) = slot_ty {
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
                let (dest, _) = self.lower_place_addr(inner);
                dest
            }
            ExprKind::Deref(inner) => {
                let addr_v = self.lower_expr(inner);
                let inner_ty = self
                    .sema
                    .expr_types
                    .get(&(inner.as_ref() as *const Expr))
                    .cloned()
                    .unwrap_or(Type::I64);
                let target_ty = match inner_ty {
                    Type::Ref { inner: t, .. } => *t,
                    _ => Type::I64,
                };
                let dest = self.new_value();
                self.emit(Instruction::LoadIndirect {
                    dest,
                    addr: addr_v,
                    ty: target_ty,
                });
                dest
            }
            ExprKind::Call {
                callee,
                args,
                type_args: _,
            } => {
                let callee = self.sema.call_callees.get(&expr.span).unwrap_or(callee);
                if callee == "Vec_new" || callee == "Vec::new" {
                    let mangled_name = self
                        .sema
                        .call_callees
                        .get(&expr.span)
                        .cloned()
                        .unwrap_or_else(|| "Vec__i64".to_string());
                    let slot = self.new_slot(
                        format!("_vec_{}", self.next_slot),
                        Type::Struct(mangled_name),
                    );
                    let base_ptr = self.new_value();
                    self.emit(Instruction::SlotAddr {
                        dest: base_ptr,
                        slot,
                    });
                    self.emit(Instruction::Call {
                        dest: None,
                        callee: "vanta_vec_init".to_string(),
                        args: vec![base_ptr],
                        ret_type: Type::Unit,
                    });
                    return base_ptr;
                }

                if callee == "String_new" || callee == "String::new" || callee == "std::string::new"
                {
                    let slot = self.new_slot(
                        format!("_str_new_{}", self.next_slot),
                        Type::Struct("String".into()),
                    );
                    let base_ptr = self.new_value();
                    self.emit(Instruction::SlotAddr {
                        dest: base_ptr,
                        slot,
                    });
                    self.emit(Instruction::Call {
                        dest: None,
                        callee: "vanta_str_init".to_string(),
                        args: vec![base_ptr],
                        ret_type: Type::Unit,
                    });
                    return base_ptr;
                }

                if callee == "String_from_str"
                    || callee == "String::from_str"
                    || callee == "std::string::from_str"
                {
                    let slot = self.new_slot(
                        format!("_str_from_{}", self.next_slot),
                        Type::Struct("String".into()),
                    );
                    let base_ptr = self.new_value();
                    self.emit(Instruction::SlotAddr {
                        dest: base_ptr,
                        slot,
                    });
                    let (arg_ptr, arg_len) = self.lower_slice_expr(&args[0]);
                    self.emit(Instruction::Call {
                        dest: None,
                        callee: "vanta_str_from_str".to_string(),
                        args: vec![base_ptr, arg_ptr, arg_len],
                        ret_type: Type::Unit,
                    });
                    return base_ptr;
                }

                self.lower_call_internal(callee, args, expr.span)
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let expr_ty = self
                    .sema
                    .expr_types
                    .get(&(expr as *const Expr))
                    .cloned()
                    .unwrap_or(Type::Unit);
                let is_aggregate = matches!(expr_ty, Type::Struct(_) | Type::Enum(_));
                let res_slot = if expr_ty != Type::Unit {
                    Some(self.new_slot(format!("_if_res_{}", self.next_slot), expr_ty.clone()))
                } else {
                    None
                };
                let res_ptr = if let Some(slot) = res_slot {
                    let ptr = self.new_value();
                    self.emit(Instruction::SlotAddr { dest: ptr, slot });
                    Some(ptr)
                } else {
                    None
                };

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
                    if let (Some(ptr), Some(tv)) = (res_ptr, then_v) {
                        if is_aggregate {
                            match &expr_ty {
                                Type::Struct(s) => self.copy_struct_bytes(tv, ptr, s),
                                Type::Enum(e) => self.copy_enum_bytes(tv, ptr, e),
                                _ => unreachable!(),
                            }
                        } else {
                            self.emit(Instruction::StoreIndirect { addr: ptr, val: tv });
                        }
                    }
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
                    if let (Some(ptr), Some(ev)) = (res_ptr, else_v) {
                        if is_aggregate {
                            match &expr_ty {
                                Type::Struct(s) => self.copy_struct_bytes(ev, ptr, s),
                                Type::Enum(e) => self.copy_enum_bytes(ev, ptr, e),
                                _ => unreachable!(),
                            }
                        } else {
                            self.emit(Instruction::StoreIndirect { addr: ptr, val: ev });
                        }
                    }
                    self.terminate(Terminator::Branch(merge_block));
                }

                self.switch_to_block(merge_block);

                if let Some(ptr) = res_ptr {
                    if is_aggregate {
                        ptr
                    } else {
                        let final_v = self.new_value();
                        self.emit(Instruction::LoadIndirect {
                            dest: final_v,
                            addr: ptr,
                            ty: expr_ty,
                        });
                        final_v
                    }
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
                let (base_ptr, len_v, elem_ty, elem_size) = self.lower_index_base_and_len(array);

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
            ExprKind::FieldAccess { .. } => {
                let (field_ptr, field_ty) = self.lower_place_addr(expr);
                let dest = self.new_value();
                self.emit(Instruction::LoadIndirect {
                    dest,
                    addr: field_ptr,
                    ty: field_ty,
                });
                dest
            }
            ExprKind::StructLiteral {
                name,
                fields,
                type_args: _,
            } => {
                let effective_name = self
                    .sema
                    .struct_literal_names
                    .get(&expr.span)
                    .map(|s| s.as_str())
                    .unwrap_or(&name.name);
                let slot = self.new_slot(
                    format!("_struct_lit_{}", self.next_val),
                    Type::Struct(effective_name.to_string()),
                );
                let base_ptr = self.new_value();
                self.emit(Instruction::SlotAddr {
                    dest: base_ptr,
                    slot,
                });
                self.lower_init_struct_fields(base_ptr, effective_name, fields);
                base_ptr
            }
            ExprKind::VariantConstructor {
                enum_name,
                variant,
                args,
                ..
            } => {
                let expr_ty = self.sema.expr_types.get(&(expr as *const Expr)).cloned();
                if let Some(Type::Enum(mangled_enum)) = expr_ty {
                    let slot = self.new_slot(
                        format!("_enum_lit_{}", self.next_val),
                        Type::Enum(mangled_enum.clone()),
                    );
                    let base_ptr = self.new_value();
                    self.emit(Instruction::SlotAddr {
                        dest: base_ptr,
                        slot,
                    });
                    self.lower_init_variant(base_ptr, &mangled_enum, &variant.name, args);
                    base_ptr
                } else if let Some(e_id) = enum_name {
                    let mangled_callee = format!("{}_{}", e_id.name, variant.name);
                    self.lower_call_internal(&mangled_callee, args, expr.span)
                } else {
                    panic!("Unknown variant constructor: {:?}", variant);
                }
            }
            ExprKind::Match {
                expr: scrutinee,
                arms,
            } => self.lower_match(scrutinee, arms, expr),
            ExprKind::Try(inner) => self.lower_try_expr(inner, expr),
            ExprKind::MethodCall {
                target,
                method,
                args,
            } => {
                let target_ty = self
                    .sema
                    .expr_types
                    .get(&(target.as_ref() as *const Expr))
                    .cloned()
                    .unwrap_or_else(|| {
                        if let ExprKind::Ident(name) = &target.kind {
                            if let Some(slot) = self.lookup_slot(name) {
                                return self
                                    .current_slots
                                    .iter()
                                    .find(|s| s.id == slot)
                                    .map(|s| s.ty.clone())
                                    .unwrap_or(Type::Unit);
                            }
                        }
                        Type::Unit
                    });

                if target_ty.is_vec()
                    || matches!(&target_ty, Type::Ref { inner, .. } if inner.is_vec())
                {
                    let elem_ty = target_ty
                        .vec_elem_type()
                        .or_else(|| match &target_ty {
                            Type::Ref { inner, .. } => inner.vec_elem_type(),
                            _ => None,
                        })
                        .unwrap_or(Type::I64);
                    let elem_size = elem_ty.size_with_structs(&self.sema.structs).max(8);
                    let (vec_ptr, _) = self.lower_place_addr(target);

                    match method.name.as_str() {
                        "len" => {
                            let dest = self.new_value();
                            self.emit(Instruction::Call {
                                dest: Some(dest),
                                callee: "vanta_vec_len".to_string(),
                                args: vec![vec_ptr],
                                ret_type: Type::I64,
                            });
                            return dest;
                        }
                        "capacity" => {
                            let dest = self.new_value();
                            self.emit(Instruction::Call {
                                dest: Some(dest),
                                callee: "vanta_vec_capacity".to_string(),
                                args: vec![vec_ptr],
                                ret_type: Type::I64,
                            });
                            return dest;
                        }
                        "push" => {
                            if let Some(arg) = args.first() {
                                if elem_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                                    if let ExprKind::Ident(arg_ident) = &arg.kind {
                                        self.mark_moved(arg_ident);
                                    }
                                }
                            }
                            let val_v = self.lower_expr(&args[0]);
                            let size_v = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: size_v,
                                val: elem_size as i64,
                                ty: Type::I64,
                            });
                            self.emit(Instruction::Call {
                                dest: None,
                                callee: "vanta_vec_push".to_string(),
                                args: vec![vec_ptr, val_v, size_v],
                                ret_type: Type::Unit,
                            });
                            let dummy = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: dummy,
                                val: 0,
                                ty: Type::I64,
                            });
                            return dummy;
                        }
                        "pop" => {
                            let size_v = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: size_v,
                                val: elem_size as i64,
                                ty: Type::I64,
                            });
                            let dest = self.new_value();
                            self.emit(Instruction::Call {
                                dest: Some(dest),
                                callee: "vanta_vec_pop".to_string(),
                                args: vec![vec_ptr, size_v],
                                ret_type: elem_ty,
                            });
                            return dest;
                        }
                        "iter" => {
                            let mangled_iter = format!("Iter__{}", elem_ty.to_mangled_string());
                            let iter_ty = Type::Struct(mangled_iter);
                            let iter_slot =
                                self.new_slot(format!("_iter_tmp_{}", self.next_slot), iter_ty);
                            let iter_ptr = self.new_value();
                            self.emit(Instruction::SlotAddr {
                                dest: iter_ptr,
                                slot: iter_slot,
                            });
                            // Field 0: vec (offset 0)
                            let f0_addr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: f0_addr,
                                base_ptr: iter_ptr,
                                offset: 0,
                            });
                            self.emit(Instruction::StoreIndirect {
                                addr: f0_addr,
                                val: vec_ptr,
                            });
                            // Field 1: index (offset 8)
                            let f1_addr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: f1_addr,
                                base_ptr: iter_ptr,
                                offset: 8,
                            });
                            let zero_v = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: zero_v,
                                val: 0,
                                ty: Type::I64,
                            });
                            self.emit(Instruction::StoreIndirect {
                                addr: f1_addr,
                                val: zero_v,
                            });
                            return iter_ptr;
                        }
                        _ => {}
                    }
                }

                if target_ty.is_string()
                    || matches!(&target_ty, Type::Ref { inner, .. } if inner.is_string())
                {
                    let (s_ptr, _) = self.lower_place_addr(target);
                    match method.name.as_str() {
                        "len_bytes" => {
                            let dest = self.new_value();
                            self.emit(Instruction::Call {
                                dest: Some(dest),
                                callee: "vanta_str_len".to_string(),
                                args: vec![s_ptr],
                                ret_type: Type::I64,
                            });
                            return dest;
                        }
                        "capacity" => {
                            let dest = self.new_value();
                            self.emit(Instruction::Call {
                                dest: Some(dest),
                                callee: "vanta_str_capacity".to_string(),
                                args: vec![s_ptr],
                                ret_type: Type::I64,
                            });
                            return dest;
                        }
                        "is_empty" => {
                            let dest = self.new_value();
                            self.emit(Instruction::Call {
                                dest: Some(dest),
                                callee: "vanta_str_is_empty".to_string(),
                                args: vec![s_ptr],
                                ret_type: Type::Bool,
                            });
                            return dest;
                        }
                        "push_str" => {
                            let (text_ptr, text_len) = self.lower_slice_expr(&args[0]);
                            self.emit(Instruction::Call {
                                dest: None,
                                callee: "vanta_str_push_str".to_string(),
                                args: vec![s_ptr, text_ptr, text_len],
                                ret_type: Type::Unit,
                            });
                            let dummy = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: dummy,
                                val: 0,
                                ty: Type::I64,
                            });
                            return dummy;
                        }
                        "clear" => {
                            self.emit(Instruction::Call {
                                dest: None,
                                callee: "vanta_str_clear".to_string(),
                                args: vec![s_ptr],
                                ret_type: Type::Unit,
                            });
                            let dummy = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: dummy,
                                val: 0,
                                ty: Type::I64,
                            });
                            return dummy;
                        }
                        "clone" => {
                            let clone_slot = self.new_slot(
                                format!("_clone_str_{}", self.next_slot),
                                Type::Struct("String".into()),
                            );
                            let clone_ptr = self.new_value();
                            self.emit(Instruction::SlotAddr {
                                dest: clone_ptr,
                                slot: clone_slot,
                            });
                            self.emit(Instruction::Call {
                                dest: None,
                                callee: "vanta_str_clone".to_string(),
                                args: vec![clone_ptr, s_ptr],
                                ret_type: Type::Unit,
                            });
                            return clone_ptr;
                        }
                        "equals" => {
                            let data_addr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: data_addr,
                                base_ptr: s_ptr,
                                offset: 0,
                            });
                            let s_data = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: s_data,
                                addr: data_addr,
                                ty: Type::I64,
                            });
                            let len_addr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: len_addr,
                                base_ptr: s_ptr,
                                offset: 8,
                            });
                            let s_len = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: s_len,
                                addr: len_addr,
                                ty: Type::I64,
                            });

                            let (other_ptr, other_len) = self.lower_slice_expr(&args[0]);
                            let dest = self.new_value();
                            self.emit(Instruction::Call {
                                dest: Some(dest),
                                callee: "vanta_str_equals".to_string(),
                                args: vec![s_data, s_len, other_ptr, other_len],
                                ret_type: Type::Bool,
                            });
                            return dest;
                        }
                        "as_str" | "as_bytes" => {
                            let (ptr_v, _len_v) = self.lower_slice_expr(expr);
                            return ptr_v;
                        }
                        "slice_bytes" => {
                            let data_addr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: data_addr,
                                base_ptr: s_ptr,
                                offset: 0,
                            });
                            let s_data = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: s_data,
                                addr: data_addr,
                                ty: Type::I64,
                            });
                            let len_addr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: len_addr,
                                base_ptr: s_ptr,
                                offset: 8,
                            });
                            let s_len = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: s_len,
                                addr: len_addr,
                                ty: Type::I64,
                            });

                            let start_v = self.lower_expr(&args[0]);
                            let end_v = self.lower_expr(&args[1]);

                            let opt_slot = self.new_slot(
                                format!("_slice_opt_{}", self.next_slot),
                                Type::Enum("Option__ref_str".into()),
                            );
                            let opt_ptr = self.new_value();
                            self.emit(Instruction::SlotAddr {
                                dest: opt_ptr,
                                slot: opt_slot,
                            });
                            self.emit(Instruction::Call {
                                dest: None,
                                callee: "vanta_str_slice_bytes".to_string(),
                                args: vec![opt_ptr, s_data, s_len, start_v, end_v],
                                ret_type: Type::Unit,
                            });
                            return opt_ptr;
                        }
                        _ => {}
                    }
                }

                if target_ty.is_str_slice() {
                    match method.name.as_str() {
                        "len" => {
                            let (_ptr_v, len_v) = self.lower_slice_expr(target);
                            return len_v;
                        }
                        "is_empty" => {
                            let (_ptr_v, len_v) = self.lower_slice_expr(target);
                            let zero = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: zero,
                                val: 0,
                                ty: Type::I64,
                            });
                            let dest = self.new_value();
                            self.emit(Instruction::Binary {
                                dest,
                                op: MirBinaryOp::Eq,
                                lhs: len_v,
                                rhs: zero,
                                ty: Type::Bool,
                            });
                            return dest;
                        }
                        "as_bytes" => {
                            let (ptr_v, _len_v) = self.lower_slice_expr(target);
                            return ptr_v;
                        }
                        "equals" => {
                            let (s_data, s_len) = self.lower_slice_expr(target);
                            let (other_ptr, other_len) = self.lower_slice_expr(&args[0]);
                            let dest = self.new_value();
                            self.emit(Instruction::Call {
                                dest: Some(dest),
                                callee: "vanta_str_equals".to_string(),
                                args: vec![s_data, s_len, other_ptr, other_len],
                                ret_type: Type::Bool,
                            });
                            return dest;
                        }
                        "slice_bytes" => {
                            let (s_data, s_len) = self.lower_slice_expr(target);
                            let start_v = self.lower_expr(&args[0]);
                            let end_v = self.lower_expr(&args[1]);

                            let opt_slot = self.new_slot(
                                format!("_slice_opt_{}", self.next_slot),
                                Type::Enum("Option__ref_str".into()),
                            );
                            let opt_ptr = self.new_value();
                            self.emit(Instruction::SlotAddr {
                                dest: opt_ptr,
                                slot: opt_slot,
                            });
                            self.emit(Instruction::Call {
                                dest: None,
                                callee: "vanta_str_slice_bytes".to_string(),
                                args: vec![opt_ptr, s_data, s_len, start_v, end_v],
                                ret_type: Type::Unit,
                            });
                            return opt_ptr;
                        }
                        _ => {}
                    }
                }

                let is_slice_or_array = target_ty.is_str_slice()
                    || target_ty.is_slice()
                    || matches!(&target_ty, Type::Ref { inner, .. } if matches!(inner.as_ref(), Type::Array { .. }));

                if is_slice_or_array && method.name == "len" {
                    if let Type::Ref { inner, .. } = &target_ty {
                        if let Type::Array { len, .. } = inner.as_ref() {
                            let len_v = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: len_v,
                                val: *len as i64,
                                ty: Type::I64,
                            });
                            return len_v;
                        }
                    }
                    let (_ptr_v, len_v) = self.lower_slice_expr(target);
                    len_v
                } else if let Some(elem_ty) = self.sema.is_option_type(&match &target_ty {
                    Type::Ref { inner, .. } => (**inner).clone(),
                    _ => target_ty.clone(),
                }) {
                    let opt_ename = match &target_ty {
                        Type::Enum(name) => name.clone(),
                        Type::Ref { inner, .. } => match &**inner {
                            Type::Enum(name) => name.clone(),
                            _ => unreachable!(),
                        },
                        _ => unreachable!(),
                    };
                    let e_def = self.sema.enums.get(&opt_ename).unwrap().clone();
                    let some_v = e_def.find_variant("Some").unwrap().clone();
                    let none_v = e_def.find_variant("None").unwrap().clone();
                    let (opt_ptr, _) = self.lower_place_addr(target);

                    match method.name.as_str() {
                        "is_some" => {
                            let tag_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: tag_val,
                                addr: opt_ptr,
                                ty: Type::I32,
                            });
                            let some_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: some_tag,
                                val: some_v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: some_tag,
                                ty: Type::Bool,
                            });
                            cond
                        }
                        "is_none" => {
                            let tag_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: tag_val,
                                addr: opt_ptr,
                                ty: Type::I32,
                            });
                            let none_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: none_tag,
                                val: none_v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: none_tag,
                                ty: Type::Bool,
                            });
                            cond
                        }
                        "unwrap_or" => {
                            let default_val = self.lower_expr(&args[0]);
                            let tag_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: tag_val,
                                addr: opt_ptr,
                                ty: Type::I32,
                            });
                            let some_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: some_tag,
                                val: some_v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: some_tag,
                                ty: Type::Bool,
                            });

                            let res_slot = self.new_slot(
                                format!("_unwrap_or_res_{}", self.next_val),
                                elem_ty.clone(),
                            );
                            let res_ptr = self.new_value();
                            self.emit(Instruction::SlotAddr {
                                dest: res_ptr,
                                slot: res_slot,
                            });

                            let some_block = self.new_block("unwrap_or.some");
                            let none_block = self.new_block("unwrap_or.none");
                            let merge_block = self.new_block("unwrap_or.merge");

                            self.terminate(Terminator::CondBranch {
                                cond,
                                then_block: some_block,
                                else_block: none_block,
                            });

                            self.switch_to_block(some_block);
                            let some_payload_ptr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: some_payload_ptr,
                                base_ptr: opt_ptr,
                                offset: some_v.payload_offsets[0],
                            });
                            if matches!(elem_ty, Type::Struct(_) | Type::Enum(_)) {
                                if let Type::Struct(s) = &elem_ty {
                                    self.copy_struct_bytes(some_payload_ptr, res_ptr, s);
                                } else if let Type::Enum(e) = &elem_ty {
                                    self.copy_enum_bytes(some_payload_ptr, res_ptr, e);
                                }
                            } else {
                                let some_val = self.new_value();
                                self.emit(Instruction::LoadIndirect {
                                    dest: some_val,
                                    addr: some_payload_ptr,
                                    ty: elem_ty.clone(),
                                });
                                self.emit(Instruction::StoreIndirect {
                                    addr: res_ptr,
                                    val: some_val,
                                });
                            }
                            self.terminate(Terminator::Branch(merge_block));

                            self.switch_to_block(none_block);
                            if matches!(elem_ty, Type::Struct(_) | Type::Enum(_)) {
                                if let Type::Struct(s) = &elem_ty {
                                    self.copy_struct_bytes(default_val, res_ptr, s);
                                } else if let Type::Enum(e) = &elem_ty {
                                    self.copy_enum_bytes(default_val, res_ptr, e);
                                }
                            } else {
                                self.emit(Instruction::StoreIndirect {
                                    addr: res_ptr,
                                    val: default_val,
                                });
                            }
                            self.terminate(Terminator::Branch(merge_block));

                            self.switch_to_block(merge_block);
                            if matches!(elem_ty, Type::Struct(_) | Type::Enum(_)) {
                                res_ptr
                            } else {
                                let final_val = self.new_value();
                                self.emit(Instruction::LoadIndirect {
                                    dest: final_val,
                                    addr: res_ptr,
                                    ty: elem_ty,
                                });
                                final_val
                            }
                        }
                        "unwrap" => {
                            let tag_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: tag_val,
                                addr: opt_ptr,
                                ty: Type::I32,
                            });
                            let some_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: some_tag,
                                val: some_v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: some_tag,
                                ty: Type::Bool,
                            });

                            let some_block = self.new_block("unwrap.some");
                            let panic_block = self.new_block("unwrap.panic");
                            self.terminate(Terminator::CondBranch {
                                cond,
                                then_block: some_block,
                                else_block: panic_block,
                            });

                            self.switch_to_block(panic_block);
                            let msg = "called Option::unwrap() on a None value";
                            let symbol = self.intern_string(msg);
                            let ptr_v = self.new_value();
                            let len_v = self.new_value();
                            self.emit(Instruction::StrSlice {
                                dest_ptr: ptr_v,
                                dest_len: len_v,
                                symbol,
                                len: msg.len(),
                            });
                            self.emit(Instruction::Call {
                                dest: None,
                                callee: "vanta_panic".to_string(),
                                args: vec![ptr_v, len_v],
                                ret_type: Type::Unit,
                            });
                            self.terminate(Terminator::Unreachable);

                            self.switch_to_block(some_block);
                            let some_payload_ptr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: some_payload_ptr,
                                base_ptr: opt_ptr,
                                offset: some_v.payload_offsets[0],
                            });
                            if matches!(elem_ty, Type::Struct(_) | Type::Enum(_)) {
                                let slot = self.new_slot(
                                    format!("_unwrap_res_{}", self.next_val),
                                    elem_ty.clone(),
                                );
                                let res_ptr = self.new_value();
                                self.emit(Instruction::SlotAddr {
                                    dest: res_ptr,
                                    slot,
                                });
                                if let Type::Struct(s) = &elem_ty {
                                    self.copy_struct_bytes(some_payload_ptr, res_ptr, s);
                                } else if let Type::Enum(e) = &elem_ty {
                                    self.copy_enum_bytes(some_payload_ptr, res_ptr, e);
                                }
                                res_ptr
                            } else {
                                let val = self.new_value();
                                self.emit(Instruction::LoadIndirect {
                                    dest: val,
                                    addr: some_payload_ptr,
                                    ty: elem_ty,
                                });
                                val
                            }
                        }
                        _ => panic!("Unknown method on Option: {}", method.name),
                    }
                } else if let Some((t_ty, _e_ty)) = self.sema.is_result_type(&match &target_ty {
                    Type::Ref { inner, .. } => (**inner).clone(),
                    _ => target_ty.clone(),
                }) {
                    let res_ename = match &target_ty {
                        Type::Enum(name) => name.clone(),
                        Type::Ref { inner, .. } => match &**inner {
                            Type::Enum(name) => name.clone(),
                            _ => unreachable!(),
                        },
                        _ => unreachable!(),
                    };
                    let e_def = self.sema.enums.get(&res_ename).unwrap().clone();
                    let ok_v = e_def.find_variant("Ok").unwrap().clone();
                    let err_v = e_def.find_variant("Err").unwrap().clone();
                    let (res_ptr, _) = self.lower_place_addr(target);

                    match method.name.as_str() {
                        "is_ok" => {
                            let tag_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: tag_val,
                                addr: res_ptr,
                                ty: Type::I32,
                            });
                            let ok_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: ok_tag,
                                val: ok_v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: ok_tag,
                                ty: Type::Bool,
                            });
                            cond
                        }
                        "is_err" => {
                            let tag_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: tag_val,
                                addr: res_ptr,
                                ty: Type::I32,
                            });
                            let err_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: err_tag,
                                val: err_v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: err_tag,
                                ty: Type::Bool,
                            });
                            cond
                        }
                        "unwrap_or" => {
                            let default_val = self.lower_expr(&args[0]);
                            let tag_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: tag_val,
                                addr: res_ptr,
                                ty: Type::I32,
                            });
                            let ok_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: ok_tag,
                                val: ok_v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: ok_tag,
                                ty: Type::Bool,
                            });

                            let res_slot = self.new_slot(
                                format!("_unwrap_or_res_{}", self.next_val),
                                t_ty.clone(),
                            );
                            let final_ptr = self.new_value();
                            self.emit(Instruction::SlotAddr {
                                dest: final_ptr,
                                slot: res_slot,
                            });

                            let ok_block = self.new_block("unwrap_or.ok");
                            let err_block = self.new_block("unwrap_or.err");
                            let merge_block = self.new_block("unwrap_or.merge");

                            self.terminate(Terminator::CondBranch {
                                cond,
                                then_block: ok_block,
                                else_block: err_block,
                            });

                            self.switch_to_block(ok_block);
                            let ok_payload_ptr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: ok_payload_ptr,
                                base_ptr: res_ptr,
                                offset: ok_v.payload_offsets[0],
                            });
                            if matches!(t_ty, Type::Struct(_) | Type::Enum(_)) {
                                if let Type::Struct(s) = &t_ty {
                                    self.copy_struct_bytes(ok_payload_ptr, final_ptr, s);
                                } else if let Type::Enum(e) = &t_ty {
                                    self.copy_enum_bytes(ok_payload_ptr, final_ptr, e);
                                }
                            } else {
                                let ok_val = self.new_value();
                                self.emit(Instruction::LoadIndirect {
                                    dest: ok_val,
                                    addr: ok_payload_ptr,
                                    ty: t_ty.clone(),
                                });
                                self.emit(Instruction::StoreIndirect {
                                    addr: final_ptr,
                                    val: ok_val,
                                });
                            }
                            self.terminate(Terminator::Branch(merge_block));

                            self.switch_to_block(err_block);
                            if matches!(t_ty, Type::Struct(_) | Type::Enum(_)) {
                                if let Type::Struct(s) = &t_ty {
                                    self.copy_struct_bytes(default_val, final_ptr, s);
                                } else if let Type::Enum(e) = &t_ty {
                                    self.copy_enum_bytes(default_val, final_ptr, e);
                                }
                            } else {
                                self.emit(Instruction::StoreIndirect {
                                    addr: final_ptr,
                                    val: default_val,
                                });
                            }
                            self.terminate(Terminator::Branch(merge_block));

                            self.switch_to_block(merge_block);
                            if matches!(t_ty, Type::Struct(_) | Type::Enum(_)) {
                                final_ptr
                            } else {
                                let final_val = self.new_value();
                                self.emit(Instruction::LoadIndirect {
                                    dest: final_val,
                                    addr: final_ptr,
                                    ty: t_ty,
                                });
                                final_val
                            }
                        }
                        "unwrap" => {
                            let tag_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: tag_val,
                                addr: res_ptr,
                                ty: Type::I32,
                            });
                            let ok_tag = self.new_value();
                            self.emit(Instruction::ConstInt {
                                dest: ok_tag,
                                val: ok_v.tag as i64,
                                ty: Type::I32,
                            });
                            let cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: cond,
                                op: MirBinaryOp::Eq,
                                lhs: tag_val,
                                rhs: ok_tag,
                                ty: Type::Bool,
                            });

                            let ok_block = self.new_block("unwrap.ok");
                            let panic_block = self.new_block("unwrap.panic");
                            self.terminate(Terminator::CondBranch {
                                cond,
                                then_block: ok_block,
                                else_block: panic_block,
                            });

                            self.switch_to_block(panic_block);
                            let msg = "called Result::unwrap() on an Err value";
                            let symbol = self.intern_string(msg);
                            let ptr_v = self.new_value();
                            let len_v = self.new_value();
                            self.emit(Instruction::StrSlice {
                                dest_ptr: ptr_v,
                                dest_len: len_v,
                                symbol,
                                len: msg.len(),
                            });
                            self.emit(Instruction::Call {
                                dest: None,
                                callee: "vanta_panic".to_string(),
                                args: vec![ptr_v, len_v],
                                ret_type: Type::Unit,
                            });
                            self.terminate(Terminator::Unreachable);

                            self.switch_to_block(ok_block);
                            let ok_payload_ptr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: ok_payload_ptr,
                                base_ptr: res_ptr,
                                offset: ok_v.payload_offsets[0],
                            });
                            if matches!(t_ty, Type::Struct(_) | Type::Enum(_)) {
                                let slot = self.new_slot(
                                    format!("_unwrap_res_{}", self.next_val),
                                    t_ty.clone(),
                                );
                                let res_ptr = self.new_value();
                                self.emit(Instruction::SlotAddr {
                                    dest: res_ptr,
                                    slot,
                                });
                                if let Type::Struct(s) = &t_ty {
                                    self.copy_struct_bytes(ok_payload_ptr, res_ptr, s);
                                } else if let Type::Enum(e) = &t_ty {
                                    self.copy_enum_bytes(ok_payload_ptr, res_ptr, e);
                                }
                                res_ptr
                            } else {
                                let val = self.new_value();
                                self.emit(Instruction::LoadIndirect {
                                    dest: val,
                                    addr: ok_payload_ptr,
                                    ty: t_ty,
                                });
                                val
                            }
                        }
                        _ => panic!("Unknown method on Result: {}", method.name),
                    }
                } else {
                    let type_name = match &target_ty {
                        Type::Struct(s) => s.clone(),
                        Type::Enum(e) => e.clone(),
                        Type::Ref { inner, .. } => match &**inner {
                            Type::Struct(s) => s.clone(),
                            Type::Enum(e) => e.clone(),
                            _ => panic!(
                                "Method call '{}' on non-struct/enum type: {:?}",
                                method.name, target_ty
                            ),
                        },
                        _ => panic!(
                            "Method call '{}' on non-struct/enum type: {:?}",
                            method.name, target_ty
                        ),
                    };

                    let mangled = if let Some(mangled) = self
                        .sema
                        .methods
                        .get(&(type_name.clone(), method.name.clone()))
                    {
                        mangled.clone()
                    } else if let Some((_, impl_def)) =
                        self.sema
                            .trait_impls
                            .iter()
                            .find(|((_, target_canon), impl_def)| {
                                target_canon == &type_name
                                    && impl_def.methods.contains_key(&method.name)
                            })
                    {
                        impl_def.methods[&method.name].clone()
                    } else if let Some(mangled) = self.sema.call_callees.get(&method.span) {
                        mangled.clone()
                    } else {
                        panic!("Method '{}' not found on '{}'", method.name, type_name);
                    };

                    let sig = self.sema.functions.get(&mangled).unwrap().clone();

                    let mut arg_vals = Vec::new();
                    // First arg is receiver pointer
                    let (receiver_ptr, _) = self.lower_place_addr(target);
                    arg_vals.push(receiver_ptr);

                    // Other args
                    for (idx, arg) in args.iter().enumerate() {
                        let param_is_slice = sig
                            .params
                            .get(idx + 1)
                            .map(|p| p.1.is_str_slice() || p.1.is_slice())
                            .unwrap_or(false);
                        if param_is_slice {
                            let (ptr_v, len_v) = self.lower_slice_expr(arg);
                            arg_vals.push(ptr_v);
                            arg_vals.push(len_v);
                        } else {
                            arg_vals.push(self.lower_expr(arg));
                        }
                    }

                    let dest = if sig.ret_type == Type::Unit {
                        None
                    } else {
                        Some(self.new_value())
                    };

                    self.emit(Instruction::Call {
                        dest,
                        callee: mangled,
                        args: arg_vals,
                        ret_type: sig.ret_type,
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
            }
        };
        self.current_span = prev_span;
        res
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
            ExprKind::Ref { expr: inner, .. } => {
                let (ptr, ty) = self.lower_place_addr(inner);
                if let Type::Array { len, .. } = ty {
                    let len_v = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest: len_v,
                        val: len as i64,
                        ty: Type::I64,
                    });
                    (ptr, len_v)
                } else {
                    let zero = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest: zero,
                        val: 0,
                        ty: Type::I64,
                    });
                    (ptr, zero)
                }
            }
            ExprKind::MethodCall { target, method, .. }
                if method.name == "as_str" || method.name == "as_bytes" =>
            {
                let (target_ptr, target_ty) = self.lower_place_addr(target);
                if target_ty.is_string() {
                    let data_addr = self.new_value();
                    self.emit(Instruction::FieldAddr {
                        dest: data_addr,
                        base_ptr: target_ptr,
                        offset: 0,
                    });
                    let dest_ptr = self.new_value();
                    self.emit(Instruction::LoadIndirect {
                        dest: dest_ptr,
                        addr: data_addr,
                        ty: Type::I64,
                    });
                    let len_addr = self.new_value();
                    self.emit(Instruction::FieldAddr {
                        dest: len_addr,
                        base_ptr: target_ptr,
                        offset: 8,
                    });
                    let dest_len = self.new_value();
                    self.emit(Instruction::LoadIndirect {
                        dest: dest_len,
                        addr: len_addr,
                        ty: Type::I64,
                    });
                    (dest_ptr, dest_len)
                } else {
                    self.lower_slice_expr(target)
                }
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

    fn copy_bytes(&mut self, src_ptr: ValueId, dest_ptr: ValueId, total_size: usize) {
        for offset in (0..total_size).step_by(8) {
            let src_field = self.new_value();
            self.emit(Instruction::FieldAddr {
                dest: src_field,
                base_ptr: src_ptr,
                offset,
            });
            let val_v = self.new_value();
            self.emit(Instruction::LoadIndirect {
                dest: val_v,
                addr: src_field,
                ty: Type::I64,
            });
            let dst_field = self.new_value();
            self.emit(Instruction::FieldAddr {
                dest: dst_field,
                base_ptr: dest_ptr,
                offset,
            });
            self.emit(Instruction::StoreIndirect {
                addr: dst_field,
                val: val_v,
            });
        }
    }

    fn copy_struct_bytes(&mut self, src_ptr: ValueId, dest_ptr: ValueId, s_name: &str) {
        let total_size = if s_name == "Vec"
            || s_name.starts_with("Vec__")
            || s_name == "String"
            || s_name.ends_with("String")
        {
            24
        } else {
            self.sema
                .structs
                .get(s_name)
                .map(|s| s.total_size)
                .unwrap_or(8)
        };
        self.copy_bytes(src_ptr, dest_ptr, total_size);
    }

    fn copy_enum_bytes(&mut self, src_ptr: ValueId, dest_ptr: ValueId, e_name: &str) {
        let total_size = self
            .sema
            .enums
            .get(e_name)
            .map(|e| e.total_size)
            .unwrap_or(8);
        self.copy_bytes(src_ptr, dest_ptr, total_size);
    }

    fn lower_init_variant(
        &mut self,
        base_ptr: ValueId,
        enum_name: &str,
        variant_name: &str,
        args: &[Expr],
    ) {
        let e_def = self
            .sema
            .enums
            .get(enum_name)
            .cloned()
            .unwrap_or_else(|| panic!("Enum '{}' not found in sema.enums", enum_name));
        let v_def = e_def
            .find_variant(variant_name)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "Variant '{}' not found in enum '{}'",
                    variant_name, enum_name
                )
            });

        // 1. Lower and store payload fields
        for (idx, arg) in args.iter().enumerate() {
            let f_ty = &v_def.payload[idx];
            if f_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                if let ExprKind::Ident(src_name) = &arg.kind {
                    self.mark_moved(src_name);
                }
            }
            let f_offset = v_def.payload_offsets[idx];
            let field_ptr = self.new_value();
            self.emit(Instruction::FieldAddr {
                dest: field_ptr,
                base_ptr,
                offset: f_offset,
            });

            if let Type::Struct(nested_name) = f_ty {
                if let ExprKind::StructLiteral {
                    fields: nested_fields,
                    ..
                } = &arg.kind
                {
                    self.lower_init_struct_fields(field_ptr, nested_name, nested_fields);
                } else {
                    let (src_ptr, _) = self.lower_place_addr(arg);
                    self.copy_struct_bytes(src_ptr, field_ptr, nested_name);
                }
            } else if let Type::Enum(nested_ename) = f_ty {
                if let ExprKind::VariantConstructor {
                    variant: v_ident,
                    args: v_args,
                    ..
                } = &arg.kind
                {
                    self.lower_init_variant(field_ptr, nested_ename, &v_ident.name, v_args);
                } else {
                    let (src_ptr, _) = self.lower_place_addr(arg);
                    self.copy_enum_bytes(src_ptr, field_ptr, nested_ename);
                }
            } else if f_ty.is_str_slice() || f_ty.is_slice() {
                let (ptr_v, len_v) = self.lower_slice_expr(arg);
                self.emit(Instruction::StoreIndirect {
                    addr: field_ptr,
                    val: ptr_v,
                });
                let len_ptr = self.new_value();
                self.emit(Instruction::FieldAddr {
                    dest: len_ptr,
                    base_ptr,
                    offset: f_offset + 8,
                });
                self.emit(Instruction::StoreIndirect {
                    addr: len_ptr,
                    val: len_v,
                });
            } else {
                let f_val = self.lower_expr(arg);
                self.emit(Instruction::StoreIndirect {
                    addr: field_ptr,
                    val: f_val,
                });
            }
        }

        // 2. Write 32-bit tag at offset 0
        let tag_val = self.new_value();
        self.emit(Instruction::ConstInt {
            dest: tag_val,
            val: v_def.tag as i64,
            ty: Type::I32,
        });
        let tag_ptr = self.new_value();
        self.emit(Instruction::FieldAddr {
            dest: tag_ptr,
            base_ptr,
            offset: 0,
        });
        self.emit(Instruction::StoreIndirect32 {
            addr: tag_ptr,
            val: tag_val,
        });
    }

    fn lower_init_struct_fields(
        &mut self,
        base_ptr: ValueId,
        s_name: &str,
        fields: &[(Ident, Expr)],
    ) {
        let s_def = self.sema.structs[s_name].clone();
        for (f_ident, f_expr) in fields {
            let f_def = s_def.find_field(&f_ident.name).unwrap();
            if f_def.ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                if let ExprKind::Ident(src_name) = &f_expr.kind {
                    self.mark_moved(src_name);
                }
            }
            let field_ptr = self.new_value();
            self.emit(Instruction::FieldAddr {
                dest: field_ptr,
                base_ptr,
                offset: f_def.offset,
            });

            if let Type::Struct(nested_name) = &f_def.ty {
                if let ExprKind::StructLiteral {
                    fields: nested_fields,
                    ..
                } = &f_expr.kind
                {
                    self.lower_init_struct_fields(field_ptr, nested_name, nested_fields);
                } else {
                    let (src_ptr, _) = self.lower_place_addr(f_expr);
                    self.copy_struct_bytes(src_ptr, field_ptr, nested_name);
                }
            } else if let Type::Enum(nested_ename) = &f_def.ty {
                if let ExprKind::VariantConstructor {
                    variant: v_ident,
                    args: v_args,
                    ..
                } = &f_expr.kind
                {
                    self.lower_init_variant(field_ptr, nested_ename, &v_ident.name, v_args);
                } else {
                    let (src_ptr, _) = self.lower_place_addr(f_expr);
                    self.copy_enum_bytes(src_ptr, field_ptr, nested_ename);
                }
            } else if f_def.ty.is_str_slice() || f_def.ty.is_slice() {
                let (ptr_v, len_v) = self.lower_slice_expr(f_expr);
                self.emit(Instruction::StoreIndirect {
                    addr: field_ptr,
                    val: ptr_v,
                });
                let len_ptr = self.new_value();
                self.emit(Instruction::FieldAddr {
                    dest: len_ptr,
                    base_ptr,
                    offset: f_def.offset + 8,
                });
                self.emit(Instruction::StoreIndirect {
                    addr: len_ptr,
                    val: len_v,
                });
            } else {
                let f_val = self.lower_expr(f_expr);
                self.emit(Instruction::StoreIndirect {
                    addr: field_ptr,
                    val: f_val,
                });
            }
        }
    }

    fn lower_call_internal(&mut self, callee: &str, args: &[Expr], _span: Span) -> ValueId {
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

        let sig_opt = self.sema.functions.get(callee).cloned();
        let mut arg_vals = Vec::new();
        for (idx, arg) in args.iter().enumerate() {
            let mut is_slice = false;
            let mut is_by_value_aggregate = false;
            if let Some(sig) = &sig_opt {
                if let Some((_, pty, _)) = sig.params.get(idx) {
                    if pty.is_str_slice() || pty.is_slice() {
                        is_slice = true;
                    }
                    if matches!(pty, Type::Struct(_) | Type::Enum(_)) {
                        is_by_value_aggregate = true;
                    }
                    if !matches!(pty, Type::Ref { .. })
                        && pty.needs_drop(&self.sema.structs, &self.sema.enums)
                    {
                        if let ExprKind::Ident(arg_ident) = &arg.kind {
                            self.mark_moved(arg_ident);
                        }
                    }
                }
            }
            if !is_slice && !is_by_value_aggregate {
                if let ExprKind::String(_) = &arg.kind {
                    is_slice = true;
                } else if let ExprKind::Ident(name) = &arg.kind {
                    if let Some(slot) = self.lookup_slot(name) {
                        if let Some(s) = self.current_slots.iter().find(|s| s.id == slot) {
                            if s.ty.is_str_slice() || s.ty.is_slice() {
                                is_slice = true;
                            } else if matches!(&s.ty, Type::Struct(_) | Type::Enum(_)) {
                                is_by_value_aggregate = true;
                            }
                        }
                    }
                }
            }

            if is_slice {
                let (ptr_v, len_v) = self.lower_slice_expr(arg);
                arg_vals.push(ptr_v);
                arg_vals.push(len_v);
            } else if is_by_value_aggregate {
                let (ptr_v, _) = self.lower_place_addr(arg);
                arg_vals.push(ptr_v);
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
            callee: callee.to_string(),
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

    fn lower_match(&mut self, scrutinee: &Expr, arms: &[MatchArm], match_expr: &Expr) -> ValueId {
        let scrut_ty = self
            .sema
            .expr_types
            .get(&(scrutinee as *const Expr))
            .cloned()
            .unwrap_or(Type::I64);

        if scrut_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
            if let ExprKind::Ident(src_name) = &scrutinee.kind {
                self.mark_moved(src_name);
            }
        }

        let scrut_slot = self.new_slot(format!("_match_scrut_{}", self.next_val), scrut_ty.clone());
        let scrut_ptr = self.new_value();
        self.emit(Instruction::SlotAddr {
            dest: scrut_ptr,
            slot: scrut_slot,
        });

        if let Type::Struct(sname) = &scrut_ty {
            if let ExprKind::StructLiteral { fields, .. } = &scrutinee.kind {
                self.lower_init_struct_fields(scrut_ptr, sname, fields);
            } else {
                let (src_ptr, _) = self.lower_place_addr(scrutinee);
                self.copy_struct_bytes(src_ptr, scrut_ptr, sname);
            }
        } else if let Type::Enum(ename) = &scrut_ty {
            if let ExprKind::VariantConstructor {
                variant: v_ident,
                args: v_args,
                ..
            } = &scrutinee.kind
            {
                self.lower_init_variant(scrut_ptr, ename, &v_ident.name, v_args);
            } else {
                let (src_ptr, _) = self.lower_place_addr(scrutinee);
                self.copy_enum_bytes(src_ptr, scrut_ptr, ename);
            }
        } else if scrut_ty.is_str_slice() || scrut_ty.is_slice() {
            let (ptr_v, len_v) = self.lower_slice_expr(scrutinee);
            self.emit(Instruction::StoreSlice {
                slot: scrut_slot,
                ptr: ptr_v,
                len: len_v,
            });
        } else {
            let val = self.lower_expr(scrutinee);
            self.emit(Instruction::Store {
                slot: scrut_slot,
                val,
            });
        }

        let match_ret_ty = self
            .sema
            .expr_types
            .get(&(match_expr as *const Expr))
            .cloned()
            .unwrap_or(Type::Unit);

        let result_slot = if match_ret_ty != Type::Unit {
            Some(self.new_slot(
                format!("_match_res_{}", self.next_val),
                match_ret_ty.clone(),
            ))
        } else {
            None
        };

        let result_ptr = if let Some(slot) = result_slot {
            if matches!(match_ret_ty, Type::Struct(_) | Type::Enum(_)) {
                let ptr = self.new_value();
                self.emit(Instruction::SlotAddr { dest: ptr, slot });
                Some(ptr)
            } else {
                None
            }
        } else {
            None
        };

        let merge_block = self.new_block("match.merge");

        for (i, arm) in arms.iter().enumerate() {
            let is_last_arm = i == arms.len() - 1;
            let arm_body_block = self.new_block(format!("match.arm_{}.body", i));
            let next_arm_block = if is_last_arm {
                merge_block
            } else {
                self.new_block(format!("match.arm_{}.test", i + 1))
            };

            // Test pattern
            match &arm.pattern.kind {
                PatternKind::Wildcard | PatternKind::Ident(_) => {
                    self.terminate(Terminator::Branch(arm_body_block));
                }
                PatternKind::Bool(b) => {
                    let val = self.new_value();
                    self.emit(Instruction::Load {
                        dest: val,
                        slot: scrut_slot,
                        ty: Type::Bool,
                    });
                    let expected = self.new_value();
                    self.emit(Instruction::ConstBool {
                        dest: expected,
                        val: *b,
                    });
                    let cond = self.new_value();
                    self.emit(Instruction::Binary {
                        dest: cond,
                        op: MirBinaryOp::Eq,
                        lhs: val,
                        rhs: expected,
                        ty: Type::Bool,
                    });
                    self.terminate(Terminator::CondBranch {
                        cond,
                        then_block: arm_body_block,
                        else_block: next_arm_block,
                    });
                }
                PatternKind::Int(n) => {
                    let val = self.new_value();
                    self.emit(Instruction::Load {
                        dest: val,
                        slot: scrut_slot,
                        ty: Type::I64,
                    });
                    let expected = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest: expected,
                        val: *n,
                        ty: Type::I64,
                    });
                    let cond = self.new_value();
                    self.emit(Instruction::Binary {
                        dest: cond,
                        op: MirBinaryOp::Eq,
                        lhs: val,
                        rhs: expected,
                        ty: Type::Bool,
                    });
                    self.terminate(Terminator::CondBranch {
                        cond,
                        then_block: arm_body_block,
                        else_block: next_arm_block,
                    });
                }
                PatternKind::Variant {
                    variant,
                    subpatterns,
                    ..
                } => {
                    let Type::Enum(ename) = &scrut_ty else {
                        unreachable!()
                    };
                    let e_def = self.sema.enums[ename].clone();
                    let v_def = e_def.find_variant(&variant.name).unwrap().clone();

                    let tag_ptr = self.new_value();
                    self.emit(Instruction::FieldAddr {
                        dest: tag_ptr,
                        base_ptr: scrut_ptr,
                        offset: 0,
                    });
                    let tag_val = self.new_value();
                    self.emit(Instruction::LoadIndirect {
                        dest: tag_val,
                        addr: tag_ptr,
                        ty: Type::I32,
                    });
                    let expected_tag = self.new_value();
                    self.emit(Instruction::ConstInt {
                        dest: expected_tag,
                        val: v_def.tag as i64,
                        ty: Type::I32,
                    });
                    let cond = self.new_value();
                    self.emit(Instruction::Binary {
                        dest: cond,
                        op: MirBinaryOp::Eq,
                        lhs: tag_val,
                        rhs: expected_tag,
                        ty: Type::Bool,
                    });

                    let has_literal_subpatterns = subpatterns
                        .iter()
                        .any(|p| matches!(p.kind, PatternKind::Bool(_) | PatternKind::Int(_)));

                    if has_literal_subpatterns {
                        let sub_test_block = self.new_block(format!("match.arm_{}.subtest", i));
                        self.terminate(Terminator::CondBranch {
                            cond,
                            then_block: sub_test_block,
                            else_block: next_arm_block,
                        });

                        self.switch_to_block(sub_test_block);
                        for (sub_idx, subpat) in subpatterns.iter().enumerate() {
                            let (exp_val, bin_ty) = match subpat.kind {
                                PatternKind::Bool(b) => {
                                    let v = self.new_value();
                                    self.emit(Instruction::ConstBool { dest: v, val: b });
                                    (v, Type::Bool)
                                }
                                PatternKind::Int(n) => {
                                    let v = self.new_value();
                                    self.emit(Instruction::ConstInt {
                                        dest: v,
                                        val: n,
                                        ty: Type::I64,
                                    });
                                    (v, Type::I64)
                                }
                                _ => continue,
                            };

                            let f_offset = v_def.payload_offsets[sub_idx];
                            let f_ty = &v_def.payload[sub_idx];
                            let f_ptr = self.new_value();
                            self.emit(Instruction::FieldAddr {
                                dest: f_ptr,
                                base_ptr: scrut_ptr,
                                offset: f_offset,
                            });
                            let f_val = self.new_value();
                            self.emit(Instruction::LoadIndirect {
                                dest: f_val,
                                addr: f_ptr,
                                ty: f_ty.clone(),
                            });

                            let sub_cond = self.new_value();
                            self.emit(Instruction::Binary {
                                dest: sub_cond,
                                op: MirBinaryOp::Eq,
                                lhs: f_val,
                                rhs: exp_val,
                                ty: bin_ty,
                            });

                            let next_sub =
                                self.new_block(format!("match.arm_{}.sub_{}", i, sub_idx + 1));
                            self.terminate(Terminator::CondBranch {
                                cond: sub_cond,
                                then_block: next_sub,
                                else_block: next_arm_block,
                            });
                            self.switch_to_block(next_sub);
                        }
                        self.terminate(Terminator::Branch(arm_body_block));
                    } else {
                        self.terminate(Terminator::CondBranch {
                            cond,
                            then_block: arm_body_block,
                            else_block: next_arm_block,
                        });
                    }
                }
            }

            // Arm body
            self.switch_to_block(arm_body_block);
            self.var_map.push(HashMap::new());
            self.drop_scopes.push(Vec::new());

            // Bind pattern variables
            match &arm.pattern.kind {
                PatternKind::Ident(name) => {
                    let b_slot = self.new_slot(name.clone(), scrut_ty.clone());
                    if scrut_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                        self.drop_scopes.last_mut().unwrap().push(DropCandidate {
                            slot: b_slot,
                            ty: scrut_ty.clone(),
                            name: name.clone(),
                            is_moved: false,
                        });
                    }
                    if matches!(scrut_ty, Type::Struct(_) | Type::Enum(_)) {
                        let dst_ptr = self.new_value();
                        self.emit(Instruction::SlotAddr {
                            dest: dst_ptr,
                            slot: b_slot,
                        });
                        self.copy_bytes(
                            scrut_ptr,
                            dst_ptr,
                            scrut_ty.size_with_context(&self.sema.structs, &self.sema.enums),
                        );
                    } else if scrut_ty.is_str_slice() || scrut_ty.is_slice() {
                        let ptr_v = self.new_value();
                        let len_v = self.new_value();
                        self.emit(Instruction::LoadSlice {
                            dest_ptr: ptr_v,
                            dest_len: len_v,
                            slot: scrut_slot,
                        });
                        self.emit(Instruction::StoreSlice {
                            slot: b_slot,
                            ptr: ptr_v,
                            len: len_v,
                        });
                    } else {
                        let val = self.new_value();
                        self.emit(Instruction::Load {
                            dest: val,
                            slot: scrut_slot,
                            ty: scrut_ty.clone(),
                        });
                        self.emit(Instruction::Store { slot: b_slot, val });
                    }
                }
                PatternKind::Variant {
                    variant,
                    subpatterns,
                    ..
                } => {
                    let Type::Enum(ename) = &scrut_ty else {
                        unreachable!()
                    };
                    let e_def = self.sema.enums[ename].clone();
                    let v_def = e_def.find_variant(&variant.name).unwrap().clone();

                    for (sub_idx, subpat) in subpatterns.iter().enumerate() {
                        let f_ty = &v_def.payload[sub_idx];
                        let f_offset = v_def.payload_offsets[sub_idx];
                        let field_ptr = self.new_value();
                        self.emit(Instruction::FieldAddr {
                            dest: field_ptr,
                            base_ptr: scrut_ptr,
                            offset: f_offset,
                        });

                        match &subpat.kind {
                            PatternKind::Ident(name) => {
                                let b_slot = self.new_slot(name.clone(), f_ty.clone());
                                if f_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                                    self.drop_scopes.last_mut().unwrap().push(DropCandidate {
                                        slot: b_slot,
                                        ty: f_ty.clone(),
                                        name: name.clone(),
                                        is_moved: false,
                                    });
                                }
                                if matches!(f_ty, Type::Struct(_) | Type::Enum(_)) {
                                    let dst_ptr = self.new_value();
                                    self.emit(Instruction::SlotAddr {
                                        dest: dst_ptr,
                                        slot: b_slot,
                                    });
                                    self.copy_bytes(
                                        field_ptr,
                                        dst_ptr,
                                        f_ty.size_with_context(
                                            &self.sema.structs,
                                            &self.sema.enums,
                                        ),
                                    );
                                } else if f_ty.is_str_slice() || f_ty.is_slice() {
                                    let ptr_v = self.new_value();
                                    self.emit(Instruction::LoadIndirect {
                                        dest: ptr_v,
                                        addr: field_ptr,
                                        ty: Type::I64,
                                    });
                                    let len_ptr = self.new_value();
                                    self.emit(Instruction::FieldAddr {
                                        dest: len_ptr,
                                        base_ptr: scrut_ptr,
                                        offset: f_offset + 8,
                                    });
                                    let len_v = self.new_value();
                                    self.emit(Instruction::LoadIndirect {
                                        dest: len_v,
                                        addr: len_ptr,
                                        ty: Type::I64,
                                    });
                                    self.emit(Instruction::StoreSlice {
                                        slot: b_slot,
                                        ptr: ptr_v,
                                        len: len_v,
                                    });
                                } else {
                                    let val = self.new_value();
                                    self.emit(Instruction::LoadIndirect {
                                        dest: val,
                                        addr: field_ptr,
                                        ty: f_ty.clone(),
                                    });
                                    self.emit(Instruction::Store { slot: b_slot, val });
                                }
                            }
                            PatternKind::Wildcard => {
                                if f_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
                                    let temp_name = format!("_wildcard_{}", self.next_val);
                                    let temp_slot = self.new_slot(temp_name.clone(), f_ty.clone());
                                    self.drop_scopes.last_mut().unwrap().push(DropCandidate {
                                        slot: temp_slot,
                                        ty: f_ty.clone(),
                                        name: temp_name,
                                        is_moved: false,
                                    });
                                    let dst_ptr = self.new_value();
                                    self.emit(Instruction::SlotAddr {
                                        dest: dst_ptr,
                                        slot: temp_slot,
                                    });
                                    self.copy_bytes(
                                        field_ptr,
                                        dst_ptr,
                                        f_ty.size_with_context(
                                            &self.sema.structs,
                                            &self.sema.enums,
                                        ),
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }

            // Lower arm body
            let arm_val = self.lower_expr(&arm.body);
            if let Some(r_slot) = result_slot {
                if !self.is_current_terminated() {
                    if matches!(match_ret_ty, Type::Struct(_) | Type::Enum(_)) {
                        let (src_ptr, _) = self.lower_place_addr(&arm.body);
                        let dst_ptr = result_ptr.unwrap();
                        self.copy_bytes(
                            src_ptr,
                            dst_ptr,
                            match_ret_ty.size_with_context(&self.sema.structs, &self.sema.enums),
                        );
                    } else if match_ret_ty.is_str_slice() || match_ret_ty.is_slice() {
                        let (ptr_v, len_v) = self.lower_slice_expr(&arm.body);
                        self.emit(Instruction::StoreSlice {
                            slot: r_slot,
                            ptr: ptr_v,
                            len: len_v,
                        });
                    } else {
                        self.emit(Instruction::Store {
                            slot: r_slot,
                            val: arm_val,
                        });
                    }
                }
            }

            let scope_candidates = self.drop_scopes.pop().unwrap_or_default();
            if !self.is_current_terminated() {
                for candidate in scope_candidates.iter().rev() {
                    if !candidate.is_moved {
                        let ptr = self.new_value();
                        self.emit(Instruction::SlotAddr {
                            dest: ptr,
                            slot: candidate.slot,
                        });
                        self.emit_drop_for_place(ptr, &candidate.ty);
                    }
                }
            }
            self.var_map.pop();

            if !self.is_current_terminated() {
                self.terminate(Terminator::Branch(merge_block));
            }

            if !is_last_arm {
                self.switch_to_block(next_arm_block);
            }
        }

        self.switch_to_block(merge_block);

        if let Some(r_slot) = result_slot {
            if matches!(match_ret_ty, Type::Struct(_) | Type::Enum(_)) {
                result_ptr.unwrap()
            } else if match_ret_ty.is_str_slice() || match_ret_ty.is_slice() {
                let ptr_v = self.new_value();
                let len_v = self.new_value();
                self.emit(Instruction::LoadSlice {
                    dest_ptr: ptr_v,
                    dest_len: len_v,
                    slot: r_slot,
                });
                ptr_v
            } else {
                let res = self.new_value();
                self.emit(Instruction::Load {
                    dest: res,
                    slot: r_slot,
                    ty: match_ret_ty,
                });
                res
            }
        } else {
            let dummy = self.new_value();
            self.emit(Instruction::ConstInt {
                dest: dummy,
                val: 0,
                ty: Type::I64,
            });
            dummy
        }
    }

    fn lower_try_expr(&mut self, inner: &Expr, try_expr: &Expr) -> ValueId {
        let mut inner_ty = self
            .sema
            .expr_types
            .get(&(inner as *const Expr))
            .cloned()
            .unwrap_or(Type::Unit);

        if inner_ty == Type::Unit {
            if let ExprKind::Call { callee, .. } = &inner.kind {
                let mangled = self.sema.call_callees.get(&inner.span).unwrap_or(callee);
                if let Some(sig) = self.sema.functions.get(mangled) {
                    inner_ty = sig.ret_type.clone();
                }
            } else if let ExprKind::Ident(name) = &inner.kind {
                if let Some(slot) = self.lookup_slot(name) {
                    if let Some(s) = self.current_slots.iter().find(|s| s.id == slot) {
                        inner_ty = s.ty.clone();
                    }
                }
            }
        }

        let _try_ret_ty = self
            .sema
            .expr_types
            .get(&(try_expr as *const Expr))
            .cloned()
            .unwrap_or(Type::Unit);

        let fn_ret_ty = self.current_fn_ret_type.clone();

        if inner_ty.needs_drop(&self.sema.structs, &self.sema.enums) {
            if let ExprKind::Ident(src_name) = &inner.kind {
                self.mark_moved(src_name);
            }
        }

        let inner_ename = match &inner_ty {
            Type::Enum(name) => name.clone(),
            _ => panic!("Try expression operand must be an enum, got {:?}", inner_ty),
        };

        let inner_slot = self.new_slot(format!("_try_inner_{}", self.next_val), inner_ty.clone());
        let inner_ptr = self.new_value();
        self.emit(Instruction::SlotAddr {
            dest: inner_ptr,
            slot: inner_slot,
        });

        if let ExprKind::VariantConstructor {
            variant: v_ident,
            args: v_args,
            ..
        } = &inner.kind
        {
            self.lower_init_variant(inner_ptr, &inner_ename, &v_ident.name, v_args);
        } else {
            let (src_ptr, _) = self.lower_place_addr(inner);
            self.copy_enum_bytes(src_ptr, inner_ptr, &inner_ename);
        }

        let e_def = self.sema.enums.get(&inner_ename).unwrap().clone();
        let is_opt = self.sema.is_option_type(&inner_ty).is_some();
        let (success_variant, error_variant) = if is_opt {
            ("Some", "None")
        } else {
            ("Ok", "Err")
        };

        let v_succ = e_def.find_variant(success_variant).unwrap().clone();
        let v_err = e_def.find_variant(error_variant).unwrap().clone();

        let tag_val = self.new_value();
        self.emit(Instruction::LoadIndirect {
            dest: tag_val,
            addr: inner_ptr,
            ty: Type::I32,
        });
        let succ_tag = self.new_value();
        self.emit(Instruction::ConstInt {
            dest: succ_tag,
            val: v_succ.tag as i64,
            ty: Type::I32,
        });
        let cond = self.new_value();
        self.emit(Instruction::Binary {
            dest: cond,
            op: MirBinaryOp::Eq,
            lhs: tag_val,
            rhs: succ_tag,
            ty: Type::Bool,
        });

        let success_block = self.new_block("try.success");
        let error_block = self.new_block("try.error");

        self.terminate(Terminator::CondBranch {
            cond,
            then_block: success_block,
            else_block: error_block,
        });

        // --- Error Path: construct error return value and return early ---
        self.switch_to_block(error_block);
        let fn_ename = match &fn_ret_ty {
            Type::Enum(e) => e.clone(),
            _ => panic!("Invalid return type for try: {:?}", fn_ret_ty),
        };
        let fn_edef = self.sema.enums.get(&fn_ename).unwrap().clone();

        let ret_slot = self.new_slot(format!("_try_ret_{}", self.next_val), fn_ret_ty.clone());
        let ret_ptr = self.new_value();
        self.emit(Instruction::SlotAddr {
            dest: ret_ptr,
            slot: ret_slot,
        });

        if is_opt {
            let ret_none_v = fn_edef.find_variant("None").unwrap();
            let tag_c = self.new_value();
            self.emit(Instruction::ConstInt {
                dest: tag_c,
                val: ret_none_v.tag as i64,
                ty: Type::I32,
            });
            self.emit(Instruction::StoreIndirect32 {
                addr: ret_ptr,
                val: tag_c,
            });
        } else {
            let ret_err_v = fn_edef.find_variant("Err").unwrap();
            let tag_c = self.new_value();
            self.emit(Instruction::ConstInt {
                dest: tag_c,
                val: ret_err_v.tag as i64,
                ty: Type::I32,
            });
            self.emit(Instruction::StoreIndirect32 {
                addr: ret_ptr,
                val: tag_c,
            });

            let err_src_ptr = self.new_value();
            self.emit(Instruction::FieldAddr {
                dest: err_src_ptr,
                base_ptr: inner_ptr,
                offset: v_err.payload_offsets[0],
            });
            let err_dest_ptr = self.new_value();
            self.emit(Instruction::FieldAddr {
                dest: err_dest_ptr,
                base_ptr: ret_ptr,
                offset: ret_err_v.payload_offsets[0],
            });
            let err_ty = &v_err.payload[0];
            if let Type::Struct(s) = err_ty {
                self.copy_struct_bytes(err_src_ptr, err_dest_ptr, s);
            } else if let Type::Enum(e) = err_ty {
                self.copy_enum_bytes(err_src_ptr, err_dest_ptr, e);
            } else {
                let val = self.new_value();
                self.emit(Instruction::LoadIndirect {
                    dest: val,
                    addr: err_src_ptr,
                    ty: err_ty.clone(),
                });
                self.emit(Instruction::StoreIndirect {
                    addr: err_dest_ptr,
                    val,
                });
            }
        }

        // Drop still-owned locals in scopes before returning
        let mut candidates_to_drop = Vec::new();
        for scope in self.drop_scopes.iter().rev() {
            for candidate in scope.iter().rev() {
                if !candidate.is_moved {
                    candidates_to_drop.push(candidate.clone());
                }
            }
        }
        for candidate in candidates_to_drop {
            let ptr = self.new_value();
            self.emit(Instruction::SlotAddr {
                dest: ptr,
                slot: candidate.slot,
            });
            self.emit_drop_for_place(ptr, &candidate.ty);
        }
        self.terminate(Terminator::Return(Some(ret_ptr)));

        // --- Success Path: extract success payload and resume ---
        self.switch_to_block(success_block);
        let succ_payload_offset = v_succ.payload_offsets[0];
        let succ_ty = &v_succ.payload[0];
        let succ_src_ptr = self.new_value();
        self.emit(Instruction::FieldAddr {
            dest: succ_src_ptr,
            base_ptr: inner_ptr,
            offset: succ_payload_offset,
        });

        if matches!(succ_ty, Type::Struct(_) | Type::Enum(_)) {
            let res_slot = self.new_slot(format!("_try_res_{}", self.next_val), succ_ty.clone());
            let res_ptr = self.new_value();
            self.emit(Instruction::SlotAddr {
                dest: res_ptr,
                slot: res_slot,
            });
            if let Type::Struct(s) = succ_ty {
                self.copy_struct_bytes(succ_src_ptr, res_ptr, s);
            } else if let Type::Enum(e) = succ_ty {
                self.copy_enum_bytes(succ_src_ptr, res_ptr, e);
            }
            res_ptr
        } else {
            let val = self.new_value();
            self.emit(Instruction::LoadIndirect {
                dest: val,
                addr: succ_src_ptr,
                ty: succ_ty.clone(),
            });
            val
        }
    }
}
