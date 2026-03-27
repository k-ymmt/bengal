# Multiple Error Reporting — Phase 2b: Top-Level Item Continuation

## Overview

Make semantic analysis continue past errors at the top-level item boundary. When analyzing a function/struct/protocol fails, emit the error to `DiagCtxt` and proceed to the next item instead of stopping.

## Scope

- Add `diag: &mut DiagCtxt` to `analyze_single_module` and `analyze_package`
- Convert Pass 2 (main check), Pass 3 (struct members), Pass 3b (protocol conformance), Pass 3c (function bodies) to emit + continue
- Pass 1a/1b remain fatal (name registration and type resolution are prerequisites)
- Wire `pipeline.rs:analyze()` to pass `diag` to `analyze_package`

## Context

| Phase | Status |
|-------|--------|
| 1: DiagCtxt foundation | Done |
| 2a: Type::Error foundation | Done |
| **2b: Top-level item continuation** | **This spec** |
| 2c: Expression-level emit + Error return | Future |
| 2d: Pipeline module-level continuation | Future |

## Design

### 1. Signature Changes

```rust
// src/semantic/mod.rs
fn analyze_single_module(
    program: &Program,
    resolver: &mut Resolver,
    is_root: bool,
    diag: &mut DiagCtxt,  // NEW
) -> Result<SemanticInfo>

pub fn analyze_package(
    graph: &ModuleGraph,
    package_name: &str,
    diag: &mut DiagCtxt,  // NEW
) -> Result<PackageSemanticInfo>
```

### 2. Pass-by-Pass Changes in `analyze_single_module`

**Pass 1a (name registration) — NO CHANGE.** Fatal errors. Duplicate names, unresolved types in definitions. Must abort because Pass 1b/2/3 depend on registered names.

**Pass 1b (type resolution) — NO CHANGE.** Fatal errors. Struct field type resolution, method name collision checks. Must abort because Pass 3 depends on resolved types.

**Pass 2 (main function check) — EMIT + CONTINUE.** Missing/malformed `main` is an error but doesn't prevent analyzing function bodies.

```rust
// Before:
if is_root {
    // ... check main ...
    return Err(sem_err(...));
}

// After:
if is_root {
    // ... check main ...
    diag.emit(sem_err(...));
    // continue — don't return
}
```

**Pass 3 (struct member analysis) — EMIT + CONTINUE per struct.**

```rust
// Before:
for struct_def in &program.structs {
    if /* skip generic */ { continue; }
    analyze_struct_members(struct_def, resolver, &mut infer_ctx)?;
    let _ = infer_ctx.apply_defaults();
    infer_ctx.reset();
}

// After:
for struct_def in &program.structs {
    if /* skip generic */ { continue; }
    if let Err(e) = analyze_struct_members(struct_def, resolver, &mut infer_ctx) {
        diag.emit(e);
    }
    let _ = infer_ctx.apply_defaults();
    infer_ctx.reset();
}
```

**Pass 3b (protocol conformance) — EMIT + CONTINUE per struct.**

The protocol conformance loop has multiple `return Err(...)` sites within nested loops (per struct, per protocol, per method/property). Convert each to `diag.emit(...)` and `continue` to the next item. Use a flag to track whether the current struct had errors.

```rust
// Before:
for struct_def in &program.structs {
    for proto_name in &struct_def.conformances {
        // ... check methods ...
        return Err(sem_err(...));  // multiple sites
    }
}

// After:
for struct_def in &program.structs {
    for proto_name in &struct_def.conformances {
        // ... check methods ...
        // At each error site: diag.emit(sem_err(...)); continue;
        // or: diag.emit(sem_err_with_help(...)); continue;
    }
}
```

**Pass 3c (function bodies) — EMIT + CONTINUE per function.**

```rust
// Before:
for func in &program.functions {
    analyze_function(func, resolver, Some(&mut ctx))?;
    let errs = ctx.apply_defaults();
    if let Some(e) = errs.into_iter().next() {
        return Err(e);
    }
    ctx.reset();
}

// After:
for func in &program.functions {
    if let Err(e) = analyze_function(func, resolver, Some(&mut ctx)) {
        diag.emit(e);
    }
    let errs = ctx.apply_defaults();
    for e in errs {
        diag.emit(e);  // emit ALL inference errors, not just first
    }
    ctx.reset();
}
```

### 3. Final Error Check

At the end of `analyze_single_module`, after all passes complete, check if DiagCtxt has errors. If so, return a sentinel error to signal that the module had issues (prevents passing invalid data to lowering):

```rust
if diag.has_errors() {
    return Err(BengalError::SemanticError {
        message: format!("{} error(s) found", diag.error_count()),
        span: Span { start: 0, end: 0 },
        help: None,
    });
}
```

Note: The actual errors are already in `diag`. This sentinel error is caught by the pipeline and discarded — the real errors come from `diag`.

### 4. Pipeline Wiring (`src/pipeline.rs`)

In `analyze()`, rename `_diag` to `diag` and pass to `analyze_package`:

```rust
let pkg_sem_info = crate::semantic::analyze_package(&parsed.graph, &parsed.package_name, diag)
    .map_err(|e| crate::error::PipelineError::package("analyze", e))?;
```

### 5. `analyze_package` Changes

Pass `diag` through to `analyze_single_module`:

```rust
let sem_info = analyze_single_module(&mod_info.ast, &mut resolver, is_root, diag)?;
```

### 6. Other Callers of `analyze_single_module`

Search for all call sites. `analyze_single_module` is also called from:
- `analyze_pre_mono` and `analyze_pre_mono_lenient` and `analyze_post_mono` — these are used by tests and the pre-mono inference pass. They need `diag` parameter too, or create a local `DiagCtxt`.

For test-facing functions (`analyze_pre_mono`, `analyze_post_mono`), create a local `DiagCtxt::new()` internally (same pattern as `lib.rs` public API).

## Changed Files

| File | Change |
|------|--------|
| `src/semantic/mod.rs` | Add `diag` to `analyze_single_module`, `analyze_package`; convert Pass 2/3/3b/3c to emit+continue; update `analyze_pre_mono`/`analyze_post_mono`/`analyze_pre_mono_lenient` to create local DiagCtxt |
| `src/pipeline.rs` | Rename `_diag` to `diag` in `analyze()`; pass to `analyze_package` |

## Test Strategy

### New test: multiple errors reported

```rust
#[test]
fn multiple_errors_reported() {
    // Source with errors in two separate functions
    let source = r#"
        func foo() -> Int32 { return true; }
        func bar() -> Int32 { return true; }
        func main() -> Int32 { return 1; }
    "#;
    // Both foo and bar have type errors — previously only foo's error was reported
    // Now both should be caught
}
```

Verify by checking that `DiagCtxt` (or pipeline error output) contains 2+ errors.

### Regression

All existing tests pass. Single-error behavior is preserved (first error is still reported; now additional errors are also reported).
