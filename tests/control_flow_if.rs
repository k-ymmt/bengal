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
fn exhaustive_return_if_else() {
    // Both branches return — no trailing return needed
    assert_eq!(
        compile_and_run(
            r#"
            func choose(x: Int32) -> Int32 {
                if x > 0 { return 1; } else { return 0; };
            }
            func main() -> Int32 { return choose(5); }
        "#,
        ),
        1
    );
}

#[test]
fn exhaustive_return_nested() {
    // Nested if/else where all leaf branches return
    assert_eq!(
        compile_and_run(
            r#"
            func classify(x: Int32) -> Int32 {
                if x > 0 {
                    if x > 100 { return 2; } else { return 1; };
                } else {
                    return 0;
                };
            }
            func main() -> Int32 { return classify(50); }
        "#,
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
