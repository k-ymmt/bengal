mod common;

use common::{compile_and_run, compile_source_should_fail};

// --- Literals and arithmetic ---

#[test]
fn literal() {
    assert_eq!(compile_and_run("42"), 42);
}

#[test]
fn addition() {
    assert_eq!(compile_and_run("2 + 3"), 5);
}

#[test]
fn subtraction() {
    assert_eq!(compile_and_run("10 - 4"), 6);
}

#[test]
fn multiplication() {
    assert_eq!(compile_and_run("3 * 7"), 21);
}

#[test]
fn division() {
    assert_eq!(compile_and_run("20 / 4"), 5);
}

// --- Precedence and associativity ---

#[test]
fn precedence() {
    assert_eq!(compile_and_run("2 + 3 * 4"), 14);
}

#[test]
fn parentheses() {
    assert_eq!(compile_and_run("(2 + 3) * 4"), 20);
}

#[test]
fn nested_parentheses() {
    assert_eq!(compile_and_run("((1 + 2) * (3 + 4))"), 21);
}

#[test]
fn left_assoc_division() {
    assert_eq!(compile_and_run("100 / 10 / 2"), 5);
}

#[test]
fn left_assoc_subtraction() {
    assert_eq!(compile_and_run("1 - 2 - 3"), -4);
}

#[test]
fn complex_precedence() {
    // 1 + 2 * 3 - 4 / 2 = 1 + 6 - 2 = 5
    assert_eq!(
        compile_and_run("func main() -> Int32 { return 1 + 2 * 3 - 4 / 2; }"),
        5
    );
}

// --- Multi-numeric types ---

#[test]
fn i64_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int64 = 100 as Int64; let y: Int64 = 200 as Int64; return (x + y) as Int32; }"
        ),
        300
    );
}

#[test]
fn i64_comparison() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int64 = 10 as Int64; let y: Int64 = 20 as Int64; let r: Int32 = if x < y { yield 1; } else { yield 0; }; return r; }"
        ),
        1
    );
}

#[test]
fn f64_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Float64 = 3.5; let y: Float64 = 1.5; return (x + y) as Int32; }"
        ),
        5
    );
}

#[test]
fn float32_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Float32 = 3.5 as Float32; let y: Float32 = 1.5 as Float32; return (x + y) as Int32; }"
        ),
        5
    );
}

#[test]
fn mixed_cast_chain() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = 42; let y: Int64 = x as Int64; let z: Int32 = y as Int32; return z; }"
        ),
        42
    );
}

// --- Cast ---

#[test]
fn cast_i32_to_i64() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int64 = 42 as Int64; return x as Int32; }"),
        42
    );
}

#[test]
fn cast_noop() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = 42 as Int32; return x; }"),
        42
    );
}

#[test]
fn cast_i32_to_f32() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Float32 = 10 as Float32; return x as Int32; }",
        ),
        10
    );
}

#[test]
fn cast_f32_to_f64() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Float32 = 7.0 as Float32; let y: Float64 = x as Float64; return y as Int32; }"
        ),
        7
    );
}

#[test]
fn cast_chain_all_types() {
    // Int32 -> Int64 -> Float64 -> Float32 -> Int32
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let a: Int32 = 42; let b: Int64 = a as Int64; let c: Float64 = b as Float64; let d: Float32 = c as Float32; return d as Int32; }"
        ),
        42
    );
}

// --- Error cases ---

#[test]
fn err_cast_bool() {
    compile_source_should_fail("func main() -> Int32 { let x = true as Int32; return x; }");
}

#[test]
fn err_mixed_arithmetic() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = 1; let y: Int64 = 2 as Int64; return x + y; }",
    );
}

#[test]
fn err_infer_mismatch() {
    compile_source_should_fail("func main() -> Int32 { let x: Int32 = 3.14; return 0; }");
}

#[test]
fn err_integer_overflow() {
    compile_source_should_fail("func main() -> Int32 { let x = 3000000000; return 0; }");
}

#[test]
fn err_as_binds_tighter_than_addition() {
    // `1 + 2 as Int64` parses as `1 + (2 as Int64)` => Int32 + Int64 => type error
    compile_source_should_fail("func main() -> Int32 { return 1 + 2 as Int64; }");
}

#[test]
fn err_comparison_type_mismatch() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int64 = 1 as Int64; let r = if x == 1 { yield 1; } else { yield 0; }; return r; }",
    );
}
