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

// --- Phase 3: control flow, bool, comparisons ---

#[test]
fn if_else_true() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if true { yield 1; } else { yield 2; }; return x; }"),
        1
    );
}

#[test]
fn if_else_false() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if false { yield 1; } else { yield 2; }; return x; }"),
        2
    );
}

#[test]
fn if_else_comparison() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if 3 > 2 { yield 10; } else { yield 20; }; return x; }"),
        10
    );
}

#[test]
fn if_no_else() {
    assert_eq!(
        compile_and_run("func main() -> i32 { if true { }; return 42; }"),
        42
    );
}

#[test]
fn while_sum() {
    assert_eq!(
        compile_and_run("func main() -> i32 { var i: i32 = 0; var s: i32 = 0; while i < 10 { s = s + i; i = i + 1; }; return s; }"),
        45
    );
}

#[test]
fn while_factorial() {
    assert_eq!(
        compile_and_run("func main() -> i32 { var n: i32 = 5; var r: i32 = 1; while n > 0 { r = r * n; n = n - 1; }; return r; }"),
        120
    );
}

#[test]
fn comparison_eq() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if 1 == 1 { yield 1; } else { yield 0; }; return x; }"),
        1
    );
}

#[test]
fn comparison_ne() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if 1 != 2 { yield 1; } else { yield 0; }; return x; }"),
        1
    );
}

#[test]
fn comparison_le() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if 3 <= 3 { yield 1; } else { yield 0; }; return x; }"),
        1
    );
}

#[test]
fn comparison_ge() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if 3 >= 4 { yield 1; } else { yield 0; }; return x; }"),
        0
    );
}

#[test]
fn logical_and() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if true && true { yield 1; } else { yield 0; }; return x; }"),
        1
    );
}

#[test]
fn logical_and_short() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if false && true { yield 1; } else { yield 0; }; return x; }"),
        0
    );
}

#[test]
fn logical_or() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if false || true { yield 1; } else { yield 0; }; return x; }"),
        1
    );
}

#[test]
fn logical_not() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if !false { yield 1; } else { yield 0; }; return x; }"),
        1
    );
}

#[test]
fn early_return() {
    assert_eq!(
        compile_and_run("func abs(x: i32) -> i32 { if x < 0 { return 0 - x; }; return x; } func main() -> i32 { return abs(0 - 5); }"),
        5
    );
}

#[test]
fn diverging_then() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if false { return 99; } else { yield 42; }; return x; }"),
        42
    );
}

#[test]
fn diverging_else() {
    assert_eq!(
        compile_and_run("func main() -> i32 { let x: i32 = if true { yield 42; } else { return 99; }; return x; }"),
        42
    );
}

#[test]
fn nested_if() {
    assert_eq!(
        compile_and_run("func clamp(x: i32, lo: i32, hi: i32) -> i32 { if x < lo { return lo; }; if x > hi { return hi; }; return x; } func main() -> i32 { return clamp(50, 0, 10); }"),
        10
    );
}

#[test]
fn unit_func() {
    assert_eq!(
        compile_and_run("func noop() { return; } func main() -> i32 { noop(); return 42; }"),
        42
    );
}

// --- Phase 3: error cases ---

#[test]
fn err_if_non_bool_cond() {
    assert!(bengal::compile_source("func main() -> i32 { if 1 { yield 1; } else { yield 2; }; return 0; }").is_err());
}

#[test]
fn err_if_branch_mismatch() {
    assert!(bengal::compile_source("func main() -> i32 { if true { yield 1; } else { yield true; }; return 0; }").is_err());
}

#[test]
fn err_while_non_bool_cond() {
    assert!(bengal::compile_source("func main() -> i32 { while 1 { }; return 0; }").is_err());
}

#[test]
fn err_yield_in_while() {
    assert!(bengal::compile_source("func main() -> i32 { while true { yield 1; }; return 0; }").is_err());
}

// --- Phase 4: break / continue ---

#[test]
fn while_break() {
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; while true { if i == 3 { break; }; i = i + 1; }; return i; }"), 3);
}

#[test]
fn while_continue() {
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; var s: i32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }"), 12);
}

#[test]
fn nested_break() {
    assert_eq!(compile_and_run("func main() -> i32 { var outer: i32 = 0; var i: i32 = 0; while i < 3 { var j: i32 = 0; while true { if j == 2 { break; }; j = j + 1; }; outer = outer + j; i = i + 1; }; return outer; }"), 6);
}

#[test]
fn break_with_var_update() {
    assert_eq!(compile_and_run("func main() -> i32 { var x: i32 = 0; while true { x = x + 10; break; }; return x; }"), 10);
}

#[test]
fn continue_skip_even() {
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; var s: i32 = 0; while i < 6 { i = i + 1; if (i / 2) * 2 == i { continue; }; s = s + i; }; return s; }"), 9);
}

#[test]
fn break_diverge_in_if_else() {
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; while i < 10 { let x: i32 = if i == 5 { break; } else { yield i; }; i = x + 1; }; return i; }"), 5);
}

#[test]
fn continue_diverge_in_if_else() {
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; var s: i32 = 0; while i < 5 { i = i + 1; let v: i32 = if i == 3 { continue; } else { yield i; }; s = s + v; }; return s; }"), 12);
}

#[test]
fn break_with_value() {
    assert_eq!(compile_and_run("func main() -> i32 { let x: i32 = while true { break 42; }; return x; }"), 42);
}

#[test]
fn break_with_value_computed() {
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; let x: i32 = while true { i = i + 1; if i == 5 { break i * 10; }; }; return x; }"), 50);
}

#[test]
fn break_with_value_nested_if() {
    assert_eq!(compile_and_run("func main() -> i32 { let x: i32 = while true { if true { break 1; } else { break 2; }; }; return x; }"), 1);
}

#[test]
fn nobreak_basic() {
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; let x: i32 = while i < 5 { if i == 3 { break 99; }; i = i + 1; } nobreak { yield 0; }; return x; }"), 99);
}

#[test]
fn nobreak_condition_false() {
    // while body has no break → while_ty is Unit, nobreak must also be Unit
    // Use mutable var to observe the value after loop
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }"), 3);
}

#[test]
fn nobreak_no_break_in_body() {
    // while body has no break → while_ty is Unit, nobreak must also be Unit
    assert_eq!(compile_and_run("func main() -> i32 { var i: i32 = 0; while i < 5 { i = i + 1; } nobreak { }; return i * 10; }"), 50);
}

// --- Phase 4: multi-numeric types ---

#[test]
fn i64_arithmetic() {
    assert_eq!(compile_and_run("func main() -> i32 { let x: i64 = 100 as i64; let y: i64 = 200 as i64; return (x + y) as i32; }"), 300);
}

#[test]
fn i64_comparison() {
    assert_eq!(compile_and_run("func main() -> i32 { let x: i64 = 10 as i64; let y: i64 = 20 as i64; let r: i32 = if x < y { yield 1; } else { yield 0; }; return r; }"), 1);
}

#[test]
fn f64_arithmetic() {
    assert_eq!(compile_and_run("func main() -> i32 { let x: f64 = 3.5; let y: f64 = 1.5; return (x + y) as i32; }"), 5);
}

#[test]
fn mixed_cast_chain() {
    assert_eq!(compile_and_run("func main() -> i32 { let x: i32 = 42; let y: i64 = x as i64; let z: i32 = y as i32; return z; }"), 42);
}

// --- Phase 4: local type inference ---

#[test]
fn infer_i32() {
    assert_eq!(compile_and_run("func main() -> i32 { let x = 10; return x; }"), 10);
}

#[test]
fn infer_i32_expr() {
    assert_eq!(compile_and_run("func main() -> i32 { let x = 1 + 2 * 3; return x; }"), 7);
}

#[test]
fn infer_bool() {
    assert_eq!(compile_and_run("func main() -> i32 { let b = true; let r: i32 = if b { yield 1; } else { yield 0; }; return r; }"), 1);
}

#[test]
fn infer_var() {
    assert_eq!(compile_and_run("func main() -> i32 { var x = 0; x = x + 1; return x; }"), 1);
}

// --- Phase 4: cast ---

#[test]
fn cast_i32_to_i64() {
    assert_eq!(compile_and_run("func main() -> i32 { let x: i64 = 42 as i64; return x as i32; }"), 42);
}

#[test]
fn cast_noop() {
    assert_eq!(compile_and_run("func main() -> i32 { let x: i32 = 42 as i32; return x; }"), 42);
}

// --- Phase 4: error cases ---

#[test]
fn err_break_outside_loop() {
    assert!(bengal::compile_source("func main() -> i32 { break; return 0; }").is_err());
}

#[test]
fn err_continue_outside_loop() {
    assert!(bengal::compile_source("func main() -> i32 { continue; return 0; }").is_err());
}

#[test]
fn err_cast_bool() {
    assert!(bengal::compile_source("func main() -> i32 { let x = true as i32; return x; }").is_err());
}

#[test]
fn err_mixed_arithmetic() {
    assert!(bengal::compile_source("func main() -> i32 { let x: i32 = 1; let y: i64 = 2 as i64; return x + y; }").is_err());
}

#[test]
fn err_infer_mismatch() {
    assert!(bengal::compile_source("func main() -> i32 { let x: i32 = 3.14; return 0; }").is_err());
}

#[test]
fn err_integer_overflow() {
    assert!(bengal::compile_source("func main() -> i32 { let x = 3000000000; return 0; }").is_err());
}

#[test]
fn err_break_value_no_nobreak() {
    assert!(bengal::compile_source("func main() -> i32 { var i: i32 = 0; let x: i32 = while i < 10 { break 1; }; return x; }").is_err());
}

#[test]
fn err_break_value_type_mismatch() {
    assert!(bengal::compile_source("func main() -> i32 { let x: i32 = while true { break true; }; return x; }").is_err());
}

#[test]
fn err_nobreak_in_while_true() {
    assert!(bengal::compile_source("func main() -> i32 { let x: i32 = while true { break 10; } nobreak { yield 20; }; return x; }").is_err());
}

#[test]
fn err_nobreak_type_mismatch() {
    assert!(bengal::compile_source("func main() -> i32 { var i: i32 = 0; let x: i32 = while i < 10 { break 1; } nobreak { yield true; }; return x; }").is_err());
}
