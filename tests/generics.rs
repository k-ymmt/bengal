mod common;

use common::compile_and_run;
use common::compile_should_fail;

#[test]
#[ignore] // requires monomorphization (Task 6)
fn generic_identity_i32() {
    assert_eq!(
        compile_and_run(
            "func identity<T>(value: T) -> T { return value; }
             func main() -> Int32 { return identity<Int32>(42); }",
        ),
        42
    );
}

#[test]
fn generic_constraint_violation() {
    let err = compile_should_fail(
        "protocol Summable { func sum() -> Int32; }
         struct Dummy { var x: Int32; }
         func getSum<T: Summable>(item: T) -> Int32 { return item.sum(); }
         func main() -> Int32 { return getSum<Dummy>(Dummy(x: 1)); }",
    );
    assert!(
        err.contains("does not conform to protocol"),
        "expected constraint violation error, got: {}",
        err
    );
}

#[test]
fn generic_missing_type_args() {
    let err = compile_should_fail(
        "func identity<T>(value: T) -> T { return value; }
         func main() -> Int32 { return identity(42); }",
    );
    assert!(
        err.contains("requires explicit type arguments"),
        "expected missing type args error, got: {}",
        err
    );
}

#[test]
fn generic_non_generic_with_type_args() {
    let err = compile_should_fail(
        "func add(a: Int32, b: Int32) -> Int32 { return a + b; }
         func main() -> Int32 { return add<Int32>(1, 2); }",
    );
    assert!(
        err.contains("does not take type arguments"),
        "expected 'does not take type arguments' error, got: {}",
        err
    );
}

#[test]
fn generic_type_arg_count_mismatch() {
    let err = compile_should_fail(
        "func identity<T>(value: T) -> T { return value; }
         func main() -> Int32 { return identity<Int32, Int64>(42); }",
    );
    assert!(
        err.contains("expected") && err.contains("type argument"),
        "expected type arg count mismatch error, got: {}",
        err
    );
}
