mod common;

use common::{compile_and_run, compile_source_should_fail};

// --- Basic function definitions ---

#[test]
fn simple() {
    assert_eq!(compile_and_run("func main() -> Int32 { return 42; }"), 42);
}

#[test]
fn call() {
    assert_eq!(
        compile_and_run(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; }\nfunc main() -> Int32 { return add(3, 4); }"
        ),
        7
    );
}

#[test]
fn call_chain() {
    assert_eq!(
        compile_and_run(
            "func double(x: Int32) -> Int32 { return x * 2; }\nfunc main() -> Int32 { return double(double(5)); }"
        ),
        20
    );
}

#[test]
fn multiple_functions() {
    assert_eq!(
        compile_and_run(
            "func square(x: Int32) -> Int32 { return x * x; }\nfunc main() -> Int32 { return square(3) + square(4); }"
        ),
        25
    );
}

#[test]
fn unit_return() {
    assert_eq!(
        compile_and_run("func noop() { return; } func main() -> Int32 { noop(); return 42; }"),
        42
    );
}

#[test]
fn fibonacci() {
    assert_eq!(
        compile_and_run(
            "func fibonacci(n: Int32) -> Int32 { var a: Int32 = 0; var b: Int32 = 1; var i: Int32 = 0; while i < n { let next: Int32 = a + b; a = b; b = next; i = i + 1; }; return a; } func main() -> Int32 { return fibonacci(10); }"
        ),
        55
    );
}

// --- New tests ---

#[test]
fn recursive_countdown() {
    assert_eq!(
        compile_and_run(
            r#"
            func countdown(n: Int32) -> Int32 {
                if n <= 0 { return 0; };
                return 1 + countdown(n - 1);
            }
            func main() -> Int32 { return countdown(5); }
        "#
        ),
        5
    );
}

#[test]
fn multi_param_function() {
    assert_eq!(
        compile_and_run(
            r#"
            func add3(a: Int32, b: Int32, c: Int32) -> Int32 {
                return a + b + c;
            }
            func main() -> Int32 { return add3(1, 2, 3); }
        "#
        ),
        6
    );
}

#[test]
fn function_returns_bool() {
    assert_eq!(
        compile_and_run(
            r#"
            func is_positive(x: Int32) -> Bool {
                if x > 0 { return true; };
                return false;
            }
            func main() -> Int32 {
                let r = if is_positive(5) { yield 1; } else { yield 0; };
                return r;
            }
        "#
        ),
        1
    );
}

// --- Error cases ---

#[test]
fn err_no_main() {
    compile_source_should_fail("func add(a: Int32, b: Int32) -> Int32 { return a + b; }");
}

#[test]
fn err_main_with_params() {
    compile_source_should_fail("func main(x: Int32) -> Int32 { return x; }");
}

#[test]
fn err_no_return() {
    compile_source_should_fail("func main() -> Int32 { let x: Int32 = 1; }");
}

#[test]
fn err_no_yield() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = { let a: Int32 = 1; }; return x; }",
    );
}

#[test]
fn err_yield_in_func() {
    compile_source_should_fail("func main() -> Int32 { yield 1; }");
}

#[test]
fn err_return_in_block() {
    compile_source_should_fail("func main() -> Int32 { let x: Int32 = { return 1; }; return x; }");
}

#[test]
fn err_return_type_mismatch() {
    compile_source_should_fail("func main() -> Int32 { return true; }");
}

#[test]
fn err_duplicate_function() {
    compile_source_should_fail(
        r#"
        func foo() -> Int32 { return 1; }
        func foo() -> Int32 { return 2; }
        func main() -> Int32 { return foo(); }
    "#,
    );
}

#[test]
fn err_wrong_arg_count() {
    compile_source_should_fail(
        r#"
        func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        func main() -> Int32 { return add(1); }
    "#,
    );
}
