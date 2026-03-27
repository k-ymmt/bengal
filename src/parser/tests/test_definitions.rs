use super::*;
use crate::error::BengalError;
use crate::lexer::tokenize;
use crate::parser::parse_interface;

// --- Phase 2 tests ---

#[test]
fn parse_func_return() {
    let program = parse_str("func main() -> Int32 { return 42; }").unwrap();
    assert_eq!(program.functions.len(), 1);
    let f = &program.functions[0];
    assert_eq!(f.name, "main");
    assert_eq!(f.params, vec![]);
    assert_eq!(f.return_type, TypeAnnotation::I32);
    assert_eq!(
        normalize_stmt(&f.body.as_ref().unwrap().stmts[0]),
        Stmt::Return(Some(e(ExprKind::Number(42))))
    );
}

#[test]
fn parse_let_return() {
    let program = parse_str("func main() -> Int32 { let x: Int32 = 10; return x; }").unwrap();
    let stmts = &program.functions[0].body.as_ref().unwrap().stmts;
    assert_eq!(stmts.len(), 2);
    assert_eq!(
        normalize_stmt(&stmts[0]),
        Stmt::Let {
            name: "x".to_string(),
            ty: Some(TypeAnnotation::I32),
            value: e(ExprKind::Number(10)),
        }
    );
    assert_eq!(
        normalize_stmt(&stmts[1]),
        Stmt::Return(Some(e(ExprKind::Ident("x".to_string()))))
    );
}

#[test]
fn parse_func_with_params() {
    let program = parse_str("func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap();
    let f = &program.functions[0];
    assert_eq!(f.name, "add");
    assert_eq!(
        f.params,
        vec![
            Param {
                name: "a".to_string(),
                ty: TypeAnnotation::I32
            },
            Param {
                name: "b".to_string(),
                ty: TypeAnnotation::I32
            },
        ]
    );
    assert_eq!(
        normalize_stmt(&f.body.as_ref().unwrap().stmts[0]),
        Stmt::Return(Some(e(ExprKind::BinaryOp {
            op: BinOp::Add,
            left: Box::new(e(ExprKind::Ident("a".to_string()))),
            right: Box::new(e(ExprKind::Ident("b".to_string()))),
        })))
    );
}

#[test]
fn parse_unit_return_function() {
    let program = parse_str("func foo() { return; }").unwrap();
    let f = &program.functions[0];
    assert_eq!(f.name, "foo");
    assert_eq!(f.return_type, TypeAnnotation::Unit);
    assert_eq!(
        normalize_stmt(&f.body.as_ref().unwrap().stmts[0]),
        Stmt::Return(None)
    );
}

#[test]
fn error_missing_type_annotation() {
    assert!(matches!(
        parse_str("func main() -> Int32 { let x: = 10; }"),
        Err(BengalError::ParseError { .. })
    ));
}

// --- Phase 3 (struct) tests ---

#[test]
fn parse_struct_stored_properties() {
    let program = parse_str(
        "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { return 0; }",
    )
    .unwrap();
    assert_eq!(program.structs.len(), 1);
    let s = &program.structs[0];
    assert_eq!(s.name, "Point");
    assert_eq!(s.members.len(), 2);
    assert!(matches!(
        &s.members[0],
        StructMember::StoredProperty { name, ty, .. } if name == "x" && *ty == TypeAnnotation::I32
    ));
    assert!(matches!(
        &s.members[1],
        StructMember::StoredProperty { name, ty, .. } if name == "y" && *ty == TypeAnnotation::I32
    ));
}

#[test]
fn parse_struct_computed_property() {
    let program = parse_str(
        "struct Foo { var x: Int32; var bar: Int32 { get { return 0; } set { self.x = newValue; } }; } func main() -> Int32 { return 0; }",
    )
    .unwrap();
    let s = &program.structs[0];
    assert_eq!(s.members.len(), 2);
    assert!(matches!(
        &s.members[1],
        StructMember::ComputedProperty { name, setter: Some(_), .. } if name == "bar"
    ));
}

#[test]
fn parse_struct_computed_property_readonly() {
    let program = parse_str(
        "struct Foo { var bar: Int32 { get { return 0; } }; } func main() -> Int32 { return 0; }",
    )
    .unwrap();
    let s = &program.structs[0];
    assert!(matches!(
        &s.members[0],
        StructMember::ComputedProperty { setter: None, .. }
    ));
}

#[test]
fn parse_struct_initializer() {
    let program = parse_str(
        "struct Foo { var x: Int32; init(x: Int32) { self.x = x; } } func main() -> Int32 { return 0; }",
    )
    .unwrap();
    let s = &program.structs[0];
    assert_eq!(s.members.len(), 2);
    assert!(matches!(
        &s.members[1],
        StructMember::Initializer { params, .. } if params.len() == 1
    ));
}

#[test]
fn parse_struct_method() {
    let source = r#"
        struct Point {
            var x: Int32;
            func sum() -> Int32 {
                return self.x;
            }
        }
        func main() -> Int32 { return 0; }
    "#;
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.structs.len(), 1);
    let s = &program.structs[0];
    assert!(
        s.members
            .iter()
            .any(|m| matches!(m, StructMember::Method { name, .. } if name == "sum"))
    );
}

#[test]
fn parse_struct_conformance() {
    let source = r#"
        struct Point: Foo, Bar {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#;
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.structs[0].conformances, vec!["Foo", "Bar"]);
}

#[test]
fn parse_protocol_def() {
    let source = r#"
        protocol Summable {
            func sum() -> Int32;
            var total: Int32 { get };
        }
        func main() -> Int32 { return 0; }
    "#;
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.protocols.len(), 1);
    assert_eq!(program.protocols[0].name, "Summable");
    assert_eq!(program.protocols[0].members.len(), 2);
}

// --- Interface mode tests ---

#[test]
fn parse_interface_function() {
    let tokens = tokenize("public func add(a: Int32, b: Int32) -> Int32;").unwrap();
    let program = parse_interface(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
    let f = &program.functions[0];
    assert_eq!(f.name, "add");
    assert_eq!(f.visibility, Visibility::Public);
    assert!(f.body.is_none());
    assert_eq!(f.params.len(), 2);
}

#[test]
fn parse_interface_generic_function() {
    let tokens = tokenize("public func identity<T>(x: T) -> T;").unwrap();
    let program = parse_interface(tokens).unwrap();
    let f = &program.functions[0];
    assert_eq!(f.type_params.len(), 1);
    assert_eq!(f.type_params[0].name, "T");
    assert!(f.body.is_none());
}

#[test]
fn parse_interface_unit_return_function() {
    let tokens = tokenize("public func doSomething();").unwrap();
    let program = parse_interface(tokens).unwrap();
    let f = &program.functions[0];
    assert_eq!(f.return_type, TypeAnnotation::Unit);
    assert!(f.body.is_none());
}

#[test]
fn parse_interface_struct() {
    let tokens = tokenize(
        "public struct Point { var x: Int32; var y: Int32; init(x: Int32, y: Int32); func sum() -> Int32; }",
    )
    .unwrap();
    let program = parse_interface(tokens).unwrap();
    let s = &program.structs[0];
    assert_eq!(s.name, "Point");
    assert_eq!(s.members.len(), 4);
    // Check init has no body
    if let StructMember::Initializer { body, .. } = &s.members[2] {
        assert!(body.is_none());
    } else {
        panic!("expected Initializer");
    }
    // Check method has no body
    if let StructMember::Method { body, .. } = &s.members[3] {
        assert!(body.is_none());
    } else {
        panic!("expected Method");
    }
}

#[test]
fn parse_interface_computed_property() {
    let tokens =
        tokenize("public struct S { var x: Int32 { get }; var y: Int32 { get set }; }").unwrap();
    let program = parse_interface(tokens).unwrap();
    let s = &program.structs[0];
    if let StructMember::ComputedProperty { getter, setter, .. } = &s.members[0] {
        assert!(getter.is_none());
        assert!(setter.is_none()); // get only — no setter
    } else {
        panic!("expected ComputedProperty");
    }
    if let StructMember::ComputedProperty { getter, setter, .. } = &s.members[1] {
        assert!(getter.is_none());
        assert!(setter.is_some()); // has setter marker
    } else {
        panic!("expected ComputedProperty");
    }
}
