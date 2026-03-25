# Exhaustive Return Analysis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow functions/methods/getters to omit a trailing `return` when all control-flow paths already diverge (return/break/continue).

**Architecture:** Add a `stmt_always_returns(stmt) -> bool` helper to semantic analysis that recursively checks whether a statement guarantees all paths return. Replace the current `matches!(stmts.last(), Some(Stmt::Return(_)))` checks with this helper in three places: functions, methods, and getters.

**Tech Stack:** Rust, Bengal semantic analysis (`src/semantic/mod.rs`)

**Note:** Per project rules, `cargo fmt` and `cargo clippy -- -D warnings` must be run before each commit.

---

### Task 1: Add failing test for exhaustive return in functions

**Files:**
- Modify: `tests/control_flow.rs`

- [ ] **Step 1: Add the test**

Add before `// --- while loops ---` in `tests/control_flow.rs`:

```rust
#[test]
fn exhaustive_return_if_else() {
    // Both branches return — no trailing return needed
    assert_eq!(
        compile_and_run(
            r#"
            func choose(x: Int32) -> Int32 {
                if x > 0 { return 1; } else { return 0; };
            }
            func main() -> Int32 { return choose(5); }
        "#,
        ),
        1
    );
}
```

Do NOT modify the existing `both_branches_diverge` test here — it has a `return 0;` workaround and will be removed in Task 5 after the feature is implemented.

- [ ] **Step 2: Run to confirm the new test fails**

Run: `cargo test --test control_flow exhaustive_return_if_else -- --test-threads=1`
Expected: FAIL with "must end with a `return` statement"

- [ ] **Step 3: Commit the failing test**

```bash
git add tests/control_flow.rs
git commit -m "Add failing test for exhaustive return analysis"
```

---

### Task 2: Implement `stmt_always_returns` helper

**Files:**
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Add the helper function**

Add the following function in `src/semantic/mod.rs`, before `analyze_function` (around line 1015):

```rust
/// Check whether a statement guarantees all control-flow paths end with `return`.
/// Used to allow functions to omit a trailing `return` when all paths diverge.
fn stmt_always_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) => true,
        Stmt::Expr(expr) => match &expr.kind {
            ExprKind::If {
                then_block,
                else_block: Some(else_blk),
                ..
            } => {
                block_always_returns(then_block) && block_always_returns(else_blk)
            }
            _ => false,
        },
        _ => false,
    }
}

/// Check whether a block guarantees all control-flow paths end with `return`.
fn block_always_returns(block: &Block) -> bool {
    match block.stmts.last() {
        Some(stmt) => stmt_always_returns(stmt),
        None => false,
    }
}
```

- [ ] **Step 2: Replace the return check in `analyze_function`**

In `analyze_function` (around line 1044-1050), replace:

```rust
    // Check that the last statement is Return
    if !matches!(stmts.last(), Some(Stmt::Return(_))) {
        return Err(sem_err(format!(
            "function `{}` must end with a `return` statement",
            func.name
        )));
    }
```

With:

```rust
    // Check that all paths end with a return
    if !block_always_returns(&func.body) {
        return Err(sem_err(format!(
            "function `{}` must end with a `return` statement",
            func.name
        )));
    }
```

Also update the empty body check above it (around line 1037-1042) — the `block_always_returns` check already handles empty bodies (returns `false`), so replace both checks:

```rust
    if !block_always_returns(&func.body) {
        return Err(sem_err(format!(
            "function `{}` must end with a `return` statement",
            func.name
        )));
    }
```

This replaces the two separate checks (empty check + last-stmt check) with a single call.

- [ ] **Step 3: Run the test to verify it passes**

Run: `cargo test --test control_flow exhaustive_return_if_else -- --test-threads=1`
Expected: PASS

- [ ] **Step 4: Run the full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add src/semantic/mod.rs
git commit -m "Add exhaustive return analysis for functions"
```

---

### Task 3: Apply to methods and getters

**Files:**
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Add failing test for method exhaustive return**

Add to `tests/methods.rs` before the `// --- Error cases ---` section:

```rust
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
```

- [ ] **Step 2: Run to confirm it fails**

Run: `cargo test --test methods method_exhaustive_return -- --test-threads=1`
Expected: FAIL with "must end with a `return` statement"

- [ ] **Step 3: Update the method return check**

In `src/semantic/mod.rs` around line 1765, replace:

```rust
                if stmts.is_empty() || !matches!(stmts.last(), Some(Stmt::Return(_))) {
                    return Err(sem_err(format!(
                        "method `{}` must end with a `return` statement",
                        mname
                    )));
                }
```

With:

```rust
                if !block_always_returns(body) {
                    return Err(sem_err(format!(
                        "method `{}` must end with a `return` statement",
                        mname
                    )));
                }
```

- [ ] **Step 4: Update the getter return check**

In `src/semantic/mod.rs` around line 1825, replace:

```rust
    if stmts.is_empty() || !matches!(stmts.last(), Some(Stmt::Return(_))) {
        return Err(sem_err("getter must end with a `return` statement"));
    }
```

With:

```rust
    if !block_always_returns(block) {
        return Err(sem_err("getter must end with a `return` statement"));
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/semantic/mod.rs tests/methods.rs
git commit -m "Apply exhaustive return analysis to methods and getters"
```

---

### Task 4: Add edge case tests

**Files:**
- Modify: `tests/control_flow.rs`
- Modify: `tests/functions.rs`
- Modify: `tests/structs.rs`

- [ ] **Step 1: Add nested if/else exhaustive return test**

Add to `tests/control_flow.rs` after `exhaustive_return_if_else`:

```rust
#[test]
fn exhaustive_return_nested() {
    // Nested if/else where all leaf branches return
    assert_eq!(
        compile_and_run(
            r#"
            func classify(x: Int32) -> Int32 {
                if x > 0 {
                    if x > 100 { return 2; } else { return 1; };
                } else {
                    return 0;
                };
            }
            func main() -> Int32 { return classify(50); }
        "#,
        ),
        1
    );
}
```

- [ ] **Step 2: Add test for partial divergence (should still error)**

Add to `tests/functions.rs` in the error cases section:

```rust
#[test]
fn err_partial_return_in_if_else() {
    // Only one branch returns — still needs trailing return
    compile_source_should_fail(r#"
        func foo(x: Int32) -> Int32 {
            if x > 0 { return 1; } else { };
        }
        func main() -> Int32 { return foo(5); }
    "#);
}

#[test]
fn err_if_without_else_return() {
    // if without else cannot exhaustively return
    compile_source_should_fail(r#"
        func foo(x: Int32) -> Int32 {
            if x > 0 { return 1; };
        }
        func main() -> Int32 { return foo(5); }
    "#);
}
```

- [ ] **Step 3: Add unit-return exhaustive test**

Add to `tests/functions.rs` before the error cases section:

```rust
#[test]
fn exhaustive_return_unit() {
    // Void function where both branches return — no trailing return needed
    assert_eq!(
        compile_and_run(r#"
            func side_effect(x: Int32) {
                if x > 0 { return; } else { return; };
            }
            func main() -> Int32 { side_effect(1); return 42; }
        "#),
        42
    );
}
```

- [ ] **Step 4: Add getter exhaustive return test**

Add to `tests/structs.rs` before the error cases section:

```rust
#[test]
fn getter_exhaustive_return() {
    assert_eq!(
        compile_and_run(r#"
            struct Classify {
                var x: Int32;
                var label: Int32 {
                    get {
                        if self.x > 0 { return 1; } else { return 0; };
                    }
                };
            }
            func main() -> Int32 {
                let c = Classify(x: 5);
                return c.label;
            }
        "#),
        1
    );
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add tests/control_flow.rs tests/functions.rs tests/structs.rs
git commit -m "Add edge case tests for exhaustive return analysis"
```

---

### Task 5: Clean up and update grammar docs

**Files:**
- Modify: `tests/control_flow.rs` (remove old workaround test)
- Modify: `docs/grammar.md`

- [ ] **Step 1: Remove the old `both_branches_diverge` workaround test**

The `both_branches_diverge` test from the reorganization used a workaround `return 0;`. Now that exhaustive return is implemented, it duplicates `exhaustive_return_if_else`. Remove the `both_branches_diverge` test entirely from `tests/control_flow.rs`.

- [ ] **Step 2: Update docs/grammar.md**

In the "Functions" section under "Semantic Rules" (around line 214), add after "All functions must end with a `return` statement.":

```
- A function may omit a trailing `return` if all control-flow paths end with `return` (e.g., `if`/`else` where both branches return).
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add tests/control_flow.rs docs/grammar.md
git commit -m "Document exhaustive return analysis in grammar"
```
