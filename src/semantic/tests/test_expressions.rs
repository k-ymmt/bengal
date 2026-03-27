use super::analyze_str;
use crate::error::BengalError;

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
