mod common;

use common::{compile_and_run, compile_source_should_fail};

#[test]
fn method_basic() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Point {
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
fn method_with_args() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Point {
                var x: Int32;
                var y: Int32;
                func add(other: Point) -> Point {
                    return Point(x: self.x + other.x, y: self.y + other.y);
                }
            }
            func main() -> Int32 {
                let a = Point(x: 1, y: 2);
                let b = Point(x: 10, y: 20);
                let c = a.add(b);
                return c.x + c.y;
            }
        "#
        ),
        33
    );
}

#[test]
fn method_chaining() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Wrapper {
                var value: Int32;
                func doubled() -> Wrapper {
                    return Wrapper(value: self.value * 2);
                }
                func get() -> Int32 {
                    return self.value;
                }
            }
            func main() -> Int32 {
                let w = Wrapper(value: 5);
                return w.doubled().doubled().get();
            }
        "#
        ),
        20
    );
}

#[test]
fn method_calls_other_method() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Calc {
                var a: Int32;
                var b: Int32;
                func sum() -> Int32 {
                    return self.a + self.b;
                }
                func doubled_sum() -> Int32 {
                    return self.sum() * 2;
                }
            }
            func main() -> Int32 {
                let c = Calc(a: 3, b: 4);
                return c.doubled_sum();
            }
        "#
        ),
        14
    );
}

#[test]
fn method_in_control_flow() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Counter {
                var n: Int32;
                func value() -> Int32 {
                    return self.n;
                }
            }
            func main() -> Int32 {
                let c = Counter(n: 10);
                let result = if c.value() > 5 {
                    yield c.value() + 1;
                } else {
                    yield 0;
                };
                return result;
            }
        "#
        ),
        11
    );
}

#[test]
fn method_unit_return() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Point {
                var x: Int32;
                func noop() {
                    return;
                }
                func get() -> Int32 {
                    return self.x;
                }
            }
            func main() -> Int32 {
                let p = Point(x: 42);
                p.noop();
                return p.get();
            }
        "#
        ),
        42
    );
}

// --- New tests ---

#[test]
fn method_returns_struct() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Pair {
                var a: Int32;
                var b: Int32;
                func swapped() -> Pair {
                    return Pair(a: self.b, b: self.a);
                }
            }
            func main() -> Int32 {
                let p = Pair(a: 10, b: 20);
                return p.swapped().a;
            }
        "#
        ),
        20
    );
}

#[test]
fn method_param_same_type() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Vec2 {
                var x: Int32;
                var y: Int32;
                func dot(other: Vec2) -> Int32 {
                    return self.x * other.x + self.y * other.y;
                }
            }
            func main() -> Int32 {
                let a = Vec2(x: 2, y: 3);
                let b = Vec2(x: 4, y: 5);
                return a.dot(b);
            }
        "#
        ),
        23 // 2*4 + 3*5 = 8 + 15
    );
}

#[test]
fn method_calls_computed_property() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Rect {
                var w: Int32;
                var h: Int32;
                var area: Int32 {
                    get { return self.w * self.h; }
                };
                func double_area() -> Int32 {
                    return self.area * 2;
                }
            }
            func main() -> Int32 {
                let r = Rect(w: 3, h: 4);
                return r.double_area();
            }
        "#
        ),
        24
    );
}

#[test]
fn method_in_while_loop() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Counter {
                var n: Int32;
                func value() -> Int32 {
                    return self.n;
                }
            }
            func main() -> Int32 {
                let c = Counter(n: 5);
                var sum: Int32 = 0;
                var i: Int32 = 0;
                while i < c.value() {
                    sum = sum + i;
                    i = i + 1;
                };
                return sum;
            }
        "#
        ),
        10 // 0+1+2+3+4
    );
}

#[test]
fn method_exhaustive_return() {
    assert_eq!(
        compile_and_run(
            r#"
            struct Chooser {
                var threshold: Int32;
                func classify(x: Int32) -> Int32 {
                    if x > self.threshold { return 1; } else { return 0; };
                }
            }
            func main() -> Int32 {
                let c = Chooser(threshold: 5);
                return c.classify(10);
            }
        "#,
        ),
        1
    );
}

// --- Error cases ---

#[test]
fn err_self_outside_struct() {
    compile_source_should_fail(
        r#"
        func main() -> Int32 { return self.x; }
    "#,
    );
}
