# Pipeline Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove direct external usage of `lower_program_with_inferred` by unifying test helpers to use the pipeline API and restricting visibility.

**Architecture:** Replace the JIT-based `compile_and_run` with delegation to `compile_to_native_and_run` (which uses pipeline API), then change `*_with_inferred` functions to `pub(crate)`.

**Tech Stack:** Rust

**Spec:** `docs/superpowers/specs/2026-03-26-pipeline-cleanup-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `tests/common/mod.rs` | Rewrite `compile_and_run` as delegation; remove JIT imports |
| `src/bir/lowering.rs` | Change `lower_program_with_inferred` and `lower_module_with_inferred` to `pub(crate)` |

---

### Task 1: Rewrite `compile_and_run` and remove JIT imports

**Files:**
- Modify: `tests/common/mod.rs`

- [ ] **Step 1: Replace `compile_and_run` body with delegation**

In `tests/common/mod.rs`, replace the entire `compile_and_run` function (lines 12-41) with:

```rust
/// Compile and run a single-file Bengal program, returning the exit code.
pub fn compile_and_run(source: &str) -> i32 {
    compile_to_native_and_run(source)
}
```

- [ ] **Step 2: Remove JIT-only imports**

Remove these 4 imports from the top of `tests/common/mod.rs`:

```rust
use bengal::bir;           // line 1
use bengal::codegen;       // line 2
use inkwell::OptimizationLevel;  // line 6
use inkwell::context::Context;   // line 7
```

Keep these imports (still used by `compile_should_fail`):
```rust
use bengal::lexer::tokenize;  // line 3
use bengal::parser::parse;    // line 4
use bengal::semantic;          // line 5
```

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: all ~340 tests pass

- [ ] **Step 4: Run clippy and fmt**

Run: `cargo clippy && cargo fmt`
Expected: no warnings, no formatting changes

- [ ] **Step 5: Commit**

```bash
git add tests/common/mod.rs
git commit -m "Replace JIT-based compile_and_run with native compile + run delegation"
```

---

### Task 2: Restrict `*_with_inferred` visibility to `pub(crate)`

**Files:**
- Modify: `src/bir/lowering.rs`

- [ ] **Step 1: Change `lower_program_with_inferred` to `pub(crate)`**

In `src/bir/lowering.rs` line 2096, change:

```rust
pub fn lower_program_with_inferred(
```

to:

```rust
pub(crate) fn lower_program_with_inferred(
```

- [ ] **Step 2: Change `lower_module_with_inferred` to `pub(crate)`**

In `src/bir/lowering.rs` line 2283, change:

```rust
pub fn lower_module_with_inferred(
```

to:

```rust
pub(crate) fn lower_module_with_inferred(
```

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: all tests pass (no external crate accesses these functions)

- [ ] **Step 4: Run clippy and fmt**

Run: `cargo clippy && cargo fmt`
Expected: no warnings, no formatting changes

- [ ] **Step 5: Commit**

```bash
git add src/bir/lowering.rs
git commit -m "Restrict lower_program_with_inferred and lower_module_with_inferred to pub(crate)"
```

---

### Task 3: Final verification

**Files:** (none — verification only)

- [ ] **Step 1: Run full test suite and measure time**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass. Note the total execution time.

- [ ] **Step 2: Verify no external references remain**

Run: `grep -r "lower_program_with_inferred\|lower_module_with_inferred" tests/`
Expected: no matches (no test files reference these functions directly)

- [ ] **Step 3: Run clippy**

Run: `cargo clippy`
Expected: no warnings
