mod common;

use common::compile_and_run;
use common::compile_should_fail;

#[test]
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

#[test]
fn generic_struct_basic() {
    assert_eq!(
        compile_and_run(
            "struct Box<T> { var value: T; }
         func main() -> Int32 {
             let b = Box<Int32>(value: 42);
             return b.value;
         }",
        ),
        42,
    );
}

#[test]
fn generic_struct_with_method() {
    assert_eq!(
        compile_and_run(
            "struct Wrapper<T> { var value: T;
             func get() -> T { return self.value; }
         }
         func main() -> Int32 {
             let w = Wrapper<Int32>(value: 7);
             return w.get();
         }",
        ),
        7,
    );
}

#[test]
fn generic_struct_multiple_type_params() {
    assert_eq!(
        compile_and_run(
            "struct Pair<A, B> { var first: A; var second: B; }
         func main() -> Int32 {
             let p = Pair<Int32, Bool>(first: 10, second: true);
             return p.first;
         }",
        ),
        10,
    );
}

#[test]
fn generic_struct_with_constraint() {
    assert_eq!(
        compile_and_run(
            "protocol Summable { func sum() -> Int32; }
         struct Point: Summable { var x: Int32; var y: Int32; func sum() -> Int32 { return self.x + self.y; } }
         struct Wrapper<T: Summable> { var value: T;
             func getSum() -> Int32 { return self.value.sum(); }
         }
         func main() -> Int32 {
             let w = Wrapper<Point>(value: Point(x: 3, y: 4));
             return w.getSum();
         }",
        ),
        7,
    );
}

#[test]
fn error_wrong_type_arg_count() {
    let err = compile_should_fail(
        "struct Pair<A, B> { var first: A; var second: B; }
         func main() -> Int32 { let p = Pair<Int32>(first: 1, second: 2); return 0; }",
    );
    assert!(
        err.contains("expected 2 type argument"),
        "expected type arg count error, got: {}",
        err
    );
}

#[test]
fn error_type_args_on_non_generic() {
    let err = compile_should_fail(
        "func bar(x: Int32) -> Int32 { return x; }
         func main() -> Int32 { return bar<Int32>(1); }",
    );
    assert!(
        err.contains("does not take type arguments"),
        "expected 'does not take type arguments' error, got: {}",
        err
    );
}

#[test]
fn generic_same_func_multiple_types() {
    assert_eq!(
        compile_and_run(
            "func identity<T>(value: T) -> T { return value; }
         func main() -> Int32 {
             let a = identity<Int32>(42);
             let b = identity<Bool>(true);
             return a;
         }",
        ),
        42,
    );
}
