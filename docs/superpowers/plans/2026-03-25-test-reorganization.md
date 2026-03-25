# Test Reorganization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `tests/compile_test.rs` into 9 feature-based test files with shared helpers, and add ~50 new tests for coverage gaps.

**Architecture:** Extract shared helper functions into `tests/common/mod.rs`, then create one test file per language feature domain. Each file imports helpers via `mod common;`. Existing tests are moved (some renamed), then new tests are added.

**Tech Stack:** Rust integration tests, inkwell (JIT), tempfile (package tests), cc linker (native tests)

**Spec:** `docs/superpowers/specs/2026-03-25-test-reorganization-design.md`

**Note:** Per project rules (`.claude/rules/rust-coding-style.md`), `cargo fmt` and `cargo clippy` must be run before each commit. Every "Commit" step below implicitly includes `cargo fmt && cargo clippy -- -D warnings` before `git add`/`git commit`.

---

### Task 1: Create shared helpers module

**Files:**
- Create: `tests/common/mod.rs`

- [ ] **Step 1: Create `tests/common/mod.rs` with all helper functions**

Extract from `tests/compile_test.rs` lines 1-26 (`compile_and_run`), lines 769-808 (`compile_to_native_and_run` + `TEST_COUNTER`), lines 1249-1256 (`compile_should_fail`), lines 1345-1391 (`compile_and_run_package` + `compile_package_should_fail`), and add the new `compile_source_should_fail` helper.

```rust
use bengal::bir;
use bengal::codegen;
use bengal::lexer::tokenize;
use bengal::parser::parse;
use bengal::semantic;
use inkwell::OptimizationLevel;
use inkwell::context::Context;
use std::sync::atomic::{AtomicU64, Ordering};

pub static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// JIT-compile and run a single-file Bengal program, returning the exit code.
pub fn compile_and_run(source: &str) -> i32 {
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

/// Compile to native object, link, run, and return the exit code.
pub fn compile_to_native_and_run(source: &str) -> i32 {
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

/// Run semantic analysis and return the error string.
/// Use for tests that specifically target semantic errors.
pub fn compile_should_fail(source: &str) -> String {
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    match semantic::analyze(&program) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected semantic error but analysis succeeded"),
    }
}

/// Run the full compilation pipeline and return the error string.
/// Use when the error phase is unimportant or for non-semantic errors.
pub fn compile_source_should_fail(source: &str) -> String {
    match bengal::compile_source(source) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected compilation error but compilation succeeded"),
    }
}

/// Compile a multi-file package, link, run, and return the exit code.
pub fn compile_and_run_package(files: &[(&str, &str)]) -> i32 {
    let dir = tempfile::TempDir::new().unwrap();

    let toml_content = format!(
        "[package]\nname = \"test_pkg\"\nentry = \"{}\"",
        files[0].0
    );
    std::fs::write(dir.path().join("Bengal.toml"), toml_content).unwrap();

    for (path, source) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full_path, source).unwrap();
    }

    let entry_path = dir.path().join(files[0].0);
    let exe_path = dir.path().join("test_exe");
    bengal::compile_package_to_executable(&entry_path, &exe_path).unwrap();

    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run compiled executable");
    output.status.code().unwrap_or(-1)
}

/// Compile a multi-file package and return the error string.
pub fn compile_package_should_fail(files: &[(&str, &str)]) -> String {
    let dir = tempfile::TempDir::new().unwrap();

    let toml_content = format!(
        "[package]\nname = \"test_pkg\"\nentry = \"{}\"",
        files[0].0
    );
    std::fs::write(dir.path().join("Bengal.toml"), toml_content).unwrap();

    for (path, source) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full_path, source).unwrap();
    }

    let entry_path = dir.path().join(files[0].0);
    let exe_path = dir.path().join("test_exe");
    let err = bengal::compile_package_to_executable(&entry_path, &exe_path).unwrap_err();
    err.to_string()
}
```

- [ ] **Step 2: Verify the helpers compile**

Run: `cargo test --no-run 2>&1 | head -20`
Expected: compiles without errors (existing tests in compile_test.rs still work)

- [ ] **Step 3: Commit**

```bash
git add tests/common/mod.rs
git commit -m "Extract shared test helpers into tests/common/mod.rs"
```

---

### Task 2: Create expressions.rs

**Files:**
- Create: `tests/expressions.rs`

- [ ] **Step 1: Create `tests/expressions.rs` with moved + new tests**

```rust
mod common;

use common::{compile_and_run, compile_source_should_fail};

// --- Literals and arithmetic ---

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

// --- Precedence and associativity ---

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

#[test]
fn complex_precedence() {
    // 1 + 2 * 3 - 4 / 2 = 1 + 6 - 2 = 5
    assert_eq!(
        compile_and_run("func main() -> Int32 { return 1 + 2 * 3 - 4 / 2; }"),
        5
    );
}

// --- Multi-numeric types ---

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
fn float32_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Float32 = 3.5 as Float32; let y: Float32 = 1.5 as Float32; return (x + y) as Int32; }"
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

// --- Cast ---

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

#[test]
fn cast_i32_to_f32() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Float32 = 10 as Float32; return x as Int32; }"),
        10
    );
}

#[test]
fn cast_f32_to_f64() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Float32 = 7.0 as Float32; let y: Float64 = x as Float64; return y as Int32; }"
        ),
        7
    );
}

#[test]
fn cast_chain_all_types() {
    // Int32 -> Int64 -> Float64 -> Float32 -> Int32
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let a: Int32 = 42; let b: Int64 = a as Int64; let c: Float64 = b as Float64; let d: Float32 = c as Float32; return d as Int32; }"
        ),
        42
    );
}

// --- Error cases ---

#[test]
fn err_cast_bool() {
    compile_source_should_fail("func main() -> Int32 { let x = true as Int32; return x; }");
}

#[test]
fn err_mixed_arithmetic() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = 1; let y: Int64 = 2 as Int64; return x + y; }",
    );
}

#[test]
fn err_infer_mismatch() {
    compile_source_should_fail("func main() -> Int32 { let x: Int32 = 3.14; return 0; }");
}

#[test]
fn err_integer_overflow() {
    compile_source_should_fail("func main() -> Int32 { let x = 3000000000; return 0; }");
}

#[test]
fn err_as_binds_tighter_than_addition() {
    // `1 + 2 as Int64` parses as `1 + (2 as Int64)` => Int32 + Int64 => type error
    compile_source_should_fail("func main() -> Int32 { return 1 + 2 as Int64; }");
}

#[test]
fn err_comparison_type_mismatch() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int64 = 1 as Int64; let r = if x == 1 { yield 1; } else { yield 0; }; return r; }",
    );
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test expressions -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/expressions.rs
git commit -m "Add expressions integration tests (moved + new coverage)"
```

---

### Task 3: Create functions.rs

**Files:**
- Create: `tests/functions.rs`

- [ ] **Step 1: Create `tests/functions.rs` with moved + new tests**

```rust
mod common;

use common::{compile_and_run, compile_source_should_fail};

// --- Basic function definitions ---

#[test]
fn simple() {
    assert_eq!(compile_and_run("func main() -> Int32 { return 42; }"), 42);
}

#[test]
fn call() {
    assert_eq!(
        compile_and_run(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; }\nfunc main() -> Int32 { return add(3, 4); }"
        ),
        7
    );
}

#[test]
fn call_chain() {
    assert_eq!(
        compile_and_run(
            "func double(x: Int32) -> Int32 { return x * 2; }\nfunc main() -> Int32 { return double(double(5)); }"
        ),
        20
    );
}

#[test]
fn multiple_functions() {
    assert_eq!(
        compile_and_run(
            "func square(x: Int32) -> Int32 { return x * x; }\nfunc main() -> Int32 { return square(3) + square(4); }"
        ),
        25
    );
}

#[test]
fn unit_return() {
    assert_eq!(
        compile_and_run("func noop() { return; } func main() -> Int32 { noop(); return 42; }"),
        42
    );
}

#[test]
fn fibonacci() {
    // Originally a regression test, retained here as a function test
    assert_eq!(
        compile_and_run(
            "func fibonacci(n: Int32) -> Int32 { var a: Int32 = 0; var b: Int32 = 1; var i: Int32 = 0; while i < n { let next: Int32 = a + b; a = b; b = next; i = i + 1; }; return a; } func main() -> Int32 { return fibonacci(10); }"
        ),
        55
    );
}

// --- New tests ---

#[test]
fn recursive_countdown() {
    assert_eq!(
        compile_and_run(r#"
            func countdown(n: Int32) -> Int32 {
                if n <= 0 { return 0; };
                return 1 + countdown(n - 1);
            }
            func main() -> Int32 { return countdown(5); }
        "#),
        5
    );
}

#[test]
fn multi_param_function() {
    assert_eq!(
        compile_and_run(r#"
            func add3(a: Int32, b: Int32, c: Int32) -> Int32 {
                return a + b + c;
            }
            func main() -> Int32 { return add3(1, 2, 3); }
        "#),
        6
    );
}

#[test]
fn function_returns_bool() {
    assert_eq!(
        compile_and_run(r#"
            func is_positive(x: Int32) -> Bool {
                if x > 0 { return true; };
                return false;
            }
            func main() -> Int32 {
                let r = if is_positive(5) { yield 1; } else { yield 0; };
                return r;
            }
        "#),
        1
    );
}

// --- Error cases ---

#[test]
fn err_no_main() {
    compile_source_should_fail("func add(a: Int32, b: Int32) -> Int32 { return a + b; }");
}

#[test]
fn err_main_with_params() {
    compile_source_should_fail("func main(x: Int32) -> Int32 { return x; }");
}

#[test]
fn err_no_return() {
    compile_source_should_fail("func main() -> Int32 { let x: Int32 = 1; }");
}

#[test]
fn err_no_yield() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = { let a: Int32 = 1; }; return x; }",
    );
}

#[test]
fn err_yield_in_func() {
    compile_source_should_fail("func main() -> Int32 { yield 1; }");
}

#[test]
fn err_return_in_block() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = { return 1; }; return x; }",
    );
}

#[test]
fn err_return_type_mismatch() {
    compile_source_should_fail("func main() -> Int32 { return true; }");
}

#[test]
fn err_duplicate_function() {
    compile_source_should_fail(r#"
        func foo() -> Int32 { return 1; }
        func foo() -> Int32 { return 2; }
        func main() -> Int32 { return foo(); }
    "#);
}

#[test]
fn err_wrong_arg_count() {
    compile_source_should_fail(r#"
        func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        func main() -> Int32 { return add(1); }
    "#);
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test functions -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/functions.rs
git commit -m "Add functions integration tests (moved + new coverage)"
```

---

### Task 4: Create variables.rs

**Files:**
- Create: `tests/variables.rs`

- [ ] **Step 1: Create `tests/variables.rs` with moved + new tests**

```rust
mod common;

use common::{compile_and_run, compile_source_should_fail};

// --- let / var bindings ---

#[test]
fn let_binding() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = 10; return x; }"),
        10
    );
}

#[test]
fn var_binding() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { var x: Int32 = 1; x = 10; return x; }"),
        10
    );
}

#[test]
fn let_arithmetic() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let a: Int32 = 2; let b: Int32 = 3; return a + b * 4; }"
        ),
        14
    );
}

#[test]
fn shadowing() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = 1; let x: Int32 = x + 10; return x; }"
        ),
        11
    );
}

#[test]
fn var_update() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 0; x = x + 1; x = x + 2; return x; }"
        ),
        3
    );
}

// --- Block expressions ---

#[test]
fn block_expression() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x: Int32 = { yield 7; }; return x; }"),
        7
    );
}

#[test]
fn block_shadow() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x: Int32 = 1; let y: Int32 = { let x: Int32 = 10; yield x + 1; }; return x + y; }"
        ),
        12
    );
}

#[test]
fn block_var_assign() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 0; x = { x = 10; yield x + 1; }; return x; }"
        ),
        11
    );
}

// --- Type inference ---

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

// --- New tests ---

#[test]
fn shadow_function_param() {
    assert_eq!(
        compile_and_run(r#"
            func foo(x: Int32) -> Int32 {
                let x: Int32 = x + 100;
                return x;
            }
            func main() -> Int32 { return foo(5); }
        "#),
        105
    );
}

#[test]
fn shadow_nested_scopes() {
    assert_eq!(
        compile_and_run(r#"
            func main() -> Int32 {
                let x: Int32 = 1;
                let y: Int32 = {
                    let x: Int32 = 10;
                    let z: Int32 = {
                        let x: Int32 = 100;
                        yield x;
                    };
                    yield x + z;
                };
                return x + y;
            }
        "#),
        111
    );
}

#[test]
fn infer_from_block_expr() {
    assert_eq!(
        compile_and_run("func main() -> Int32 { let x = { yield 1 + 2; }; return x; }"),
        3
    );
}

#[test]
fn infer_from_if_else() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x = if true { yield 1; } else { yield 2; }; return x; }"
        ),
        1
    );
}

#[test]
fn infer_float64() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { let x = 3.14; return x as Int32; }"
        ),
        3
    );
}

// --- Error cases ---

#[test]
fn err_undefined_var() {
    compile_source_should_fail("func main() -> Int32 { return x; }");
}

#[test]
fn err_immutable_assign() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = 1; x = 2; return x; }",
    );
}

#[test]
fn err_type_annotation_mismatch() {
    // Integer literal defaults to Int32, but annotation says Int64
    compile_source_should_fail("func main() -> Int32 { let x: Int64 = 10; return 0; }");
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test variables -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/variables.rs
git commit -m "Add variables integration tests (moved + new coverage)"
```

---

### Task 5: Create control_flow.rs

**Files:**
- Create: `tests/control_flow.rs`

- [ ] **Step 1: Create `tests/control_flow.rs` with moved + new tests**

```rust
mod common;

use common::{compile_and_run, compile_source_should_fail};

// --- if / else ---

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

// --- Comparisons ---

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

// --- Logical operators ---

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
fn logical_not_with_comparison() {
    assert_eq!(
        compile_and_run(r#"
            func main() -> Int32 {
                let x: Int32 = 3;
                let r = if !(x > 5) { yield 1; } else { yield 0; };
                return r;
            }
        "#),
        1
    );
}

// --- Early return and divergence ---

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
fn both_branches_diverge() {
    assert_eq!(
        compile_and_run(r#"
            func choose(x: Int32) -> Int32 {
                if x > 0 { return 1; } else { return 0; };
            }
            func main() -> Int32 { return choose(5); }
        "#),
        1
    );
}

// --- while loops ---

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
fn while_false_body_not_executed() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var x: Int32 = 42; while false { x = 0; }; return x; }"
        ),
        42
    );
}

// --- break / continue ---

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
fn continue_nested_loops() {
    // Inner continue should not affect outer loop accumulation
    assert_eq!(
        compile_and_run(r#"
            func main() -> Int32 {
                var outer: Int32 = 0;
                var i: Int32 = 0;
                while i < 3 {
                    i = i + 1;
                    var j: Int32 = 0;
                    while j < 4 {
                        j = j + 1;
                        if j == 2 { continue; };
                        outer = outer + 1;
                    };
                };
                return outer;
            }
        "#),
        9 // 3 outer iterations * 3 inner (4 - 1 skipped) = 9
    );
}

// --- break with value ---

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
fn break_with_complex_expr() {
    assert_eq!(
        compile_and_run(r#"
            func main() -> Int32 {
                var i: Int32 = 3;
                var j: Int32 = 5;
                let x: Int32 = while true {
                    break (i + 1) * (j - 1);
                };
                return x;
            }
        "#),
        16 // (3+1) * (5-1) = 4 * 4 = 16
    );
}

// --- nobreak ---

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
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }"
        ),
        3
    );
}

#[test]
fn nobreak_no_break_in_body() {
    assert_eq!(
        compile_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 5 { i = i + 1; } nobreak { }; return i * 10; }"
        ),
        50
    );
}

// --- Short-circuit evaluation ---
// Note: Bengal has no closures, so we use block expressions with side effects
// instead of function calls to verify short-circuit behavior.

#[test]
fn short_circuit_and() {
    // false && f() should not call f, so var stays 0
    assert_eq!(
        compile_and_run(r#"
            func main() -> Int32 {
                var x: Int32 = 0;
                if false && { x = 1; yield true; } { };
                return x;
            }
        "#),
        0
    );
}

#[test]
fn short_circuit_or() {
    // true || f() should not call f, so var stays 0
    assert_eq!(
        compile_and_run(r#"
            func main() -> Int32 {
                var x: Int32 = 0;
                if true || { x = 1; yield true; } { };
                return x;
            }
        "#),
        0
    );
}

// --- Error cases ---

#[test]
fn err_if_non_bool_cond() {
    compile_source_should_fail(
        "func main() -> Int32 { if 1 { yield 1; } else { yield 2; }; return 0; }",
    );
}

#[test]
fn err_if_branch_mismatch() {
    compile_source_should_fail(
        "func main() -> Int32 { if true { yield 1; } else { yield true; }; return 0; }",
    );
}

#[test]
fn err_while_non_bool_cond() {
    compile_source_should_fail("func main() -> Int32 { while 1 { }; return 0; }");
}

#[test]
fn err_yield_in_while() {
    compile_source_should_fail("func main() -> Int32 { while true { yield 1; }; return 0; }");
}

#[test]
fn err_break_outside_loop() {
    compile_source_should_fail("func main() -> Int32 { break; return 0; }");
}

#[test]
fn err_continue_outside_loop() {
    compile_source_should_fail("func main() -> Int32 { continue; return 0; }");
}

#[test]
fn err_break_value_no_nobreak() {
    compile_source_should_fail(
        "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; }; return x; }",
    );
}

#[test]
fn err_break_value_type_mismatch() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = while true { break true; }; return x; }",
    );
}

#[test]
fn err_nobreak_in_while_true() {
    compile_source_should_fail(
        "func main() -> Int32 { let x: Int32 = while true { break 10; } nobreak { yield 20; }; return x; }",
    );
}

#[test]
fn err_nobreak_type_mismatch() {
    compile_source_should_fail(
        "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; } nobreak { yield true; }; return x; }",
    );
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test control_flow -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/control_flow.rs
git commit -m "Add control flow integration tests (moved + new coverage)"
```

---

### Task 6: Create structs.rs

**Files:**
- Create: `tests/structs.rs`

- [ ] **Step 1: Create `tests/structs.rs` with new JIT versions + new tests**

```rust
mod common;

use common::{compile_and_run, compile_should_fail, compile_source_should_fail};

// --- Basic struct ---

#[test]
fn basic() {
    assert_eq!(
        compile_and_run(r#"
            struct Point { var x: Int32; var y: Int32; }
            func main() -> Int32 {
                let p = Point(x: 3, y: 4);
                return p.x + p.y;
            }
        "#),
        7
    );
}

#[test]
fn function_arg_return() {
    assert_eq!(
        compile_and_run(r#"
            struct Point { var x: Int32; var y: Int32; }
            func make_point(a: Int32, b: Int32) -> Point {
                return Point(x: a, y: b);
            }
            func sum(p: Point) -> Int32 { return p.x + p.y; }
            func main() -> Int32 {
                let p = make_point(10, 20);
                return sum(p);
            }
        "#),
        30
    );
}

#[test]
fn nested_struct() {
    assert_eq!(
        compile_and_run(r#"
            struct Inner { var value: Int32; }
            struct Outer { var inner: Inner; var extra: Int32; }
            func main() -> Int32 {
                let i = Inner(value: 10);
                let o = Outer(inner: i, extra: 20);
                return o.inner.value + o.extra;
            }
        "#),
        30
    );
}

#[test]
fn explicit_init() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        10
    );
}

#[test]
fn zero_arg_init() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        99
    );
}

// --- Computed properties ---

#[test]
fn computed_property_get() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        12
    );
}

#[test]
fn computed_property_get_set() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        10
    );
}

#[test]
fn computed_property_multi() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        19 // (3+4) + (3*4) = 7 + 12 = 19
    );
}

#[test]
fn field_assign_complex_expr() {
    assert_eq!(
        compile_and_run(r#"
            struct Box { var value: Int32; }
            func main() -> Int32 {
                var b = Box(value: 0);
                b.value = if true { yield 42; } else { yield 0; };
                return b.value;
            }
        "#),
        42
    );
}

// --- Error cases ---

#[test]
fn err_recursive_struct() {
    compile_should_fail(r#"
        struct Node {
            var child: Node;
        }
        func main() -> Int32 { return 0; }
    "#);
}

#[test]
fn err_duplicate_member() {
    compile_should_fail(r#"
        struct Bad {
            var x: Int32;
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#);
}

#[test]
fn err_init_missing_field() {
    compile_should_fail(r#"
        struct Pair {
            var a: Int32;
            var b: Int32;
            init(x: Int32) {
                self.a = x;
            }
        }
        func main() -> Int32 { return 0; }
    "#);
}

#[test]
fn err_let_struct_field_assign() {
    compile_source_should_fail(r#"
        struct Point { var x: Int32; var y: Int32; }
        func main() -> Int32 {
            let p = Point(x: 1, y: 2);
            p.x = 10;
            return p.x;
        }
    "#);
}

#[test]
fn err_memberwise_with_explicit_init() {
    compile_source_should_fail(r#"
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
    "#);
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test structs -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/structs.rs
git commit -m "Add structs integration tests (new JIT tests + coverage)"
```

---

### Task 7: Create methods.rs

**Files:**
- Create: `tests/methods.rs`

- [ ] **Step 1: Create `tests/methods.rs` with moved + new tests**

```rust
mod common;

use common::{compile_and_run, compile_source_should_fail};

#[test]
fn method_basic() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        7
    );
}

#[test]
fn method_with_args() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        33
    );
}

#[test]
fn method_chaining() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        20
    );
}

#[test]
fn method_calls_other_method() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        14
    );
}

#[test]
fn method_in_control_flow() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        11
    );
}

#[test]
fn method_unit_return() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        42
    );
}

// --- New tests ---

#[test]
fn method_returns_struct() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        20
    );
}

#[test]
fn method_param_same_type() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        23 // 2*4 + 3*5 = 8 + 15
    );
}

#[test]
fn method_calls_computed_property() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        24
    );
}

#[test]
fn method_in_while_loop() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        10 // 0+1+2+3+4
    );
}

// --- Error cases ---

#[test]
fn err_self_outside_struct() {
    compile_source_should_fail(r#"
        func main() -> Int32 { return self.x; }
    "#);
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test methods -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/methods.rs
git commit -m "Add methods integration tests (moved + new coverage)"
```

---

### Task 8: Create protocols.rs

**Files:**
- Create: `tests/protocols.rs`

- [ ] **Step 1: Create `tests/protocols.rs` with moved + new tests**

```rust
mod common;

use common::{compile_and_run, compile_should_fail};

// --- Conformance ---

#[test]
fn protocol_basic_conformance() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        7
    );
}

#[test]
fn protocol_multiple_methods() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        30
    );
}

#[test]
fn protocol_property_get() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        12
    );
}

#[test]
fn protocol_stored_property_satisfies_get() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        42
    );
}

#[test]
fn protocol_multiple_conformance() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        20
    );
}

#[test]
fn protocol_property_get_set() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        99
    );
}

#[test]
fn protocol_method_with_params() {
    assert_eq!(
        compile_and_run(r#"
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
        "#),
        130
    );
}

// --- Error cases ---

#[test]
fn protocol_error_missing_method() {
    let err = compile_should_fail(r#"
        protocol Summable {
            func sum() -> Int32;
        }
        struct Empty: Summable {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(err.contains("does not implement method"), "got: {}", err);
}

#[test]
fn protocol_error_return_type_mismatch() {
    let err = compile_should_fail(r#"
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
    "#);
    assert!(err.contains("return type"), "got: {}", err);
}

#[test]
fn protocol_error_unknown_protocol() {
    let err = compile_should_fail(r#"
        struct Bad: NonExistent {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(err.contains("unknown protocol"), "got: {}", err);
}

#[test]
fn protocol_error_missing_property() {
    let err = compile_should_fail(r#"
        protocol HasTotal {
            var total: Int32 { get };
        }
        struct Bad: HasTotal {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(err.contains("does not implement property"), "got: {}", err);
}

#[test]
fn protocol_error_missing_setter() {
    let err = compile_should_fail(r#"
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
    "#);
    assert!(err.contains("requires a setter"), "got: {}", err);
}

#[test]
fn err_param_count_mismatch() {
    let err = compile_should_fail(r#"
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
    "#);
    assert!(
        err.contains("parameter") || err.contains("does not implement"),
        "got: {}",
        err
    );
}

#[test]
fn err_param_type_mismatch() {
    let err = compile_should_fail(r#"
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
    "#);
    assert!(
        err.contains("parameter") || err.contains("does not implement"),
        "got: {}",
        err
    );
}

#[test]
fn err_duplicate_protocol() {
    compile_should_fail(r#"
        protocol Foo {
            func bar() -> Int32;
        }
        protocol Foo {
            func baz() -> Int32;
        }
        func main() -> Int32 { return 0; }
    "#);
}

#[test]
fn err_property_type_mismatch() {
    let err = compile_should_fail(r#"
        protocol HasValue {
            var value: Int32 { get };
        }
        struct Bad: HasValue {
            var value: Bool;
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(
        err.contains("type") || err.contains("does not implement"),
        "got: {}",
        err
    );
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test protocols -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/protocols.rs
git commit -m "Add protocols integration tests (moved + new coverage)"
```

---

### Task 9: Create modules.rs

**Files:**
- Create: `tests/modules.rs`

- [ ] **Step 1: Create `tests/modules.rs` with moved + new tests**

```rust
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

// --- Error cases ---

#[test]
fn err_super_at_root() {
    let err = compile_package_should_fail(&[
        (
            "main.bengal",
            r#"
            import super::something::Foo;
            func main() -> Int32 { return 0; }
        "#,
        ),
    ]);
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
        err.contains("not found") || err.contains("does not export") || err.contains("cannot"),
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
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test modules -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/modules.rs
git commit -m "Add modules integration tests (moved + new coverage)"
```

---

### Task 10: Create native_emit.rs

**Files:**
- Create: `tests/native_emit.rs`

- [ ] **Step 1: Create `tests/native_emit.rs` with retained smoke tests + new tests**

```rust
mod common;

use common::compile_to_native_and_run;

#[test]
fn native_bare_expression() {
    assert_eq!(compile_to_native_and_run("42"), 42);
}

#[test]
fn native_arithmetic() {
    assert_eq!(
        compile_to_native_and_run("func main() -> Int32 { return 2 + 3 * 4; }"),
        14
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
fn native_control_flow() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { var i: Int32 = 0; while i < 10 { i = i + 1; }; return i; }"
        ),
        10
    );
}

#[test]
fn native_type_cast() {
    assert_eq!(
        compile_to_native_and_run(
            "func main() -> Int32 { let x: Int64 = 100 as Int64; return x as Int32; }"
        ),
        100
    );
}

#[test]
fn native_struct_basic() {
    assert_eq!(
        compile_to_native_and_run(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x + p.y; }"
        ),
        7
    );
}

#[test]
fn native_method_call() {
    assert_eq!(
        compile_to_native_and_run(r#"
            struct Counter {
                var n: Int32;
                func value() -> Int32 {
                    return self.n;
                }
            }
            func main() -> Int32 {
                let c = Counter(n: 42);
                return c.value();
            }
        "#),
        42
    );
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test native_emit -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/native_emit.rs
git commit -m "Add native emit smoke tests"
```

---

### Task 11: Remove old compile_test.rs and verify

**Files:**
- Delete: `tests/compile_test.rs`

- [ ] **Step 1: Run all new tests to verify everything passes**

Run: `cargo test --test expressions --test functions --test variables --test control_flow --test structs --test methods --test protocols --test modules --test native_emit -- --test-threads=1`
Expected: all tests pass

- [ ] **Step 2: Delete the old monolithic test file**

```bash
rm tests/compile_test.rs
```

- [ ] **Step 3: Run full test suite to confirm nothing is broken**

Run: `cargo test`
Expected: all tests pass, no regressions

- [ ] **Step 4: Run cargo fmt and cargo clippy**

Run: `cargo fmt && cargo clippy -- -D warnings`
Expected: no warnings

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Remove monolithic compile_test.rs in favor of feature-based test files"
```

---

### Task 12: Fix any failing new tests

This task handles the case where some new tests expose bugs or make incorrect assumptions about compiler behavior.

**Files:**
- Modify: whichever test files have failures

- [ ] **Step 1: Run the full test suite and note any failures**

Run: `cargo test 2>&1 | grep -E "^test .* FAILED|^failures:"`
Expected: ideally no failures; if there are, proceed to fix

- [ ] **Step 2: For each failing new test, investigate and fix**

Common fixes:
- If the compiler correctly rejects something the test expected to succeed, fix the test source code
- If the compiler accepts something the test expected to fail, the test expectation is wrong — update the test
- If the compiler panics, that's a real bug — note it but don't fix the compiler in this task

- [ ] **Step 3: Run full test suite again**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit fixes**

```bash
git add -A
git commit -m "Fix test assumptions for new integration tests"
```
