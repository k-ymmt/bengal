use super::super::{DiagCtxt, analyze_single_module, resolver};
use super::analyze_str;
use crate::error::BengalError;
use crate::lexer::tokenize;
use crate::parser::parse;

// --- Struct tests ---

#[test]
fn ok_struct_basic() {
    analyze_str(
        "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1, y: 2); return p.x; }",
    )
    .unwrap();
}

#[test]
fn ok_struct_field_assign() {
    analyze_str(
        "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); p.x = 10; return p.x; }",
    )
    .unwrap();
}

#[test]
fn ok_struct_explicit_init() {
    analyze_str(
        "struct Foo { var x: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { var f = Foo(val: 42); return f.x; }",
    )
    .unwrap();
}

#[test]
fn ok_struct_computed_getter() {
    analyze_str(
        "struct Foo { var x: Int32; var double: Int32 { get { return self.x; } }; } func main() -> Int32 { var f = Foo(x: 1); return f.double; }",
    )
    .unwrap();
}

#[test]
fn ok_struct_computed_setter() {
    analyze_str(
        "struct Foo { var x: Int32; var bar: Int32 { get { return 0; } set { self.x = newValue; } }; } func main() -> Int32 { var f = Foo(x: 1); f.bar = 10; return f.x; }",
    )
    .unwrap();
}

#[test]
fn ok_struct_empty_init() {
    analyze_str("struct Empty { } func main() -> Int32 { var e = Empty(); return 0; }").unwrap();
}

#[test]
fn err_undefined_struct() {
    let err = analyze_str("func main() -> Int32 { var f = Foo(x: 1); return 0; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_duplicate_field() {
    let err = analyze_str(
        "struct Foo { var x: Int32; var x: Int32; } func main() -> Int32 { return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_multiple_init() {
    let err = analyze_str(
        "struct Foo { var x: Int32; init(x: Int32) { self.x = x; } init(y: Int32) { self.x = y; } } func main() -> Int32 { return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_init_arg_label_mismatch() {
    let err = analyze_str(
        "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(a: 1, b: 2); return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_init_arg_type_mismatch() {
    let err = analyze_str(
        "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: true); return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_init_arg_count_mismatch() {
    let err = analyze_str(
        "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1); return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_no_such_field() {
    let err = analyze_str(
        "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); return p.y; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_field_access_on_non_struct() {
    let err = analyze_str("func main() -> Int32 { let x: Int32 = 1; return x.y; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_immutable_field_assign() {
    let err = analyze_str(
        "struct Point { var x: Int32; } func main() -> Int32 { let p = Point(x: 1); p.x = 10; return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_readonly_computed_assign() {
    let err = analyze_str(
        "struct Foo { var bar: Int32 { get { return 0; } }; } func main() -> Int32 { var f = Foo(); f.bar = 10; return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_self_outside_struct() {
    let err = analyze_str("func main() -> Int32 { return self.x; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_duplicate_definition_struct_func() {
    let err = analyze_str(
        "struct Foo { var x: Int32; } func Foo() -> Int32 { return 0; } func main() -> Int32 { return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_memberwise_unavailable_with_explicit_init() {
    let err = analyze_str(
        "struct Foo { var x: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { var f = Foo(x: 1); return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_init_missing_field_initialization() {
    let err = analyze_str(
        "struct Foo { var x: Int32; var y: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

// --- Multi-error tests ---

#[test]
fn multiple_errors_reported() {
    // Source with errors in two separate functions
    let source = r#"
        func foo() -> Int32 { return true; }
        func bar() -> Int32 { return true; }
        func main() -> Int32 { return 1; }
    "#;
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    let mut resolver = resolver::Resolver::new();
    let mut diag = DiagCtxt::new();

    let result = analyze_single_module(&program, &mut resolver, true, &mut diag);
    assert!(result.is_err(), "expected error due to type mismatches");
    // Both foo and bar have type errors — previously only foo's was reported
    assert!(
        diag.error_count() >= 2,
        "expected at least 2 errors, got {}",
        diag.error_count()
    );
}

#[test]
fn multiple_errors_in_single_function() {
    // Source with multiple type errors within one function body
    let source = r#"
        func main() -> Int32 {
            let x: Int32 = true;
            let y: Bool = 42;
            return 0;
        }
    "#;
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    let mut resolver = resolver::Resolver::new();
    let mut diag = DiagCtxt::new();

    let result = analyze_single_module(&program, &mut resolver, true, &mut diag);
    assert!(result.is_err(), "expected error due to type mismatches");
    // Both let bindings have type errors — both should be reported
    assert!(
        diag.error_count() >= 2,
        "expected at least 2 errors, got {}",
        diag.error_count()
    );
}
