mod common;
use common::{compile_and_run, compile_should_fail};

#[test]
fn array_literal_and_access() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let a = [10, 20, 30]; return a[1]; }"),
        20
    );
}

#[test]
fn array_index_assign() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { var a = [1, 2, 3]; a[0] = 42; return a[0]; }"),
        42
    );
}

#[test]
fn array_with_type_annotation() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let a: [Int32; 3] = [1, 2, 3]; return a[2]; }"),
        3
    );
}

#[test]
fn array_as_function_param() {
    assert_eq!(
        compile_and_run(
            "func sum(arr: [Int32; 3]) -> Int32 { return arr[0] + arr[1] + arr[2]; }
             func main() -> Int32 { return sum([10, 20, 30]); }"
        ),
        60
    );
}

#[test]
fn array_as_return_type() {
    assert_eq!(
        compile_and_run(
            "func make() -> [Int32; 2] { return [5, 6]; }
             func main() -> Int32 { let a = make(); return a[0] + a[1]; }"
        ),
        11
    );
}

#[test]
fn error_mixed_element_types() {
    let err = compile_should_fail("func main() -> Int32 { let a = [1, true]; return 0; }");
    assert!(err.contains("same type"), "got: {}", err);
}

#[test]
fn error_size_mismatch() {
    let err = compile_should_fail("func main() -> Int32 { let a: [Int32; 3] = [1, 2]; return 0; }");
    assert!(err.contains("size"), "got: {}", err);
}

#[test]
fn error_constant_oob() {
    let err = compile_should_fail("func main() -> Int32 { let a = [1, 2, 3]; return a[3]; }");
    assert!(err.contains("out of bounds"), "got: {}", err);
}

#[test]
fn error_index_non_array() {
    let err = compile_should_fail("func main() -> Int32 { let x = 5; return x[0]; }");
    assert!(err.contains("cannot index"), "got: {}", err);
}

#[test]
fn error_immutable_index_assign() {
    let err = compile_should_fail("func main() -> Int32 { let a = [1, 2]; a[0] = 5; return 0; }");
    assert!(err.contains("immutable"), "got: {}", err);
}

#[test]
fn error_non_integer_index() {
    let err = compile_should_fail("func main() -> Int32 { let a = [1, 2, 3]; return a[true]; }");
    assert!(err.contains("integer"), "got: {}", err);
}
