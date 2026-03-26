# Error Correction Suggestions ("did you mean ...?")

## Overview

Add "did you mean ...?" suggestions to semantic errors for undefined variables, functions, structs, protocols, and struct fields. Uses Levenshtein edit distance to find the closest match among in-scope names.

## Scope

- New `src/suggest.rs` module with `edit_distance` and `find_suggestion`
- Add `help: Option<String>` to `SemanticError` and `BengalDiagnostic`
- Integrate suggestions into all "undefined" error sites in `src/semantic/mod.rs`

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

### 2. Error Type Changes

#### `src/error.rs`

Add `help: Option<String>` to `SemanticError`:

```rust
SemanticError { message: String, span: Span, help: Option<String> },
```

Add `help: Option<String>` to `BengalDiagnostic`:

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

In `into_diagnostic`, `SemanticError` passes `help` through to `BengalDiagnostic.help`. All other variants set `help: None`.

### 3. Semantic Analyzer Integration

#### Helper functions in `src/semantic/mod.rs`

Keep the existing `sem_err` for errors without suggestions (add `help: None`). Add `sem_err_with_help`:

```rust
fn sem_err(message: impl Into<String>, span: Span) -> BengalError {
    BengalError::SemanticError { message: message.into(), span, help: None }
}

fn sem_err_with_help(message: impl Into<String>, span: Span, help: Option<String>) -> BengalError {
    BengalError::SemanticError { message: message.into(), span, help }
}
```

#### Candidate sources per error type

| Error | Candidates |
|-------|-----------|
| undefined variable | All entries in `resolver.scopes` (all scope levels) |
| undefined function | `resolver.functions` keys + `resolver.imported_funcs` keys |
| undefined struct | `resolver.struct_defs` keys + `resolver.imported_structs` keys |
| unknown protocol | `resolver.protocol_defs` keys + `resolver.imported_protocols` keys |
| unknown field | `struct_info.field_index` keys + `struct_info.computed_index` keys |

#### Suggestion integration pattern

At each "undefined" error site:

1. Collect candidate names from the appropriate scope
2. Call `find_suggestion(unknown_name, candidates)`
3. If `Some(suggestion)`, create `help: Some(format!("did you mean '{suggestion}'?"))`
4. Use `sem_err_with_help` instead of `sem_err`

### 4. Resolver Method for Variable Candidates

Add a method to `Resolver` to collect all variable names across scopes:

```rust
fn all_variable_names(&self) -> impl Iterator<Item = &str>
```

This iterates all scopes (inner to outer) and yields each variable name. Used by the "undefined variable" error sites.

## Changed Files

| File | Change |
|------|--------|
| `src/suggest.rs` | **New** — `edit_distance`, `find_suggestion` |
| `src/lib.rs` | Add `pub mod suggest;` |
| `src/error.rs` | Add `help` to `SemanticError` and `BengalDiagnostic`; update `into_diagnostic` |
| `src/semantic/mod.rs` | Add `sem_err_with_help`; integrate `find_suggestion` at all "undefined" error sites |
| `src/semantic/resolver.rs` | Add `all_variable_names` method |

## Test Strategy

### Unit tests in `src/suggest.rs`

- `edit_distance("", "")` = 0
- `edit_distance("abc", "abc")` = 0
- `edit_distance("abc", "abd")` = 1
- `edit_distance("abc", "abcd")` = 1
- `edit_distance("kitten", "sitting")` = 3
- `find_suggestion("fo", ["foo", "bar"])` = Some("foo")
- `find_suggestion("xyz", ["foo", "bar"])` = None (too distant)
- `find_suggestion("foob", ["foo", "foobar"])` = Some("foo") (closer match)

### Integration tests

Add tests that compile source with typos and verify the error message contains "did you mean":

- Undefined variable typo: `fob` when `foo` is defined → "did you mean 'foo'?"
- Undefined function typo: `ad` when `add` is defined → "did you mean 'add'?"
- Undefined struct typo: `Pint` when `Point` is defined → "did you mean 'Point'?"
- No suggestion for completely unrelated name → no "did you mean" in output
- All existing tests pass (regression)
