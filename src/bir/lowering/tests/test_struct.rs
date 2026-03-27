use super::lower_str;

#[test]
fn lower_struct_init_basic() {
    let output = lower_str(
        "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 1, y: 2); return p.x; }",
    );
    assert!(output.contains("struct_init @Point"));
    assert!(output.contains("field_get"));
}

#[test]
fn lower_struct_field_get() {
    let output = lower_str(
        "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1, y: 2); return p.x; }",
    );
    assert!(output.contains("struct_init @Point"));
    assert!(output.contains(r#"field_get"#));
    assert!(output.contains(r#""x""#));
}

#[test]
fn lower_struct_field_set() {
    let output = lower_str(
        "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); p.x = 10; return p.x; }",
    );
    assert!(output.contains("struct_init @Point"));
    assert!(output.contains("field_set"));
    assert!(output.contains("field_get"));
}

#[test]
fn lower_struct_as_function_arg() {
    let output = lower_str(
        "struct Point { var x: Int32; } func get_x(p: Point) -> Int32 { return p.x; } func main() -> Int32 { return get_x(Point(x: 42)); }",
    );
    assert!(output.contains("@get_x(%0: Point)"));
    assert!(output.contains("field_get"));
}

#[test]
fn lower_struct_as_return_value() {
    let output = lower_str(
        "struct Point { var x: Int32; } func make() -> Point { return Point(x: 5); } func main() -> Int32 { let p = make(); return p.x; }",
    );
    assert!(output.contains("@make() -> Point"));
    assert!(output.contains("struct_init @Point"));
}

#[test]
fn lower_struct_in_if_expr() {
    let output = lower_str(
        "struct Point { var x: Int32; } func main() -> Int32 { let p = if true { yield Point(x: 1); } else { yield Point(x: 2); }; return p.x; }",
    );
    assert!(output.contains("struct_init @Point"));
    assert!(output.contains("field_get"));
}

#[test]
fn lower_struct_computed_property() {
    let output = lower_str(
        "struct Foo { var x: Int32; var double: Int32 { get { return self.x; } }; } func main() -> Int32 { var f = Foo(x: 5); return f.double; }",
    );
    assert!(output.contains("struct_init @Foo"));
    // Getter is inlined -- field_get on self.x
    assert!(output.contains("field_get"));
}

#[test]
fn lower_struct_explicit_init() {
    let output = lower_str(
        "struct Foo { var x: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { var f = Foo(val: 42); return f.x; }",
    );
    assert!(output.contains("struct_init @Foo"));
    assert!(output.contains("literal 42 : Int32"));
}

#[test]
fn lower_struct_nested_field_assign() {
    let output = lower_str(
        "struct Inner { var x: Int32; } struct Outer { var inner: Inner; } func main() -> Int32 { var o = Outer(inner: Inner(x: 1)); o.inner.x = 10; return o.inner.x; }",
    );
    assert!(output.contains("field_get"));
    assert!(output.contains("field_set"));
}

#[test]
fn lower_struct_mutable_in_loop() {
    let output = lower_str(
        "struct Acc { var val: Int32; } func main() -> Int32 { var a = Acc(val: 0); var i: Int32 = 0; while i < 3 { a.val = a.val + 1; i = i + 1; }; return a.val; }",
    );
    assert!(output.contains("struct_init @Acc"));
    assert!(output.contains("field_get"));
    assert!(output.contains("field_set"));
}

#[test]
fn lower_struct_computed_setter() {
    let output = lower_str(
        "struct Foo { var x: Int32; var bar: Int32 { get { return 0; } set { self.x = newValue; } }; } func main() -> Int32 { var f = Foo(x: 1); f.bar = 10; return f.x; }",
    );
    // Setter is inlined -- field_set on self.x via setter body
    assert!(output.contains("field_set"));
}

#[test]
fn lower_struct_init_field_access() {
    // Point(x: 1).x should now work (struct in expression position)
    let output =
        lower_str("struct Point { var x: Int32; } func main() -> Int32 { return Point(x: 1).x; }");
    assert!(output.contains("struct_init @Point"));
    assert!(output.contains("field_get"));
}
