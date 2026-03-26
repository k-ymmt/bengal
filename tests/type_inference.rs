mod common;
use common::{compile_and_run, compile_should_fail};

#[test]
fn infer_i64_from_annotation() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int64 = 42; return 0; }"),
        0
    );
}

#[test]
fn infer_i64_from_return() {
    assert_eq!(
        compile_and_run(
            "func to_i64() -> Int64 { return 42; }
             func main() -> Int32 { to_i64(); return 0; }"
        ),
        0
    );
}

#[test]
fn infer_i64_from_function_arg() {
    assert_eq!(
        compile_and_run(
            "func takes_i64(x: Int64) -> Int64 { return x; }
             func main() -> Int32 { takes_i64(42); return 0; }"
        ),
        0
    );
}

#[test]
fn infer_i64_from_binary_op() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 {
                let x: Int64 = 100;
                let y = x + 42;
                return 0;
            }"
        ),
        0
    );
}

#[test]
fn infer_i64_from_assignment() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { var x: Int64 = 0; x = 42; return 0; }"),
        0
    );
}

#[test]
fn infer_default_i32() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = 42; return x; }"),
        42
    );
}

#[test]
fn infer_default_f64() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = 3.14; return 0; }"),
        0
    );
}

#[test]
fn infer_f32_from_annotation() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Float32 = 3.14; return 0; }"),
        0
    );
}

#[test]
fn infer_i32_from_comparison() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 {
                let x: Int32 = 10;
                if x > 5 { return 1; };
                return 0;
            }"
        ),
        1
    );
}

#[test]
fn infer_multiple_literals_same_type() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 {
                let a: Int64 = 1;
                let b = a + 2 + 3;
                return 0;
            }"
        ),
        0
    );
}

#[test]
fn infer_literal_in_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 {
                let x: Int32 = if true { yield 1; } else { yield 2; };
                return x;
            }"
        ),
        1
    );
}

#[test]
fn err_incompatible_literal_types() {
    // Integer literal cannot unify with Bool
    let err = compile_should_fail("func main() -> Int32 { let x: Bool = 42; return 0; }");
    assert!(
        err.contains("unify") || err.contains("mismatch"),
        "got: {}",
        err
    );
}
