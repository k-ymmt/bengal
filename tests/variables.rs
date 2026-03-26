mod common;

use common::{compile_and_run, compile_source_should_fail};

// --- let / var bindings ---

#[test]
fn let_binding() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = 10; return x; }"),
        10
    );
}

#[test]
fn var_binding() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { var x: Int32 = 1; x = 10; return x; }"),
        10
    );
}

#[test]
fn let_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let a: Int32 = 2; let b: Int32 = 3; return a + b * 4; }"
        ),
        14
    );
}

#[test]
fn shadowing() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = 1; let x: Int32 = x + 10; return x; }"
        ),
        11
    );
}

#[test]
fn var_update() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 0; x = x + 1; x = x + 2; return x; }"
        ),
        3
    );
}

// --- Block expressions ---

#[test]
fn block_expression() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = { yield 7; }; return x; }"),
        7
    );
}

#[test]
fn block_shadow() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = 1; let y: Int32 = { let x: Int32 = 10; yield x + 1; }; return x + y; }"
        ),
        12
    );
}

#[test]
fn block_var_assign() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 0; x = { x = 10; yield x + 1; }; return x; }"
        ),
        11
    );
}

// --- Type inference ---

#[test]
fn infer_i32() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = 10; return x; }"),
        10
    );
}

#[test]
fn infer_i32_expr() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = 1 + 2 * 3; return x; }"),
        7
    );
}

#[test]
fn infer_bool() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let b = true; let r: Int32 = if b { yield 1; } else { yield 0; }; return r; }"
        ),
        1
    );
}

#[test]
fn infer_var() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { var x = 0; x = x + 1; return x; }"),
        1
    );
}

// --- New tests ---

#[test]
fn shadow_function_param() {
    assert_eq!(
        compile_and_run(
            r#"
            func foo(x: Int32) -> Int32 {
                let x: Int32 = x + 100;
                return x;
            }
            func main() -> Int32 { return foo(5); }
        "#
        ),
        105
    );
}

#[test]
fn shadow_nested_scopes() {
    assert_eq!(
        compile_and_run(
            r#"
            func main() -> Int32 {
                let x: Int32 = 1;
                let y: Int32 = {
                    let x: Int32 = 10;
                    let z: Int32 = {
                        let x: Int32 = 100;
                        yield x;
                    };
                    yield x + z;
                };
                return x + y;
            }
        "#
        ),
        111
    );
}

#[test]
fn infer_from_block_expr() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = { yield 1 + 2; }; return x; }"),
        3
    );
}

#[test]
fn infer_from_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x = if true { yield 1; } else { yield 2; }; return x; }"
        ),
        1
    );
}

#[test]
fn infer_float64() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = 3.14; return x as Int32; }"),
        3
    );
}

// --- Error cases ---

#[test]
fn err_undefined_var() {
    compile_source_should_fail("func main() -> Int32 { return x; }");
}

#[test]
fn err_immutable_assign() {
    compile_source_should_fail("func main() -> Int32 { let x: Int32 = 1; x = 2; return x; }");
}

#[test]
fn err_type_annotation_mismatch() {
    // Bool value cannot be assigned to Int32 variable
    compile_source_should_fail("func main() -> Int32 { let x: Int32 = true; return 0; }");
}
