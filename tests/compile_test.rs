fn compile_and_run(source: &str) -> i32 {
    let wasm_bytes = bengal::compile_source(source).unwrap();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm_bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let main = instance
        .get_typed_func::<(), i32>(&mut store, "main")
        .unwrap();
    main.call(&mut store, ()).unwrap()
}

#[test]
fn literal() {
    assert_eq!(compile_and_run("42"), 42);
}

#[test]
fn addition() {
    assert_eq!(compile_and_run("2 + 3"), 5);
}

#[test]
fn subtraction() {
    assert_eq!(compile_and_run("10 - 4"), 6);
}

#[test]
fn multiplication() {
    assert_eq!(compile_and_run("3 * 7"), 21);
}

#[test]
fn division() {
    assert_eq!(compile_and_run("20 / 4"), 5);
}

#[test]
fn precedence() {
    assert_eq!(compile_and_run("2 + 3 * 4"), 14);
}

#[test]
fn parentheses() {
    assert_eq!(compile_and_run("(2 + 3) * 4"), 20);
}

#[test]
fn nested_parentheses() {
    assert_eq!(compile_and_run("((1 + 2) * (3 + 4))"), 21);
}

#[test]
fn left_assoc_division() {
    assert_eq!(compile_and_run("100 / 10 / 2"), 5);
}

#[test]
fn left_assoc_subtraction() {
    assert_eq!(compile_and_run("1 - 2 - 3"), -4);
}

// --- Phase 2: functions, variables, blocks ---

#[test]
fn fn_simple() {
    assert_eq!(compile_and_run("func main() -> i32 { return 42; }"), 42);
}

#[test]
fn fn_with_let() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = 10; return x; }"),
        10
    );
}

#[test]
fn fn_with_var() {
    assert_eq!(
        compile_and_run("func main() -> i32 { var x: i32 = 1; x = 10; return x; }"),
        10
    );
}

#[test]
fn fn_let_arithmetic() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let a: i32 = 2; let b: i32 = 3; return a + b * 4; }"),
        14
    );
}

#[test]
fn fn_call() {
    assert_eq!(
        compile_and_run("func add(a: i32, b: i32) -> i32 { return a + b; }\nfunc main() -> i32 { return add(3, 4); }"),
        7
    );
}

#[test]
fn fn_call_chain() {
    assert_eq!(
        compile_and_run("func double(x: i32) -> i32 { return x * 2; }\nfunc main() -> i32 { return double(double(5)); }"),
        20
    );
}

#[test]
fn fn_shadowing() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = 1; let x: i32 = x + 10; return x; }"),
        11
    );
}

#[test]
fn fn_var_update() {
    assert_eq!(
        compile_and_run("func main() -> i32 { var x: i32 = 0; x = x + 1; x = x + 2; return x; }"),
        3
    );
}

#[test]
fn fn_block_expr() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = { yield 7; }; return x; }"),
        7
    );
}

#[test]
fn fn_block_shadow() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = 1; let y: i32 = { let x: i32 = 10; yield x + 1; }; return x + y; }"),
        12
    );
}

#[test]
fn fn_block_var_assign() {
    assert_eq!(
        compile_and_run("func main() -> i32 { var x: i32 = 0; x = { x = 10; yield x + 1; }; return x; }"),
        11
    );
}

#[test]
fn fn_multiple_funcs() {
    assert_eq!(
        compile_and_run("func square(x: i32) -> i32 { return x * x; }\nfunc main() -> i32 { return square(3) + square(4); }"),
        25
    );
}

// --- Phase 2: error cases ---

#[test]
fn err_no_main() {
    assert!(bengal::compile_source("func add(a: i32, b: i32) -> i32 { return a + b; }").is_err());
}

#[test]
fn err_main_with_params() {
    assert!(bengal::compile_source("func main(x: i32) -> i32 { return x; }").is_err());
}

#[test]
fn err_undefined_var() {
    assert!(bengal::compile_source("func main() -> i32 { return x; }").is_err());
}

#[test]
fn err_immutable_assign() {
    assert!(bengal::compile_source("func main() -> i32 { let x: i32 = 1; x = 2; return x; }").is_err());
}

#[test]
fn err_no_return() {
    assert!(bengal::compile_source("func main() -> i32 { let x: i32 = 1; }").is_err());
}

#[test]
fn err_no_yield() {
    assert!(bengal::compile_source("func main() -> i32 { let x: i32 = { let a: i32 = 1; }; return x; }").is_err());
}

#[test]
fn err_yield_in_func() {
    assert!(bengal::compile_source("func main() -> i32 { yield 1; }").is_err());
}

#[test]
fn err_return_in_block() {
    assert!(bengal::compile_source("func main() -> i32 { let x: i32 = { return 1; }; return x; }").is_err());
}
