use std::collections::HashMap;

use super::super::instruction::*;
use super::{StmtResult, convert_binop, convert_compare_op, semantic_type_to_bir};
use crate::parser::ast::*;

impl super::Lowering {
    // ========== Expr ==========

    pub(super) fn lower_expr(&mut self, expr: &Expr) -> Value {
        match &expr.kind {
            ExprKind::Number(n) => {
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: *n,
                    ty: BirType::I32,
                });
                self.value_types.insert(result, BirType::I32);
                result
            }
            ExprKind::Float(f) => {
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: f.to_bits() as i64,
                    ty: BirType::F64,
                });
                self.value_types.insert(result, BirType::F64);
                result
            }
            ExprKind::Bool(b) => {
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: if *b { 1 } else { 0 },
                    ty: BirType::Bool,
                });
                self.value_types.insert(result, BirType::Bool);
                result
            }
            ExprKind::Ident(name) => self.lookup_var(name),
            ExprKind::UnaryOp { op, operand } => self.lower_unary_op(op, operand),
            ExprKind::BinaryOp { op, left, right } => self.lower_binary_op(expr, *op, left, right),
            ExprKind::Call {
                name,
                args,
                type_args,
            } => self.lower_call(expr, name, args, type_args),
            ExprKind::Block(block) => {
                self.push_scope();
                let (result, mut inner_regions) = self.lower_block_stmts(block);
                self.pop_scope();
                self.pending_regions.append(&mut inner_regions);
                match result {
                    Some(StmtResult::Yield(v)) => v,
                    _ => unreachable!(
                        "block expression must yield (semantic analysis guarantees this)"
                    ),
                }
            }
            ExprKind::If {
                condition,
                then_block,
                else_block,
            } => self.lower_if(condition, then_block, else_block.as_ref()),
            ExprKind::While {
                condition,
                body,
                nobreak,
            } => self.lower_while(condition, body, nobreak.as_ref()),
            ExprKind::Cast { expr, target_type } => self.lower_cast(expr, target_type),
            ExprKind::StructInit {
                name,
                args,
                type_args,
            } => self.lower_struct_init(expr, name, args, type_args),
            ExprKind::FieldAccess { object, field } => self.lower_field_access(expr, object, field),
            ExprKind::SelfRef => {
                let self_name = self.self_var_name.as_ref().unwrap();
                if self.in_init_body {
                    return self.record_error(
                        "bare `self` in initializer body is not supported; use self.field instead",
                        Some(expr.span),
                    );
                }
                self.lookup_var(self_name)
            }
            ExprKind::MethodCall {
                object,
                method,
                args,
            } => self.lower_method_call(expr, object, method, args),
            ExprKind::ArrayLiteral { elements } => self.lower_array_literal(elements),
            ExprKind::IndexAccess { object, index } => self.lower_index_access(object, index),
        }
    }

    fn lower_unary_op(&mut self, op: &UnaryOp, operand: &Expr) -> Value {
        let operand_val = self.lower_expr(operand);
        match op {
            UnaryOp::Not => {
                let result = self.fresh_value();
                self.emit(Instruction::Not {
                    result,
                    operand: operand_val,
                });
                self.value_types.insert(result, BirType::Bool);
                result
            }
        }
    }

    fn lower_binary_op(&mut self, _expr: &Expr, op: BinOp, left: &Expr, right: &Expr) -> Value {
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                let lhs = self.lower_expr(left);
                let rhs = self.lower_expr(right);
                let ty = self.value_types.get(&lhs).cloned().unwrap_or(BirType::I32);
                let result = self.fresh_value();
                self.emit(Instruction::BinaryOp {
                    result,
                    op: convert_binop(op),
                    lhs,
                    rhs,
                    ty: ty.clone(),
                });
                self.value_types.insert(result, ty);
                result
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                let lhs = self.lower_expr(left);
                let rhs = self.lower_expr(right);
                let operand_ty = self.value_types.get(&lhs).cloned().unwrap_or(BirType::I32);
                let result = self.fresh_value();
                self.emit(Instruction::Compare {
                    result,
                    op: convert_compare_op(op),
                    lhs,
                    rhs,
                    ty: operand_ty,
                });
                self.value_types.insert(result, BirType::Bool);
                result
            }
            BinOp::And => {
                let result = self.lower_short_circuit_and(left, right);
                self.value_types.insert(result, BirType::Bool);
                result
            }
            BinOp::Or => {
                let result = self.lower_short_circuit_or(left, right);
                self.value_types.insert(result, BirType::Bool);
                result
            }
        }
    }

    fn lower_call(
        &mut self,
        expr: &Expr,
        name: &str,
        args: &[Expr],
        type_args: &[TypeAnnotation],
    ) -> Value {
        // Use explicit type_args if present, otherwise check inferred
        let effective_type_args: Vec<TypeAnnotation> = if type_args.is_empty() {
            self.inferred_type_args
                .get(&expr.id)
                .cloned()
                .unwrap_or_default()
        } else {
            type_args.to_vec()
        };
        let bir_type_args: Vec<BirType> = effective_type_args
            .iter()
            .map(|ta| self.convert_type_with_structs(ta))
            .collect();
        let sem = self.sem_info.as_ref().unwrap();
        if sem.struct_init_calls.contains(&expr.id) {
            let struct_info = sem.struct_defs.get(name).unwrap().clone();
            if struct_info.init.body.is_some() {
                let field_values = self.lower_explicit_init(name, &struct_info, expr);
                return self.emit_struct_init(name, &field_values, &bir_type_args);
            }
            // No-arg memberwise init (call syntax with no custom init body)
            return self.emit_struct_init(name, &[], &bir_type_args);
        }
        let arg_vals: Vec<Value> = args.iter().map(|a| self.lower_expr(a)).collect();
        let resolved = self.resolve_name(name);
        let ty = self
            .func_sigs
            .get(&resolved)
            .cloned()
            .unwrap_or(BirType::I32);
        // Resolve TypeParam return types for the local value_types map
        // so that subsequent instructions (BinaryOp, FieldGet, etc.)
        // get the resolved concrete type.
        let resolved_ty = if !bir_type_args.is_empty() {
            use crate::bir::mono::resolve_bir_type_lenient;
            let subst: HashMap<String, BirType> = self
                .func_type_param_names
                .get(&resolved)
                .map(|params| {
                    params
                        .iter()
                        .zip(bir_type_args.iter())
                        .map(|(p, a)| (p.clone(), a.clone()))
                        .collect()
                })
                .unwrap_or_default();
            if subst.is_empty() {
                ty.clone()
            } else {
                resolve_bir_type_lenient(&ty, &subst)
            }
        } else {
            ty.clone()
        };
        let result = self.fresh_value();
        self.emit(Instruction::Call {
            result,
            func_name: resolved,
            args: arg_vals,
            type_args: bir_type_args,
            ty: ty.clone(),
        });
        self.value_types.insert(result, resolved_ty);
        result
    }

    fn lower_cast(&mut self, expr: &Expr, target_type: &TypeAnnotation) -> Value {
        let operand = self.lower_expr(expr);
        let from_ty = self
            .value_types
            .get(&operand)
            .cloned()
            .unwrap_or(BirType::I32);
        let to_ty = self.convert_type_with_structs(target_type);
        let result = self.fresh_value();
        self.emit(Instruction::Cast {
            result,
            operand,
            from_ty,
            to_ty: to_ty.clone(),
        });
        self.value_types.insert(result, to_ty);
        result
    }

    fn lower_struct_init(
        &mut self,
        expr: &Expr,
        name: &str,
        args: &[(String, Expr)],
        type_args: &[TypeAnnotation],
    ) -> Value {
        // Use explicit type_args if present, otherwise check inferred
        let effective_type_args: Vec<TypeAnnotation> = if type_args.is_empty() {
            self.inferred_type_args
                .get(&expr.id)
                .cloned()
                .unwrap_or_default()
        } else {
            type_args.to_vec()
        };
        let bir_type_args: Vec<BirType> = effective_type_args
            .iter()
            .map(|ta| self.convert_type_with_structs(ta))
            .collect();
        let sem = self.sem_info.as_ref().unwrap();
        let struct_info = sem.struct_defs.get(name).unwrap().clone();
        if struct_info.init.body.is_some() {
            let field_values = self.lower_explicit_init(name, &struct_info, expr);
            return self.emit_struct_init(name, &field_values, &bir_type_args);
        }
        let field_values: Vec<(String, Value)> = args
            .iter()
            .map(|(label, arg_expr)| (label.clone(), self.lower_expr(arg_expr)))
            .collect();
        self.emit_struct_init(name, &field_values, &bir_type_args)
    }

    fn lower_method_call(
        &mut self,
        expr: &Expr,
        object: &Expr,
        method: &str,
        args: &[Expr],
    ) -> Value {
        let obj_val = self.lower_expr(object);
        let (struct_name, struct_type_args) = match self.value_types.get(&obj_val) {
            Some(BirType::Struct {
                name: n,
                type_args: ta,
            }) => (n.clone(), ta.clone()),
            Some(BirType::TypeParam(type_param_name)) => {
                return self.lower_protocol_method_call(
                    expr,
                    obj_val,
                    type_param_name.clone(),
                    method,
                    args,
                );
            }
            _ => {
                return self.record_error("method call on non-struct value", Some(expr.span));
            }
        };
        let local_mangled = format!("{}_{}", struct_name, method);
        let resolved = self.resolve_name(&local_mangled);
        let ret_ty = self
            .func_sigs
            .get(&resolved)
            .cloned()
            .unwrap_or(BirType::Unit);
        let mut call_args = vec![obj_val];
        for arg in args {
            call_args.push(self.lower_expr(arg));
        }
        // Resolve TypeParam return type for value_types
        let resolved_ret_ty = if !struct_type_args.is_empty() {
            use crate::bir::mono::resolve_bir_type_lenient;
            let subst: HashMap<String, BirType> = self
                .func_type_param_names
                .get(&resolved)
                .map(|params| {
                    params
                        .iter()
                        .zip(struct_type_args.iter())
                        .map(|(p, a)| (p.clone(), a.clone()))
                        .collect()
                })
                .unwrap_or_default();
            if subst.is_empty() {
                ret_ty.clone()
            } else {
                resolve_bir_type_lenient(&ret_ty, &subst)
            }
        } else {
            ret_ty.clone()
        };
        let result = self.fresh_value();
        self.emit(Instruction::Call {
            result,
            func_name: resolved,
            args: call_args,
            type_args: struct_type_args,
            ty: ret_ty.clone(),
        });
        self.value_types.insert(result, resolved_ret_ty);
        result
    }

    /// Protocol method call on constrained type parameter.
    fn lower_protocol_method_call(
        &mut self,
        expr: &Expr,
        obj_val: Value,
        type_param_name: String,
        method: &str,
        args: &[Expr],
    ) -> Value {
        let bound = self
            .current_type_params
            .iter()
            .find(|tp| tp.name == type_param_name)
            .and_then(|tp| tp.bound.clone());
        let proto_name = match bound {
            Some(b) => b,
            None => {
                return self.record_error(
                    format!(
                        "type parameter `{}` has no protocol constraint for method `{}`",
                        type_param_name, method
                    ),
                    Some(expr.span),
                );
            }
        };
        // Look up the protocol's method signature for the return type
        let ret_ty = self
            .sem_info
            .as_ref()
            .and_then(|si| si.protocols.get(&proto_name))
            .and_then(|pi| pi.methods.iter().find(|m| m.name == method))
            .map(|m| semantic_type_to_bir(&m.return_type))
            .unwrap_or(BirType::Unit);
        let func_name = format!("{}_{}", proto_name, method);
        let mut call_args = vec![obj_val];
        for arg in args {
            call_args.push(self.lower_expr(arg));
        }
        let result = self.fresh_value();
        self.emit(Instruction::Call {
            result,
            func_name,
            args: call_args,
            type_args: vec![BirType::TypeParam(type_param_name)],
            ty: ret_ty.clone(),
        });
        self.value_types.insert(result, ret_ty);
        result
    }

    fn lower_array_literal(&mut self, elements: &[Expr]) -> Value {
        let elem_values: Vec<Value> = elements.iter().map(|e| self.lower_expr(e)).collect();
        // Determine element BirType from first element
        let elem_ty = if let Some(first) = elem_values.first() {
            self.value_types.get(first).cloned().unwrap_or(BirType::I32)
        } else {
            BirType::I32
        };
        let arr_ty = BirType::Array {
            element: Box::new(elem_ty),
            size: elem_values.len() as u64,
        };
        let result = self.fresh_value();
        self.emit(Instruction::ArrayInit {
            result,
            ty: arr_ty.clone(),
            elements: elem_values,
        });
        self.value_types.insert(result, arr_ty);
        result
    }

    fn lower_index_access(&mut self, object: &Expr, index: &Expr) -> Value {
        let arr_val = self.lower_expr(object);
        let idx_val = self.lower_expr(index);
        let arr_ty = self
            .value_types
            .get(&arr_val)
            .cloned()
            .unwrap_or(BirType::I32);
        let (elem_ty, size) = match &arr_ty {
            BirType::Array { element, size } => (*element.clone(), *size),
            _ => unreachable!("IndexAccess on non-array type (semantic guarantees this)"),
        };
        let result = self.fresh_value();
        self.emit(Instruction::ArrayGet {
            result,
            ty: elem_ty.clone(),
            array: arr_val,
            index: idx_val,
            array_size: size,
        });
        self.value_types.insert(result, elem_ty);
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::bir::lowering::lower_program;
    use crate::bir::printer::print_module;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::semantic;

    fn lower_str(input: &str) -> String {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let module = lower_program(&program, &sem_info).unwrap();
        print_module(&module)
    }

    #[test]
    fn lower_simple_return() {
        let output = lower_str("func main() -> Int32 { return 42; }");
        let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 42 : Int32
    return %0
}
";
        assert_eq!(output, expected);
    }

    #[test]
    fn lower_let_return() {
        let output = lower_str("func main() -> Int32 { let x: Int32 = 10; return x; }");
        let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 10 : Int32
    return %0
}
";
        assert_eq!(output, expected);
    }

    #[test]
    fn lower_call() {
        let output = lower_str(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }",
        );
        let expected = "\
bir @add(%0: Int32, %1: Int32) -> Int32 {
bb0:
    %2 = binary_op add %0, %1 : Int32
    return %2
}
bir @main() -> Int32 {
bb0:
    %0 = literal 3 : Int32
    %1 = literal 4 : Int32
    %2 = call @add(%0, %1) : Int32
    return %2
}
";
        assert_eq!(output, expected);
    }

    #[test]
    fn lower_block_scope() {
        let output = lower_str(
            "func main() -> Int32 { let x: Int32 = 1; let y: Int32 = { let x: Int32 = 10; yield x + 1; }; return x + y; }",
        );
        let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 1 : Int32
    %1 = literal 10 : Int32
    %2 = literal 1 : Int32
    %3 = binary_op add %1, %2 : Int32
    %4 = binary_op add %0, %3 : Int32
    return %4
}
";
        assert_eq!(output, expected);
    }
}
