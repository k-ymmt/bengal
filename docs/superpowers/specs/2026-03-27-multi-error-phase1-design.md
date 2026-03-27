# Multiple Error Reporting — Phase 1: DiagCtxt Foundation

## Overview

Introduce a `DiagCtxt` (diagnostic context) for accumulating multiple compilation errors, following the Rust compiler's architecture. Phase 1 creates the foundation: the `DiagCtxt` struct and pipeline plumbing. Error collection logic is deferred to subsequent phases.

## Scope

- Create `DiagCtxt` struct in `src/error.rs`
- Thread `&mut DiagCtxt` through pipeline functions (`analyze`, `lower`, `monomorphize`, `codegen`)
- Public API (`lib.rs`) and CLI (`main.rs`) create `DiagCtxt` internally
- No changes to error handling behavior — each phase still returns on first error

## Context: Multi-Error Reporting Phases

| Phase | Sub-project | Status |
|-------|------------|--------|
| **1** | **DiagCtxt foundation + pipeline plumbing** | **This spec** |
| 2 | Semantic analysis multi-error | Future |
| 3 | Type inference integration | Future |
| 4 | Lowering multi-error | Future |
| 5 | Codegen module-level collection | Future |
| 6 | CLI / public API integration | Future |

## Design

### 1. DiagCtxt Struct (`src/error.rs`)

```rust
pub struct DiagCtxt {
    errors: Vec<BengalError>,
    limit: usize,
}

impl DiagCtxt {
    /// Create a new diagnostic context with the default error limit (128).
    pub fn new() -> Self

    /// Emit an error. Returns false if limit has been reached.
    pub fn emit(&mut self, err: BengalError) -> bool

    /// Number of errors emitted so far.
    pub fn error_count(&self) -> usize

    /// Whether any errors have been emitted.
    pub fn has_errors(&self) -> bool

    /// Consume the context. Returns Err with all errors if any were emitted, Ok(()) otherwise.
    pub fn finish(self) -> std::result::Result<(), Vec<BengalError>>

    /// Take all collected errors, leaving the context empty.
    pub fn take_errors(&mut self) -> Vec<BengalError>
}
```

Default limit: 128 (matches Rust compiler). `emit` stops collecting when limit is reached but returns false so callers can detect this.

Uses `&mut DiagCtxt` (no interior mutability needed since pipeline phases run sequentially).

### 2. Pipeline Plumbing (`src/pipeline.rs`)

Add `diag: &mut DiagCtxt` parameter to these functions:

```rust
pub fn analyze(parsed: ParsedPackage, diag: &mut DiagCtxt) -> Result<AnalyzedPackage, PipelineError>
pub fn lower(analyzed: AnalyzedPackage, diag: &mut DiagCtxt) -> Result<LoweredPackage, PipelineError>
pub fn monomorphize(lowered: LoweredPackage, diag: &mut DiagCtxt) -> Result<MonomorphizedPackage, PipelineError>
pub fn codegen(mono: MonomorphizedPackage, diag: &mut DiagCtxt) -> Result<CompiledPackage, PipelineError>
```

Functions NOT changed (rationale):
- `parse` / `parse_source` — parse errors are fatal (no recovery without error tokens)
- `optimize` — never fails
- `link` — external process, single pass

In Phase 1, the `diag` parameter is accepted but **not used** inside these functions. Existing `?`-based early return behavior is preserved.

### 3. Public API (`src/lib.rs`)

All public functions create `DiagCtxt::new()` internally and pass it through:

```rust
pub fn compile_to_executable(entry_path: &Path, output_path: &Path) -> Result<(), PipelineError> {
    let mut diag = DiagCtxt::new();
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze(parsed, &mut diag)?;
    let lowered = pipeline::lower(analyzed, &mut diag)?;
    let optimized = pipeline::optimize(lowered);
    let mono = pipeline::monomorphize(optimized, &mut diag)?;
    let compiled = pipeline::codegen(mono, &mut diag)?;
    pipeline::link(compiled, output_path)
}
```

Same pattern for `compile_to_bir`, `compile_source_to_bir`, `compile_to_objects`, `compile_source_to_objects`.

### 4. CLI (`src/main.rs`)

The `compile` subcommand creates `DiagCtxt::new()` and passes it to pipeline functions. The `eval` subcommand uses `lib.rs` public API (which handles DiagCtxt internally).

## Changed Files

| File | Change |
|------|--------|
| `src/error.rs` | Add `DiagCtxt` struct with `new`, `emit`, `error_count`, `has_errors`, `finish`, `take_errors` |
| `src/pipeline.rs` | Add `diag: &mut DiagCtxt` parameter to `analyze`, `lower`, `monomorphize`, `codegen`; update internal tests |
| `src/lib.rs` | Create `DiagCtxt::new()` in each public function; pass to pipeline |
| `src/main.rs` | Create `DiagCtxt::new()` in `compile` subcommand; pass to pipeline |

## Not Changed

- `tests/common/mod.rs` — uses `lib.rs` public API which handles DiagCtxt internally
- All test files (`tests/*.rs`) — no signature changes visible externally
- `src/bir/lowering.rs` — DiagCtxt integration deferred to Phase 4
- `src/semantic/mod.rs` — DiagCtxt integration deferred to Phase 2

## Test Strategy

### DiagCtxt unit tests in `src/error.rs`

- `emit` adds errors, `error_count` returns correct count
- `has_errors` returns true after emit, false before
- `finish` returns `Ok(())` when no errors, `Err(vec)` when errors exist
- `take_errors` returns errors and leaves context empty
- Error limit: after 128 emits, further emits return false and are not stored
- Empty DiagCtxt has `error_count() == 0` and `has_errors() == false`

### Regression

All existing tests pass unchanged (pipeline behavior is identical in Phase 1).
