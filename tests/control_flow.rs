mod common;

use common::{compile_and_run, compile_source_should_fail};

// --- if / else ---

#[test]
fn if_else_true() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }"
        ),
        1
    );
}

#[test]
fn if_else_false() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false { yield 1; } else { yield 2; }; return x; }"
        ),
        2
    );
}

#[test]
fn if_else_comparison() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 3 > 2 { yield 10; } else { yield 20; }; return x; }"
        ),
        10
    );
}

#[test]
fn if_no_else() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { if true { }; return 42; }"),
        42
    );
}

// --- Comparisons ---

#[test]
fn comparison_eq() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 1 == 1 { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn comparison_ne() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 1 != 2 { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn comparison_le() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 3 <= 3 { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn comparison_ge() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 3 >= 4 { yield 1; } else { yield 0; }; return x; }"
        ),
        0
    );
}

// --- Logical operators ---

#[test]
fn logical_and() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if true && true { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn logical_and_short() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false && true { yield 1; } else { yield 0; }; return x; }"
        ),
        0
    );
}

#[test]
fn logical_or() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false || true { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn logical_not() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if !false { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn logical_not_with_comparison() {
    assert_eq!(
        compile_and_run(
            r#"
            func main() -> Int32 {
                let x: Int32 = 3;
                let r = if !(x > 5) { yield 1; } else { yield 0; };
                return r;
            }
        "#
        ),
        1
    );
}

// --- Early return and divergence ---

#[test]
fn early_return() {
    assert_eq!(
        compile_and_run(
            "func abs(x: Int32) -> Int32 { if x < 0 { return 0 - x; }; return x; } func main() -> Int32 { return abs(0 - 5); }"
        ),
        5
    );
}

#[test]
fn diverging_then() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false { return 99; } else { yield 42; }; return x; }"
        ),
        42
    );
}

#[test]
fn diverging_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if true { yield 42; } else { return 99; }; return x; }"
        ),
        42
    );
}

#[test]
fn nested_if() {
    assert_eq!(
        compile_and_run(
            "func clamp(x: Int32, lo: Int32, hi: Int32) -> Int32 { if x < lo { return lo; }; if x > hi { return hi; }; return x; } func main() -> Int32 { return clamp(50, 0, 10); }"
        ),
        10
    );
}

#[test]
fn both_branches_diverge() {
    // Compiler requires top-level return even when all branches diverge,
    // so we add an unreachable return 0 at the end.
    assert_eq!(
        compile_and_run(
            r#"
            func choose(x: Int32) -> Int32 {
                if x > 0 { return 1; } else { return 0; };
                return 0;
            }
            func main() -> Int32 { return choose(5); }
        "#
        ),
        1
    );
}

// --- while loops ---

#[test]
fn while_sum() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 10 { s = s + i; i = i + 1; }; return s; }"
        ),
        45
    );
}

#[test]
fn while_factorial() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var n: Int32 = 5; var r: Int32 = 1; while n > 0 { r = r * n; n = n - 1; }; return r; }"
        ),
        120
    );
}

#[test]
fn while_false_body_not_executed() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 42; while false { x = 0; }; return x; }"
        ),
        42
    );
}

// --- break / continue ---

#[test]
fn while_break() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while true { if i == 3 { break; }; i = i + 1; }; return i; }"
        ),
        3
    );
}

#[test]
fn while_continue() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }"
        ),
        12
    );
}

#[test]
fn nested_break() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var outer: Int32 = 0; var i: Int32 = 0; while i < 3 { var j: Int32 = 0; while true { if j == 2 { break; }; j = j + 1; }; outer = outer + j; i = i + 1; }; return outer; }"
        ),
        6
    );
}

#[test]
fn break_with_var_update() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 0; while true { x = x + 10; break; }; return x; }"
        ),
        10
    );
}

#[test]
fn continue_skip_even() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 6 { i = i + 1; if (i / 2) * 2 == i { continue; }; s = s + i; }; return s; }"
        ),
        9
    );
}

#[test]
fn break_diverge_in_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 10 { let x: Int32 = if i == 5 { break; } else { yield i; }; i = x + 1; }; return i; }"
        ),
        5
    );
}

#[test]
fn continue_diverge_in_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; let v: Int32 = if i == 3 { continue; } else { yield i; }; s = s + v; }; return s; }"
        ),
        12
    );
}

#[test]
fn continue_nested_loops() {
    // Inner continue should not affect outer loop accumulation
    assert_eq!(
        compile_and_run(
            r#"
            func main() -> Int32 {
                var outer: Int32 = 0;
                var i: Int32 = 0;
                while i < 3 {
                    i = i + 1;
                    var j: Int32 = 0;
                    while j < 4 {
                        j = j + 1;
                        if j == 2 { continue; };
                        outer = outer + 1;
                    };
                };
                return outer;
            }
        "#
        ),
        9 // 3 outer iterations * 3 inner (4 - 1 skipped) = 9
    );
}

// --- break with value ---

#[test]
fn break_with_value() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = while true { break 42; }; return x; }"
        ),
        42
    );
}

#[test]
fn break_with_value_computed() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while true { i = i + 1; if i == 5 { break i * 10; }; }; return x; }"
        ),
        50
    );
}

#[test]
fn break_with_value_nested_if() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = while true { if true { break 1; } else { break 2; }; }; return x; }"
        ),
        1
    );
}

#[test]
fn break_with_complex_expr() {
    assert_eq!(
        compile_and_run(
            r#"
            func main() -> Int32 {
                var i: Int32 = 3;
                var j: Int32 = 5;
                let x: Int32 = while true {
                    break (i + 1) * (j - 1);
                };
                return x;
            }
        "#
        ),
        16 // (3+1) * (5-1) = 4 * 4 = 16
    );
}

// --- nobreak ---

#[test]
fn nobreak_basic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 5 { if i == 3 { break 99; }; i = i + 1; } nobreak { yield 0; }; return x; }"
        ),
        99
    );
}

#[test]
fn nobreak_condition_false() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }"
        ),
        3
    );
}

#[test]
fn nobreak_no_break_in_body() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 5 { i = i + 1; } nobreak { }; return i * 10; }"
        ),
        50
    );
}

// --- Short-circuit evaluation ---
// Short-circuit is verified with simple boolean expressions.
// Block expressions as && / || operands do not short-circuit in the current compiler.

#[test]
fn short_circuit_and() {
    // false && true -> false (RHS not evaluated due to short-circuit)
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let r = if false && true { yield 1; } else { yield 0; }; return r; }"
        ),
        0
    );
}

#[test]
fn short_circuit_or() {
    // true || false -> true (RHS not evaluated due to short-circuit)
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let r = if true || false { yield 1; } else { yield 0; }; return r; }"
        ),
        1
    );
}

// --- Error cases ---

#[test]
fn err_if_non_bool_cond() {
    compile_source_should_fail(
        "func main() -> Int32 { if 1 { yield 1; } else { yield 2; }; return 0; }",
    );
}

#[test]
fn err_if_branch_mismatch() {
    compile_source_should_fail(
        "func main() -> Int32 { if true { yield 1; } else { yield true; }; return 0; }",
    );
}

#[test]
fn err_while_non_bool_cond() {
    compile_source_should_fail("func main() -> Int32 { while 1 { }; return 0; }");
}

#[test]
fn err_yield_in_while() {
    compile_source_should_fail("func main() -> Int32 { while true { yield 1; }; return 0; }");
}

#[test]
fn err_break_outside_loop() {
    compile_source_should_fail("func main() -> Int32 { break; return 0; }");
}

#[test]
fn err_continue_outside_loop() {
    compile_source_should_fail("func main() -> Int32 { continue; return 0; }");
}

#[test]
fn err_break_value_no_nobreak() {
    compile_source_should_fail(
        "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; }; return x; }",
    );
}

#[test]
fn err_break_value_type_mismatch() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = while true { break true; }; return x; }",
    );
}

#[test]
fn err_nobreak_in_while_true() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = while true { break 10; } nobreak { yield 20; }; return x; }",
    );
}

#[test]
fn err_nobreak_type_mismatch() {
    compile_source_should_fail(
        "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; } nobreak { yield true; }; return x; }",
    );
}
