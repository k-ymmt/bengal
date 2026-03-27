use super::super::instruction::*;
use super::StmtResult;
use crate::parser::ast::*;
use crate::semantic::resolver;

impl super::Lowering {
    // ========== Init ==========

    pub(super) fn lower_explicit_init(
        &mut self,
        struct_name: &str,
        struct_info: &resolver::StructInfo,
        expr: &Expr,
    ) -> Vec<(String, Value)> {
        let init_info = &struct_info.init;
        let body = init_info.body.as_ref().unwrap().clone();

        // Evaluate init arguments
        let arg_values: Vec<Value> = match &expr.kind {
            ExprKind::StructInit { args, .. } => {
                args.iter().map(|(_, e)| self.lower_expr(e)).collect()
            }
            ExprKind::Call { args, .. } => args.iter().map(|e| self.lower_expr(e)).collect(),
            _ => unreachable!(),
        };

        // Set up self context -- per-field variable tracking (hybrid model)
        let temp_self = format!("__init_{}", self.next_value);
        let prev_self_var = self.self_var_name.clone();
        let prev_in_init = self.in_init_body;
        let prev_init_struct = self.init_struct_name.clone();
        self.self_var_name = Some(temp_self.clone());
        self.in_init_body = true;
        self.init_struct_name = Some(struct_name.to_string());

        self.push_scope();

        // Define init parameters as local variables
        for (i, (param_name, _)) in init_info.params.iter().enumerate() {
            self.define_var(param_name.clone(), arg_values[i]);
        }

        // Execute init body -- self.field = val goes through FieldAssign init-body path
        let (_, mut init_regions) = self.lower_block_stmts(&body);
        self.pending_regions.append(&mut init_regions);

        // Collect resulting field values (all fields should be initialized)
        let result: Vec<(String, Value)> = struct_info
            .fields
            .iter()
            .map(|(fname, _)| {
                let key = format!("{}.{}", temp_self, fname);
                let val = self.lookup_var(&key);
                (fname.clone(), val)
            })
            .collect();

        self.pop_scope();
        self.self_var_name = prev_self_var;
        self.in_init_body = prev_in_init;
        self.init_struct_name = prev_init_struct;

        result
    }

    // ========== Stmt ==========

    pub(super) fn lower_stmt(&mut self, stmt: &Stmt) -> StmtResult {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let val = self.lower_expr(value);
                self.define_var(name.clone(), val);
                StmtResult::None
            }
            Stmt::Var { name, value, .. } => {
                let val = self.lower_expr(value);
                self.define_var(name.clone(), val);
                let ty = self.value_types.get(&val).cloned().unwrap_or(BirType::I32);
                if !self.mutable_vars.iter().any(|(n, _)| n == name) {
                    self.mutable_vars.push((name.clone(), ty));
                }
                StmtResult::None
            }
            Stmt::Assign { name, value } => {
                let val = self.lower_expr(value);
                self.assign_var(name, val);
                StmtResult::None
            }
            Stmt::Return(Some(expr)) => {
                let val = self.lower_expr(expr);
                if let Some((cont_bb, _param, ty)) = self.getter_return_bb.clone() {
                    self.seal_block(Terminator::Br {
                        target: cont_bb,
                        args: vec![(val, ty)],
                    });
                    let dead_bb = self.fresh_block();
                    self.start_block(dead_bb, vec![]);
                    StmtResult::ReturnVoid
                } else {
                    StmtResult::Return(val)
                }
            }
            Stmt::Return(None) => StmtResult::ReturnVoid,
            Stmt::Yield(expr) => {
                let val = self.lower_expr(expr);
                StmtResult::Yield(val)
            }
            Stmt::Expr(expr) => {
                self.last_expr_diverged = false;
                let _val = self.lower_expr(expr);
                if self.last_expr_diverged {
                    StmtResult::ReturnVoid
                } else {
                    StmtResult::None
                }
            }
            Stmt::Break(opt_expr) => self.lower_break(opt_expr.as_ref()),
            Stmt::Continue => self.lower_continue(),
            Stmt::FieldAssign {
                object,
                field,
                value,
            } => self.lower_field_assign(object, field, value),
            Stmt::IndexAssign {
                object,
                index,
                value,
            } => self.lower_index_assign(object, index, value),
        }
    }

    fn lower_break(&mut self, opt_expr: Option<&Expr>) -> StmtResult {
        let loop_ctx = self.loop_stack.last().unwrap();
        let header_bb = loop_ctx.header_bb;
        let exit_bb = loop_ctx.exit_bb;
        let mutable_vars = self.collect_mutable_var_values();
        let args: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(_, v, t)| (*v, t.clone()))
            .collect();
        let value = match opt_expr {
            Some(expr) => {
                let val = self.lower_expr(expr);
                let ty = self.value_types.get(&val).cloned().unwrap_or(BirType::I32);
                self.loop_stack.last_mut().unwrap().break_ty = Some(ty.clone());
                Some((val, ty))
            }
            None => None,
        };
        self.seal_block(Terminator::BrBreak {
            header_bb,
            exit_bb,
            args,
            value,
        });
        let dummy_bb = self.fresh_block();
        self.start_block(dummy_bb, vec![]);
        StmtResult::Break
    }

    fn lower_continue(&mut self) -> StmtResult {
        let loop_ctx = self.loop_stack.last().unwrap();
        let header_bb = loop_ctx.header_bb;
        let mutable_vars = self.collect_mutable_var_values();
        let args: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(_, v, t)| (*v, t.clone()))
            .collect();
        self.seal_block(Terminator::BrContinue { header_bb, args });
        let dummy_bb = self.fresh_block();
        self.start_block(dummy_bb, vec![]);
        StmtResult::Continue
    }

    fn lower_field_assign(&mut self, object: &Expr, field: &str, value: &Expr) -> StmtResult {
        // 1. Init body + SelfRef: per-field variable path
        if self.in_init_body {
            if let ExprKind::SelfRef = &object.kind {
                // Reject computed property setter on self during init
                let struct_name = self.init_struct_name.as_ref().unwrap().clone();
                let sem = self.sem_info.as_ref().unwrap();
                if let Some(info) = sem.struct_defs.get(&struct_name)
                    && info.computed.iter().any(|p| p.name == field)
                {
                    self.record_error(
                        format!(
                            "computed property setter `{}` on `self` in initializer body \
                             is not supported (self is not fully materialized during init)",
                            field
                        ),
                        Some(object.span),
                    );
                    return StmtResult::None;
                }
                // Stored field -- write to per-field variable
                let new_val = self.lower_expr(value);
                let self_name = self.self_var_name.as_ref().unwrap().clone();
                let key = format!("{}.{}", self_name, field);
                if self.try_lookup_var(&key).is_some() {
                    self.assign_var(&key, new_val);
                } else {
                    self.define_var(key, new_val);
                }
                return StmtResult::None;
            }
            // 2. Init body + nested self path: error
            if self.expr_refers_to_self(object) {
                self.record_error(
                    "nested field assignment through `self` in initializer body is not supported",
                    Some(object.span),
                );
                return StmtResult::None;
            }
        }

        // 3. General case: try computed setter, then recursive field assign
        if self.try_lower_computed_setter(object, field, value) {
            return StmtResult::None;
        }

        let new_val = self.lower_expr(value);
        self.lower_field_assign_recursive(object, field, new_val);
        StmtResult::None
    }

    fn lower_index_assign(&mut self, object: &Expr, index: &Expr, value: &Expr) -> StmtResult {
        let idx_val = self.lower_expr(index);
        let new_val = self.lower_expr(value);

        // The object must be a variable (semantic analysis ensures this)
        let var_name = match &object.kind {
            ExprKind::Ident(name) => name.clone(),
            _ => unreachable!("IndexAssign on non-ident (semantic guarantees this)"),
        };
        let arr_val = self.lookup_var(&var_name);
        let arr_ty = self
            .value_types
            .get(&arr_val)
            .cloned()
            .unwrap_or(BirType::I32);
        let size = match &arr_ty {
            BirType::Array { size, .. } => *size,
            _ => unreachable!("IndexAssign on non-array type (semantic guarantees this)"),
        };
        let result = self.fresh_value();
        self.emit(Instruction::ArraySet {
            result,
            ty: arr_ty.clone(),
            array: arr_val,
            index: idx_val,
            value: new_val,
            array_size: size,
        });
        self.value_types.insert(result, arr_ty);
        self.assign_var(&var_name, result);
        StmtResult::None
    }
}
