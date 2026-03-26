# Type Inference Error Improvement Design

## Overview

Improve error reporting for two type inference failure modes in the Bengal compiler:

1. **Unresolvable type parameters** — `default_value<T>()` called without enough context to
   infer `T` currently panics via `unreachable!()` instead of returning a clean diagnostic.
2. **Literal type conflicts** — `choose(42, 3.14)` where `T` receives conflicting integer/float
   constraints surfaces as "undefined function" instead of a unification error.

Both issues stem from the same root cause: `analyze_pre_mono` silently discards inference errors,
allowing downstream code to encounter unresolved type variables.

## Goals

1. Replace the `unreachable!` panic with a clean error:
   `cannot infer type parameter 'T' for function 'default_value'; add explicit type annotation`
2. Surface literal type conflicts with full context:
   `type parameter 'T' in function 'choose' has conflicting constraints: integer literal (from argument 'a') vs float literal (from argument 'b')`
3. Collect errors across all functions instead of stopping at the first failure.
4. Attach accurate source spans to inference errors.

## Non-Goals

- Multi-error display infrastructure (the compiler currently returns a single `BengalError`;
  we return the first collected error for now and leave `BengalError::Multiple` for later).
- Cross-function type inference.
- Changing the unification algorithm itself.

## Approach: Provenance Tracking in InferenceContext

### Why This Approach

When an InferVar is created for a type parameter at a call site, we record *where it came from*
(type parameter name, function name, argument name, source span). This metadata — called
**provenance** — is then available when errors occur, enabling detailed diagnostics without
changing the `unify()` signature or scattering `.map_err()` wrappers across every call site.

### Alternatives Considered

**B. Error wrapping at call sites:** Each `unify()` call site wraps errors with `.map_err()`.
Simpler initially but doesn't scale: every new language feature that calls `unify()` needs its
own wrapper, and omissions degrade error quality silently.

**C. Hybrid (provenance for apply_defaults, wrapping for unify):** Uses the best tool for each
case but introduces two patterns. Rejected in favor of a unified approach.

## Detailed Design

### 1. Add `span` to `Expr`

The AST `Expr` struct currently has no source location. Add a `span` field:

```rust
// parser/ast.rs
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,  // NEW
}
```

The parser's `expr()` factory method captures span from the current token position.
Test helpers use `Span { start: 0, end: 0 }`.

**Ripple effect:** Adding `span` to `Expr` requires updating every site that constructs
an `Expr` struct literal:

- `src/parser/mod.rs`: The `expr()` helper (covers most cases) plus test helpers
  (`e()`, `normalize_expr()`) and pattern matches using `Expr { .. }`.
- `src/monomorphize.rs`: All `Expr { id, kind }` constructions in expression rewrite
  functions. These should propagate `span` from the source expression being rewritten
  (the input `&Expr` is already available at each rewrite site).
- Pattern matches across test files: Use `..` rest syntax to avoid breakage.

### 2. VarProvenance

```rust
// semantic/infer.rs
#[derive(Debug, Clone)]
pub struct VarProvenance {
    pub type_param_name: String,
    pub def_name: String,
    pub arg_name: Option<String>,
    pub span: Span,
}
```

Added to `InferenceContext`:

```rust
pub struct InferenceContext {
    var_states: Vec<VarState>,
    var_kinds: Vec<VarKind>,
    var_provenance: Vec<Option<VarProvenance>>,  // NEW
    pub pending_type_args: Vec<(NodeId, Vec<InferVarId>, Vec<TypeParam>, String)>,
    pending_int_range_checks: Vec<(InferVarId, i64)>,
}
```

New methods:

- `fresh_var_with_provenance(prov) -> InferVarId` — creates a var and records provenance.
- `set_provenance(id, prov)` — sets or replaces provenance on an existing var.
- `update_arg_name(id, name)` — sets `arg_name` on an existing var's provenance.
- `propagate_provenance(from, to)` — copies provenance from `from`'s root to `to`'s root
  if `to` has none. Called at two points in `unify()`:
  1. During `link()` operations (InferVar-InferVar unification).
  2. During `set_resolved()` when an InferVar is resolved to an IntegerLiteral or
     FloatLiteral type — provenance flows from the InferVar to the literal var.
     This is critical for the `choose(42, 3.14)` scenario (see Section 4a).
- `get_provenance(id) -> Option<&VarProvenance>` — retrieves provenance for a var.

`reset()` also clears `var_provenance`.

### 3. `apply_defaults` Error Collection

Change signature from `Result<(), BengalError>` to `Vec<BengalError>`:

```rust
pub fn apply_defaults(&mut self) -> Vec<BengalError> {
    let mut errors = Vec::new();
    for id in 0..self.var_states.len() {
        let id = id as InferVarId;
        let resolved = self.deep_resolve(Type::InferVar(id));
        match resolved {
            Type::InferVar(_) => {
                match self.var_kinds[id as usize] {
                    VarKind::IntegerLiteral => self.set_resolved(id, Type::I32),
                    VarKind::FloatLiteral => self.set_resolved(id, Type::F64),
                    VarKind::General => {
                        if let Some(prov) = &self.var_provenance[id as usize] {
                            errors.push(BengalError::SemanticError {
                                message: format!(
                                    "cannot infer type parameter '{}' for function '{}'; \
                                     add explicit type annotation",
                                    prov.type_param_name, prov.def_name
                                ),
                                span: prov.span,
                            });
                        } else {
                            errors.push(unify_err(
                                "cannot infer type; add explicit type annotation",
                            ));
                        }
                    }
                }
            }
            Type::IntegerLiteral(_) => self.set_resolved(id, Type::I32),
            Type::FloatLiteral(_) => self.set_resolved(id, Type::F64),
            _ => {}
        }
    }
    // Range checks: same logic but push errors instead of early return
    let range_checks: Vec<_> = self.pending_int_range_checks.clone();
    for &(id, value) in &range_checks {
        let resolved = self.deep_resolve(Type::InferVar(id));
        if let Type::I32 = resolved {
            if value < i32::MIN as i64 || value > i32::MAX as i64 {
                errors.push(unify_err(format!(
                    "integer literal `{}` is out of range for `Int32`", value
                )));
            }
        }
    }
    errors
}
```

### 4. Unification Error Enhancement

#### 4a. Provenance Propagation Chain

To understand why provenance propagation matters, trace the `choose(42, 3.14)` scenario:

1. Call site creates `InferVar(0)` for type param `T` with provenance
   `{ type_param_name: "T", def_name: "choose", span: ... }`.
2. Argument `42` creates `IntegerLiteral(1)` (no provenance).
3. Argument `3.14` creates `FloatLiteral(2)` (no provenance).
4. Unification: `IntegerLiteral(1)` with `InferVar(0)`.
   - `InferVar(0)` is resolved to `IntegerLiteral(1)` via `set_resolved`.
   - **At this point, propagate provenance from var 0 to var 1.**
   - Now `IntegerLiteral(1)` has provenance `{ ..., arg_name: Some("a") }`.
5. Unification: `FloatLiteral(2)` with the resolved type of `InferVar(0)` = `IntegerLiteral(1)`.
   - This hits `IntegerLiteral` vs `FloatLiteral` — conflict detected.
   - Both var 1 (provenance from step 4) and var 2 (provenance set via `update_arg_name`
     for argument "b") have provenance available for the error message.

**Key rule:** `propagate_provenance` must be called in `unify()` whenever:
- An `InferVar` is linked to another `InferVar` (existing `link()` path).
- An `InferVar` is resolved to an `IntegerLiteral(id)` or `FloatLiteral(id)` —
  provenance flows from the InferVar to the literal var's root.

Additionally, `update_arg_name` for literal vars is set through a different path:
when `analyze_expr` creates an `IntegerLiteral` or `FloatLiteral` for an argument,
the `arg_name` is set on the literal var by looking at the corresponding parameter
name in the calling context (see Section 7).

#### 4b. Error Generation

When `unify()` encounters incompatible literal kinds (IntegerLiteral vs FloatLiteral),
it uses provenance from both sides to generate a detailed message:

```rust
(Type::IntegerLiteral(id1), Type::FloatLiteral(id2))
| (Type::FloatLiteral(id2), Type::IntegerLiteral(id1)) => {
    let prov1 = self.var_provenance.get(self.find(id1) as usize).cloned().flatten();
    let prov2 = self.var_provenance.get(self.find(id2) as usize).cloned().flatten();

    if let (Some(p1), Some(p2)) = (&prov1, &prov2) {
        Err(BengalError::SemanticError {
            message: format!(
                "type parameter '{}' in function '{}' has conflicting constraints: \
                 integer literal (from argument '{}') vs float literal (from argument '{}')",
                p1.type_param_name, p1.def_name,
                p1.arg_name.as_deref().unwrap_or("?"),
                p2.arg_name.as_deref().unwrap_or("?"),
            ),
            span: p1.span,
        })
    } else {
        Err(unify_err("cannot unify integer literal with float literal"))
    }
}
```

### 5. `analyze_pre_mono` Flow Change

```rust
let mut all_errors: Vec<BengalError> = Vec::new();

for func in &program.functions {
    if !func.type_params.is_empty() {
        continue;
    }

    match analyze_function(func, &mut resolver, Some(&mut ctx)) {
        Ok(()) => {
            let default_errors = ctx.apply_defaults();
            if default_errors.is_empty() {
                ctx.record_inferred_type_args(&mut inferred);
            } else {
                all_errors.extend(default_errors);
            }
        }
        Err(e) => {
            all_errors.push(e);
        }
    }
    ctx.reset();
}

if let Err(e) = validate_inferred_constraints(&inferred, program) {
    all_errors.push(e);
}

if !all_errors.is_empty() {
    return Err(all_errors.remove(0));
}

Ok(inferred)
```

The first error is returned. Future work can introduce `BengalError::Multiple`
to report all errors at once.

### 6. Defensive `type_to_annotation`

Add a fallible variant to prevent panics even if the flow is accidentally
called with unresolved variables:

```rust
pub fn try_type_to_annotation(ty: &Type) -> Result<TypeAnnotation, BengalError> {
    match ty {
        Type::InferVar(_) | Type::IntegerLiteral(_) | Type::FloatLiteral(_) => {
            Err(unify_err("unresolved type variable in type_to_annotation"))
        }
        // ... same mapping as type_to_annotation, wrapped in Ok(...)
    }
}
```

Existing `type_to_annotation` delegates to `try_type_to_annotation(...).unwrap()`.

Note: this path should be unreachable if `apply_defaults` error collection works correctly —
`record_inferred_type_args` is only called when `apply_defaults` returns no errors. The
`try_` variant is purely a defensive measure to convert a panic into a catchable error.

### 7. Extend `FuncSig` to Include Parameter Names

`FuncSig.params` is currently `Vec<Type>`, which does not carry parameter names.
To provide argument names in error messages, extend it to `Vec<(String, Type)>`,
matching the existing pattern used by `MethodInfo` and `StructInitInfo`:

```rust
// semantic/resolver.rs
pub struct FuncSig {
    pub type_params: Vec<TypeParam>,
    pub params: Vec<(String, Type)>,  // was Vec<Type>
    pub return_type: Type,
}
```

All call sites that construct or destructure `FuncSig.params` must be updated
to use the `(name, ty)` tuple form.

### 8. Call Site Provenance Registration

In `analyze_expr` Call handler (`semantic/mod.rs`):

```rust
if let Some(ref mut c) = ctx {
    let var_ids: Vec<InferVarId> = sig.type_params.iter().map(|tp| {
        c.fresh_var_with_provenance(VarProvenance {
            type_param_name: tp.name.clone(),
            def_name: name.clone(),
            arg_name: None,
            span: expr.span,
        })
    }).collect();
    c.register_call_site(expr.id, var_ids.clone(), sig.type_params.clone(), name.clone());
    // ... build subst map
}

// During argument unification:
for (arg, (param_name, param_ty)) in args.iter().zip(sig.params.iter()) {
    let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut())?;
    let effective_ty = substitute_type(param_ty, &subst);
    if let Some(ref mut c) = ctx {
        // Update arg_name for bare InferVar parameters (e.g., `a: T`)
        if let Type::InferVar(id) = &effective_ty {
            c.update_arg_name(*id, param_name.clone());
        }
        // Also set arg_name on literal vars created for this argument,
        // so provenance propagation can carry the argument name through.
        if let Type::IntegerLiteral(id) | Type::FloatLiteral(id) = &arg_ty {
            c.set_provenance(*id, VarProvenance {
                type_param_name: String::new(), // will be filled by propagation
                def_name: name.clone(),
                arg_name: Some(param_name.clone()),
                span: arg.span,
            });
        }
        c.unify(arg_ty.clone(), effective_ty)?;
    }
}
```

Same pattern for `StructInit` handler.

**Known limitation:** When the parameter type is not a bare type parameter but a
generic container (e.g., `value: Box<T>`), `substitute_type` produces
`Generic { name: "Box", args: [InferVar(0)] }` rather than a bare `InferVar`.
In this case `update_arg_name` is skipped and the error message will show `?`
for the argument name. This is acceptable for the initial implementation and
can be improved later by walking the type tree to find nested InferVars.

## Files Changed

| File | Change |
|------|--------|
| `parser/ast.rs` | Add `span: Span` to `Expr` |
| `parser/mod.rs` | Capture span in `expr()`, update test helpers |
| `monomorphize.rs` | Propagate `expr.span` in all `Expr` rewrites |
| `semantic/resolver.rs` | Extend `FuncSig.params` to `Vec<(String, Type)>` |
| `semantic/infer.rs` | `VarProvenance`, `apply_defaults` → `Vec<BengalError>`, `try_type_to_annotation`, provenance propagation |
| `semantic/mod.rs` | `analyze_pre_mono` error collection, provenance registration in Call/StructInit |
| `tests/type_inference.rs` | Update existing error tests, add new test cases |

## Test Plan

### Updated Tests

- `error_unresolvable_type`: Remove `#[should_panic]`, assert on `"cannot infer type parameter 'T'"` and `"default_value"`
- `error_integer_float_mismatch`: Remove `"undefined function"` fallback, assert on `"conflicting constraints"` or `"cannot unify"`

### New Tests

- `error_partial_inference_failure`: `make<A, B>(a: A) -> B` called as `make(42)` — `B` unresolvable
- `error_multiple_inference_failures`: Two unresolvable calls in one function — first error reported
- `error_struct_init_inference_failure`: Generic struct init without enough info
- Unit tests for `apply_defaults` with provenance
- Unit tests for provenance propagation during link/unify

### Existing Tests Must Still Pass

All tests in `tests/type_inference.rs` that currently pass must continue to pass.
The inference behavior itself is unchanged — only error reporting improves.
