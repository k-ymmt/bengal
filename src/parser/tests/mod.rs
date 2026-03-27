mod test_definitions;
mod test_expressions;
mod test_modules;

use super::*;
use crate::lexer::tokenize;

fn parse_str(input: &str) -> Result<Program> {
    let tokens = tokenize(input).unwrap();
    parse(tokens)
}

fn e(kind: ExprKind) -> Expr {
    Expr {
        id: NodeId(0),
        kind,
        span: Span { start: 0, end: 0 },
    }
}

fn normalize_expr(expr: &Expr) -> Expr {
    let kind = match &expr.kind {
        ExprKind::Number(n) => ExprKind::Number(*n),
        ExprKind::Float(f) => ExprKind::Float(*f),
        ExprKind::Bool(b) => ExprKind::Bool(*b),
        ExprKind::Ident(s) => ExprKind::Ident(s.clone()),
        ExprKind::BinaryOp { op, left, right } => ExprKind::BinaryOp {
            op: *op,
            left: Box::new(normalize_expr(left)),
            right: Box::new(normalize_expr(right)),
        },
        ExprKind::UnaryOp { op, operand } => ExprKind::UnaryOp {
            op: *op,
            operand: Box::new(normalize_expr(operand)),
        },
        ExprKind::Call {
            name,
            type_args,
            args,
        } => ExprKind::Call {
            name: name.clone(),
            type_args: type_args.clone(),
            args: args.iter().map(normalize_expr).collect(),
        },
        ExprKind::Block(block) => ExprKind::Block(normalize_block(block)),
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => ExprKind::If {
            condition: Box::new(normalize_expr(condition)),
            then_block: normalize_block(then_block),
            else_block: else_block.as_ref().map(normalize_block),
        },
        ExprKind::While {
            condition,
            body,
            nobreak,
        } => ExprKind::While {
            condition: Box::new(normalize_expr(condition)),
            body: normalize_block(body),
            nobreak: nobreak.as_ref().map(normalize_block),
        },
        ExprKind::Cast { expr, target_type } => ExprKind::Cast {
            expr: Box::new(normalize_expr(expr)),
            target_type: target_type.clone(),
        },
        ExprKind::StructInit {
            name,
            type_args,
            args,
        } => ExprKind::StructInit {
            name: name.clone(),
            type_args: type_args.clone(),
            args: args
                .iter()
                .map(|(l, e)| (l.clone(), normalize_expr(e)))
                .collect(),
        },
        ExprKind::FieldAccess { object, field } => ExprKind::FieldAccess {
            object: Box::new(normalize_expr(object)),
            field: field.clone(),
        },
        ExprKind::SelfRef => ExprKind::SelfRef,
        ExprKind::MethodCall {
            object,
            method,
            args,
        } => ExprKind::MethodCall {
            object: Box::new(normalize_expr(object)),
            method: method.clone(),
            args: args.iter().map(normalize_expr).collect(),
        },
        ExprKind::ArrayLiteral { elements } => ExprKind::ArrayLiteral {
            elements: elements.iter().map(normalize_expr).collect(),
        },
        ExprKind::IndexAccess { object, index } => ExprKind::IndexAccess {
            object: Box::new(normalize_expr(object)),
            index: Box::new(normalize_expr(index)),
        },
    };
    Expr {
        id: NodeId(0),
        kind,
        span: Span { start: 0, end: 0 },
    }
}

fn normalize_stmt(stmt: &Stmt) -> Stmt {
    match stmt {
        Stmt::Let { name, ty, value } => Stmt::Let {
            name: name.clone(),
            ty: ty.clone(),
            value: normalize_expr(value),
        },
        Stmt::Var { name, ty, value } => Stmt::Var {
            name: name.clone(),
            ty: ty.clone(),
            value: normalize_expr(value),
        },
        Stmt::Assign { name, value } => Stmt::Assign {
            name: name.clone(),
            value: normalize_expr(value),
        },
        Stmt::Return(opt) => Stmt::Return(opt.as_ref().map(normalize_expr)),
        Stmt::Yield(expr) => Stmt::Yield(normalize_expr(expr)),
        Stmt::Break(opt) => Stmt::Break(opt.as_ref().map(normalize_expr)),
        Stmt::Continue => Stmt::Continue,
        Stmt::Expr(expr) => Stmt::Expr(normalize_expr(expr)),
        Stmt::FieldAssign {
            object,
            field,
            value,
        } => Stmt::FieldAssign {
            object: Box::new(normalize_expr(object)),
            field: field.clone(),
            value: normalize_expr(value),
        },
        Stmt::IndexAssign {
            object,
            index,
            value,
        } => Stmt::IndexAssign {
            object: Box::new(normalize_expr(object)),
            index: Box::new(normalize_expr(index)),
            value: normalize_expr(value),
        },
    }
}

fn normalize_block(block: &Block) -> Block {
    Block {
        stmts: block.stmts.iter().map(normalize_stmt).collect(),
    }
}

fn parse_expr_str(input: &str) -> Expr {
    let program = parse_str(input).unwrap();
    let expr = match program.functions[0].body.stmts.last().unwrap() {
        Stmt::Return(Some(expr)) => expr.clone(),
        _ => panic!("expected Return statement"),
    };
    normalize_expr(&expr)
}

fn collect_expr_ids(expr: &Expr, ids: &mut Vec<NodeId>) {
    ids.push(expr.id);
    match &expr.kind {
        ExprKind::BinaryOp { left, right, .. } => {
            collect_expr_ids(left, ids);
            collect_expr_ids(right, ids);
        }
        ExprKind::UnaryOp { operand, .. } => {
            collect_expr_ids(operand, ids);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                collect_expr_ids(arg, ids);
            }
        }

        ExprKind::Cast { expr, .. } => {
            collect_expr_ids(expr, ids);
        }
        ExprKind::Block(block) => collect_block_expr_ids(block, ids),
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_expr_ids(condition, ids);
            collect_block_expr_ids(then_block, ids);
            if let Some(b) = else_block {
                collect_block_expr_ids(b, ids);
            }
        }
        ExprKind::While {
            condition,
            body,
            nobreak,
        } => {
            collect_expr_ids(condition, ids);
            collect_block_expr_ids(body, ids);
            if let Some(b) = nobreak {
                collect_block_expr_ids(b, ids);
            }
        }
        ExprKind::StructInit { args, .. } => {
            for (_, arg) in args {
                collect_expr_ids(arg, ids);
            }
        }
        ExprKind::FieldAccess { object, .. } => {
            collect_expr_ids(object, ids);
        }
        _ => {}
    }
}

fn collect_block_expr_ids(block: &Block, ids: &mut Vec<NodeId>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let { value, .. } | Stmt::Var { value, .. } | Stmt::Assign { value, .. } => {
                collect_expr_ids(value, ids);
            }
            Stmt::Return(Some(e)) | Stmt::Yield(e) | Stmt::Break(Some(e)) | Stmt::Expr(e) => {
                collect_expr_ids(e, ids);
            }
            Stmt::FieldAssign { object, value, .. } => {
                collect_expr_ids(object, ids);
                collect_expr_ids(value, ids);
            }
            Stmt::IndexAssign {
                object,
                index,
                value,
            } => {
                collect_expr_ids(object, ids);
                collect_expr_ids(index, ids);
                collect_expr_ids(value, ids);
            }
            _ => {}
        }
    }
}
