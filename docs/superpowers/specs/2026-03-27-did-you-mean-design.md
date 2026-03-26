# Error Correction Suggestions ("did you mean ...?")

## Overview

Add "did you mean ...?" suggestions to semantic errors for undefined variables, functions, structs, protocols, struct fields, and methods. Uses Levenshtein edit distance to find the closest match among in-scope names.

## Scope

- New `src/suggest.rs` module with `edit_distance` and `find_suggestion`
- Add `help: Option<String>` to `SemanticError` and `BengalDiagnostic`
- Integrate suggestions into all "undefined"/"unknown"/"has no" error sites in `src/semantic/mod.rs`

## Design

### 1. Similarity Calculation (`src/suggest.rs`)

New module with two public functions:

```rust
/// Compute Levenshtein edit distance between two strings.
pub fn edit_distance(a: &str, b: &str) -> usize

/// Find the best match for `name` among `candidates`.
/// Returns Some(candidate) if edit_distance <= max(name.len(), candidate.len()) / 3.
pub fn find_suggestion<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str>
```

`find_suggestion` scans all candidates, computes edit distance for each, and returns the candidate with the smallest distance that is within the threshold. Ties go to the first match found.

Standard Levenshtein implementation using a 2-row DP matrix (O(n*m) time, O(min(n,m)) space). No external crate needed.

Add `pub mod suggest;` to `src/lib.rs`.

### 2. Error Type Changes

#### `src/error.rs`

Add `help: Option<String>` to `SemanticError`:

```rust
SemanticError { message: String, span: Span, help: Option<String> },
```

Add `help: Option<String>` to `BengalDiagnostic` with miette's `#[help]` attribute:

```rust
#[derive(Debug, Diagnostic, Error)]
#[error("{message}")]
pub struct BengalDiagnostic {
    pub message: String,
    #[source_code]
    pub src_code: NamedSource<String>,
    #[label("{label}")]
    pub span: Option<SourceSpan>,
    pub label: String,
    #[help]
    pub help: Option<String>,
}
```

Update `into_diagnostic` for `SemanticError` to pass `help` through:

```rust
BengalError::SemanticError { message, span, help } => BengalDiagnostic {
    message,
    src_code: source,
    span: Some(SourceSpan::new(span.start.into(), span.end - span.start)),
    label: "here".to_string(),
    help,
},
```

All other variants set `help: None`.

#### Blast radius of adding `help` to `SemanticError`

`BengalError::SemanticError` is constructed in 4 files:

- `src/semantic/mod.rs` — via `sem_err()` helper (~162 sites). Only `sem_err` needs `help: None` added.
- `src/semantic/infer.rs` — 3 direct construction sites (lines 8, 310, 436). Add `help: None`.
- `src/semantic/resolver.rs` — 1 direct construction site (line 229). Add `help: None`.
- `src/error.rs` — only in `into_diagnostic` (pattern match, not construction).

### 3. Semantic Analyzer Integration

#### Helper functions in `src/semantic/mod.rs`

Keep the existing `sem_err` signature (message only, hardcoded zero span, `help: None`). Add a new helper for errors with suggestions:

```rust
fn sem_err(message: impl Into<String>) -> BengalError {
    BengalError::SemanticError {
        message: message.into(),
        span: Span { start: 0, end: 0 },
        help: None,
    }
}

fn sem_err_with_help(message: impl Into<String>, span: Span, help: Option<String>) -> BengalError {
    BengalError::SemanticError {
        message: message.into(),
        span,
        help,
    }
}
```

Existing `sem_err` call sites are NOT changed (they continue to pass only a message). Only "undefined" error sites that should show suggestions switch to `sem_err_with_help`.

#### Candidate sources per error type

| Error pattern | Candidates |
|--------------|-----------|
| undefined variable | All entries in `resolver.scopes` (all scope levels) |
| undefined function | `resolver.functions` keys + `resolver.imported_funcs` keys |
| undefined struct / undefined type | `resolver.struct_defs` keys + `resolver.imported_structs` keys |
| unknown protocol | `resolver.protocol_defs` keys + `resolver.imported_protocols` keys |
| has no field | `struct_info.field_index` keys + `struct_info.computed_index` keys |
| has no method | `struct_info.method_index` keys |

#### Resolver accessor methods

Add methods to `Resolver` in `src/semantic/resolver.rs`:

```rust
pub fn all_variable_names(&self) -> impl Iterator<Item = &str>
pub fn all_function_names(&self) -> impl Iterator<Item = &str>
pub fn all_struct_names(&self) -> impl Iterator<Item = &str>
pub fn all_protocol_names(&self) -> impl Iterator<Item = &str>
```

Each method chains local + imported name collections. For fields/methods, candidates come from `StructInfo` directly (already available at error sites as a local variable), not through Resolver.

#### Suggestion integration pattern

At each "undefined"/"unknown"/"has no" error site:

1. Collect candidate names from the appropriate source
2. Call `find_suggestion(unknown_name, candidates)`
3. If `Some(suggestion)`, create `help: Some(format!("did you mean '{suggestion}'?"))`
4. Use `sem_err_with_help(message, span, help)` instead of `sem_err(message)`

Note: many existing error sites use `sem_err` with a zero span. Where the `Expr.span` is available, pass it to `sem_err_with_help`. Where no span is available, pass `Span { start: 0, end: 0 }`.

## Changed Files

| File | Change |
|------|--------|
| `src/suggest.rs` | **New** — `edit_distance`, `find_suggestion` |
| `src/lib.rs` | Add `pub mod suggest;` |
| `src/error.rs` | Add `help` to `SemanticError` and `BengalDiagnostic`; update `into_diagnostic` |
| `src/semantic/mod.rs` | Update `sem_err` to include `help: None`; add `sem_err_with_help`; integrate `find_suggestion` at all "undefined"/"unknown"/"has no" error sites |
| `src/semantic/resolver.rs` | Add accessor methods (`all_variable_names`, etc.); add `help: None` to direct SemanticError construction |
| `src/semantic/infer.rs` | Add `help: None` to 3 direct SemanticError construction sites |

## Test Strategy

### Unit tests in `src/suggest.rs`

- `edit_distance("", "")` = 0
- `edit_distance("abc", "abc")` = 0
- `edit_distance("abc", "abd")` = 1
- `edit_distance("abc", "abcd")` = 1
- `edit_distance("kitten", "sitting")` = 3
- `find_suggestion("fo", ["foo", "bar"])` = Some("foo")
- `find_suggestion("xyz", ["foo", "bar"])` = None (too distant)

### Integration tests

Add tests in existing test files that compile source with typos and verify the error contains "did you mean":

- Undefined variable typo: `fob` when `foo` is defined → "did you mean 'foo'?"
- Undefined function typo: `ad` when `add` is defined → "did you mean 'add'?"
- Undefined struct typo: `Pint` when `Point` is defined → "did you mean 'Point'?"
- Unknown field typo: `p.xx` when `Point` has `x` → "did you mean 'x'?"
- No suggestion for completely unrelated name → no "did you mean" in output
- All existing tests pass (regression)
