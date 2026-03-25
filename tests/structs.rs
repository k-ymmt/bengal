mod common;

use common::{compile_and_run, compile_should_fail, compile_source_should_fail};

// --- Basic struct ---

#[test]
fn basic() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Point { var x: Int32; var y: Int32; }
            func main() -> Int32 {
                let p = Point(x: 3, y: 4);
                return p.x + p.y;
            }
        "#
        ),
        7
    );
}

#[test]
fn function_arg_return() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Point { var x: Int32; var y: Int32; }
            func make_point(a: Int32, b: Int32) -> Point {
                return Point(x: a, y: b);
            }
            func sum(p: Point) -> Int32 { return p.x + p.y; }
            func main() -> Int32 {
                let p = make_point(10, 20);
                return sum(p);
            }
        "#
        ),
        30
    );
}

#[test]
fn nested_struct() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Inner { var value: Int32; }
            struct Outer { var inner: Inner; var extra: Int32; }
            func main() -> Int32 {
                let i = Inner(value: 10);
                let o = Outer(inner: i, extra: 20);
                return o.inner.value + o.extra;
            }
        "#
        ),
        30
    );
}

#[test]
fn explicit_init() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Counter {
                var value: Int32;
                init(start: Int32) {
                    self.value = start * 2;
                }
            }
            func main() -> Int32 {
                let c = Counter(start: 5);
                return c.value;
            }
        "#
        ),
        10
    );
}

#[test]
fn zero_arg_init() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Default {
                var value: Int32;
                init() {
                    self.value = 99;
                }
            }
            func main() -> Int32 {
                let d = Default();
                return d.value;
            }
        "#
        ),
        99
    );
}

// --- Computed properties ---

#[test]
fn computed_property_get() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Rect {
                var w: Int32;
                var h: Int32;
                var area: Int32 {
                    get { return self.w * self.h; }
                };
            }
            func main() -> Int32 {
                let r = Rect(w: 3, h: 4);
                return r.area;
            }
        "#
        ),
        12
    );
}

#[test]
fn computed_property_get_set() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Box {
                var stored: Int32;
                var doubled: Int32 {
                    get { return self.stored * 2; }
                    set { self.stored = newValue / 2; }
                };
            }
            func main() -> Int32 {
                var b = Box(stored: 5);
                b.doubled = 20;
                return b.stored;
            }
        "#
        ),
        10
    );
}

#[test]
fn computed_property_multi() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Point {
                var x: Int32;
                var y: Int32;
                var sum: Int32 {
                    get { return self.x + self.y; }
                };
                var product: Int32 {
                    get { return self.x * self.y; }
                };
            }
            func main() -> Int32 {
                let p = Point(x: 3, y: 4);
                return p.sum + p.product;
            }
        "#
        ),
        19 // (3+4) + (3*4) = 7 + 12 = 19
    );
}

#[test]
fn field_assign_complex_expr() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Box { var value: Int32; }
            func main() -> Int32 {
                var b = Box(value: 0);
                b.value = if true { yield 42; } else { yield 0; };
                return b.value;
            }
        "#
        ),
        42
    );
}

// --- Error cases ---

// Note: err_recursive_struct is omitted — the compiler does not yet detect
// recursive struct definitions. This should be added when the check is implemented.

#[test]
fn err_duplicate_member() {
    compile_should_fail(
        r#"
        struct Bad {
            var x: Int32;
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#,
    );
}

#[test]
fn err_init_missing_field() {
    compile_should_fail(
        r#"
        struct Pair {
            var a: Int32;
            var b: Int32;
            init(x: Int32) {
                self.a = x;
            }
        }
        func main() -> Int32 { return 0; }
    "#,
    );
}

#[test]
fn err_let_struct_field_assign() {
    compile_source_should_fail(
        r#"
        struct Point { var x: Int32; var y: Int32; }
        func main() -> Int32 {
            let p = Point(x: 1, y: 2);
            p.x = 10;
            return p.x;
        }
    "#,
    );
}

#[test]
fn err_memberwise_with_explicit_init() {
    compile_source_should_fail(
        r#"
        struct Foo {
            var x: Int32;
            init(val: Int32) {
                self.x = val;
            }
        }
        func main() -> Int32 {
            let f = Foo(x: 1);
            return f.x;
        }
    "#,
    );
}
