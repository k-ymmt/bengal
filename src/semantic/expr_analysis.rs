use crate::error::DiagCtxt;
use crate::parser::ast::*;
use crate::suggest::find_suggestion;

use super::expr_call_analysis;
use super::expr_method_analysis;
use super::function_analysis::{analyze_block_expr, analyze_control_block, analyze_loop_block};
use super::infer::InferenceContext;
use super::resolver::Resolver;
use super::types::Type;
use super::{resolve_type_checked, sem_err};

pub(super) fn analyze_expr(
    expr: &Expr,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Type {
    match &expr.kind {
        ExprKind::Number(n) => {
            if let Some(ref mut c) = ctx {
                // In inference mode, create an IntegerLiteral variable and defer
                // the range check until after the concrete type is resolved.
                let id = c.fresh_integer();
                c.register_int_range_check(id, *n);
                Type::IntegerLiteral(id)
            } else {
                if *n < i32::MIN as i64 || *n > i32::MAX as i64 {
                    diag.emit(sem_err(format!(
                        "integer literal `{}` is out of range for `Int32`",
                        n
                    )));
                    return Type::Error;
                }
                Type::I32
            }
        }
        ExprKind::Bool(_) => Type::Bool,
        ExprKind::Ident(name) => match resolver.lookup_var(name) {
            Some(info) => info.ty.clone(),
            None => {
                let help = find_suggestion(name, resolver.all_variable_names())
                    .map(|s| format!("did you mean '{s}'?"));
                diag.emit(super::sem_err_with_help(
                    format!("undefined variable `{}`", name),
                    expr.span,
                    help,
                ));
                Type::Error
            }
        },
        ExprKind::UnaryOp { op, operand } => {
            let operand_ty = analyze_expr(operand, resolver, ctx.as_deref_mut(), diag);
            if operand_ty == Type::Error {
                return Type::Error;
            }
            match op {
                UnaryOp::Not => {
                    if operand_ty != Type::Bool {
                        diag.emit(sem_err("operand of `!` must be `Bool`"));
                        return Type::Error;
                    }
                    Type::Bool
                }
            }
        }
        ExprKind::BinaryOp { op, left, right } => {
            let left_ty = analyze_expr(left, resolver, ctx.as_deref_mut(), diag);
            let right_ty = analyze_expr(right, resolver, ctx.as_deref_mut(), diag);
            if left_ty == Type::Error || right_ty == Type::Error {
                return Type::Error;
            }
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    if let Some(ref mut c) = ctx {
                        // In inference mode, unify left and right operands
                        if let Err(e) = c.unify(left_ty.clone(), right_ty.clone()) {
                            diag.emit(e);
                            return Type::Error;
                        }
                        left_ty
                    } else {
                        if !left_ty.is_numeric() || left_ty != right_ty {
                            diag.emit(sem_err(format!(
                                "arithmetic operation requires matching numeric operands, found `{}` and `{}`",
                                left_ty, right_ty
                            )));
                            return Type::Error;
                        }
                        left_ty
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(left_ty.clone(), right_ty.clone()) {
                            diag.emit(e);
                            return Type::Error;
                        }
                        Type::Bool
                    } else {
                        if !left_ty.is_numeric() || left_ty != right_ty {
                            diag.emit(sem_err(format!(
                                "comparison requires matching numeric operands, found `{}` and `{}`",
                                left_ty, right_ty
                            )));
                            return Type::Error;
                        }
                        Type::Bool
                    }
                }
                // Logical: bool x bool → bool
                BinOp::And | BinOp::Or => {
                    if left_ty != Type::Bool || right_ty != Type::Bool {
                        diag.emit(sem_err("logical operation requires `Bool` operands"));
                        return Type::Error;
                    }
                    Type::Bool
                }
            }
        }
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            expr_call_analysis::analyze_call_expr(expr, name, type_args, args, resolver, ctx, diag)
        }
        ExprKind::Block(block) => analyze_block_expr(block, resolver, ctx, diag),
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            let cond_ty = analyze_expr(condition, resolver, ctx.as_deref_mut(), diag);
            if cond_ty == Type::Error {
                return Type::Error;
            }
            if cond_ty != Type::Bool {
                diag.emit(sem_err("if condition must be `Bool`"));
                return Type::Error;
            }

            let then_ty = analyze_control_block(then_block, resolver, ctx.as_deref_mut(), diag);

            match else_block {
                Some(else_blk) => {
                    let else_ty =
                        analyze_control_block(else_blk, resolver, ctx.as_deref_mut(), diag);
                    // Type merging with divergence
                    match (then_ty, else_ty) {
                        (Some(t1), Some(t2)) => {
                            if t1 == Type::Error || t2 == Type::Error {
                                return Type::Error;
                            }
                            if let Some(ref mut c) = ctx {
                                if let Err(e) = c.unify(t1.clone(), t2.clone()) {
                                    diag.emit(e);
                                    return Type::Error;
                                }
                                t1
                            } else {
                                if t1 != t2 {
                                    diag.emit(sem_err(format!(
                                        "if/else branch type mismatch: `{}` vs `{}`",
                                        t1, t2
                                    )));
                                    return Type::Error;
                                }
                                t1
                            }
                        }
                        (None, Some(t)) => t, // then diverges, use else type
                        (Some(t), None) => t, // else diverges, use then type
                        (None, None) => Type::Unit, // both diverge
                    }
                }
                None => {
                    // if without else: type is Unit
                    if let Some(ref ty) = then_ty
                        && *ty != Type::Unit
                        && *ty != Type::Error
                    {
                        diag.emit(sem_err(
                            "if without else must have unit type (use `yield` in both branches for a value)",
                        ));
                        return Type::Error;
                    }
                    Type::Unit
                }
            }
        }
        ExprKind::While {
            condition,
            body,
            nobreak,
        } => {
            let cond_ty = analyze_expr(condition, resolver, ctx.as_deref_mut(), diag);
            if cond_ty == Type::Error {
                return Type::Error;
            }
            if cond_ty != Type::Bool {
                diag.emit(sem_err("while condition must be `Bool`"));
                return Type::Error;
            }
            let is_while_true = condition.kind == ExprKind::Bool(true);

            resolver.enter_loop();
            analyze_loop_block(body, resolver, ctx.as_deref_mut(), diag);
            let break_ty = resolver.exit_loop();

            let while_ty = break_ty.unwrap_or(Type::Unit);

            match (is_while_true, nobreak) {
                (true, Some(_)) => {
                    diag.emit(sem_err("`nobreak` is unreachable in `while true`"));
                    return Type::Error;
                }
                (false, None) if while_ty != Type::Unit => {
                    diag.emit(sem_err(
                        "`while` with non-unit break requires `nobreak` block",
                    ));
                    return Type::Error;
                }
                (false, Some(nobreak_block)) => {
                    let nobreak_ty =
                        analyze_control_block(nobreak_block, resolver, ctx.as_deref_mut(), diag);
                    if let Some(t) = nobreak_ty {
                        if t == Type::Error {
                            return Type::Error;
                        }
                        if let Some(ref mut c) = ctx {
                            if let Err(e) = c.unify(t.clone(), while_ty.clone()) {
                                diag.emit(e);
                                return Type::Error;
                            }
                        } else if t != while_ty {
                            diag.emit(sem_err(format!(
                                "nobreak type `{}` does not match while type `{}`",
                                t, while_ty
                            )));
                            return Type::Error;
                        }
                    }
                }
                _ => {}
            }

            while_ty
        }
        ExprKind::Float(_) => {
            if let Some(ref mut c) = ctx {
                Type::FloatLiteral(c.fresh_float())
            } else {
                Type::F64
            }
        }
        ExprKind::StructInit {
            name,
            type_args,
            args,
        } => expr_call_analysis::analyze_struct_init_expr(
            expr, name, type_args, args, resolver, ctx, diag,
        ),
        ExprKind::FieldAccess { object, field } => {
            expr_call_analysis::analyze_field_access_expr(expr, object, field, resolver, ctx, diag)
        }
        ExprKind::SelfRef => match &resolver.self_context {
            Some(ctx) => Type::Struct(ctx.struct_name.clone()),
            None => {
                diag.emit(sem_err(
                    "`self` can only be used inside struct initializers, computed properties, or methods",
                ));
                Type::Error
            }
        },
        ExprKind::Cast {
            expr: cast_expr,
            target_type,
        } => {
            let source_ty = analyze_expr(cast_expr, resolver, ctx.as_deref_mut(), diag);
            if source_ty == Type::Error {
                return Type::Error;
            }
            let target_ty = match resolve_type_checked(target_type, resolver) {
                Ok(t) => t,
                Err(e) => {
                    diag.emit(e);
                    return Type::Error;
                }
            };
            if !source_ty.is_numeric() || !target_ty.is_numeric() {
                diag.emit(sem_err(format!(
                    "cannot cast `{}` to `{}`",
                    source_ty, target_ty
                )));
                return Type::Error;
            }
            target_ty
        }
        ExprKind::MethodCall {
            object,
            method,
            args,
        } => expr_method_analysis::analyze_method_call_expr(
            expr, object, method, args, resolver, ctx, diag,
        ),
        ExprKind::ArrayLiteral { elements } => {
            if elements.is_empty() {
                diag.emit(sem_err("cannot infer type of empty array literal"));
                return Type::Error;
            }
            let first_ty = analyze_expr(&elements[0], resolver, ctx.as_deref_mut(), diag);
            if first_ty == Type::Error {
                return Type::Error;
            }
            let mut any_error = false;
            for elem in &elements[1..] {
                let elem_ty = analyze_expr(elem, resolver, ctx.as_deref_mut(), diag);
                if elem_ty == Type::Error {
                    any_error = true;
                    continue;
                }
                if let Some(ref mut c) = ctx {
                    if c.unify(elem_ty.clone(), first_ty.clone()).is_err() {
                        diag.emit(sem_err(format!(
                            "array elements must all have the same type: expected '{}', found '{}'",
                            first_ty, elem_ty
                        )));
                        any_error = true;
                    }
                } else if elem_ty != first_ty {
                    diag.emit(sem_err(format!(
                        "array elements must all have the same type: expected '{}', found '{}'",
                        first_ty, elem_ty
                    )));
                    any_error = true;
                }
            }
            if any_error {
                return Type::Error;
            }
            Type::Array {
                element: Box::new(first_ty),
                size: elements.len() as u64,
            }
        }
        ExprKind::IndexAccess { object, index } => {
            let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
            let idx_ty = analyze_expr(index, resolver, ctx, diag);
            if obj_ty == Type::Error || idx_ty == Type::Error {
                return Type::Error;
            }
            match &obj_ty {
                Type::Array { element, size } => {
                    if !idx_ty.is_integer() {
                        diag.emit(sem_err(format!(
                            "array index must be an integer type, found '{}'",
                            idx_ty
                        )));
                        return Type::Error;
                    }
                    // Compile-time bounds check for constant indices
                    if let ExprKind::Number(n) = &index.kind {
                        let idx = *n;
                        if idx < 0 || idx as u64 >= *size {
                            diag.emit(sem_err(format!(
                                "array index {} is out of bounds for array of size {}",
                                idx, size
                            )));
                            return Type::Error;
                        }
                    }
                    *element.clone()
                }
                _ => {
                    diag.emit(sem_err(format!("cannot index into type '{}'", obj_ty)));
                    Type::Error
                }
            }
        }
    }
}
