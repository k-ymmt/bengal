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

    let return_type = resolve_type(&func.return_type);

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Yield is not allowed in function bodies
        if matches!(stmt, Stmt::Yield(_)) {
            return Err(sem_err(
                "`yield` cannot be used in function body (use `return` instead)",
            ));
        }

        // Return is only allowed as the last statement
        if matches!(stmt, Stmt::Return(_)) && !is_last {
            return Err(sem_err("`return` must be the last statement in the function"));
        }

        analyze_stmt(stmt, resolver)?;

        // If this is the Return statement, check type matches
        if let Stmt::Return(expr) = stmt {
            let expr_ty = analyze_expr(expr, resolver)?;
            if expr_ty != return_type {
                return Err(sem_err(format!(
                    "return type mismatch: expected `{:?}`, found `{:?}`",
                    return_type, expr_ty
                )));
            }
        }
    }

    resolver.pop_scope();
    Ok(())
}

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

        // Return is not allowed in block expressions (Phase 2)
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

fn analyze_stmt(stmt: &Stmt, resolver: &mut Resolver) -> Result<()> {
    match stmt {
        Stmt::Let { name, ty, value } => {
            let _val_ty = analyze_expr(value, resolver)?;
            let var_ty = resolve_type(ty);
            resolver.define_var(
                name.clone(),
                VarInfo {
                    ty: var_ty,
                    mutable: false,
                },
            );
        }
        Stmt::Var { name, ty, value } => {
            let _val_ty = analyze_expr(value, resolver)?;
            let var_ty = resolve_type(ty);
            resolver.define_var(
                name.clone(),
                VarInfo {
                    ty: var_ty,
                    mutable: true,
                },
            );
        }
        Stmt::Assign { name, value } => {
            let _val_ty = analyze_expr(value, resolver)?;
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
                }
            }
        }
        Stmt::Return(expr) => {
            let _ty = analyze_expr(expr, resolver)?;
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
        Expr::Ident(name) => match resolver.lookup_var(name) {
            Some(info) => Ok(info.ty.clone()),
            None => Err(sem_err(format!("undefined variable `{}`", name))),
        },
        Expr::BinaryOp { left, right, .. } => {
            let left_ty = analyze_expr(left, resolver)?;
            let right_ty = analyze_expr(right, resolver)?;
            if left_ty != Type::I32 || right_ty != Type::I32 {
                return Err(sem_err("binary operation requires `i32` operands"));
            }
            Ok(Type::I32)
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

    // --- Normal cases ---

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

    // --- Error cases ---

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
    fn err_return_not_last() {
        let err = analyze_str(
            "func main() -> i32 { return 1; let x: i32 = 2; }",
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
}
