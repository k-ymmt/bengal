use super::*;
use crate::bir;
use crate::lexer::tokenize;
use crate::parser::parse;
use crate::semantic;

fn compile_and_run(source: &str) -> i32 {
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
    bir::optimize_module(&mut bir_module);

    let context = Context::create();
    let module = compile_to_module(&context, &bir_module).unwrap();

    let engine = module
        .create_jit_execution_engine(OptimizationLevel::None)
        .unwrap();

    unsafe {
        let main_fn = engine
            .get_function::<unsafe extern "C" fn() -> i32>("main")
            .unwrap();
        main_fn.call()
    }
}

#[test]
fn test_literal_return() {
    assert_eq!(compile_and_run("func main() -> Int32 { return 42; }"), 42);
}

#[test]
fn test_arithmetic() {
    assert_eq!(compile_and_run("func main() -> Int32 { return 2 + 3; }"), 5);
}

#[test]
fn test_call() {
    assert_eq!(
        compile_and_run(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }"
        ),
        7
    );
}

#[test]
fn test_let_variable() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = 10; return x + 1; }"),
        11
    );
}

#[test]
fn test_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }"
        ),
        1
    );
}

#[test]
fn test_divergent_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false { return 99; } else { yield 42; }; return x; }"
        ),
        42
    );
}

#[test]
fn test_while() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var s: Int32 = 0; var i: Int32 = 0; while i < 3 { s = s + i; i = i + 1; }; return s; }"
        ),
        3
    );
}

#[test]
fn test_break() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while true { if i == 3 { break; }; i = i + 1; }; return i; }"
        ),
        3
    );
}

#[test]
fn test_continue() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }"
        ),
        12
    );
}

#[test]
fn test_break_value_mutable_var() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { i = i + 1; if i == 5 { break i * 10; }; } nobreak { yield 0; }; return x + i; }"
        ),
        55
    );
}

#[test]
fn test_nobreak_condition_false() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }"
        ),
        3
    );
}

#[test]
fn test_cast() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int64 = 100 as Int64; return x as Int32; }"),
        100
    );
}

#[test]
fn test_unit_call() {
    assert_eq!(
        compile_and_run("func noop() { return; } func main() -> Int32 { noop(); return 42; }"),
        42
    );
}

#[test]
fn test_float() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Float64 = 3.5; let y: Float64 = 1.5; return (x + y) as Int32; }"
        ),
        5
    );
}

#[test]
fn test_comparison() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 3 > 2 { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn test_i64_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int64 = 10 as Int64; let y: Int64 = 20 as Int64; return (x + y) as Int32; }"
        ),
        30
    );
}

#[test]
fn test_object_emit() {
    let source = "func main() -> Int32 { return 42; }";
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
    bir::optimize_module(&mut bir_module);

    let obj_bytes = compile(&bir_module).unwrap();
    assert!(!obj_bytes.is_empty(), "object output must not be empty");
}

// --- Phase 3: Struct codegen tests ---

#[test]
fn test_struct_basic() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x + p.y; }"
        ),
        7
    );
}

#[test]
fn test_struct_field_assign() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1, y: 2); p.x = 10; return p.x; }"
        ),
        10
    );
}

#[test]
fn test_struct_as_function_arg() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func get_x(p: Point) -> Int32 { return p.x; } func main() -> Int32 { let p = Point(x: 42, y: 0); return get_x(p); }"
        ),
        42
    );
}

#[test]
fn test_struct_as_return_value() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func make_point() -> Point { return Point(x: 5, y: 6); } func main() -> Int32 { let p = make_point(); return p.x + p.y; }"
        ),
        11
    );
}

#[test]
fn test_struct_in_if() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = if true { yield Point(x: 1, y: 2); } else { yield Point(x: 3, y: 4); }; return p.x; }"
        ),
        1
    );
}

#[test]
fn test_struct_computed_property() {
    assert_eq!(
        compile_and_run(
            "struct Rect { var w: Int32; var h: Int32; var area: Int32 { get { return self.w * self.h; } }; } func main() -> Int32 { let r = Rect(w: 3, h: 4); return r.area; }"
        ),
        12
    );
}

#[test]
fn test_struct_explicit_init() {
    assert_eq!(
        compile_and_run(
            "struct Counter { var count: Int32; init(start: Int32) { self.count = start * 2; } } func main() -> Int32 { let c = Counter(start: 5); return c.count; }"
        ),
        10
    );
}

#[test]
fn test_struct_nested_field_assign() {
    assert_eq!(
        compile_and_run(
            "struct Inner { var x: Int32; } struct Outer { var inner: Inner; } func main() -> Int32 { var o = Outer(inner: Inner(x: 1)); o.inner.x = 10; return o.inner.x; }"
        ),
        10
    );
}

#[test]
fn test_struct_param_no_local_init() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func sum(p: Point) -> Int32 { return p.x + p.y; } func main() -> Int32 { return sum(Point(x: 10, y: 20)); }"
        ),
        30
    );
}

#[test]
fn test_struct_mutable_in_loop() {
    assert_eq!(
        compile_and_run(
            "struct Acc { var val: Int32; } func main() -> Int32 { var a = Acc(val: 0); var i: Int32 = 0; while i < 5 { a.val = a.val + i; i = i + 1; }; return a.val; }"
        ),
        10
    );
}

#[test]
fn test_struct_valued_while_break() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var i: Int32 = 0; let p = while i < 10 { i = i + 1; if i == 3 { break Point(x: i, y: i * 2); }; } nobreak { yield Point(x: 0, y: 0); }; return p.x + p.y; }"
        ),
        9
    );
}

#[test]
fn test_struct_computed_setter() {
    assert_eq!(
        compile_and_run(
            "struct Foo { var x: Int32; var bar: Int32 { get { return self.x; } set { self.x = newValue * 2; } }; } func main() -> Int32 { var f = Foo(x: 1); f.bar = 5; return f.x; }"
        ),
        10
    );
}

#[test]
fn test_struct_single_field() {
    assert_eq!(
        compile_and_run(
            "struct Wrapper { var val: Int32; } func main() -> Int32 { let w = Wrapper(val: 99); return w.val; }"
        ),
        99
    );
}

#[test]
fn test_struct_pass_through_calls() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func identity(p: Point) -> Point { return p; } func add_one(p: Point) -> Point { return Point(x: p.x + 1, y: p.y + 1); } func main() -> Int32 { let p = Point(x: 1, y: 2); let q = add_one(identity(p)); return q.x + q.y; }"
        ),
        5
    );
}

#[test]
fn test_struct_object_emit() {
    let source = "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x + p.y; }";
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze_post_mono(&program).unwrap();
    let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
    bir::optimize_module(&mut bir_module);

    let obj_bytes = compile(&bir_module).unwrap();
    assert!(!obj_bytes.is_empty(), "object output must not be empty");
}

#[test]
fn test_struct_init_field_access() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { return Point(x: 1, y: 2).x; }"
        ),
        1
    );
}

#[test]
fn test_struct_empty() {
    assert_eq!(
        compile_and_run("struct Empty {} func main() -> Int32 { let e = Empty(); return 0; }"),
        0
    );
}

#[test]
fn test_struct_continue_in_loop() {
    assert_eq!(
        compile_and_run(
            "struct Acc { var val: Int32; } func main() -> Int32 { var a = Acc(val: 0); var i: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; a.val = a.val + 1; }; return a.val; }"
        ),
        4
    );
}

#[test]
fn test_struct_nobreak_yield() {
    assert_eq!(
        compile_and_run(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = while false { break Point(x: 0, y: 0); } nobreak { yield Point(x: 7, y: 8); }; return p.x + p.y; }"
        ),
        15
    );
}
