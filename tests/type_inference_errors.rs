mod common;

use common::{compile_and_run, compile_should_fail};

// Error cases

#[test]
fn error_unresolvable_type() {
    let result = compile_should_fail(
        "func default_value<T>() -> T { return 0; }
         func main() -> Int32 { let x = default_value(); return 0; }",
    );
    assert!(
        result.contains("cannot infer type parameter 'T'") && result.contains("default_value"),
        "Expected detailed inference error, got: {}",
        result
    );
}

#[test]
fn error_partial_type_args() {
    let result = compile_should_fail(
        "func pair<A, B>(a: A, b: B) -> Int32 { return 0; }
         func main() -> Int32 { return pair<Int32>(42, true); }",
    );
    assert!(
        result.contains("type argument") || result.contains("expected"),
        "Expected type arg error, got: {}",
        result
    );
}

#[test]
fn error_integer_float_mismatch() {
    let result = compile_should_fail(
        "func choose<T>(a: T, b: T) -> T { return a; }
         func main() -> Int32 { choose(42, 3.14); return 0; }",
    );
    assert!(
        result.contains("conflicting constraints") || result.contains("cannot unify"),
        "Expected type conflict error, got: {}",
        result
    );
}

#[test]
fn error_partial_inference_failure() {
    let result = compile_should_fail(
        "func make<A, B>(a: A) -> B { return a; }
         func main() -> Int32 { let x = make(42); return 0; }",
    );
    assert!(
        result.contains("cannot infer type parameter 'B'"),
        "Expected inference error for B, got: {}",
        result
    );
}

#[test]
fn error_multiple_inference_failures() {
    let result = compile_should_fail(
        "func default_a<T>() -> T { return 0; }
         func default_b<U>() -> U { return 0; }
         func main() -> Int32 {
             let x = default_a();
             let y = default_b();
             return 0;
         }",
    );
    assert!(
        result.contains("cannot infer type parameter"),
        "Expected inference error, got: {}",
        result
    );
}

#[test]
fn error_struct_init_inference_failure() {
    let result = compile_should_fail(
        "struct Holder<T> { var value: T; }
         func get_holder<T>() -> Holder<T> { return Holder(value: 0); }
         func main() -> Int32 { let h = get_holder(); return 0; }",
    );
    assert!(
        result.contains("cannot infer type parameter"),
        "Expected inference error, got: {}",
        result
    );
}

// Explicit type args coexistence with inference

#[test]
fn explicit_type_args_with_inference_coexist() {
    assert_eq!(
        compile_and_run(
            "func identity<T>(value: T) -> T { return value; }
             struct Box<T> { var value: T; }
             func main() -> Int32 {
                let a = identity<Int32>(42);
                let b = identity(100);
                let c = Box(value: 10);
                return a + b + c.value;
             }"
        ),
        152
    );
}
