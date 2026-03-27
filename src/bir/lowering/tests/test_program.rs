use std::collections::HashMap;

use crate::bir::lowering::lower_program;
use crate::bir::printer::print_module;
use crate::lexer::tokenize;
use crate::parser::parse;
use crate::semantic;

use crate::bir::lowering::lower_module;

#[test]
fn lower_err_recursive_struct() {
    let tokens =
        tokenize("struct Node { var next: Node; } func main() -> Int32 { return 0; }").unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let result = lower_program(&program, &sem_info);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("recursive struct"));
}

#[test]
fn lower_err_recursive_struct_has_span() {
    let source = "struct A { var b: B; } struct B { var a: A; } func main() -> Int32 { return 0; }";
    let tokens = crate::lexer::tokenize(source).unwrap();
    let program = crate::parser::parse(tokens).unwrap();
    let sem_info = crate::semantic::analyze_post_mono(&program).unwrap();
    let result = lower_program(&program, &sem_info);
    match result {
        Err(crate::error::BengalError::LoweringError { span, .. }) => {
            assert!(span.is_some(), "recursive struct error should include span");
        }
        other => panic!("expected LoweringError, got {:?}", other),
    }
}

#[test]
fn lower_module_mangles_function_names() {
    let input = "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }";
    let tokens = tokenize(input).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();

    // Build name_map: main stays as "main", add gets mangled
    let mut name_map = HashMap::new();
    name_map.insert("main".to_string(), "main".to_string());
    name_map.insert(
        "add".to_string(),
        crate::mangle::mangle_function("my_app", &[""], "add", &[]),
    );

    let module = lower_module(&program, &sem_info, &name_map).unwrap();
    let output = print_module(&module);

    // "main" function name should NOT be mangled
    assert!(output.contains("@main("));
    // "add" function name should be mangled
    let mangled_add = crate::mangle::mangle_function("my_app", &[""], "add", &[]);
    assert!(
        output.contains(&format!("@{}(", mangled_add)),
        "expected mangled add function, got:\n{}",
        output
    );
    // Call to add should also use the mangled name
    assert!(
        output.contains(&format!("call @{}", mangled_add)),
        "expected mangled call target, got:\n{}",
        output
    );
}

#[test]
fn lower_module_mangles_method_names() {
    let input = "struct Point { var x: Int32; func get_x() -> Int32 { return self.x; } } func main() -> Int32 { var p = Point(x: 42); return p.get_x(); }";
    let tokens = tokenize(input).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();

    let mut name_map = HashMap::new();
    name_map.insert("main".to_string(), "main".to_string());
    name_map.insert(
        "Point_get_x".to_string(),
        crate::mangle::mangle_method("my_app", &[""], "Point", "get_x", &[]),
    );

    let module = lower_module(&program, &sem_info, &name_map).unwrap();
    let output = print_module(&module);

    let mangled_method = crate::mangle::mangle_method("my_app", &[""], "Point", "get_x", &[]);
    // The method function should have the mangled name
    assert!(
        output.contains(&format!("@{}", mangled_method)),
        "expected mangled method name, got:\n{}",
        output
    );
    // The call to the method should also use the mangled name
    assert!(
        output.contains(&format!("call @{}", mangled_method)),
        "expected mangled method call, got:\n{}",
        output
    );
}

#[test]
fn lower_err_read_before_init() {
    let tokens = tokenize(
        "struct Foo { var x: Int32; init(val: Int32) { let y: Int32 = self.x; self.x = val; } } func main() -> Int32 { var f = Foo(val: 1); return f.x; }",
    ).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let result = lower_program(&program, &sem_info);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("read-before-init"));
}

#[test]
fn lower_err_bare_self_in_init() {
    let tokens = tokenize(
        "struct Foo { var x: Int32; init(val: Int32) { self.x = val; let s = self; } } func main() -> Int32 { var f = Foo(val: 1); return f.x; }",
    ).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let result = lower_program(&program, &sem_info);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("bare `self`"));
}

#[test]
fn lower_err_computed_on_self_in_init() {
    let tokens = tokenize(
        "struct Foo { var x: Int32; var double: Int32 { get { return self.x; } }; init(val: Int32) { self.x = val; let d: Int32 = self.double; } } func main() -> Int32 { var f = Foo(val: 1); return f.x; }",
    ).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let result = lower_program(&program, &sem_info);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("computed property")
    );
}

#[test]
fn lower_err_nested_self_field_assign_in_init() {
    let tokens = tokenize(
        "struct Inner { var x: Int32; } struct Outer { var inner: Inner; init() { self.inner = Inner(x: 0); self.inner.x = 10; } } func main() -> Int32 { var o = Outer(); return o.inner.x; }",
    ).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let result = lower_program(&program, &sem_info);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("nested field assignment")
    );
}
