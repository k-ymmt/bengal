# Pipeline Cleanup: Remove Direct lower_program_with_inferred Usage

## Overview

Eliminate direct external usage of `lower_program_with_inferred` by unifying the JIT test helper (`compile_and_run`) to use native compile + run (pipeline API), and restrict `*_with_inferred` functions to crate-internal visibility.

## Scope

- Replace `compile_and_run` (JIT) with delegation to `compile_to_native_and_run` (native)
- Change `lower_program_with_inferred` and `lower_module_with_inferred` from `pub` to `pub(crate)`
- Keep `lower_program` and `lower_module` (wrappers) as public API for unit tests

## Motivation

The TODO item "1.6.1a: `lower_program_with_inferred` の削除（パイプライン統一の確認後）" calls for removing direct calls to `lower_program_with_inferred` now that the pipeline is unified. The only external caller is `tests/common/mod.rs` (`compile_and_run`), which bypasses the pipeline to do JIT execution. Replacing this with native compile + run aligns with Swift and Rust compiler testing practices.

## Design

### 1. `compile_and_run` Rewrite

Replace the JIT implementation in `tests/common/mod.rs` with a simple delegation:

```rust
pub fn compile_and_run(source: &str) -> i32 {
    compile_to_native_and_run(source)
}
```

`compile_to_native_and_run` already uses the pipeline API (`compile_source_to_objects`), so this achieves pipeline unification.

Remove JIT-only imports that become unused:
- `bengal::bir`
- `bengal::codegen`
- `bengal::lexer::tokenize`
- `bengal::parser::parse`
- `bengal::semantic`
- `inkwell::OptimizationLevel`
- `inkwell::context::Context`

### 2. Visibility Restriction

In `src/bir/lowering.rs`, change:

| Function | From | To |
|----------|------|-----|
| `lower_program_with_inferred` | `pub` | `pub(crate)` |
| `lower_module_with_inferred` | `pub` | `pub(crate)` |

Functions kept as `pub`:
- `lower_program` — unit test wrapper, re-exported via `bir/mod.rs`
- `lower_module` — unit test wrapper, re-exported via `bir/mod.rs`

No changes needed in `bir/mod.rs` since it only re-exports `lower_program` and `lower_module`.

## Test Strategy

No new tests required. The existing ~340 tests serve as regression tests. All must pass after the change.

After implementation, measure `cargo test` wall-clock time before and after to quantify the JIT → native impact.

## Changed Files

| File | Change |
|------|--------|
| `tests/common/mod.rs` | Rewrite `compile_and_run` as delegation; remove JIT imports |
| `src/bir/lowering.rs` | Change `lower_program_with_inferred` and `lower_module_with_inferred` to `pub(crate)` |

## Not Changed

- `src/bir/mod.rs` — re-exports only `lower_program` and `lower_module` (unchanged)
- `src/pipeline.rs` — uses `lower_module_with_inferred` which remains accessible as `pub(crate)`
- `src/lib.rs` — test uses `lower_program` (unchanged)
- All test files (`tests/*.rs`) — call `compile_and_run` which still has the same signature
