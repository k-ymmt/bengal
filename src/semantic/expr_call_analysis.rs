use std::collections::HashMap;

use crate::error::{DiagCtxt, Span};
use crate::parser::ast::*;
use crate::suggest::find_suggestion;

use super::expr_analysis::analyze_expr;
use super::infer::{self, InferVarId, InferenceContext};
use super::resolver::Resolver;
use super::types::Type;
use super::{resolve_type_checked, sem_err, sem_err_with_help, substitute_type, types_compatible};

pub(super) fn analyze_call_expr(
    expr: &Expr,
    name: &str,
    type_args: &[TypeAnnotation],
    args: &[Expr],
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Type {
    // Empty-arg call may be a struct init
    if args.is_empty()
        && let Some(struct_info) = resolver.lookup_struct(name)
    {
        let struct_info = struct_info.clone();
        if struct_info.init.params.is_empty() {
            resolver.record_struct_init_call(expr.id);
            return Type::Struct(name.to_string());
        } else {
            diag.emit(sem_err(format!(
                "struct `{}` initializer expects {} arguments, but 0 were given",
                name,
                struct_info.init.params.len()
            )));
            return Type::Error;
        }
    }
    let sig = match resolver.lookup_func(name) {
        Some(s) => s.clone(),
        None => {
            let help = find_suggestion(name, resolver.all_function_names())
                .map(|s| format!("did you mean '{s}'?"));
            diag.emit(sem_err_with_help(
                format!("undefined function `{}`", name),
                expr.span,
                help,
            ));
            return Type::Error;
        }
    };
    if args.len() != sig.params.len() {
        diag.emit(sem_err(format!(
            "function `{}` expects {} arguments, but {} were given",
            name,
            sig.params.len(),
            args.len()
        )));
        return Type::Error;
    }

    // Build type param substitution map
    let subst: HashMap<String, Type> = if !type_args.is_empty() {
        // Explicit type args provided
        let mut map = HashMap::new();
        let mut failed = false;
        for (tp, ta) in sig.type_params.iter().zip(type_args.iter()) {
            match resolve_type_checked(ta, resolver) {
                Ok(resolved) => {
                    map.insert(tp.name.clone(), resolved);
                }
                Err(e) => {
                    diag.emit(e);
                    failed = true;
                }
            }
        }
        if failed {
            return Type::Error;
        }
        map
    } else if !sig.type_params.is_empty() {
        if let Some(ref mut c) = ctx {
            // Inference mode: create InferVars for each type param
            let var_ids: Vec<InferVarId> = sig
                .type_params
                .iter()
                .map(|tp| {
                    c.fresh_var_with_provenance(infer::VarProvenance {
                        type_param_name: tp.name.clone(),
                        def_name: name.to_string(),
                        arg_name: None,
                        span: expr.span,
                    })
                })
                .collect();
            c.register_call_site(
                expr.id,
                var_ids.clone(),
                sig.type_params.clone(),
                name.to_string(),
            );
            sig.type_params
                .iter()
                .zip(var_ids.iter())
                .map(|(tp, &id)| (tp.name.clone(), Type::InferVar(id)))
                .collect()
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    let mut any_arg_error = false;
    for (arg, (param_name, expected_ty)) in args.iter().zip(sig.params.iter()) {
        let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut(), diag);
        if arg_ty == Type::Error {
            any_arg_error = true;
            continue;
        }
        let effective_ty = substitute_type(expected_ty, &subst);
        if let Some(ref mut c) = ctx {
            if let Type::InferVar(id) = &effective_ty {
                c.update_arg_name(*id, param_name.clone());
            }
            if let Type::IntegerLiteral(id) | Type::FloatLiteral(id) = &arg_ty {
                c.set_provenance(
                    *id,
                    infer::VarProvenance {
                        type_param_name: String::new(),
                        def_name: name.to_string(),
                        arg_name: Some(param_name.clone()),
                        span: arg.span,
                    },
                );
            }
            // In inference mode, unify arg type with expected parameter type
            if let Err(e) = c.unify(arg_ty.clone(), effective_ty) {
                diag.emit(e);
                any_arg_error = true;
            }
        } else if !types_compatible(&arg_ty, &effective_ty) {
            diag.emit(sem_err(format!(
                "argument type mismatch: expected `{}`, found `{}`",
                effective_ty, arg_ty
            )));
            any_arg_error = true;
        }
    }
    if any_arg_error {
        return Type::Error;
    }
    substitute_type(&sig.return_type, &subst)
}

pub(super) fn analyze_struct_init_expr(
    expr: &Expr,
    name: &str,
    type_args: &[TypeAnnotation],
    args: &[(String, Expr)],
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Type {
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
            return Type::Error;
        }
    };
    let init = &struct_info.init;
    if args.len() != init.params.len() {
        diag.emit(sem_err(format!(
            "struct `{}` initializer expects {} arguments, but {} were given",
            name,
            init.params.len(),
            args.len()
        )));
        return Type::Error;
    }

    // Build type param substitution map
    let subst: HashMap<String, Type> = if !type_args.is_empty() {
        // Explicit type args provided
        let mut map = HashMap::new();
        let mut failed = false;
        for (tp, ta) in struct_info.type_params.iter().zip(type_args.iter()) {
            match resolve_type_checked(ta, resolver) {
                Ok(resolved) => {
                    map.insert(tp.name.clone(), resolved);
                }
                Err(e) => {
                    diag.emit(e);
                    failed = true;
                }
            }
        }
        if failed {
            return Type::Error;
        }
        map
    } else if !struct_info.type_params.is_empty() {
        if let Some(ref mut c) = ctx {
            // Inference mode: create InferVars for each type param
            let var_ids: Vec<InferVarId> = struct_info
                .type_params
                .iter()
                .map(|tp| {
                    c.fresh_var_with_provenance(infer::VarProvenance {
                        type_param_name: tp.name.clone(),
                        def_name: name.to_string(),
                        arg_name: None,
                        span: expr.span,
                    })
                })
                .collect();
            c.register_call_site(
                expr.id,
                var_ids.clone(),
                struct_info.type_params.clone(),
                name.to_string(),
            );
            struct_info
                .type_params
                .iter()
                .zip(var_ids.iter())
                .map(|(tp, &id)| (tp.name.clone(), Type::InferVar(id)))
                .collect()
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    let mut any_arg_error = false;
    for ((label, arg_expr), (param_name, param_ty)) in args.iter().zip(init.params.iter()) {
        if label != param_name {
            diag.emit(sem_err(format!(
                "expected argument label `{}`, found `{}`",
                param_name, label
            )));
            any_arg_error = true;
            continue;
        }
        let arg_ty = analyze_expr(arg_expr, resolver, ctx.as_deref_mut(), diag);
        if arg_ty == Type::Error {
            any_arg_error = true;
            continue;
        }
        let effective_ty = substitute_type(param_ty, &subst);
        if let Some(ref mut c) = ctx {
            if let Type::InferVar(id) = &effective_ty {
                c.update_arg_name(*id, param_name.clone());
            }
            if let Type::IntegerLiteral(id) | Type::FloatLiteral(id) = &arg_ty {
                c.set_provenance(
                    *id,
                    infer::VarProvenance {
                        type_param_name: String::new(),
                        def_name: name.to_string(),
                        arg_name: Some(param_name.clone()),
                        span: arg_expr.span,
                    },
                );
            }
            if let Err(e) = c.unify(arg_ty.clone(), effective_ty) {
                diag.emit(e);
                any_arg_error = true;
            }
        } else if !types_compatible(&arg_ty, &effective_ty) {
            diag.emit(sem_err(format!(
                "argument type mismatch: expected `{}`, found `{}`",
                effective_ty, arg_ty
            )));
            any_arg_error = true;
        }
    }
    if any_arg_error {
        return Type::Error;
    }

    // Build the result type
    if subst.is_empty() && struct_info.type_params.is_empty() {
        Type::Struct(name.to_string())
    } else if !subst.is_empty() {
        let args: Vec<Type> = struct_info
            .type_params
            .iter()
            .map(|tp| {
                subst
                    .get(&tp.name)
                    .cloned()
                    .unwrap_or_else(|| Type::TypeParam {
                        name: tp.name.clone(),
                        bound: tp.bound.clone(),
                    })
            })
            .collect();
        Type::Generic {
            name: name.to_string(),
            args,
        }
    } else {
        Type::Struct(name.to_string())
    }
}

pub(super) fn analyze_field_access_expr(
    expr: &Expr,
    object: &Expr,
    field: &str,
    resolver: &mut Resolver,
    ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Type {
    let obj_ty = analyze_expr(object, resolver, ctx, diag);
    if obj_ty == Type::Error {
        return Type::Error;
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
                    return Type::Error;
                }
            };
            if let Some(&idx) = struct_info.field_index.get(field) {
                struct_info.fields[idx].1.clone()
            } else if let Some(&idx) = struct_info.computed_index.get(field) {
                struct_info.computed[idx].ty.clone()
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
                    expr.span,
                    help,
                ));
                Type::Error
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
                    return Type::Error;
                }
            };
            let subst: HashMap<String, Type> = struct_info
                .type_params
                .iter()
                .zip(args.iter())
                .map(|(tp, arg)| (tp.name.clone(), arg.clone()))
                .collect();
            if let Some(&idx) = struct_info.field_index.get(field) {
                substitute_type(&struct_info.fields[idx].1, &subst)
            } else if let Some(&idx) = struct_info.computed_index.get(field) {
                substitute_type(&struct_info.computed[idx].ty, &subst)
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
                    expr.span,
                    help,
                ));
                Type::Error
            }
        }
        _ => {
            diag.emit(sem_err(format!(
                "field access on non-struct type `{}`",
                obj_ty
            )));
            Type::Error
        }
    }
}
