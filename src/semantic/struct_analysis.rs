use std::collections::HashSet;

use crate::error::{DiagCtxt, Result, Span};
use crate::parser::ast::*;
use crate::suggest::find_suggestion;

use super::infer::InferenceContext;
use super::resolver::{self, Resolver, VarInfo};
use super::stmt_analysis::analyze_stmt;
use super::types::Type;
use super::{block_always_returns, resolve_type_checked, sem_err, sem_err_with_help};

pub(super) fn analyze_struct_members(
    struct_def: &StructDef,
    resolver: &mut Resolver,
    ctx: &mut InferenceContext,
    diag: &mut DiagCtxt,
) {
    use resolver::SelfContext;

    for member in &struct_def.members {
        match member {
            StructMember::Initializer {
                visibility: _,
                params,
                body,
            } => {
                let prev_self = resolver.self_context.clone();
                resolver.self_context = Some(SelfContext {
                    struct_name: struct_def.name.clone(),
                    mutable: true,
                });
                let prev_return = resolver.current_return_type.clone();
                resolver.current_return_type = Some(Type::Unit);

                resolver.push_scope();
                for param in params {
                    let param_ty = match resolve_type_checked(&param.ty, resolver) {
                        Ok(t) => t,
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    };
                    resolver.define_var(
                        param.name.clone(),
                        VarInfo {
                            ty: param_ty,
                            mutable: false,
                        },
                    );
                }
                let body_block = body.as_ref().unwrap();
                for stmt in &body_block.stmts {
                    analyze_stmt(stmt, resolver, Some(ctx), diag);
                }
                resolver.pop_scope();

                if let Err(e) = check_all_fields_initialized(&struct_def.name, body_block, resolver)
                {
                    diag.emit(e);
                }

                resolver.current_return_type = prev_return;
                resolver.self_context = prev_self;
            }
            StructMember::ComputedProperty {
                ty, getter, setter, ..
            } => {
                let resolved_ty = match resolve_type_checked(ty, resolver) {
                    Ok(t) => t,
                    Err(e) => {
                        diag.emit(e);
                        continue;
                    }
                };

                // Analyze getter
                {
                    let prev_self = resolver.self_context.clone();
                    resolver.self_context = Some(SelfContext {
                        struct_name: struct_def.name.clone(),
                        mutable: false,
                    });
                    let prev_return = resolver.current_return_type.clone();
                    resolver.current_return_type = Some(resolved_ty.clone());

                    resolver.push_scope();
                    analyze_getter_block(getter.as_ref().unwrap(), resolver, ctx, diag);
                    resolver.pop_scope();

                    resolver.current_return_type = prev_return;
                    resolver.self_context = prev_self;
                }

                // Analyze setter
                if let Some(setter_block) = setter {
                    let prev_self = resolver.self_context.clone();
                    resolver.self_context = Some(SelfContext {
                        struct_name: struct_def.name.clone(),
                        mutable: true,
                    });
                    let prev_return = resolver.current_return_type.clone();
                    resolver.current_return_type = Some(Type::Unit);

                    resolver.push_scope();
                    resolver.define_var(
                        "newValue".to_string(),
                        VarInfo {
                            ty: resolved_ty.clone(),
                            mutable: false,
                        },
                    );
                    for stmt in &setter_block.stmts {
                        analyze_stmt(stmt, resolver, Some(ctx), diag);
                    }
                    resolver.pop_scope();

                    resolver.current_return_type = prev_return;
                    resolver.self_context = prev_self;
                }
            }
            StructMember::StoredProperty { .. } => {}
            StructMember::Method {
                visibility: _,
                name: mname,
                params,
                return_type,
                body,
            } => {
                let resolved_return = match resolve_type_checked(return_type, resolver) {
                    Ok(t) => t,
                    Err(e) => {
                        diag.emit(e);
                        continue;
                    }
                };
                let prev_self = resolver.self_context.clone();
                resolver.self_context = Some(SelfContext {
                    struct_name: struct_def.name.clone(),
                    mutable: false,
                });
                let prev_return = resolver.current_return_type.clone();
                resolver.current_return_type = Some(resolved_return);

                resolver.push_scope();
                for param in params {
                    let param_ty = match resolve_type_checked(&param.ty, resolver) {
                        Ok(t) => t,
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    };
                    resolver.define_var(
                        param.name.clone(),
                        VarInfo {
                            ty: param_ty,
                            mutable: false,
                        },
                    );
                }

                let body_block = body.as_ref().unwrap();
                if !block_always_returns(body_block) {
                    diag.emit(sem_err(format!(
                        "method `{}` must end with a `return` statement",
                        mname
                    )));
                    resolver.pop_scope();
                    resolver.current_return_type = prev_return;
                    resolver.self_context = prev_self;
                    continue;
                }
                let stmts = &body_block.stmts;
                for stmt in stmts {
                    if matches!(stmt, Stmt::Yield(_)) {
                        diag.emit(sem_err(
                            "`yield` cannot be used in method body (use `return` instead)",
                        ));
                        break;
                    }
                    analyze_stmt(stmt, resolver, Some(ctx), diag);
                }

                resolver.pop_scope();
                resolver.current_return_type = prev_return;
                resolver.self_context = prev_self;
            }
        }
    }
}

pub(super) fn check_all_fields_initialized(
    struct_name: &str,
    body: &Block,
    resolver: &Resolver,
) -> Result<()> {
    let struct_info = resolver
        .lookup_struct(struct_name)
        .ok_or_else(|| {
            let help = find_suggestion(struct_name, resolver.all_struct_names())
                .map(|s| format!("did you mean '{s}'?"));
            sem_err_with_help(
                format!("undefined struct `{}`", struct_name),
                Span { start: 0, end: 0 },
                help,
            )
        })?
        .clone();

    let mut initialized: HashSet<String> = HashSet::new();
    for stmt in &body.stmts {
        if matches!(stmt, Stmt::Return(_)) {
            break;
        }
        if let Stmt::FieldAssign { object, field, .. } = stmt
            && matches!(object.kind, ExprKind::SelfRef)
        {
            initialized.insert(field.clone());
        }
    }

    for (field_name, _) in &struct_info.fields {
        if !initialized.contains(field_name) {
            return Err(sem_err(format!(
                "stored property `{}` not initialized in `{}` initializer",
                field_name, struct_name
            )));
        }
    }

    Ok(())
}

fn analyze_getter_block(
    block: &Block,
    resolver: &mut Resolver,
    ctx: &mut InferenceContext,
    diag: &mut DiagCtxt,
) {
    if !block_always_returns(block) {
        diag.emit(sem_err("getter must end with a `return` statement"));
        return;
    }
    for stmt in &block.stmts {
        analyze_stmt(stmt, resolver, Some(ctx), diag);
    }
}

pub(super) fn check_assignment_target_mutable(expr: &Expr, resolver: &Resolver) -> Result<()> {
    match &expr.kind {
        ExprKind::Ident(name) => match resolver.lookup_var(name) {
            Some(info) if !info.mutable => Err(sem_err(format!(
                "cannot assign to field of immutable variable `{}`",
                name
            ))),
            Some(_) => Ok(()),
            None => {
                let help = find_suggestion(name, resolver.all_variable_names())
                    .map(|s| format!("did you mean '{s}'?"));
                Err(sem_err_with_help(
                    format!("undefined variable `{}`", name),
                    expr.span,
                    help,
                ))
            }
        },
        ExprKind::FieldAccess { object, .. } => check_assignment_target_mutable(object, resolver),
        ExprKind::SelfRef => match &resolver.self_context {
            Some(ctx) if ctx.mutable => Ok(()),
            _ => Err(sem_err("`self` is not mutable in this context")),
        },
        _ => Err(sem_err("invalid assignment target")),
    }
}
