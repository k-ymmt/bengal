# Multiple Error Reporting — Phase 2c: Expression-Level Emit + Error Return

## Overview

Convert `analyze_expr` and `analyze_stmt` from `Result<Type>`/`Result<()>` to `Type`/`()`, emitting errors to `DiagCtxt` and returning `Type::Error` on failure. This enables collecting multiple errors within a single function body.

## Scope

- Change `analyze_expr` return type from `Result<Type>` to `Type`
- Change `analyze_stmt` return type from `Result<()>` to `()`
- Thread `diag: &mut DiagCtxt` through all expression/statement analysis functions
- Replace all `?` operator usage with `diag.emit()` + `Type::Error` return
- Add cascading error suppression: skip type checks when operands are `Type::Error`

## Context

| Phase | Status |
|-------|--------|
| 1: DiagCtxt foundation | Done |
| 2a: Type::Error foundation | Done |
| 2b: Top-level item continuation | Done |
| **2c: Expression-level emit + Error return** | **This spec** |
| 2d: Pipeline module-level continuation | Future |

## Design

### 1. Core Signature Changes

```rust
// analyze_expr: Result<Type> → Type
fn analyze_expr(expr: &Expr, resolver: &mut Resolver, ctx: ..., diag: &mut DiagCtxt) -> Type

// analyze_stmt: Result<()> → ()
fn analyze_stmt(stmt: &Stmt, resolver: &mut Resolver, ctx: ..., diag: &mut DiagCtxt) -> ()

// analyze_function: Result<()> → ()
fn analyze_function(func: &Function, resolver: &mut Resolver, ctx: ..., diag: &mut DiagCtxt) -> ()

// analyze_struct_members: Result<()> → ()
fn analyze_struct_members(struct_def: &StructDef, resolver: &mut Resolver, ctx: ..., diag: &mut DiagCtxt) -> ()
```

Functions that remain `Result`:
- `check_block_always_returns` — flow analysis, not type checking. Stays `Result<()>`.
- `analyze_single_module` — returns `Result<SemanticInfo>` (sentinel error if `diag.has_errors()`)

### 2. Error Emission Pattern

Every `return Err(sem_err(...))` or `?` usage becomes:

```rust
// Before:
let ty = analyze_expr(expr, resolver, ctx)?;

// After:
let ty = analyze_expr(expr, resolver, ctx, diag);
```

```rust
// Before:
return Err(sem_err("undefined variable"));

// After:
diag.emit(sem_err("undefined variable"));
return Type::Error;
```

```rust
// Before (in analyze_stmt):
return Err(sem_err("type mismatch"));

// After:
diag.emit(sem_err("type mismatch"));
return;
```

### 3. Cascading Error Suppression

When an operand is `Type::Error`, skip further type checks to avoid noisy cascading errors:

```rust
let lhs_ty = analyze_expr(lhs, resolver, ctx, diag);
let rhs_ty = analyze_expr(rhs, resolver, ctx, diag);
if lhs_ty == Type::Error || rhs_ty == Type::Error {
    return Type::Error;
}
// ... proceed with type checking only if both types are valid ...
```

This pattern applies to:
- Binary operations (arithmetic, comparison, logical)
- Function call argument type checking
- Assignment type checking
- Return type checking
- If/else branch type matching
- Struct field type checking
- Array element type checking
- Cast operations

### 4. Block Analysis

`analyze_block` (if it exists as a separate function) returns the type of the last expression. If any statement emits an error, the block continues. The block's type is determined by the last expression's type (which may be `Type::Error`).

### 5. Callers in `analyze_single_module`

Phase 2b already converted the top-level loops to use `if let Err(e)`. After Phase 2c, `analyze_function` and `analyze_struct_members` no longer return `Result`, so the `if let Err` wrappers are removed — they just call the function directly (errors go to `diag`):

```rust
// Phase 2b:
if let Err(e) = analyze_function(func, resolver, Some(&mut ctx)) {
    diag.emit(e);
}

// Phase 2c:
analyze_function(func, resolver, Some(&mut ctx), diag);
// errors already emitted to diag inside
```

### 6. `check_block_always_returns` Integration

This function checks whether all code paths return a value. It returns `Result<()>`. Since it's a flow analysis (not type checking), it keeps returning `Result<()>`. Its callers emit the error to diag:

```rust
if let Err(e) = check_block_always_returns(&func.body, ...) {
    diag.emit(e);
}
```

## Changed Files

| File | Change |
|------|--------|
| `src/semantic/mod.rs` | Change signatures of analyze_expr/analyze_stmt/analyze_function/analyze_struct_members; replace all `?` with emit+Error; add cascading suppression; update callers in analyze_single_module |

## Test Strategy

### New test: multiple errors within one function

```rust
#[test]
fn multiple_errors_in_single_function() {
    // Source with multiple type errors in one function body
    let source = r#"
        func main() -> Int32 {
            let x: Int32 = true;
            let y: Bool = 42;
            return 0;
        }
    "#;
    // Both let bindings have type errors — both should be reported
}
```

### Regression

All existing tests pass. Error messages remain the same (just more of them reported now).
