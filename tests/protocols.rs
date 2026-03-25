mod common;

use common::{compile_and_run, compile_should_fail};

// --- Conformance ---

#[test]
fn protocol_basic_conformance() {
    assert_eq!(
        compile_and_run(
            r#"
            protocol Summable {
                func sum() -> Int32;
            }
            struct Point: Summable {
                var x: Int32;
                var y: Int32;
                func sum() -> Int32 {
                    return self.x + self.y;
                }
            }
            func main() -> Int32 {
                let p = Point(x: 3, y: 4);
                return p.sum();
            }
        "#
        ),
        7
    );
}

#[test]
fn protocol_multiple_methods() {
    assert_eq!(
        compile_and_run(
            r#"
            protocol Describable {
                func first() -> Int32;
                func second() -> Int32;
            }
            struct Pair: Describable {
                var a: Int32;
                var b: Int32;
                func first() -> Int32 {
                    return self.a;
                }
                func second() -> Int32 {
                    return self.b;
                }
            }
            func main() -> Int32 {
                let p = Pair(a: 10, b: 20);
                return p.first() + p.second();
            }
        "#
        ),
        30
    );
}

#[test]
fn protocol_property_get() {
    assert_eq!(
        compile_and_run(
            r#"
            protocol HasTotal {
                var total: Int32 { get };
            }
            struct Numbers: HasTotal {
                var a: Int32;
                var b: Int32;
                var total: Int32 {
                    get { return self.a + self.b; }
                };
            }
            func main() -> Int32 {
                let n = Numbers(a: 5, b: 7);
                return n.total;
            }
        "#
        ),
        12
    );
}

#[test]
fn protocol_stored_property_satisfies_get() {
    assert_eq!(
        compile_and_run(
            r#"
            protocol HasValue {
                var value: Int32 { get };
            }
            struct Box: HasValue {
                var value: Int32;
            }
            func main() -> Int32 {
                let b = Box(value: 42);
                return b.value;
            }
        "#
        ),
        42
    );
}

#[test]
fn protocol_multiple_conformance() {
    assert_eq!(
        compile_and_run(
            r#"
            protocol Addable {
                func sum() -> Int32;
            }
            protocol Scalable {
                func scale(factor: Int32) -> Int32;
            }
            struct Value: Addable, Scalable {
                var n: Int32;
                func sum() -> Int32 {
                    return self.n;
                }
                func scale(factor: Int32) -> Int32 {
                    return self.n * factor;
                }
            }
            func main() -> Int32 {
                let v = Value(n: 5);
                return v.sum() + v.scale(3);
            }
        "#
        ),
        20
    );
}

#[test]
fn protocol_property_get_set() {
    assert_eq!(
        compile_and_run(
            r#"
            protocol Resettable {
                var current: Int32 { get set };
            }
            struct Counter: Resettable {
                var value: Int32;
                var current: Int32 {
                    get { return self.value; }
                    set { self.value = newValue; }
                };
            }
            func main() -> Int32 {
                var c = Counter(value: 10);
                c.current = 99;
                return c.current;
            }
        "#
        ),
        99
    );
}

#[test]
fn protocol_method_with_params() {
    assert_eq!(
        compile_and_run(
            r#"
            protocol Transformer {
                func transform(x: Int32, y: Int32) -> Int32;
            }
            struct Adder: Transformer {
                var base: Int32;
                func transform(x: Int32, y: Int32) -> Int32 {
                    return self.base + x + y;
                }
            }
            func main() -> Int32 {
                let a = Adder(base: 100);
                return a.transform(10, 20);
            }
        "#
        ),
        130
    );
}

// --- Error cases ---

#[test]
fn protocol_error_missing_method() {
    let err = compile_should_fail(
        r#"
        protocol Summable {
            func sum() -> Int32;
        }
        struct Empty: Summable {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#,
    );
    assert!(err.contains("does not implement method"), "got: {}", err);
}

#[test]
fn protocol_error_return_type_mismatch() {
    let err = compile_should_fail(
        r#"
        protocol Summable {
            func sum() -> Int32;
        }
        struct Bad: Summable {
            var x: Int32;
            func sum() -> Bool {
                return true;
            }
        }
        func main() -> Int32 { return 0; }
    "#,
    );
    assert!(err.contains("return type"), "got: {}", err);
}

#[test]
fn protocol_error_unknown_protocol() {
    let err = compile_should_fail(
        r#"
        struct Bad: NonExistent {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#,
    );
    assert!(err.contains("unknown protocol"), "got: {}", err);
}

#[test]
fn protocol_error_missing_property() {
    let err = compile_should_fail(
        r#"
        protocol HasTotal {
            var total: Int32 { get };
        }
        struct Bad: HasTotal {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#,
    );
    assert!(err.contains("does not implement property"), "got: {}", err);
}

#[test]
fn protocol_error_missing_setter() {
    let err = compile_should_fail(
        r#"
        protocol Writable {
            var value: Int32 { get set };
        }
        struct ReadOnly: Writable {
            var x: Int32;
            var value: Int32 {
                get { return self.x; }
            };
        }
        func main() -> Int32 { return 0; }
    "#,
    );
    assert!(err.contains("requires a setter"), "got: {}", err);
}

#[test]
fn err_param_count_mismatch() {
    let err = compile_should_fail(
        r#"
        protocol HasOp {
            func op(x: Int32) -> Int32;
        }
        struct Bad: HasOp {
            var n: Int32;
            func op() -> Int32 {
                return self.n;
            }
        }
        func main() -> Int32 { return 0; }
    "#,
    );
    assert!(
        err.contains("parameter") || err.contains("does not implement"),
        "got: {}",
        err
    );
}

#[test]
fn err_param_type_mismatch() {
    let err = compile_should_fail(
        r#"
        protocol HasOp {
            func op(x: Int32) -> Int32;
        }
        struct Bad: HasOp {
            var n: Int32;
            func op(x: Bool) -> Int32 {
                return self.n;
            }
        }
        func main() -> Int32 { return 0; }
    "#,
    );
    assert!(
        err.contains("parameter") || err.contains("does not implement"),
        "got: {}",
        err
    );
}

#[test]
fn err_duplicate_protocol() {
    compile_should_fail(
        r#"
        protocol Foo {
            func bar() -> Int32;
        }
        protocol Foo {
            func baz() -> Int32;
        }
        func main() -> Int32 { return 0; }
    "#,
    );
}

#[test]
fn err_property_type_mismatch() {
    let err = compile_should_fail(
        r#"
        protocol HasValue {
            var value: Int32 { get };
        }
        struct Bad: HasValue {
            var value: Bool;
        }
        func main() -> Int32 { return 0; }
    "#,
    );
    assert!(
        err.contains("type") || err.contains("does not implement"),
        "got: {}",
        err
    );
}
