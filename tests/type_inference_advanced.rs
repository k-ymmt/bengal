mod common;

use common::{compile_and_run, compile_should_fail};

// --- Task 9: Generic type argument inference ---

#[test]
fn infer_generic_identity() {
    assert_eq!(
        compile_and_run(
            "func identity<T>(value: T) -> T { return value; }
             func main() -> Int32 { return identity(42); }"
        ),
        42
    );
}

#[test]
fn infer_generic_struct_init() {
    assert_eq!(
        compile_and_run(
            "struct Box<T> { var value: T; }
             func main() -> Int32 {
                let b = Box(value: 42);
                return b.value;
             }"
        ),
        42
    );
}

#[test]
fn infer_generic_from_expected_type() {
    assert_eq!(
        compile_and_run(
            "struct Box<T> { var value: T; }
             func main() -> Int32 {
                let b: Box<Int64> = Box(value: 42);
                return 0;
             }"
        ),
        0
    );
}

#[test]
fn infer_generic_multiple_type_params() {
    assert_eq!(
        compile_and_run(
            "struct Pair<A, B> { var first: A; var second: B; }
             func main() -> Int32 {
                let p = Pair(first: 42, second: true);
                return p.first;
             }"
        ),
        42
    );
}

#[test]
fn infer_generic_chained_calls() {
    assert_eq!(
        compile_and_run(
            "func identity<T>(value: T) -> T { return value; }
             func main() -> Int32 {
                let x = identity(identity(42));
                return x;
             }"
        ),
        42
    );
}

#[test]
fn explicit_type_args_still_work() {
    assert_eq!(
        compile_and_run(
            "func identity<T>(value: T) -> T { return value; }
             func main() -> Int32 { return identity<Int32>(42); }"
        ),
        42
    );
}

// --- Task 10: Loop inference ---

#[test]
fn loop_break_unit() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { while true { break; }; return 0; }"),
        0
    );
}

#[test]
fn loop_no_break_unit() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 {
            var i: Int32 = 0;
            while i < 3 { i = i + 1; };
            return i;
         }"
        ),
        3
    );
}

#[test]
fn loop_break_with_value() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 {
            let x: Int32 = while true { break 42; };
            return x;
         }"
        ),
        42
    );
}

#[test]
fn loop_break_infer_i64() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 {
            let x: Int64 = while true { break 42; };
            return 0;
         }"
        ),
        0
    );
}

#[test]
fn loop_nobreak_infer_i64() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 {
            var i: Int32 = 0;
            let x: Int64 = while i < 10 {
                if i == 5 { break 99; };
                i = i + 1;
            } nobreak { yield 0; };
            return 0;
         }"
        ),
        0
    );
}

// --- Task 11: Protocol constraint validation for inferred type args ---

#[test]
fn infer_constraint_violation() {
    let result = compile_should_fail(
        "protocol Summable { func sum() -> Int32; }
         struct Wrapper<T: Summable> { var value: T; }
         func main() -> Int32 {
            let w = Wrapper(value: true);
            return 0;
         }",
    );
    assert!(
        result.contains("does not conform")
            || result.contains("constraint")
            || result.contains("Summable"),
        "Expected constraint error, got: {}",
        result
    );
}

#[test]
fn infer_constraint_satisfied() {
    // Wrapper<T: Summable> with inferred T = MyNum should pass constraint check
    assert_eq!(
        compile_and_run(
            "protocol Summable { func sum() -> Int32; }
             struct MyNum: Summable {
                var x: Int32;
                func sum() -> Int32 { return self.x; }
             }
             struct Wrapper<T: Summable> { var value: T; }
             func main() -> Int32 {
                let w = Wrapper(value: MyNum(x: 42));
                return 0;
             }"
        ),
        0
    );
}

#[test]
fn infer_constraint_violation_func() {
    let result = compile_should_fail(
        "protocol Printable { func show() -> Int32; }
         func wrap<T: Printable>(value: T) -> T { return value; }
         func main() -> Int32 {
            let x = wrap(42);
            return 0;
         }",
    );
    assert!(
        result.contains("does not conform") || result.contains("Printable"),
        "Expected constraint error, got: {}",
        result
    );
}

#[test]
fn infer_constraint_satisfied_func() {
    // extract<T: Showable> with inferred T = Val should pass constraint check
    assert_eq!(
        compile_and_run(
            "protocol Showable { func show() -> Int32; }
             struct Val: Showable {
                var n: Int32;
                func show() -> Int32 { return self.n; }
             }
             func use_val(v: Val) -> Int32 { return v.n; }
             func extract<T: Showable>(value: T) -> T { return value; }
             func main() -> Int32 {
                return use_val(extract(Val(n: 7)));
             }"
        ),
        7
    );
}

// --- Task 13: Comprehensive end-to-end test suite ---

// Numeric literal edge cases

#[test]
fn infer_yield_in_block() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int64 = { yield 42; }; return 0; }"),
        0
    );
}

#[test]
fn infer_array_element_type() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let arr: [Int64; 3] = [1, 2, 3]; return 0; }"),
        0
    );
}

// Method/field on generic struct

#[test]
fn method_on_inferred_generic_struct() {
    assert_eq!(
        compile_and_run(
            "struct Box<T> { var value: T;
                func get() -> T { return self.value; }
             }
             func main() -> Int32 {
                let b = Box(value: 42);
                return b.get();
             }"
        ),
        42
    );
}

#[test]
fn field_assign_on_generic_struct() {
    assert_eq!(
        compile_and_run(
            "struct Box<T> { var value: T; }
             func main() -> Int32 {
                var b = Box(value: 0);
                b.value = 42;
                return b.value;
             }"
        ),
        42
    );
}
