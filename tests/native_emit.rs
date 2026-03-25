mod common;

use common::compile_to_native_and_run;

#[test]
fn native_bare_expression() {
    assert_eq!(compile_to_native_and_run("42"), 42);
}

#[test]
fn native_arithmetic() {
    assert_eq!(
        compile_to_native_and_run("func main() -> Int32 { return 2 + 3 * 4; }"),
        14
    );
}

#[test]
fn native_function_call() {
    assert_eq!(
        compile_to_native_and_run(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }"
        ),
        7
    );
}

#[test]
fn native_control_flow() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 10 { i = i + 1; }; return i; }"
        ),
        10
    );
}

#[test]
fn native_type_cast() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { let x: Int64 = 100 as Int64; return x as Int32; }"
        ),
        100
    );
}

#[test]
fn native_struct_basic() {
    assert_eq!(
        compile_to_native_and_run(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x + p.y; }"
        ),
        7
    );
}

#[test]
fn native_method_call() {
    assert_eq!(
        compile_to_native_and_run(
            r#"
            struct Counter {
                var n: Int32;
                func value() -> Int32 {
                    return self.n;
                }
            }
            func main() -> Int32 {
                let c = Counter(n: 42);
                return c.value();
            }
        "#
        ),
        42
    );
}
