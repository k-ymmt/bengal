mod common;

use common::compile_and_run;

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
