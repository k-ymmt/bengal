pub mod resolver;
pub mod types;

use crate::error::{BengalError, Result, Span};
use crate::parser::ast::*;
use resolver::{FuncSig, Resolver, VarInfo};
use types::{resolve_type, Type};

fn sem_err(message: impl Into<String>) -> BengalError {
    BengalError::SemanticError {
        message: message.into(),
        span: Span { start: 0, end: 0 },
    }
}

pub fn analyze(program: &Program) -> Result<()> {
    let mut resolver = Resolver::new();

    // Pass 1: collect all function signatures
    for func in &program.functions {
        let params: Vec<Type> = func.params.iter().map(|p| resolve_type(&p.ty)).collect();
        let return_type = resolve_type(&func.return_type);
        resolver.define_func(func.name.clone(), FuncSig { params, return_type });
    }

    // Pass 2: verify main function exists with correct signature
    match resolver.lookup_func("main") {
        None => return Err(sem_err("no `main` function found")),
        Some(sig) => {
            if !sig.params.is_empty() {
                return Err(sem_err("`main` function must have no parameters"));
            }
            if sig.return_type != Type::I32 {
                return Err(sem_err("`main` function must return `i32`"));
            }
        }
    }

    // Pass 3: analyze each function body
    for func in &program.functions {
        analyze_function(func, &mut resolver)?;
    }

    Ok(())
}

fn analyze_function(func: &Function, resolver: &mut Resolver) -> Result<()> {
    let return_type = resolve_type(&func.return_type);
    resolver.current_return_type = Some(return_type.clone());
    resolver.push_scope();

    // Register function parameters as immutable variables
    for param in &func.params {
        resolver.define_var(
            param.name.clone(),
            VarInfo {
                ty: resolve_type(&param.ty),
                mutable: false,
            },
        );
    }

    let stmts = &func.body.stmts;

    if stmts.is_empty() {
        return Err(sem_err(format!(
            "function `{}` must end with a `return` statement",
            func.name
        )));
    }

    // Check that the last statement is Return
    if !matches!(stmts.last(), Some(Stmt::Return(_))) {
        return Err(sem_err(format!(
            "function `{}` must end with a `return` statement",
            func.name
        )));
    }

    for (_i, stmt) in stmts.iter().enumerate() {
        // Yield is not allowed in function bodies
        if matches!(stmt, Stmt::Yield(_)) {
            return Err(sem_err(
                "`yield` cannot be used in function body (use `return` instead)",
            ));
        }

        analyze_stmt(stmt, resolver)?;
    }

    resolver.pop_scope();
    resolver.current_return_type = None;
    Ok(())
}

/// Analyze a block expression (Expr::Block) — yield required, return forbidden
fn analyze_block_expr(block: &Block, resolver: &mut Resolver) -> Result<Type> {
    resolver.push_scope();

    let stmts = &block.stmts;

    if stmts.is_empty() {
        return Err(sem_err("block expression must end with a `yield` statement"));
    }

    // Check that the last statement is Yield
    if !matches!(stmts.last(), Some(Stmt::Yield(_))) {
        return Err(sem_err("block expression must end with a `yield` statement"));
    }

    let mut yield_type = Type::I32; // will be overwritten

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Return is not allowed in block expressions
        if matches!(stmt, Stmt::Return(_)) {
            return Err(sem_err(
                "`return` cannot be used inside a block expression",
            ));
        }

        // Yield is only allowed as the last statement
        if matches!(stmt, Stmt::Yield(_)) && !is_last {
            return Err(sem_err(
                "`yield` must be the last statement in the block expression",
            ));
        }

        analyze_stmt(stmt, resolver)?;

        // If this is the Yield statement, get the type
        if let Stmt::Yield(expr) = stmt {
            yield_type = analyze_expr(expr, resolver)?;
        }
    }

    resolver.pop_scope();
    Ok(yield_type)
}

/// Analyze a control block (if then/else) — yield and return both allowed.
/// Returns Some(type) if block yields a value, None if block diverges via return.
fn analyze_control_block(block: &Block, resolver: &mut Resolver) -> Result<Option<Type>> {
    resolver.push_scope();

    let stmts = &block.stmts;

    if stmts.is_empty() {
        resolver.pop_scope();
        return Ok(Some(Type::Unit));
    }

    let mut result: Option<Type> = None;

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Yield is only allowed as the last statement
        if matches!(stmt, Stmt::Yield(_)) && !is_last {
            return Err(sem_err(
                "`yield` must be the last statement in the block",
            ));
        }

        analyze_stmt(stmt, resolver)?;

        if is_last {
            match stmt {
                Stmt::Yield(expr) => {
                    let ty = analyze_expr(expr, resolver)?;
                    result = Some(ty);
                }
                Stmt::Return(_) => {
                    // Block diverges (return exits function)
                    result = None;
                }
                _ => {
                    result = Some(Type::Unit);
                }
            }
        }
    }

    resolver.pop_scope();
    Ok(result)
}

/// Analyze a loop body block — return allowed, yield forbidden.
fn analyze_loop_block(block: &Block, resolver: &mut Resolver) -> Result<()> {
    resolver.push_scope();

    for stmt in &block.stmts {
        if matches!(stmt, Stmt::Yield(_)) {
            return Err(sem_err(
                "`yield` cannot be used in a while loop body",
            ));
        }
        analyze_stmt(stmt, resolver)?;
    }

    resolver.pop_scope();
    Ok(())
}

fn analyze_stmt(stmt: &Stmt, resolver: &mut Resolver) -> Result<()> {
    match stmt {
        Stmt::Let { name, ty, value } => {
            let val_ty = analyze_expr(value, resolver)?;
            let var_ty = resolve_type(ty);
            if val_ty != var_ty {
                return Err(sem_err(format!(
                    "type mismatch: expected `{:?}`, found `{:?}`",
                    var_ty, val_ty
                )));
            }
            resolver.define_var(
                name.clone(),
                VarInfo {
                    ty: var_ty,
                    mutable: false,
                },
            );
        }
        Stmt::Var { name, ty, value } => {
            let val_ty = analyze_expr(value, resolver)?;
            let var_ty = resolve_type(ty);
            if val_ty != var_ty {
                return Err(sem_err(format!(
                    "type mismatch: expected `{:?}`, found `{:?}`",
                    var_ty, val_ty
                )));
            }
            resolver.define_var(
                name.clone(),
                VarInfo {
                    ty: var_ty,
                    mutable: true,
                },
            );
        }
        Stmt::Assign { name, value } => {
            let val_ty = analyze_expr(value, resolver)?;
            match resolver.lookup_var(name) {
                None => {
                    return Err(sem_err(format!("undefined variable `{}`", name)));
                }
                Some(info) => {
                    if !info.mutable {
                        return Err(sem_err(format!(
                            "cannot assign to immutable variable `{}`",
                            name
                        )));
                    }
                    if val_ty != info.ty {
                        return Err(sem_err(format!(
                            "type mismatch in assignment: expected `{:?}`, found `{:?}`",
                            info.ty, val_ty
                        )));
                    }
                }
            }
        }
        Stmt::Return(Some(expr)) => {
            let ty = analyze_expr(expr, resolver)?;
            if let Some(ref return_type) = resolver.current_return_type {
                if ty != *return_type {
                    return Err(sem_err(format!(
                        "return type mismatch: expected `{:?}`, found `{:?}`",
                        return_type, ty
                    )));
                }
            }
        }
        Stmt::Return(None) => {
            if let Some(ref return_type) = resolver.current_return_type {
                if *return_type != Type::Unit {
                    return Err(sem_err(format!(
                        "return type mismatch: expected `{:?}`, found `Unit`",
                        return_type
                    )));
                }
            }
        }
        Stmt::Yield(expr) => {
            let _ty = analyze_expr(expr, resolver)?;
        }
        Stmt::Expr(expr) => {
            let _ty = analyze_expr(expr, resolver)?;
        }
    }
    Ok(())
}

fn analyze_expr(expr: &Expr, resolver: &mut Resolver) -> Result<Type> {
    match expr {
        Expr::Number(_) => Ok(Type::I32),
        Expr::Bool(_) => Ok(Type::Bool),
        Expr::Ident(name) => match resolver.lookup_var(name) {
            Some(info) => Ok(info.ty.clone()),
            None => Err(sem_err(format!("undefined variable `{}`", name))),
        },
        Expr::UnaryOp { op, operand } => {
            let operand_ty = analyze_expr(operand, resolver)?;
            match op {
                UnaryOp::Not => {
                    if operand_ty != Type::Bool {
                        return Err(sem_err("operand of `!` must be `bool`"));
                    }
                    Ok(Type::Bool)
                }
            }
        }
        Expr::BinaryOp { op, left, right } => {
            let left_ty = analyze_expr(left, resolver)?;
            let right_ty = analyze_expr(right, resolver)?;
            match op {
                // Arithmetic: i32 x i32 → i32
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    if left_ty != Type::I32 || right_ty != Type::I32 {
                        return Err(sem_err("arithmetic operation requires `i32` operands"));
                    }
                    Ok(Type::I32)
                }
                // Comparison: i32 x i32 → bool
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    if left_ty != Type::I32 || right_ty != Type::I32 {
                        return Err(sem_err("comparison operation requires `i32` operands"));
                    }
                    Ok(Type::Bool)
                }
                // Logical: bool x bool → bool
                BinOp::And | BinOp::Or => {
                    if left_ty != Type::Bool || right_ty != Type::Bool {
                        return Err(sem_err("logical operation requires `bool` operands"));
                    }
                    Ok(Type::Bool)
                }
            }
        }
        Expr::Call { name, args } => {
            let sig = resolver
                .lookup_func(name)
                .ok_or_else(|| sem_err(format!("undefined function `{}`", name)))?
                .clone();
            if args.len() != sig.params.len() {
                return Err(sem_err(format!(
                    "function `{}` expects {} arguments, but {} were given",
                    name,
                    sig.params.len(),
                    args.len()
                )));
            }
            for (arg, expected_ty) in args.iter().zip(sig.params.iter()) {
                let arg_ty = analyze_expr(arg, resolver)?;
                if arg_ty != *expected_ty {
                    return Err(sem_err(format!(
                        "argument type mismatch: expected `{:?}`, found `{:?}`",
                        expected_ty, arg_ty
                    )));
                }
            }
            Ok(sig.return_type.clone())
        }
        Expr::Block(block) => analyze_block_expr(block, resolver),
        Expr::If {
            condition,
            then_block,
            else_block,
        } => {
            let cond_ty = analyze_expr(condition, resolver)?;
            if cond_ty != Type::Bool {
                return Err(sem_err("if condition must be `bool`"));
            }

            let then_ty = analyze_control_block(then_block, resolver)?;

            match else_block {
                Some(else_blk) => {
                    let else_ty = analyze_control_block(else_blk, resolver)?;
                    // Type merging with divergence
                    match (then_ty, else_ty) {
                        (Some(t1), Some(t2)) => {
                            if t1 != t2 {
                                return Err(sem_err(format!(
                                    "if/else branch type mismatch: `{:?}` vs `{:?}`",
                                    t1, t2
                                )));
                            }
                            Ok(t1)
                        }
                        (None, Some(t)) => Ok(t), // then diverges, use else type
                        (Some(t), None) => Ok(t), // else diverges, use then type
                        (None, None) => Ok(Type::Unit), // both diverge
                    }
                }
                None => {
                    // if without else: type is Unit
                    if let Some(ref ty) = then_ty {
                        if *ty != Type::Unit {
                            return Err(sem_err(
                                "if without else must have unit type (use `yield` in both branches for a value)",
                            ));
                        }
                    }
                    Ok(Type::Unit)
                }
            }
        }
        Expr::While { condition, body } => {
            let cond_ty = analyze_expr(condition, resolver)?;
            if cond_ty != Type::Bool {
                return Err(sem_err("while condition must be `bool`"));
            }
            analyze_loop_block(body, resolver)?;
            Ok(Type::Unit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn analyze_str(input: &str) -> Result<()> {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        analyze(&program)
    }

    // --- Phase 2 normal cases (maintained) ---

    #[test]
    fn ok_let_and_return() {
        assert!(analyze_str("func main() -> i32 { let x: i32 = 10; return x; }").is_ok());
    }

    #[test]
    fn ok_var_and_assign() {
        assert!(
            analyze_str("func main() -> i32 { var x: i32 = 1; x = 2; return x; }").is_ok()
        );
    }

    #[test]
    fn ok_block_expr_yield() {
        assert!(
            analyze_str("func main() -> i32 { let x: i32 = { yield 10; }; return x; }")
                .is_ok()
        );
    }

    // --- Phase 3 normal cases ---

    #[test]
    fn ok_if_else() {
        assert!(analyze_str(
            "func main() -> i32 { let x: i32 = if true { yield 1; } else { yield 2; }; return x; }"
        ).is_ok());
    }

    #[test]
    fn ok_while() {
        assert!(analyze_str("func main() -> i32 { while false { }; return 0; }").is_ok());
    }

    #[test]
    fn ok_early_return() {
        assert!(
            analyze_str("func main() -> i32 { if 1 < 2 { return 10; }; return 20; }").is_ok()
        );
    }

    #[test]
    fn ok_diverging_then() {
        assert!(analyze_str(
            "func main() -> i32 { let x: i32 = if true { return 1; } else { yield 2; }; return x; }"
        ).is_ok());
    }

    #[test]
    fn ok_diverging_else() {
        assert!(analyze_str(
            "func main() -> i32 { let x: i32 = if true { yield 1; } else { return 2; }; return x; }"
        ).is_ok());
    }

    #[test]
    fn ok_unit_func() {
        assert!(analyze_str(
            "func foo() { return; } func main() -> i32 { foo(); return 0; }"
        ).is_ok());
    }

    #[test]
    fn ok_bool_let() {
        assert!(analyze_str(
            "func main() -> i32 { let b: bool = true && false; if b { yield 1; } else { yield 0; }; return 0; }"
        ).is_ok());
    }

    // --- Phase 2 error cases (maintained) ---

    #[test]
    fn err_undefined_variable() {
        let err = analyze_str("func main() -> i32 { return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_immutable_assign() {
        let err =
            analyze_str("func main() -> i32 { let x: i32 = 1; x = 2; return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_no_return() {
        let err = analyze_str("func main() -> i32 { let x: i32 = 1; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_no_yield_in_block() {
        let err = analyze_str(
            "func main() -> i32 { let x: i32 = { let a: i32 = 1; }; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_yield_in_function_body() {
        let err = analyze_str("func main() -> i32 { yield 1; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_return_in_block_expr() {
        let err = analyze_str(
            "func main() -> i32 { let x: i32 = { return 1; }; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_yield_not_last() {
        let err = analyze_str(
            "func main() -> i32 { let x: i32 = { yield 1; let y: i32 = 2; }; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_undefined_function() {
        let err = analyze_str("func main() -> i32 { return foo(1); }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_wrong_arg_count() {
        let err = analyze_str(
            "func add(a: i32, b: i32) -> i32 { return a + b; } func main() -> i32 { return add(1); }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_no_main() {
        let err = analyze_str("func add(a: i32, b: i32) -> i32 { return a + b; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_main_with_params() {
        let err = analyze_str("func main(x: i32) -> i32 { return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    // --- Phase 3 error cases ---

    #[test]
    fn err_if_non_bool_condition() {
        let err = analyze_str(
            "func main() -> i32 { if 1 { yield 1; } else { yield 2; }; return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_if_branch_type_mismatch() {
        let err = analyze_str(
            "func main() -> i32 { if true { yield 1; } else { yield true; }; return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_while_non_bool_condition() {
        let err = analyze_str("func main() -> i32 { while 1 { }; return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_yield_in_while() {
        let err = analyze_str(
            "func main() -> i32 { while true { yield 1; }; return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_let_type_mismatch_bool_to_i32() {
        let err =
            analyze_str("func main() -> i32 { let x: i32 = true; return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_let_type_mismatch_i32_to_bool() {
        let err =
            analyze_str("func main() -> i32 { let x: bool = 42; return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_assign_type_mismatch() {
        let err = analyze_str(
            "func main() -> i32 { var x: i32 = 0; x = false; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }
}
