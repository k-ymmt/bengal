use std::collections::HashMap;

use crate::error::{DiagCtxt, Span};
use crate::parser::ast::*;
use crate::suggest::find_suggestion;

use super::expr_analysis::analyze_expr;
use super::infer::InferenceContext;
use super::resolver::{Resolver, VarInfo};
use super::struct_analysis::check_assignment_target_mutable;
use super::types::Type;
use super::{
    check_type_match, resolve_type_checked, sem_err, sem_err_with_help, substitute_type,
    types_compatible,
};

pub(super) fn analyze_stmt(
    stmt: &Stmt,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) {
    match stmt {
        Stmt::Let { name, ty, value } => {
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            let var_ty = if val_ty == Type::Error {
                // Still register the variable with the declared type if available
                match ty {
                    Some(ann) => match resolve_type_checked(ann, resolver) {
                        Ok(declared) => declared,
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    },
                    None => Type::Error,
                }
            } else {
                match ty {
                    Some(ann) => match resolve_type_checked(ann, resolver) {
                        Ok(declared) => {
                            if let Some(ref mut c) = ctx {
                                if let Err(e) = c.unify(val_ty.clone(), declared.clone()) {
                                    diag.emit(e);
                                }
                            } else if let Err(e) = check_type_match(&declared, &val_ty) {
                                diag.emit(e);
                            }
                            declared
                        }
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    },
                    None => val_ty,
                }
            };
            resolver.define_var(
                name.clone(),
                VarInfo {
                    ty: var_ty,
                    mutable: false,
                },
            );
        }
        Stmt::Var { name, ty, value } => {
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            let var_ty = if val_ty == Type::Error {
                match ty {
                    Some(ann) => match resolve_type_checked(ann, resolver) {
                        Ok(declared) => declared,
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    },
                    None => Type::Error,
                }
            } else {
                match ty {
                    Some(ann) => match resolve_type_checked(ann, resolver) {
                        Ok(declared) => {
                            if let Some(ref mut c) = ctx {
                                if let Err(e) = c.unify(val_ty.clone(), declared.clone()) {
                                    diag.emit(e);
                                }
                            } else if let Err(e) = check_type_match(&declared, &val_ty) {
                                diag.emit(e);
                            }
                            declared
                        }
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    },
                    None => val_ty,
                }
            };
            resolver.define_var(
                name.clone(),
                VarInfo {
                    ty: var_ty,
                    mutable: true,
                },
            );
        }
        Stmt::Assign { name, value } => {
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            match resolver.lookup_var(name) {
                None => {
                    let help = find_suggestion(name, resolver.all_variable_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("undefined variable `{}`", name),
                        Span { start: 0, end: 0 },
                        help,
                    ));
                }
                Some(info) => {
                    if !info.mutable {
                        diag.emit(sem_err(format!(
                            "cannot assign to immutable variable `{}`",
                            name
                        )));
                        return;
                    }
                    if val_ty == Type::Error {
                        return;
                    }
                    let expected_ty = info.ty.clone();
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(val_ty.clone(), expected_ty) {
                            diag.emit(e);
                        }
                    } else if val_ty != expected_ty {
                        diag.emit(sem_err(format!(
                            "type mismatch in assignment: expected `{}`, found `{}`",
                            expected_ty, val_ty
                        )));
                    }
                }
            }
        }
        Stmt::Return(Some(expr)) => {
            let ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
            if ty == Type::Error {
                return;
            }
            if let Some(ref return_type) = resolver.current_return_type {
                if let Some(ref mut c) = ctx {
                    // In inference mode, unify return value with return type
                    // (but skip TypeParam since those are generic and will be checked later)
                    if !matches!(return_type, Type::TypeParam { .. })
                        && let Err(e) = c.unify(ty.clone(), return_type.clone())
                    {
                        diag.emit(e);
                    }
                } else if !types_compatible(&ty, return_type) {
                    diag.emit(sem_err(format!(
                        "return type mismatch: expected `{}`, found `{}`",
                        return_type, ty
                    )));
                }
            }
        }
        Stmt::Return(None) => {
            if let Some(ref return_type) = resolver.current_return_type
                && !types_compatible(&Type::Unit, return_type)
            {
                diag.emit(sem_err(format!(
                    "return type mismatch: expected `{}`, found `()`",
                    return_type
                )));
            }
        }
        Stmt::Yield(expr) => {
            let _ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
        }
        Stmt::Expr(expr) => {
            let _ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
        }
        Stmt::Break(opt_expr) => {
            if !resolver.in_loop() {
                diag.emit(sem_err("break outside of loop"));
                return;
            }
            let break_ty = match opt_expr {
                Some(expr) => {
                    let ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
                    if ty == Type::Error {
                        return;
                    }
                    ty
                }
                None => Type::Unit,
            };
            if let Some(ref mut c) = ctx {
                // In inference mode, unify with existing break type instead of equality check
                if let Err(e) = resolver.set_or_unify_break_type(break_ty, c) {
                    diag.emit(e);
                }
            } else if let Err(e) = resolver.set_break_type(break_ty) {
                diag.emit(e);
            }
        }
        Stmt::Continue => {
            if !resolver.in_loop() {
                diag.emit(sem_err("continue outside of loop"));
            }
        }
        Stmt::FieldAssign {
            object,
            field,
            value,
        } => {
            let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            if obj_ty == Type::Error || val_ty == Type::Error {
                return;
            }
            match &obj_ty {
                Type::Struct(struct_name) => {
                    let struct_info = match resolver.lookup_struct(struct_name) {
                        Some(s) => s.clone(),
                        None => {
                            let help = find_suggestion(struct_name, resolver.all_struct_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined struct `{}`", struct_name),
                                Span { start: 0, end: 0 },
                                help,
                            ));
                            return;
                        }
                    };
                    let field_ty = if let Some(&idx) = struct_info.field_index.get(field.as_str()) {
                        struct_info.fields[idx].1.clone()
                    } else if let Some(&idx) = struct_info.computed_index.get(field.as_str()) {
                        let prop = &struct_info.computed[idx];
                        if !prop.has_setter {
                            diag.emit(sem_err(format!(
                                "computed property `{}` is read-only (no setter)",
                                field
                            )));
                            return;
                        }
                        prop.ty.clone()
                    } else {
                        let help = find_suggestion(
                            field,
                            struct_info
                                .field_index
                                .keys()
                                .chain(struct_info.computed_index.keys())
                                .map(|s| s.as_str()),
                        )
                        .map(|s| format!("did you mean '{s}'?"));
                        diag.emit(sem_err_with_help(
                            format!("struct `{}` has no field `{}`", struct_name, field),
                            Span { start: 0, end: 0 },
                            help,
                        ));
                        return;
                    };
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(val_ty.clone(), field_ty) {
                            diag.emit(e);
                        }
                    } else if val_ty != field_ty {
                        diag.emit(sem_err(format!(
                            "type mismatch in field assignment: expected `{}`, found `{}`",
                            field_ty, val_ty
                        )));
                    }
                    if let Err(e) = check_assignment_target_mutable(object, resolver) {
                        diag.emit(e);
                    }
                }
                Type::Generic { name, args } => {
                    let struct_info = match resolver.lookup_struct(name) {
                        Some(s) => s.clone(),
                        None => {
                            let help = find_suggestion(name, resolver.all_struct_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined struct `{}`", name),
                                Span { start: 0, end: 0 },
                                help,
                            ));
                            return;
                        }
                    };
                    let subst: HashMap<String, Type> = struct_info
                        .type_params
                        .iter()
                        .zip(args.iter())
                        .map(|(tp, arg)| (tp.name.clone(), arg.clone()))
                        .collect();
                    let field_ty = if let Some(&idx) = struct_info.field_index.get(field.as_str()) {
                        substitute_type(&struct_info.fields[idx].1, &subst)
                    } else if let Some(&idx) = struct_info.computed_index.get(field.as_str()) {
                        let prop = &struct_info.computed[idx];
                        if !prop.has_setter {
                            diag.emit(sem_err(format!(
                                "computed property `{}` is read-only (no setter)",
                                field
                            )));
                            return;
                        }
                        substitute_type(&prop.ty, &subst)
                    } else {
                        let help = find_suggestion(
                            field,
                            struct_info
                                .field_index
                                .keys()
                                .chain(struct_info.computed_index.keys())
                                .map(|s| s.as_str()),
                        )
                        .map(|s| format!("did you mean '{s}'?"));
                        diag.emit(sem_err_with_help(
                            format!("struct `{}` has no field `{}`", name, field),
                            Span { start: 0, end: 0 },
                            help,
                        ));
                        return;
                    };
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(val_ty.clone(), field_ty) {
                            diag.emit(e);
                        }
                    } else if val_ty != field_ty {
                        diag.emit(sem_err(format!(
                            "type mismatch in field assignment: expected `{}`, found `{}`",
                            field_ty, val_ty
                        )));
                    }
                    if let Err(e) = check_assignment_target_mutable(object, resolver) {
                        diag.emit(e);
                    }
                }
                _ => {
                    diag.emit(sem_err(format!(
                        "field assignment on non-struct type `{}`",
                        obj_ty
                    )));
                }
            }
        }
        Stmt::IndexAssign {
            object,
            index,
            value,
        } => {
            let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
            let idx_ty = analyze_expr(index, resolver, ctx.as_deref_mut(), diag);
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            if obj_ty == Type::Error || idx_ty == Type::Error || val_ty == Type::Error {
                return;
            }
            match &obj_ty {
                Type::Array { element, size } => {
                    if !idx_ty.is_integer() {
                        diag.emit(sem_err(format!(
                            "array index must be an integer type, found '{}'",
                            idx_ty
                        )));
                        return;
                    }
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(val_ty.clone(), *element.clone()) {
                            diag.emit(e);
                        }
                    } else if val_ty != **element {
                        diag.emit(sem_err(format!(
                            "type mismatch in index assignment: expected '{}', found '{}'",
                            element, val_ty
                        )));
                    }
                    // Compile-time bounds check for constant indices
                    if let ExprKind::Number(n) = &index.kind {
                        let idx = *n;
                        if idx < 0 || idx as u64 >= *size {
                            diag.emit(sem_err(format!(
                                "array index {} is out of bounds for array of size {}",
                                idx, size
                            )));
                        }
                    }
                    // Check mutability: object must be a mutable variable
                    match &object.kind {
                        ExprKind::Ident(name) => match resolver.lookup_var(name) {
                            Some(info) if !info.mutable => {
                                diag.emit(sem_err(format!(
                                    "cannot assign to index of immutable variable '{}'",
                                    name
                                )));
                            }
                            Some(_) => {}
                            None => {
                                let help = find_suggestion(name, resolver.all_variable_names())
                                    .map(|s| format!("did you mean '{s}'?"));
                                diag.emit(sem_err_with_help(
                                    format!("undefined variable '{}'", name),
                                    Span { start: 0, end: 0 },
                                    help,
                                ));
                            }
                        },
                        _ => {
                            diag.emit(sem_err("cannot assign to index of non-variable expression"));
                        }
                    }
                }
                _ => {
                    diag.emit(sem_err(format!("cannot index into type '{}'", obj_ty)));
                }
            }
        }
    }
}
