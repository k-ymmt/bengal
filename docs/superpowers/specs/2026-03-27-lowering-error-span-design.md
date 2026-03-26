# Add Span to LoweringError

## Overview

Add source location (Span) to `LoweringError` so that lowering-phase errors can pinpoint the exact source position. Also add `Span` fields to `StructDef` and `Function` AST nodes to support this.

## Scope

- Add `span: Option<Span>` to `LoweringError` variant in `BengalError`
- Add `span: Span` to `StructDef` and `Function` AST nodes
- Update parser to record Span for struct definitions and functions
- Update lowering to pass Span into errors
- CodegenError Span is out of scope — deferred as a future task (requires BIR Span/debug info design)

## Design

### 1. Error Type Change

In `src/error.rs`, change `LoweringError`:

```rust
// Before
LoweringError { message: String },

// After
LoweringError { message: String, span: Option<Span> },
```

`Option<Span>` because some error sites may not have source location available.

Update the `into_diagnostic` match arm for `LoweringError` to produce a `SourceSpan` when `span` is `Some`, matching the pattern used by `SemanticError`.

### 2. AST Span Addition

In `src/parser/ast.rs`, add `span: Span` to:

```rust
pub struct StructDef {
    // ... existing fields ...
    pub span: Span,
}

pub struct Function {
    // ... existing fields ...
    pub span: Span,
}
```

### 3. Parser Changes

In `src/parser/mod.rs`:

- `parse_struct_def`: record start position before parsing, compute `Span { start, end }` after parsing completes, set on `StructDef`
- `parse_function`: same pattern — record start, compute Span after body is parsed, set on `Function`

### 4. Lowering Changes

In `src/bir/lowering.rs`:

**`record_error` method:** Add `span: Option<Span>` parameter:

```rust
fn record_error(&mut self, message: impl Into<String>, span: Option<Span>) -> Value
```

All call sites in `lower_expr` pass `Some(expr.span)`.

**Struct validation errors (Unit field, recursive struct):** Look up the corresponding `StructDef` from `program.structs` to obtain `StructDef.span`. Pass it into `LoweringError`.

**`check_acyclic_structs`:** Add a parameter for struct name → Span mapping so that cycle errors can include the source location of the offending struct.

### 5. All LoweringError Construction Sites

Every place that constructs `BengalError::LoweringError` must now include `span`:

- `record_error` calls → `span` parameter (from `Expr.span`)
- Unit field validation in `lower_program_with_inferred` → `StructDef.span` from `program.structs`
- Unit field validation in `lower_module_with_inferred` → same
- Recursive struct in `check_acyclic_structs` → struct name → Span map

## Changed Files

| File | Change |
|------|--------|
| `src/error.rs` | Add `span: Option<Span>` to `LoweringError`; update `into_diagnostic` |
| `src/parser/ast.rs` | Add `span: Span` to `StructDef` and `Function` |
| `src/parser/mod.rs` | Record Span in `parse_struct_def` and `parse_function` |
| `src/bir/lowering.rs` | Add Span to `record_error`; pass Span in struct validation; update `check_acyclic_structs` |
| `TODO.md` | Add CodegenError Span as future task |

## Test Strategy

- All existing tests must pass (regression)
- Existing lowering error tests (`lower_err_recursive_struct`, `lower_err_read_before_init`, etc.) continue to pass
- Add a new test verifying that `LoweringError` includes a `Span` (e.g., recursive struct error returns error with non-None span)

## Future Work

- CodegenError Span: requires adding Span/NodeId to BIR instructions, which is a prerequisite for debug info (DWARF) generation. Design as a separate project.
