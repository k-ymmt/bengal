use super::*;
use crate::error::BengalError;

// --- Phase 1 compatibility tests ---

#[test]
fn precedence_mul_over_add() {
    let expr = parse_expr_str("2 + 3 * 4");
    assert_eq!(
        expr,
        e(ExprKind::BinaryOp {
            op: BinOp::Add,
            left: Box::new(e(ExprKind::Number(2))),
            right: Box::new(e(ExprKind::BinaryOp {
                op: BinOp::Mul,
                left: Box::new(e(ExprKind::Number(3))),
                right: Box::new(e(ExprKind::Number(4))),
            })),
        })
    );
}

#[test]
fn parentheses_override_precedence() {
    let expr = parse_expr_str("(2 + 3) * 4");
    assert_eq!(
        expr,
        e(ExprKind::BinaryOp {
            op: BinOp::Mul,
            left: Box::new(e(ExprKind::BinaryOp {
                op: BinOp::Add,
                left: Box::new(e(ExprKind::Number(2))),
                right: Box::new(e(ExprKind::Number(3))),
            })),
            right: Box::new(e(ExprKind::Number(4))),
        })
    );
}

#[test]
fn single_number() {
    assert_eq!(parse_expr_str("10"), e(ExprKind::Number(10)));
}

#[test]
fn error_incomplete_expr() {
    assert!(matches!(
        parse_str("1 + "),
        Err(BengalError::ParseError { .. })
    ));
}

#[test]
fn error_unconsumed_token() {
    assert!(matches!(
        parse_str("1 2"),
        Err(BengalError::ParseError { .. })
    ));
}

#[test]
fn error_unconsumed_rparen() {
    assert!(matches!(
        parse_str("2 + 3)"),
        Err(BengalError::ParseError { .. })
    ));
}

#[test]
fn parse_phase1_compat() {
    let program = parse_str("2 + 3 * 4").unwrap();
    assert_eq!(program.functions.len(), 1);
    let f = &program.functions[0];
    assert_eq!(f.name, "main");
    assert_eq!(f.params, vec![]);
    assert_eq!(f.return_type, TypeAnnotation::I32);
    assert_eq!(f.body.stmts.len(), 1);
    assert!(matches!(
        &f.body.stmts[0],
        Stmt::Return(Some(Expr {
            kind: ExprKind::BinaryOp { .. },
            ..
        }))
    ));
}

#[test]
fn parse_comparison() {
    let expr = parse_expr_str("1 < 2");
    assert_eq!(
        expr,
        e(ExprKind::BinaryOp {
            op: BinOp::Lt,
            left: Box::new(e(ExprKind::Number(1))),
            right: Box::new(e(ExprKind::Number(2))),
        })
    );
}

#[test]
fn parse_logical_precedence() {
    let expr = parse_expr_str("true && false || !true");
    assert_eq!(
        expr,
        e(ExprKind::BinaryOp {
            op: BinOp::Or,
            left: Box::new(e(ExprKind::BinaryOp {
                op: BinOp::And,
                left: Box::new(e(ExprKind::Bool(true))),
                right: Box::new(e(ExprKind::Bool(false))),
            })),
            right: Box::new(e(ExprKind::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(e(ExprKind::Bool(true))),
            })),
        })
    );
}

// --- Phase 4 tests ---

#[test]
fn parse_let_type_inference() {
    let program = parse_str("func main() -> Int32 { let x = 42; return x; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    assert_eq!(
        normalize_stmt(&stmts[0]),
        Stmt::Let {
            name: "x".to_string(),
            ty: None,
            value: e(ExprKind::Number(42)),
        }
    );
}

#[test]
fn parse_let_with_i64() {
    let program = parse_str("func main() -> Int32 { let x: Int64 = 42; return 0; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    assert_eq!(
        normalize_stmt(&stmts[0]),
        Stmt::Let {
            name: "x".to_string(),
            ty: Some(TypeAnnotation::I64),
            value: e(ExprKind::Number(42)),
        }
    );
}

#[test]
fn parse_cast_expr() {
    let expr = parse_expr_str("42 as Int64");
    assert_eq!(
        expr,
        e(ExprKind::Cast {
            expr: Box::new(e(ExprKind::Number(42))),
            target_type: TypeAnnotation::I64,
        })
    );
}

#[test]
fn parse_break_no_value() {
    let program = parse_str("func main() -> Int32 { while true { break; }; return 0; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    if let Stmt::Expr(Expr {
        kind: ExprKind::While { body, .. },
        ..
    }) = &stmts[0]
    {
        assert_eq!(body.stmts[0], Stmt::Break(None));
    } else {
        panic!("expected while");
    }
}

#[test]
fn parse_break_with_value() {
    let program =
        parse_str("func main() -> Int32 { while true { break 10; }; return 0; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    if let Stmt::Expr(Expr {
        kind: ExprKind::While { body, .. },
        ..
    }) = &stmts[0]
    {
        assert_eq!(
            normalize_stmt(&body.stmts[0]),
            Stmt::Break(Some(e(ExprKind::Number(10))))
        );
    } else {
        panic!("expected while");
    }
}

#[test]
fn parse_continue() {
    let program =
        parse_str("func main() -> Int32 { while true { continue; }; return 0; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    if let Stmt::Expr(Expr {
        kind: ExprKind::While { body, .. },
        ..
    }) = &stmts[0]
    {
        assert_eq!(body.stmts[0], Stmt::Continue);
    } else {
        panic!("expected while");
    }
}

#[test]
#[allow(clippy::approx_constant)]
fn parse_float_literal() {
    let expr = parse_expr_str("3.14");
    assert_eq!(expr, e(ExprKind::Float(3.14)));
}

#[test]
fn parse_cast_precedence() {
    let expr = parse_expr_str("1 + 2 as Int64");
    assert_eq!(
        expr,
        e(ExprKind::BinaryOp {
            op: BinOp::Add,
            left: Box::new(e(ExprKind::Number(1))),
            right: Box::new(e(ExprKind::Cast {
                expr: Box::new(e(ExprKind::Number(2))),
                target_type: TypeAnnotation::I64,
            })),
        })
    );
}

#[test]
fn parse_while_nobreak() {
    let program = parse_str(
        "func main() -> Int32 { while true { break 1; } nobreak { yield 2; }; return 0; }",
    )
    .unwrap();
    let stmts = &program.functions[0].body.stmts;
    if let Stmt::Expr(Expr {
        kind: ExprKind::While { nobreak, .. },
        ..
    }) = &stmts[0]
    {
        assert!(nobreak.is_some());
        let nb = nobreak.as_ref().unwrap();
        assert_eq!(
            normalize_stmt(&nb.stmts[0]),
            Stmt::Yield(e(ExprKind::Number(2)))
        );
    } else {
        panic!("expected while");
    }
}

// --- NodeId allocation tests ---

#[test]
fn node_ids_are_unique() {
    let program = parse_str("func main() -> Int32 { return 1 + 2 * 3; }").unwrap();
    if let Stmt::Return(Some(expr)) = &program.functions[0].body.stmts[0] {
        let mut ids = Vec::new();
        collect_expr_ids(expr, &mut ids);
        assert_eq!(ids.len(), 5);
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "all NodeIds must be unique");
    } else {
        panic!("expected return");
    }
}

#[test]
fn node_ids_are_sequential() {
    let program = parse_str("func main() -> Int32 { return a + b; }").unwrap();
    if let Stmt::Return(Some(expr)) = &program.functions[0].body.stmts[0] {
        let mut ids = Vec::new();
        collect_expr_ids(expr, &mut ids);
        let mut sorted: Vec<u32> = ids.iter().map(|id| id.0).collect();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2]);
    } else {
        panic!("expected return");
    }
}

// --- Block / if / while / field ---

#[test]
fn parse_block_expr_yield() {
    let program =
        parse_str("func main() -> Int32 { let x: Int32 = { yield 10; }; return x; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    assert_eq!(stmts.len(), 2);
    assert_eq!(
        normalize_stmt(&stmts[0]),
        Stmt::Let {
            name: "x".to_string(),
            ty: Some(TypeAnnotation::I32),
            value: e(ExprKind::Block(Block {
                stmts: vec![Stmt::Yield(e(ExprKind::Number(10)))],
            })),
        }
    );
    assert_eq!(
        normalize_stmt(&stmts[1]),
        Stmt::Return(Some(e(ExprKind::Ident("x".to_string()))))
    );
}

#[test]
fn parse_if_else() {
    let program =
        parse_str("func main() -> Int32 { if true { yield 1; } else { yield 2; }; return 0; }")
            .unwrap();
    let stmts = &program.functions[0].body.stmts;
    assert_eq!(stmts.len(), 2);
    assert!(matches!(
        &stmts[0],
        Stmt::Expr(Expr {
            kind: ExprKind::If {
                else_block: Some(_),
                ..
            },
            ..
        })
    ));
}

#[test]
fn parse_while() {
    let program = parse_str("func main() -> Int32 { while false { }; return 0; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    assert_eq!(stmts.len(), 2);
    assert!(matches!(
        &stmts[0],
        Stmt::Expr(Expr {
            kind: ExprKind::While { .. },
            ..
        })
    ));
}

#[test]
fn parse_field_access() {
    let expr = parse_expr_str("f.x");
    assert_eq!(
        expr,
        e(ExprKind::FieldAccess {
            object: Box::new(e(ExprKind::Ident("f".to_string()))),
            field: "x".to_string(),
        })
    );
}

#[test]
fn parse_field_access_chain() {
    let expr = parse_expr_str("a.b.c");
    assert_eq!(
        expr,
        e(ExprKind::FieldAccess {
            object: Box::new(e(ExprKind::FieldAccess {
                object: Box::new(e(ExprKind::Ident("a".to_string()))),
                field: "b".to_string(),
            })),
            field: "c".to_string(),
        })
    );
}

#[test]
fn parse_self_field_access() {
    let expr = parse_expr_str("self.foo");
    assert_eq!(
        expr,
        e(ExprKind::FieldAccess {
            object: Box::new(e(ExprKind::SelfRef)),
            field: "foo".to_string(),
        })
    );
}

#[test]
fn parse_struct_init() {
    let expr = parse_expr_str("Foo(x: 1, y: 2)");
    assert_eq!(
        expr,
        e(ExprKind::StructInit {
            name: "Foo".to_string(),
            type_args: vec![],
            args: vec![
                ("x".to_string(), e(ExprKind::Number(1))),
                ("y".to_string(), e(ExprKind::Number(2))),
            ],
        })
    );
}

#[test]
fn parse_empty_call_remains_call() {
    let expr = parse_expr_str("Foo()");
    assert_eq!(
        expr,
        e(ExprKind::Call {
            name: "Foo".to_string(),
            type_args: vec![],
            args: vec![],
        })
    );
}

#[test]
fn parse_field_assign() {
    let program =
        parse_str("func main() -> Int32 { var f: Int32 = 0; f.x = 10; return 0; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    assert!(matches!(
        normalize_stmt(&stmts[1]),
        Stmt::FieldAssign { field, .. } if field == "x"
    ));
}

#[test]
fn parse_self_field_assign() {
    let program = parse_str("func main() -> Int32 { self.foo = 42; return 0; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    let s = normalize_stmt(&stmts[0]);
    if let Stmt::FieldAssign {
        object,
        field,
        value,
    } = &s
    {
        assert_eq!(object.kind, ExprKind::SelfRef);
        assert_eq!(field, "foo");
        assert_eq!(value.kind, ExprKind::Number(42));
    } else {
        panic!("expected FieldAssign");
    }
}

#[test]
fn parse_named_type_annotation() {
    let program = parse_str("func main() -> Int32 { var f: Foo = Foo(x: 1); return 0; }").unwrap();
    let stmts = &program.functions[0].body.stmts;
    if let Stmt::Var { ty: Some(ty), .. } = &stmts[0] {
        assert_eq!(*ty, TypeAnnotation::Named("Foo".to_string()));
    } else {
        panic!("expected Var with Named type");
    }
}

#[test]
fn parse_call_then_field_access() {
    let expr = parse_expr_str("get_foo().x");
    assert_eq!(
        expr,
        e(ExprKind::FieldAccess {
            object: Box::new(e(ExprKind::Call {
                name: "get_foo".to_string(),
                type_args: vec![],
                args: vec![],
            })),
            field: "x".to_string(),
        })
    );
}
