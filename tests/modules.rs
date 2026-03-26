mod common;

use common::{compile_and_run, compile_and_run_package, compile_package_should_fail};

// --- Cross-module function call ---

#[test]
fn cross_module_function_call() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module math;
            import math::add;
            func main() -> Int32 {
                return add(1, 2);
            }
        "#,
        ),
        (
            "math.bengal",
            r#"
            public func add(a: Int32, b: Int32) -> Int32 {
                return a + b;
            }
        "#,
        ),
    ]);
    assert_eq!(result, 3);
}

// --- Visibility ---

#[test]
fn visibility_internal_denied() {
    let err = compile_package_should_fail(&[
        (
            "main.bengal",
            r#"
            module math;
            import math::helper;
            func main() -> Int32 { return 0; }
        "#,
        ),
        (
            "math.bengal",
            r#"
            func helper() -> Int32 { return 1; }
        "#,
        ),
    ]);
    assert!(
        err.contains("cannot")
            || err.contains("not accessible")
            || err.contains("visibility")
            || err.contains("not found"),
        "error was: {}",
        err
    );
}

#[test]
fn package_visibility() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module util;
            import util::helper;
            func main() -> Int32 {
                return helper();
            }
        "#,
        ),
        (
            "util.bengal",
            r#"
            package func helper() -> Int32 { return 99; }
        "#,
        ),
    ]);
    assert_eq!(result, 99);
}

// --- Struct across modules ---

#[test]
fn struct_across_modules() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module shapes;
            import shapes::Point;
            func main() -> Int32 {
                let p = Point(x: 3, y: 4);
                return p.x + p.y;
            }
        "#,
        ),
        (
            "shapes.bengal",
            r#"
            public struct Point {
                public var x: Int32;
                public var y: Int32;
            }
        "#,
        ),
    ]);
    assert_eq!(result, 7);
}

// --- Import forms ---

#[test]
fn glob_import() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module math;
            import math::*;
            func main() -> Int32 {
                return add(10, mul(2, 3));
            }
        "#,
        ),
        (
            "math.bengal",
            r#"
            public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
            public func mul(a: Int32, b: Int32) -> Int32 { return a * b; }
        "#,
        ),
    ]);
    assert_eq!(result, 16);
}

#[test]
fn group_import() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module math;
            import math::{add, mul};
            func main() -> Int32 {
                return add(2, mul(3, 4));
            }
        "#,
        ),
        (
            "math.bengal",
            r#"
            public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
            public func mul(a: Int32, b: Int32) -> Int32 { return a * b; }
        "#,
        ),
    ]);
    assert_eq!(result, 14);
}

// --- Method call across modules ---

#[test]
fn method_call_across_modules() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module shapes;
            import shapes::Circle;
            func main() -> Int32 {
                let c = Circle(radius: 5);
                return c.area();
            }
        "#,
        ),
        (
            "shapes.bengal",
            r#"
            public struct Circle {
                public var radius: Int32;
                public func area() -> Int32 {
                    return self.radius * self.radius;
                }
            }
        "#,
        ),
    ]);
    assert_eq!(result, 25);
}

// --- Multiple modules ---

#[test]
fn three_modules() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module math;
            module util;
            import math::add;
            import util::double;
            func main() -> Int32 {
                return add(double(3), double(4));
            }
        "#,
        ),
        (
            "math.bengal",
            r#"
            public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        "#,
        ),
        (
            "util.bengal",
            r#"
            public func double(x: Int32) -> Int32 { return x * 2; }
        "#,
        ),
    ]);
    assert_eq!(result, 14);
}

// --- Backward compatibility ---

#[test]
fn single_file_backward_compat() {
    // No Bengal.toml - existing compile_and_run should still work
    let result = compile_and_run("func main() -> Int32 { return 42; }");
    assert_eq!(result, 42);
}

// --- Relative imports ---

#[test]
fn self_relative_import() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module lib;
            import lib::get_value;
            func main() -> Int32 {
                return get_value();
            }
        "#,
        ),
        (
            "lib/module.bengal",
            r#"
            module helper;
            import self::helper::compute;
            public func get_value() -> Int32 {
                return compute();
            }
        "#,
        ),
        (
            "lib/helper.bengal",
            r#"
            public func compute() -> Int32 { return 42; }
        "#,
        ),
    ]);
    assert_eq!(result, 42);
}

#[test]
fn super_import() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module common;
            module sub;
            import sub::get_value;
            func main() -> Int32 {
                return get_value();
            }
        "#,
        ),
        (
            "common.bengal",
            r#"
            public func shared() -> Int32 { return 77; }
        "#,
        ),
        (
            "sub.bengal",
            r#"
            import super::common::shared;
            public func get_value() -> Int32 {
                return shared();
            }
        "#,
        ),
    ]);
    assert_eq!(result, 77);
}

#[test]
#[ignore] // public import (re-export) syntax is not yet implemented in the parser
fn re_export_public_import() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module facade;
            import facade::helper;
            func main() -> Int32 {
                return helper();
            }
        "#,
        ),
        (
            "facade/module.bengal",
            r#"
            module internal;
            public import self::internal::helper;
        "#,
        ),
        (
            "facade/internal.bengal",
            r#"
            public func helper() -> Int32 { return 55; }
        "#,
        ),
    ]);
    assert_eq!(result, 55);
}

#[test]
fn hierarchical_modules() {
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module a;
            import a::get_deep;
            func main() -> Int32 {
                return get_deep();
            }
        "#,
        ),
        (
            "a/module.bengal",
            r#"
            module b;
            import self::b::deep_value;
            public func get_deep() -> Int32 {
                return deep_value();
            }
        "#,
        ),
        (
            "a/b.bengal",
            r#"
            public func deep_value() -> Int32 { return 123; }
        "#,
        ),
    ]);
    assert_eq!(result, 123);
}

// --- Cross-module type inference ---

#[test]
fn cross_module_literal_inference() {
    // Type inference for numeric literals works across module boundaries
    let result = compile_and_run_package(&[
        (
            "main.bengal",
            r#"
            module math;
            import math::add_i64;
            func main() -> Int32 {
                add_i64(1, 2);
                return 0;
            }
        "#,
        ),
        (
            "math.bengal",
            r#"
            public func add_i64(a: Int64, b: Int64) -> Int64 {
                return a + b;
            }
        "#,
        ),
    ]);
    assert_eq!(result, 0);
}

// --- Error cases ---

#[test]
fn err_super_at_root() {
    let err = compile_package_should_fail(&[(
        "main.bengal",
        r#"
            import super::something::Foo;
            func main() -> Int32 { return 0; }
        "#,
    )]);
    assert!(
        err.contains("super") || err.contains("root") || err.contains("parent"),
        "error was: {}",
        err
    );
}

#[test]
fn err_import_nonexistent_symbol() {
    let err = compile_package_should_fail(&[
        (
            "main.bengal",
            r#"
            module math;
            import math::nonexistent;
            func main() -> Int32 { return 0; }
        "#,
        ),
        (
            "math.bengal",
            r#"
            public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        "#,
        ),
    ]);
    assert!(
        err.contains("not found")
            || err.contains("does not export")
            || err.contains("cannot")
            || err.contains("no item")
            || err.contains("unresolved"),
        "error was: {}",
        err
    );
}

#[test]
fn err_circular_module() {
    let err = compile_package_should_fail(&[
        (
            "main.bengal",
            r#"
            module a;
            func main() -> Int32 { return 0; }
        "#,
        ),
        (
            "a.bengal",
            r#"
            module b;
        "#,
        ),
        (
            "b.bengal",
            r#"
            module a;
        "#,
        ),
    ]);
    assert!(
        err.contains("circular") || err.contains("cycle"),
        "error was: {}",
        err
    );
}
