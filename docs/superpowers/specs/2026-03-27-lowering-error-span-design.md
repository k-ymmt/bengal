# Add Span to LoweringError

## Overview

Add source location (Span) to `LoweringError` so that lowering-phase errors can pinpoint the exact source position. Also add `Span` fields to `StructDef` and `Function` AST nodes to support this.

## Scope

- Add `span: Option<Span>` to `LoweringError` variant in `BengalError`
- Add `span: Span` to `StructDef` and `Function` AST nodes
- Update parser to record Span for struct definitions and functions
- Update lowering to pass Span into errors
- Update all synthetic `Function`/`StructDef` construction sites
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

Update the `into_diagnostic` match arm for `LoweringError`: when `span` is `Some`, produce a `SourceSpan` (same pattern as `SemanticError`); when `None`, set `span: None` (current behavior).

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
- Single-expression wrapper in `parse` (wraps bare expression in synthetic `Function`): use `Span { start: 0, end: source.len() }` or the expression's own span

### 4. Lowering Changes

In `src/bir/lowering.rs`:

**`record_error` method:** Add `span: Option<Span>` parameter:

```rust
fn record_error(&mut self, message: impl Into<String>, span: Option<Span>) -> Value
```

All `record_error` call sites (in `lower_expr`, `lower_stmt`, and init/method body lowering) pass `Some(expr.span)` where an `Expr` is available. Errors from expressions inside init bodies get their span from the expression, not from `StructMember::Initializer`.

**Struct validation errors:** Build a name-to-span lookup before the validation loop:

```rust
let struct_spans: HashMap<&str, Span> = program.structs.iter()
    .map(|s| (s.name.as_str(), s.span))
    .collect();
```

Then look up `struct_spans[&name]` when constructing `LoweringError` for Unit field and recursive struct errors.

**`check_acyclic_structs`:** Add a `struct_spans: &HashMap<&str, Span>` parameter. The inner `fn visit()` also receives this parameter so it can include the span of the offending struct in the cycle error.

**Synthetic `Function` construction:** Two sites in lowering.rs construct synthetic `Function` structs when flattening struct methods into top-level functions. Use the parent `StructDef.span` for these (the method's own Span doesn't exist since methods aren't top-level `Function` nodes in the AST).

### 5. All LoweringError Construction Sites

Every place that constructs `BengalError::LoweringError` must now include `span`:

- `record_error` calls → `span` parameter (from `Expr.span` at each call site)
- Unit field validation in `lower_program_with_inferred` → `StructDef.span` via `struct_spans` map
- Unit field validation in `lower_module_with_inferred` → same
- Recursive struct in `check_acyclic_structs` → `struct_spans` map

## Changed Files

| File | Change |
|------|--------|
| `src/error.rs` | Add `span: Option<Span>` to `LoweringError`; update `into_diagnostic` |
| `src/parser/ast.rs` | Add `span: Span` to `StructDef` and `Function` |
| `src/parser/mod.rs` | Record Span in `parse_struct_def`, `parse_function`, and single-expression wrapper |
| `src/bir/lowering.rs` | Add Span to `record_error`; build `struct_spans` map; pass Span in struct validation; update `check_acyclic_structs` and inner `visit`; set Span on synthetic `Function` constructions |
| `TODO.md` | Add CodegenError Span as future task |

## Test Strategy

- All existing tests must pass (regression)
- Existing lowering error tests (`lower_err_recursive_struct`, `lower_err_read_before_init`, etc.) continue to pass
- Add a new test that constructs a recursive struct, calls `lower_program`, and destructures the error to verify `span.is_some()`:

```rust
match result {
    Err(BengalError::LoweringError { span, .. }) => {
        assert!(span.is_some(), "LoweringError should include span");
    }
    other => panic!("expected LoweringError, got {:?}", other),
}
```

## Future Work

- CodegenError Span: requires adding Span/NodeId to BIR instructions, which is a prerequisite for debug info (DWARF) generation. Design as a separate project.
