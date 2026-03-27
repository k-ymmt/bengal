use std::collections::HashMap;

use crate::error::Result;
use crate::parser::ast::*;

use super::sem_err;

/// Validate that a `main` function exists with no parameters and returns Int32.
pub fn validate_main(program: &Program) -> Result<()> {
    match program.functions.iter().find(|f| f.name == "main") {
        None => Err(sem_err("no `main` function found")),
        Some(main_fn) => {
            if !main_fn.params.is_empty() {
                return Err(sem_err("`main` function must have no parameters"));
            }
            let is_i32 = matches!(main_fn.return_type, TypeAnnotation::I32)
                || matches!(&main_fn.return_type, TypeAnnotation::Named(n) if n == "Int32");
            if !is_i32 {
                return Err(sem_err("`main` function must return `Int32`"));
            }
            Ok(())
        }
    }
}

/// Validate generic usage before full semantic analysis.
///
/// Walks all expressions and checks:
/// - Generic func/struct called without type args
/// - Type argument count mismatch
/// - Non-generic func/struct called with type args
/// - Constraint violations (type arg doesn't conform to required protocol)
pub fn validate_generics(program: &Program) -> Result<()> {
    // Build lookup maps from the program AST
    let func_map: HashMap<String, &Function> = program
        .functions
        .iter()
        .map(|f| (f.name.clone(), f))
        .collect();
    let struct_map: HashMap<String, &StructDef> = program
        .structs
        .iter()
        .map(|s| (s.name.clone(), s))
        .collect();

    // Walk all function bodies
    for func in &program.functions {
        validate_generics_block(&func.body, &func_map, &struct_map)?;
    }

    // Walk all struct member bodies
    for struct_def in &program.structs {
        for member in &struct_def.members {
            match member {
                StructMember::Initializer { body, .. } => {
                    validate_generics_block(body, &func_map, &struct_map)?;
                }
                StructMember::Method { body, .. } => {
                    validate_generics_block(body, &func_map, &struct_map)?;
                }
                StructMember::ComputedProperty { getter, setter, .. } => {
                    validate_generics_block(getter, &func_map, &struct_map)?;
                    if let Some(setter_block) = setter {
                        validate_generics_block(setter_block, &func_map, &struct_map)?;
                    }
                }
                StructMember::StoredProperty { .. } => {}
            }
        }
    }

    Ok(())
}

fn validate_generics_block(
    block: &Block,
    func_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDef>,
) -> Result<()> {
    for stmt in &block.stmts {
        validate_generics_stmt(stmt, func_map, struct_map)?;
    }
    Ok(())
}

fn validate_generics_stmt(
    stmt: &Stmt,
    func_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDef>,
) -> Result<()> {
    match stmt {
        Stmt::Let { value, .. } | Stmt::Var { value, .. } | Stmt::Assign { value, .. } => {
            validate_generics_expr(value, func_map, struct_map)?;
        }
        Stmt::Return(Some(expr)) | Stmt::Yield(expr) | Stmt::Break(Some(expr)) => {
            validate_generics_expr(expr, func_map, struct_map)?;
        }
        Stmt::Expr(expr) => {
            validate_generics_expr(expr, func_map, struct_map)?;
        }
        Stmt::FieldAssign { object, value, .. } => {
            validate_generics_expr(object, func_map, struct_map)?;
            validate_generics_expr(value, func_map, struct_map)?;
        }
        Stmt::IndexAssign {
            object,
            index,
            value,
        } => {
            validate_generics_expr(object, func_map, struct_map)?;
            validate_generics_expr(index, func_map, struct_map)?;
            validate_generics_expr(value, func_map, struct_map)?;
        }
        Stmt::Return(None) | Stmt::Break(None) | Stmt::Continue => {}
    }
    Ok(())
}

fn validate_generics_expr(
    expr: &Expr,
    func_map: &HashMap<String, &Function>,
    struct_map: &HashMap<String, &StructDef>,
) -> Result<()> {
    match &expr.kind {
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            // Check function generics
            if let Some(func_def) = func_map.get(name) {
                let num_type_params = func_def.type_params.len();
                if !type_args.is_empty() && num_type_params == 0 {
                    return Err(sem_err(format!(
                        "function `{}` does not take type arguments",
                        name
                    )));
                }
                if !type_args.is_empty() && type_args.len() != num_type_params {
                    return Err(sem_err(format!(
                        "function `{}` expected {} type argument(s), but {} were given",
                        name,
                        num_type_params,
                        type_args.len()
                    )));
                }
                // Check constraint violations
                if !type_args.is_empty() {
                    validate_constraints(
                        &func_def.type_params,
                        type_args,
                        struct_map,
                        &format!("function `{}`", name),
                    )?;
                }
            }
            // Recurse into arguments
            for arg in args {
                validate_generics_expr(arg, func_map, struct_map)?;
            }
        }
        ExprKind::StructInit {
            name,
            type_args,
            args,
        } => {
            // Check struct generics
            if let Some(struct_def) = struct_map.get(name) {
                let num_type_params = struct_def.type_params.len();
                if !type_args.is_empty() && num_type_params == 0 {
                    return Err(sem_err(format!(
                        "struct `{}` does not take type arguments",
                        name
                    )));
                }
                if !type_args.is_empty() && type_args.len() != num_type_params {
                    return Err(sem_err(format!(
                        "struct `{}` expected {} type argument(s), but {} were given",
                        name,
                        num_type_params,
                        type_args.len()
                    )));
                }
                // Check constraint violations
                if !type_args.is_empty() {
                    validate_constraints(
                        &struct_def.type_params,
                        type_args,
                        struct_map,
                        &format!("struct `{}`", name),
                    )?;
                }
            }
            // Recurse into field arguments
            for (_, arg_expr) in args {
                validate_generics_expr(arg_expr, func_map, struct_map)?;
            }
        }
        ExprKind::BinaryOp { left, right, .. } => {
            validate_generics_expr(left, func_map, struct_map)?;
            validate_generics_expr(right, func_map, struct_map)?;
        }
        ExprKind::UnaryOp { operand, .. } => {
            validate_generics_expr(operand, func_map, struct_map)?;
        }
        ExprKind::Block(block) => {
            validate_generics_block(block, func_map, struct_map)?;
        }
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            validate_generics_expr(condition, func_map, struct_map)?;
            validate_generics_block(then_block, func_map, struct_map)?;
            if let Some(else_blk) = else_block {
                validate_generics_block(else_blk, func_map, struct_map)?;
            }
        }
        ExprKind::While {
            condition,
            body,
            nobreak,
        } => {
            validate_generics_expr(condition, func_map, struct_map)?;
            validate_generics_block(body, func_map, struct_map)?;
            if let Some(nb) = nobreak {
                validate_generics_block(nb, func_map, struct_map)?;
            }
        }
        ExprKind::Cast { expr, .. } => {
            validate_generics_expr(expr, func_map, struct_map)?;
        }
        ExprKind::FieldAccess { object, .. } => {
            validate_generics_expr(object, func_map, struct_map)?;
        }
        ExprKind::MethodCall { object, args, .. } => {
            validate_generics_expr(object, func_map, struct_map)?;
            for arg in args {
                validate_generics_expr(arg, func_map, struct_map)?;
            }
        }
        ExprKind::ArrayLiteral { elements } => {
            for elem in elements {
                validate_generics_expr(elem, func_map, struct_map)?;
            }
        }
        ExprKind::IndexAccess { object, index } => {
            validate_generics_expr(object, func_map, struct_map)?;
            validate_generics_expr(index, func_map, struct_map)?;
        }
        ExprKind::Number(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Ident(_)
        | ExprKind::SelfRef => {}
    }
    Ok(())
}

/// Check that type arguments satisfy the protocol constraints of type parameters.
fn validate_constraints(
    type_params: &[TypeParam],
    type_args: &[TypeAnnotation],
    struct_map: &HashMap<String, &StructDef>,
    context: &str,
) -> Result<()> {
    for (tp, ta) in type_params.iter().zip(type_args.iter()) {
        if let Some(ref bound) = tp.bound {
            // The type argument must be a named type (struct) that conforms to the protocol
            let arg_name = match ta {
                TypeAnnotation::Named(name) => name,
                TypeAnnotation::Generic { name, .. } => name,
                _ => continue, // primitives can't conform to protocols (for now)
            };
            if let Some(struct_def) = struct_map.get(arg_name) {
                if !struct_def.conformances.contains(bound) {
                    return Err(sem_err(format!(
                        "type `{}` does not conform to protocol `{}` (required by {} type parameter `{}`)",
                        arg_name, bound, context, tp.name
                    )));
                }
            } else {
                // Not a known struct — primitives don't conform to protocols
                return Err(sem_err(format!(
                    "type `{}` does not conform to protocol `{}`",
                    arg_name, bound
                )));
            }
        }
    }
    Ok(())
}
