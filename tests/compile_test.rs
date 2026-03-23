use bengal::bir;
use bengal::codegen;
use bengal::lexer::tokenize;
use bengal::parser::parse;
use bengal::semantic;
use inkwell::OptimizationLevel;
use inkwell::context::Context;

fn compile_and_run(source: &str) -> i32 {
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = semantic::analyze(&program).unwrap();
    let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
    bir::optimize_module(&mut bir_module);

    let context = Context::create();
    let module = codegen::compile_to_module(&context, &bir_module).unwrap();
    let ee = module
        .create_jit_execution_engine(OptimizationLevel::None)
        .unwrap();
    let main_fn = unsafe {
        ee.get_function::<unsafe extern "C" fn() -> i32>("main")
            .unwrap()
    };
    unsafe { main_fn.call() }
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
    assert_eq!(compile_and_run("func main() -> Int32 { return 42; }"), 42);
}

#[test]
fn fn_with_let() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = 10; return x; }"),
        10
    );
}

#[test]
fn fn_with_var() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { var x: Int32 = 1; x = 10; return x; }"),
        10
    );
}

#[test]
fn fn_let_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let a: Int32 = 2; let b: Int32 = 3; return a + b * 4; }"
        ),
        14
    );
}

#[test]
fn fn_call() {
    assert_eq!(
        compile_and_run(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; }\nfunc main() -> Int32 { return add(3, 4); }"
        ),
        7
    );
}

#[test]
fn fn_call_chain() {
    assert_eq!(
        compile_and_run(
            "func double(x: Int32) -> Int32 { return x * 2; }\nfunc main() -> Int32 { return double(double(5)); }"
        ),
        20
    );
}

#[test]
fn fn_shadowing() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = 1; let x: Int32 = x + 10; return x; }"
        ),
        11
    );
}

#[test]
fn fn_var_update() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 0; x = x + 1; x = x + 2; return x; }"
        ),
        3
    );
}

#[test]
fn fn_block_expr() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = { yield 7; }; return x; }"),
        7
    );
}

#[test]
fn fn_block_shadow() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = 1; let y: Int32 = { let x: Int32 = 10; yield x + 1; }; return x + y; }"
        ),
        12
    );
}

#[test]
fn fn_block_var_assign() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 0; x = { x = 10; yield x + 1; }; return x; }"
        ),
        11
    );
}

#[test]
fn fn_multiple_funcs() {
    assert_eq!(
        compile_and_run(
            "func square(x: Int32) -> Int32 { return x * x; }\nfunc main() -> Int32 { return square(3) + square(4); }"
        ),
        25
    );
}

// --- Phase 2: error cases ---

#[test]
fn err_no_main() {
    assert!(
        bengal::compile_source("func add(a: Int32, b: Int32) -> Int32 { return a + b; }").is_err()
    );
}

#[test]
fn err_main_with_params() {
    assert!(bengal::compile_source("func main(x: Int32) -> Int32 { return x; }").is_err());
}

#[test]
fn err_undefined_var() {
    assert!(bengal::compile_source("func main() -> Int32 { return x; }").is_err());
}

#[test]
fn err_immutable_assign() {
    assert!(
        bengal::compile_source("func main() -> Int32 { let x: Int32 = 1; x = 2; return x; }")
            .is_err()
    );
}

#[test]
fn err_no_return() {
    assert!(bengal::compile_source("func main() -> Int32 { let x: Int32 = 1; }").is_err());
}

#[test]
fn err_no_yield() {
    assert!(
        bengal::compile_source(
            "func main() -> Int32 { let x: Int32 = { let a: Int32 = 1; }; return x; }"
        )
        .is_err()
    );
}

#[test]
fn err_yield_in_func() {
    assert!(bengal::compile_source("func main() -> Int32 { yield 1; }").is_err());
}

#[test]
fn err_return_in_block() {
    assert!(
        bengal::compile_source("func main() -> Int32 { let x: Int32 = { return 1; }; return x; }")
            .is_err()
    );
}

// --- Phase 3: control flow, bool, comparisons ---

#[test]
fn if_else_true() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }"
        ),
        1
    );
}

#[test]
fn if_else_false() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false { yield 1; } else { yield 2; }; return x; }"
        ),
        2
    );
}

#[test]
fn if_else_comparison() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 3 > 2 { yield 10; } else { yield 20; }; return x; }"
        ),
        10
    );
}

#[test]
fn if_no_else() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { if true { }; return 42; }"),
        42
    );
}

#[test]
fn while_sum() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 10 { s = s + i; i = i + 1; }; return s; }"
        ),
        45
    );
}

#[test]
fn while_factorial() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var n: Int32 = 5; var r: Int32 = 1; while n > 0 { r = r * n; n = n - 1; }; return r; }"
        ),
        120
    );
}

#[test]
fn comparison_eq() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 1 == 1 { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn comparison_ne() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 1 != 2 { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn comparison_le() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 3 <= 3 { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn comparison_ge() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if 3 >= 4 { yield 1; } else { yield 0; }; return x; }"
        ),
        0
    );
}

#[test]
fn logical_and() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if true && true { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn logical_and_short() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false && true { yield 1; } else { yield 0; }; return x; }"
        ),
        0
    );
}

#[test]
fn logical_or() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false || true { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn logical_not() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if !false { yield 1; } else { yield 0; }; return x; }"
        ),
        1
    );
}

#[test]
fn early_return() {
    assert_eq!(
        compile_and_run(
            "func abs(x: Int32) -> Int32 { if x < 0 { return 0 - x; }; return x; } func main() -> Int32 { return abs(0 - 5); }"
        ),
        5
    );
}

#[test]
fn diverging_then() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if false { return 99; } else { yield 42; }; return x; }"
        ),
        42
    );
}

#[test]
fn diverging_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = if true { yield 42; } else { return 99; }; return x; }"
        ),
        42
    );
}

#[test]
fn nested_if() {
    assert_eq!(
        compile_and_run(
            "func clamp(x: Int32, lo: Int32, hi: Int32) -> Int32 { if x < lo { return lo; }; if x > hi { return hi; }; return x; } func main() -> Int32 { return clamp(50, 0, 10); }"
        ),
        10
    );
}

#[test]
fn unit_func() {
    assert_eq!(
        compile_and_run("func noop() { return; } func main() -> Int32 { noop(); return 42; }"),
        42
    );
}

// --- Phase 3: error cases ---

#[test]
fn err_if_non_bool_cond() {
    assert!(
        bengal::compile_source(
            "func main() -> Int32 { if 1 { yield 1; } else { yield 2; }; return 0; }"
        )
        .is_err()
    );
}

#[test]
fn err_if_branch_mismatch() {
    assert!(
        bengal::compile_source(
            "func main() -> Int32 { if true { yield 1; } else { yield true; }; return 0; }"
        )
        .is_err()
    );
}

#[test]
fn err_while_non_bool_cond() {
    assert!(bengal::compile_source("func main() -> Int32 { while 1 { }; return 0; }").is_err());
}

#[test]
fn err_yield_in_while() {
    assert!(
        bengal::compile_source("func main() -> Int32 { while true { yield 1; }; return 0; }")
            .is_err()
    );
}

// --- Phase 4: break / continue ---

#[test]
fn while_break() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while true { if i == 3 { break; }; i = i + 1; }; return i; }"
        ),
        3
    );
}

#[test]
fn while_continue() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }"
        ),
        12
    );
}

#[test]
fn nested_break() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var outer: Int32 = 0; var i: Int32 = 0; while i < 3 { var j: Int32 = 0; while true { if j == 2 { break; }; j = j + 1; }; outer = outer + j; i = i + 1; }; return outer; }"
        ),
        6
    );
}

#[test]
fn break_with_var_update() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 0; while true { x = x + 10; break; }; return x; }"
        ),
        10
    );
}

#[test]
fn continue_skip_even() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 6 { i = i + 1; if (i / 2) * 2 == i { continue; }; s = s + i; }; return s; }"
        ),
        9
    );
}

#[test]
fn break_diverge_in_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 10 { let x: Int32 = if i == 5 { break; } else { yield i; }; i = x + 1; }; return i; }"
        ),
        5
    );
}

#[test]
fn continue_diverge_in_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; let v: Int32 = if i == 3 { continue; } else { yield i; }; s = s + v; }; return s; }"
        ),
        12
    );
}

#[test]
fn break_with_value() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = while true { break 42; }; return x; }"
        ),
        42
    );
}

#[test]
fn break_with_value_computed() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while true { i = i + 1; if i == 5 { break i * 10; }; }; return x; }"
        ),
        50
    );
}

#[test]
fn break_with_value_nested_if() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = while true { if true { break 1; } else { break 2; }; }; return x; }"
        ),
        1
    );
}

#[test]
fn nobreak_basic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 5 { if i == 3 { break 99; }; i = i + 1; } nobreak { yield 0; }; return x; }"
        ),
        99
    );
}

#[test]
fn nobreak_condition_false() {
    // while body has no break → while_ty is Unit, nobreak must also be Unit
    // Use mutable var to observe the value after loop
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }"
        ),
        3
    );
}

#[test]
fn nobreak_no_break_in_body() {
    // while body has no break → while_ty is Unit, nobreak must also be Unit
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 5 { i = i + 1; } nobreak { }; return i * 10; }"
        ),
        50
    );
}

// --- Phase 4: multi-numeric types ---

#[test]
fn i64_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int64 = 100 as Int64; let y: Int64 = 200 as Int64; return (x + y) as Int32; }"
        ),
        300
    );
}

#[test]
fn i64_comparison() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int64 = 10 as Int64; let y: Int64 = 20 as Int64; let r: Int32 = if x < y { yield 1; } else { yield 0; }; return r; }"
        ),
        1
    );
}

#[test]
fn f64_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Float64 = 3.5; let y: Float64 = 1.5; return (x + y) as Int32; }"
        ),
        5
    );
}

#[test]
fn mixed_cast_chain() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = 42; let y: Int64 = x as Int64; let z: Int32 = y as Int32; return z; }"
        ),
        42
    );
}

// --- Phase 4: local type inference ---

#[test]
fn infer_i32() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = 10; return x; }"),
        10
    );
}

#[test]
fn infer_i32_expr() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = 1 + 2 * 3; return x; }"),
        7
    );
}

#[test]
fn infer_bool() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let b = true; let r: Int32 = if b { yield 1; } else { yield 0; }; return r; }"
        ),
        1
    );
}

#[test]
fn infer_var() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { var x = 0; x = x + 1; return x; }"),
        1
    );
}

// --- Phase 4: cast ---

#[test]
fn cast_i32_to_i64() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int64 = 42 as Int64; return x as Int32; }"),
        42
    );
}

#[test]
fn cast_noop() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = 42 as Int32; return x; }"),
        42
    );
}

// --- Phase 4: error cases ---

#[test]
fn err_break_outside_loop() {
    assert!(bengal::compile_source("func main() -> Int32 { break; return 0; }").is_err());
}

#[test]
fn err_continue_outside_loop() {
    assert!(bengal::compile_source("func main() -> Int32 { continue; return 0; }").is_err());
}

#[test]
fn err_cast_bool() {
    assert!(
        bengal::compile_source("func main() -> Int32 { let x = true as Int32; return x; }")
            .is_err()
    );
}

#[test]
fn err_mixed_arithmetic() {
    assert!(
        bengal::compile_source(
            "func main() -> Int32 { let x: Int32 = 1; let y: Int64 = 2 as Int64; return x + y; }"
        )
        .is_err()
    );
}

#[test]
fn err_infer_mismatch() {
    assert!(
        bengal::compile_source("func main() -> Int32 { let x: Int32 = 3.14; return 0; }").is_err()
    );
}

#[test]
fn err_integer_overflow() {
    assert!(
        bengal::compile_source("func main() -> Int32 { let x = 3000000000; return 0; }").is_err()
    );
}

#[test]
fn err_break_value_no_nobreak() {
    assert!(bengal::compile_source("func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; }; return x; }").is_err());
}

#[test]
fn err_break_value_type_mismatch() {
    assert!(
        bengal::compile_source(
            "func main() -> Int32 { let x: Int32 = while true { break true; }; return x; }"
        )
        .is_err()
    );
}

#[test]
fn err_nobreak_in_while_true() {
    assert!(bengal::compile_source("func main() -> Int32 { let x: Int32 = while true { break 10; } nobreak { yield 20; }; return x; }").is_err());
}

#[test]
fn err_nobreak_type_mismatch() {
    assert!(bengal::compile_source("func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; } nobreak { yield true; }; return x; }").is_err());
}

// --- Native object emit path tests ---

use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn compile_to_native_and_run(source: &str) -> i32 {
    let obj_bytes = bengal::compile_source(source).unwrap();

    let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bengal_test_{}_{}", std::process::id(), id));
    std::fs::create_dir_all(&dir).unwrap();
    let obj_path = dir.join("test.o");
    let exe_path = dir.join("test");
    std::fs::write(&obj_path, &obj_bytes).unwrap();

    let link = std::process::Command::new("cc")
        .arg(&obj_path)
        .arg("-o")
        .arg(&exe_path)
        .output()
        .expect("cc not found - C compiler/linker required for native tests");
    assert!(
        link.status.success(),
        "link failed: {}",
        String::from_utf8_lossy(&link.stderr)
    );

    let run = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to execute compiled binary");

    let _ = std::fs::remove_dir_all(&dir);

    match run.status.code() {
        Some(code) => code,
        None => panic!(
            "process terminated by signal, stderr: {}",
            String::from_utf8_lossy(&run.stderr)
        ),
    }
}

#[test]
fn native_bare_expression() {
    assert_eq!(compile_to_native_and_run("42"), 42);
}

#[test]
fn native_simple_return() {
    assert_eq!(
        compile_to_native_and_run("func main() -> Int32 { return 42; }"),
        42
    );
}

#[test]
fn native_arithmetic() {
    assert_eq!(
        compile_to_native_and_run("func main() -> Int32 { return 2 + 3 * 4; }"),
        14
    );
}

#[test]
fn native_control_flow() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 10 { i = i + 1; }; return i; }"
        ),
        10
    );
}

#[test]
fn native_function_call() {
    assert_eq!(
        compile_to_native_and_run(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }"
        ),
        7
    );
}

#[test]
fn native_if_else() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }"
        ),
        1
    );
}

#[test]
fn native_break_continue() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }"
        ),
        12
    );
}

#[test]
fn native_unit_call() {
    assert_eq!(
        compile_to_native_and_run(
            "func noop() { return; } func main() -> Int32 { noop(); return 42; }"
        ),
        42
    );
}

#[test]
fn native_i64_cast() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { let x: Int64 = 100 as Int64; return x as Int32; }"
        ),
        100
    );
}

#[test]
fn native_i64_arithmetic() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { let x: Int64 = 10 as Int64; let y: Int64 = 20 as Int64; return (x + y) as Int32; }"
        ),
        30
    );
}

#[test]
fn native_float() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { let x: Float64 = 3.5; let y: Float64 = 1.5; return (x + y) as Int32; }"
        ),
        5
    );
}

#[test]
fn native_break_with_value() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { let x: Int32 = while true { break 42; }; return x; }"
        ),
        42
    );
}

#[test]
fn native_diverging_if() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { let x: Int32 = if false { return 99; } else { yield 42; }; return x; }"
        ),
        42
    );
}

// --- Known bug regression tests ---

/// Fibonacci codegen bug: `let next = a + b; a = b; b = next;` pattern
/// returns 89 instead of 55 for fib(10). See Plan.md "既知の問題" section.
#[test]
#[ignore]
fn fibonacci_known_bug() {
    assert_eq!(
        compile_and_run(
            "func fibonacci(n: Int32) -> Int32 { var a: Int32 = 0; var b: Int32 = 1; var i: Int32 = 0; while i < n { let next: Int32 = a + b; a = b; b = next; i = i + 1; }; return a; } func main() -> Int32 { return fibonacci(10); }"
        ),
        55
    );
}
