use super::*;
use crate::error::BengalError;
use crate::lexer::tokenize;
use crate::parser::parse;

fn analyze_str(input: &str) -> Result<SemanticInfo> {
    let tokens = tokenize(input).unwrap();
    let program = parse(tokens).unwrap();
    analyze_post_mono(&program)
}

// --- Phase 2 normal cases (maintained) ---

#[test]
fn ok_let_and_return() {
    assert!(analyze_str("func main() -> Int32 { let x: Int32 = 10; return x; }").is_ok());
}

#[test]
fn ok_var_and_assign() {
    assert!(analyze_str("func main() -> Int32 { var x: Int32 = 1; x = 2; return x; }").is_ok());
}

#[test]
fn ok_block_expr_yield() {
    assert!(
        analyze_str("func main() -> Int32 { let x: Int32 = { yield 10; }; return x; }").is_ok()
    );
}

// --- Phase 3 normal cases ---

#[test]
fn ok_if_else() {
    assert!(analyze_str(
        "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }"
    )
    .is_ok());
}

#[test]
fn ok_while() {
    assert!(analyze_str("func main() -> Int32 { while false { }; return 0; }").is_ok());
}

#[test]
fn ok_early_return() {
    assert!(analyze_str("func main() -> Int32 { if 1 < 2 { return 10; }; return 20; }").is_ok());
}

#[test]
fn ok_diverging_then() {
    assert!(analyze_str(
        "func main() -> Int32 { let x: Int32 = if true { return 1; } else { yield 2; }; return x; }"
    )
    .is_ok());
}

#[test]
fn ok_diverging_else() {
    assert!(analyze_str(
        "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { return 2; }; return x; }"
    )
    .is_ok());
}

#[test]
fn ok_unit_func() {
    assert!(
        analyze_str("func foo() { return; } func main() -> Int32 { foo(); return 0; }").is_ok()
    );
}

#[test]
fn ok_bool_let() {
    assert!(analyze_str(
        "func main() -> Int32 { let b: Bool = true && false; if b { yield 1; } else { yield 0; }; return 0; }"
    )
    .is_ok());
}

// --- Phase 2 error cases (maintained) ---

#[test]
fn err_undefined_variable() {
    let err = analyze_str("func main() -> Int32 { return x; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_immutable_assign() {
    let err =
        analyze_str("func main() -> Int32 { let x: Int32 = 1; x = 2; return x; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_no_return() {
    let err = analyze_str("func main() -> Int32 { let x: Int32 = 1; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_no_yield_in_block() {
    let err =
        analyze_str("func main() -> Int32 { let x: Int32 = { let a: Int32 = 1; }; return x; }")
            .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_yield_in_function_body() {
    let err = analyze_str("func main() -> Int32 { yield 1; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_return_in_block_expr() {
    let err = analyze_str("func main() -> Int32 { let x: Int32 = { return 1; }; return x; }")
        .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_yield_not_last() {
    let err = analyze_str(
        "func main() -> Int32 { let x: Int32 = { yield 1; let y: Int32 = 2; }; return x; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_undefined_function() {
    let err = analyze_str("func main() -> Int32 { return foo(1); }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_wrong_arg_count() {
    let err = analyze_str(
        "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(1); }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_no_main() {
    let err = analyze_str("func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_main_with_params() {
    let err = analyze_str("func main(x: Int32) -> Int32 { return x; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

// --- Phase 3 error cases ---

#[test]
fn err_if_non_bool_condition() {
    let err =
        analyze_str("func main() -> Int32 { if 1 { yield 1; } else { yield 2; }; return 0; }")
            .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_if_branch_type_mismatch() {
    let err = analyze_str(
        "func main() -> Int32 { if true { yield 1; } else { yield true; }; return 0; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_while_non_bool_condition() {
    let err = analyze_str("func main() -> Int32 { while 1 { }; return 0; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_yield_in_while() {
    let err =
        analyze_str("func main() -> Int32 { while true { yield 1; }; return 0; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_let_type_mismatch_bool_to_i32() {
    let err = analyze_str("func main() -> Int32 { let x: Int32 = true; return x; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_let_type_mismatch_i32_to_bool() {
    let err = analyze_str("func main() -> Int32 { let x: Bool = 42; return 0; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_assign_type_mismatch() {
    let err =
        analyze_str("func main() -> Int32 { var x: Int32 = 0; x = false; return x; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

// --- Phase 4 normal cases ---

#[test]
fn ok_type_inference() {
    analyze_str("func main() -> Int32 { let x = 10; return x; }").unwrap();
}

#[test]
fn ok_cast_i64() {
    analyze_str("func main() -> Int32 { let x: Int64 = 42 as Int64; return x as Int32; }").unwrap();
}

#[test]
fn ok_float_literal() {
    analyze_str("func main() -> Int32 { let x = 3.14; let y: Int32 = 0; return y; }").unwrap();
}

#[test]
fn ok_break_in_if() {
    analyze_str(
        "func main() -> Int32 { var i: Int32 = 0; while i < 3 { if i == 1 { break; }; i = i + 1; }; return i; }",
    )
    .unwrap();
}

#[test]
fn ok_continue_in_if() {
    analyze_str(
        "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }",
    )
    .unwrap();
}

#[test]
fn ok_break_in_if_else() {
    analyze_str(
        "func main() -> Int32 { var i: Int32 = 0; while i < 10 { let x: Int32 = if i == 5 { break; } else { yield i; }; i = i + 1; }; return i; }",
    )
    .unwrap();
}

#[test]
fn ok_i64_function() {
    analyze_str(
        "func add_i64(a: Int64, b: Int64) -> Int64 { return a + b; } func main() -> Int32 { return add_i64(1 as Int64, 2 as Int64) as Int32; }",
    )
    .unwrap();
}

#[test]
fn ok_while_true_break_value() {
    analyze_str("func main() -> Int32 { let x: Int32 = while true { break 10; }; return x; }")
        .unwrap();
}

#[test]
fn ok_while_true_break_unit() {
    analyze_str("func main() -> Int32 { while true { break; }; return 42; }").unwrap();
}

#[test]
fn ok_while_cond_nobreak() {
    analyze_str(
        "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { if i == 5 { break 99; }; i = i + 1; } nobreak { yield 0; }; return x; }",
    )
    .unwrap();
}

#[test]
fn ok_while_cond_unit_nobreak() {
    analyze_str(
        "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }",
    )
    .unwrap();
}

#[test]
fn ok_i32_max() {
    analyze_str("func main() -> Int32 { let x = 2147483647; return x; }").unwrap();
}

// --- Phase 4 error cases ---

#[test]
fn err_break_outside_loop() {
    let err = analyze_str("func main() -> Int32 { break; return 0; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_continue_outside_loop() {
    let err = analyze_str("func main() -> Int32 { continue; return 0; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_nobreak_in_while_true() {
    let err = analyze_str(
        "func main() -> Int32 { let x: Int32 = while true { break 10; } nobreak { yield 20; }; return x; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_non_unit_break_without_nobreak() {
    let err = analyze_str(
        "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; }; return x; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_nobreak_type_mismatch() {
    let err = analyze_str(
        "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; } nobreak { yield true; }; return x; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_cast_type_mismatch() {
    let err =
        analyze_str("func main() -> Int32 { let x: Int32 = 42 as Int64; return x; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_arithmetic_type_mismatch() {
    let err = analyze_str(
        "func main() -> Int32 { let x: Int32 = 1; let y: Int64 = 2 as Int64; return x + y; }",
    )
    .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_cast_bool() {
    let err =
        analyze_str("func main() -> Int32 { let b: Bool = true; let x = b as Int32; return x; }")
            .unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_float_to_i32() {
    let err = analyze_str("func main() -> Int32 { let x: Int32 = 3.14; return x; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_integer_out_of_range() {
    let err = analyze_str("func main() -> Int32 { let x = 3000000000; return 0; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

#[test]
fn err_integer_out_of_range_with_cast() {
    let err = analyze_str("func main() -> Int32 { return 3000000000 as Int64; }").unwrap_err();
    assert!(matches!(err, BengalError::SemanticError { .. }));
}

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

#[test]
fn multiple_errors_reported() {
    // Source with errors in two separate functions
    let source = r#"
        func foo() -> Int32 { return true; }
        func bar() -> Int32 { return true; }
        func main() -> Int32 { return 1; }
    "#;
    let tokens = crate::lexer::tokenize(source).unwrap();
    let program = crate::parser::parse(tokens).unwrap();
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
    let tokens = crate::lexer::tokenize(source).unwrap();
    let program = crate::parser::parse(tokens).unwrap();
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

mod module_tests {
    use super::super::*;
    use crate::package::build_module_graph;
    use std::fs;
    use tempfile::TempDir;

    fn analyze_test_package(files: &[(&str, &str)]) -> Result<PackageSemanticInfo> {
        let dir = TempDir::new().unwrap();
        for (path, source) in files {
            let full_path = dir.path().join(path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full_path, source).unwrap();
        }
        let entry = dir.path().join(files[0].0);
        let graph = build_module_graph(&entry)?;
        let mut diag = DiagCtxt::new();
        let result = analyze_package(&graph, "test_pkg", &mut diag);
        if result.is_err() {
            // Return the first real error from diag instead of the sentinel
            let errors = diag.take_errors();
            if let Some(first) = errors.into_iter().next() {
                return Err(first);
            }
        }
        result
    }

    #[test]
    fn cross_module_function_import() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::add; func main() -> Int32 { return add(1, 2); }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn visibility_violation_internal() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::helper; func main() -> Int32 { return helper(); }",
            ),
            ("math.bengal", "func helper() -> Int32 { return 1; }"),
        ]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("cannot"),
            "expected 'cannot' in error: {}",
            msg
        );
    }

    #[test]
    fn glob_import() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::*; func main() -> Int32 { return add(1, 2); }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn cross_module_struct_import() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module shapes; import shapes::Point; func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x; }",
            ),
            (
                "shapes.bengal",
                "public struct Point { public var x: Int32; public var y: Int32; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn glob_import_skips_internal() {
        // Internal symbols should NOT be imported by glob
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::*; func main() -> Int32 { return secret(); }",
            ),
            (
                "math.bengal",
                "func secret() -> Int32 { return 42; } public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("undefined function") || msg.contains("secret"),
            "expected undefined function error, got: {}",
            msg
        );
    }

    #[test]
    fn group_import() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::{add, sub}; func main() -> Int32 { return add(1, sub(3, 1)); }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; } public func sub(a: Int32, b: Int32) -> Int32 { return a - b; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn unresolved_import_symbol() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::nonexistent; func main() -> Int32 { return 0; }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("nonexistent"),
            "expected error about 'nonexistent', got: {}",
            msg
        );
    }

    #[test]
    fn package_visibility_accessible() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::helper; func main() -> Int32 { return helper(); }",
            ),
            (
                "math.bengal",
                "package func helper() -> Int32 { return 42; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn non_root_module_no_main_required() {
        // Child modules should not require a main function
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::add; func main() -> Int32 { return add(1, 2); }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        // Verify the graph has 2 modules
        let info = result.unwrap();
        assert_eq!(info.module_infos.len(), 2);
    }

    #[test]
    fn super_at_root_is_error() {
        let result = analyze_test_package(&[(
            "main.bengal",
            "import super::foo; func main() -> Int32 { return 0; }",
        )]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("super"),
            "expected error about 'super', got: {}",
            msg
        );
    }

    #[test]
    fn unresolved_module_in_import() {
        let result = analyze_test_package(&[(
            "main.bengal",
            "import nonexistent::foo; func main() -> Int32 { return 0; }",
        )]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found") || msg.contains("nonexistent"),
            "expected error about unresolved module, got: {}",
            msg
        );
    }
}
