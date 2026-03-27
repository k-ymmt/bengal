use std::collections::HashMap;

use crate::error::{DiagCtxt, Span};
use crate::parser::ast::*;
use crate::suggest::find_suggestion;

use super::expr_analysis::analyze_expr;
use super::infer::InferenceContext;
use super::resolver::Resolver;
use super::types::Type;
use super::{sem_err, sem_err_with_help, substitute_type};

pub(super) fn analyze_method_call_expr(
    expr: &Expr,
    object: &Expr,
    method: &str,
    args: &[Expr],
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Type {
    let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
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
            let method_info = match struct_info.method_index.get(method) {
                Some(&idx) => struct_info.methods[idx].clone(),
                None => {
                    let help = find_suggestion(
                        method,
                        struct_info.method_index.keys().map(|s| s.as_str()),
                    )
                    .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("type `{}` has no method `{}`", struct_name, method),
                        expr.span,
                        help,
                    ));
                    return Type::Error;
                }
            };
            if args.len() != method_info.params.len() {
                diag.emit(sem_err(format!(
                    "method `{}` expects {} argument(s) but {} were given",
                    method,
                    method_info.params.len(),
                    args.len()
                )));
                return Type::Error;
            }
            let mut any_arg_error = false;
            for (arg, (param_name, param_ty)) in args.iter().zip(method_info.params.iter()) {
                let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut(), diag);
                if arg_ty == Type::Error {
                    any_arg_error = true;
                    continue;
                }
                if let Some(ref mut c) = ctx {
                    if !matches!(param_ty, Type::TypeParam { .. })
                        && let Err(e) = c.unify(arg_ty.clone(), param_ty.clone())
                    {
                        diag.emit(e);
                        any_arg_error = true;
                    }
                } else if arg_ty != *param_ty {
                    diag.emit(sem_err(format!(
                        "expected `{}` but got `{}` in argument `{}` of method `{}`",
                        param_ty, arg_ty, param_name, method
                    )));
                    any_arg_error = true;
                }
            }
            if any_arg_error {
                return Type::Error;
            }
            method_info.return_type
        }
        Type::Generic {
            name,
            args: type_args,
        } => {
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
            // Build substitution map: type_param_name -> actual type arg
            let subst: HashMap<String, Type> = struct_info
                .type_params
                .iter()
                .zip(type_args.iter())
                .map(|(tp, arg)| (tp.name.clone(), arg.clone()))
                .collect();
            let method_info = match struct_info.method_index.get(method) {
                Some(&idx) => struct_info.methods[idx].clone(),
                None => {
                    let help = find_suggestion(
                        method,
                        struct_info.method_index.keys().map(|s| s.as_str()),
                    )
                    .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("type `{}` has no method `{}`", name, method),
                        expr.span,
                        help,
                    ));
                    return Type::Error;
                }
            };
            if args.len() != method_info.params.len() {
                diag.emit(sem_err(format!(
                    "method `{}` expects {} argument(s) but {} were given",
                    method,
                    method_info.params.len(),
                    args.len()
                )));
                return Type::Error;
            }
            let mut any_arg_error = false;
            for (arg, (param_name, param_ty)) in args.iter().zip(method_info.params.iter()) {
                let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut(), diag);
                if arg_ty == Type::Error {
                    any_arg_error = true;
                    continue;
                }
                let expected_ty = substitute_type(param_ty, &subst);
                if let Some(ref mut c) = ctx {
                    if !matches!(expected_ty, Type::TypeParam { .. })
                        && let Err(e) = c.unify(arg_ty.clone(), expected_ty)
                    {
                        diag.emit(e);
                        any_arg_error = true;
                    }
                } else if arg_ty != expected_ty {
                    diag.emit(sem_err(format!(
                        "expected `{}` but got `{}` in argument `{}` of method `{}`",
                        expected_ty, arg_ty, param_name, method
                    )));
                    any_arg_error = true;
                }
            }
            if any_arg_error {
                return Type::Error;
            }
            substitute_type(&method_info.return_type, &subst)
        }
        Type::TypeParam {
            name: _,
            bound: Some(proto),
        } => {
            let proto_info = match resolver.lookup_protocol(proto) {
                Some(p) => p.clone(),
                None => {
                    let help = find_suggestion(proto, resolver.all_protocol_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("undefined protocol `{}`", proto),
                        expr.span,
                        help,
                    ));
                    return Type::Error;
                }
            };
            let method_sig = match proto_info.methods.iter().find(|m| m.name == *method) {
                Some(sig) => sig.clone(),
                None => {
                    let help =
                        find_suggestion(method, proto_info.methods.iter().map(|m| m.name.as_str()))
                            .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("protocol `{}` has no method `{}`", proto, method),
                        expr.span,
                        help,
                    ));
                    return Type::Error;
                }
            };
            if args.len() != method_sig.params.len() {
                diag.emit(sem_err(format!(
                    "method `{}` expects {} argument(s) but {} were given",
                    method,
                    method_sig.params.len(),
                    args.len()
                )));
                return Type::Error;
            }
            let mut any_arg_error = false;
            for (arg, param) in args.iter().zip(method_sig.params.iter()) {
                let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut(), diag);
                if arg_ty == Type::Error {
                    any_arg_error = true;
                    continue;
                }
                if let Some(ref mut c) = ctx {
                    if !matches!(param.1, Type::TypeParam { .. })
                        && let Err(e) = c.unify(arg_ty.clone(), param.1.clone())
                    {
                        diag.emit(e);
                        any_arg_error = true;
                    }
                } else if arg_ty != param.1 {
                    diag.emit(sem_err(format!(
                        "expected `{}` but got `{}` in argument `{}` of method `{}`",
                        param.1, arg_ty, param.0, method
                    )));
                    any_arg_error = true;
                }
            }
            if any_arg_error {
                return Type::Error;
            }
            method_sig.return_type.clone()
        }
        Type::TypeParam { name, bound: None } => {
            diag.emit(sem_err(format!(
                "method call on unconstrained type parameter `{}`",
                name
            )));
            Type::Error
        }
        _ => {
            diag.emit(sem_err(format!(
                "method call on non-struct type `{}`",
                obj_ty
            )));
            Type::Error
        }
    }
}
