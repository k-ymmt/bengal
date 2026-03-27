use crate::error::DiagCtxt;
use crate::parser::ast::*;

use super::expr_analysis::analyze_expr;
use super::infer::InferenceContext;
use super::resolver::{Resolver, VarInfo};
use super::sem_err;
use super::stmt_analysis::analyze_stmt;
use super::types::Type;
use super::{block_always_returns, resolve_type_checked};

pub(super) fn analyze_function(
    func: &Function,
    resolver: &mut Resolver,
    ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) {
    // Push type params into scope for the duration of this function analysis
    resolver.push_type_params(&func.type_params);

    let return_type = match resolve_type_checked(&func.return_type, resolver) {
        Ok(t) => t,
        Err(e) => {
            diag.emit(e);
            resolver.pop_type_params(func.type_params.len());
            return;
        }
    };
    resolver.current_return_type = Some(return_type.clone());
    resolver.push_scope();

    // Register function parameters as immutable variables
    for param in &func.params {
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

    let body = func.body.as_ref().unwrap();
    let stmts = &body.stmts;

    // Check that all paths end with a return
    if !block_always_returns(body) {
        diag.emit(sem_err(format!(
            "function `{}` must end with a `return` statement",
            func.name
        )));
        resolver.pop_scope();
        resolver.current_return_type = None;
        resolver.pop_type_params(func.type_params.len());
        return;
    }

    let mut ctx = ctx;
    for stmt in stmts.iter() {
        // Yield is not allowed in function bodies
        if matches!(stmt, Stmt::Yield(_)) {
            diag.emit(sem_err(
                "`yield` cannot be used in function body (use `return` instead)",
            ));
            resolver.pop_scope();
            resolver.current_return_type = None;
            resolver.pop_type_params(func.type_params.len());
            return;
        }

        analyze_stmt(stmt, resolver, ctx.as_deref_mut(), diag);
    }

    resolver.pop_scope();
    resolver.current_return_type = None;
    resolver.pop_type_params(func.type_params.len());
}

/// Analyze a block expression (Expr::Block) — yield required, return forbidden
pub(super) fn analyze_block_expr(
    block: &Block,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Type {
    resolver.push_scope();

    let stmts = &block.stmts;

    if stmts.is_empty() {
        diag.emit(sem_err(
            "block expression must end with a `yield` statement",
        ));
        resolver.pop_scope();
        return Type::Error;
    }

    // Check that the last statement is Yield
    if !matches!(stmts.last(), Some(Stmt::Yield(_))) {
        diag.emit(sem_err(
            "block expression must end with a `yield` statement",
        ));
        resolver.pop_scope();
        return Type::Error;
    }

    let mut yield_type = Type::I32; // will be overwritten

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Return is not allowed in block expressions
        if matches!(stmt, Stmt::Return(_)) {
            diag.emit(sem_err("`return` cannot be used inside a block expression"));
            resolver.pop_scope();
            return Type::Error;
        }

        // Yield is only allowed as the last statement
        if matches!(stmt, Stmt::Yield(_)) && !is_last {
            diag.emit(sem_err(
                "`yield` must be the last statement in the block expression",
            ));
            resolver.pop_scope();
            return Type::Error;
        }

        analyze_stmt(stmt, resolver, ctx.as_deref_mut(), diag);

        // If this is the Yield statement, get the type
        if let Stmt::Yield(expr) = stmt {
            yield_type = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
        }
    }

    resolver.pop_scope();
    yield_type
}

/// Analyze a control block (if then/else) — yield and return both allowed.
/// Returns Some(type) if block yields a value, None if block diverges via return.
pub(super) fn analyze_control_block(
    block: &Block,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Option<Type> {
    resolver.push_scope();

    let stmts = &block.stmts;

    if stmts.is_empty() {
        resolver.pop_scope();
        return Some(Type::Unit);
    }

    let mut result: Option<Type> = None;

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Yield is only allowed as the last statement
        if matches!(stmt, Stmt::Yield(_)) && !is_last {
            diag.emit(sem_err("`yield` must be the last statement in the block"));
            resolver.pop_scope();
            return Some(Type::Error);
        }

        analyze_stmt(stmt, resolver, ctx.as_deref_mut(), diag);

        if is_last {
            match stmt {
                Stmt::Yield(expr) => {
                    let ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
                    result = Some(ty);
                }
                Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue => {
                    // Block diverges (control flow exits)
                    result = None;
                }
                _ => {
                    result = Some(Type::Unit);
                }
            }
        }
    }

    resolver.pop_scope();
    result
}

/// Analyze a loop body block — return allowed, yield forbidden.
pub(super) fn analyze_loop_block(
    block: &Block,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) {
    resolver.push_scope();

    for stmt in &block.stmts {
        if matches!(stmt, Stmt::Yield(_)) {
            diag.emit(sem_err("`yield` cannot be used in a while loop body"));
            resolver.pop_scope();
            return;
        }
        analyze_stmt(stmt, resolver, ctx.as_deref_mut(), diag);
    }

    resolver.pop_scope();
}
