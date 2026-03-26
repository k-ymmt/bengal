# Type Inference Error Improvement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve error reporting for unresolvable type parameters (currently panics) and literal type conflicts (currently surfaces as "undefined function") by adding provenance tracking to InferenceContext.

**Architecture:** Add `VarProvenance` metadata to each inference variable, tracking which type parameter, function, argument, and source span it came from. Change `apply_defaults` to collect errors instead of early-returning. Change `analyze_pre_mono` to collect errors across functions instead of silently discarding them. Add `span` to `Expr` for accurate source location reporting. Extend `FuncSig.params` to `Vec<(String, Type)>` for argument names in errors.

**Tech Stack:** Rust, LLVM (codegen unchanged)

**Spec:** `docs/superpowers/specs/2026-03-26-type-inference-error-improvement-design.md`

---

### Task 1: Add `span` to `Expr`

Mechanical change — adds source span to every expression node. No behavior change.

**Files:**
- Modify: `src/parser/ast.rs:270-273`
- Modify: `src/parser/mod.rs:22-26` (factory method)
- Modify: `src/parser/mod.rs:1206-1297` (test helpers)
- Modify: `src/monomorphize.rs` (all `Expr { id, kind }` constructions)

- [ ] **Step 1: Add `span` field to `Expr` struct**

In `src/parser/ast.rs`, change:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
}
```

Add the import at the top of `src/parser/ast.rs`:

```rust
use crate::error::Span;
```

- [ ] **Step 2: Update parser `expr()` factory to capture span**

In `src/parser/mod.rs:22-26`, the `expr()` method needs a `span` parameter. Since each call site has different parsing context, the simplest approach is to record the start position before parsing and the end position after:

```rust
fn expr(&mut self, kind: ExprKind, span: Span) -> Expr {
    let id = NodeId(self.next_id);
    self.next_id += 1;
    Expr { id, kind, span }
}
```

Add a helper method to get current position:

```rust
fn current_span_start(&self) -> usize {
    self.tokens[self.pos].span.start
}

fn prev_span_end(&self) -> usize {
    if self.pos > 0 {
        self.tokens[self.pos - 1].span.end
    } else {
        0
    }
}

fn span_from(&self, start: usize) -> Span {
    Span { start, end: self.prev_span_end() }
}
```

Update all `self.expr(ExprKind::...)` call sites (27 total in parser/mod.rs) to pass span. For each expression parse function, save `start` at entry and pass `self.span_from(start)` when constructing the expr. For example:

```rust
// parse_primary: Number literal
Token::NumberLiteral(n) => {
    let span = self.peek().span;
    let n = *n;
    self.advance();
    Ok(self.expr(ExprKind::Number(n), span))
}
```

For binary ops (left-recursive):

```rust
// parse_or
fn parse_or(&mut self) -> Result<Expr> {
    let mut left = self.parse_and()?;
    while self.peek().node == Token::PipePipe {
        let start = left.span.start;
        self.advance();
        let right = self.parse_and()?;
        let end = right.span.end;
        left = self.expr(ExprKind::BinaryOp {
            op: BinOp::Or,
            left: Box::new(left),
            right: Box::new(right),
        }, Span { start, end });
    }
    Ok(left)
}
```

- [ ] **Step 3: Update parser test helpers**

In `src/parser/mod.rs` test helpers, use `Span { start: 0, end: 0 }`:

```rust
fn e(kind: ExprKind) -> Expr {
    Expr {
        id: NodeId(0),
        kind,
        span: Span { start: 0, end: 0 },
    }
}

fn normalize_expr(expr: &Expr) -> Expr {
    // ... existing normalization logic ...
    Expr {
        id: NodeId(0),
        kind,
        span: Span { start: 0, end: 0 },
    }
}
```

- [ ] **Step 4: Update monomorphize.rs `Expr` constructions**

In `src/monomorphize.rs`, every `Expr { id: expr.id, kind: ... }` must propagate span from the source expression. There are 8 construction sites — 4 in `substitute_expr` (lines ~520, ~557, ~645) and 4 in `rewrite_expr` (lines ~927, ~938, ~977, ~1055):

```rust
// Change all occurrences from:
Expr { id: expr.id, kind }
// To:
Expr { id: expr.id, kind, span: expr.span }

// And the return statements:
return Expr { id: expr.id, kind: ExprKind::Call { ... } }
// To:
return Expr { id: expr.id, kind: ExprKind::Call { ... }, span: expr.span }
```

- [ ] **Step 5: Run `cargo fmt` and `cargo clippy`, then compile**

Run: `cargo fmt && cargo clippy 2>&1 | head -50`
Expected: Clean compile, no warnings.

- [ ] **Step 6: Run full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: All existing tests pass (except `error_unresolvable_type` which `#[should_panic]` and may need temporary adjustment if span changes affect the panic message — that's OK, we'll fix it in Task 6).

- [ ] **Step 7: Commit**

```bash
git add src/parser/ast.rs src/parser/mod.rs src/monomorphize.rs
git commit -m "Add span field to Expr for source location tracking"
```

---

### Task 2: Extend `FuncSig.params` to include parameter names

Change `FuncSig.params` from `Vec<Type>` to `Vec<(String, Type)>`.

**Files:**
- Modify: `src/semantic/resolver.rs:15-19`
- Modify: `src/semantic/mod.rs` (4 FuncSig construction sites + all `sig.params` usage sites)

- [ ] **Step 1: Change `FuncSig` struct**

In `src/semantic/resolver.rs:15-19`:

```rust
#[derive(Debug, Clone)]
pub struct FuncSig {
    pub type_params: Vec<TypeParam>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
}
```

- [ ] **Step 2: Update FuncSig construction sites in `semantic/mod.rs`**

There are 4 sites that build `FuncSig`. Each currently builds `params: Vec<Type>` from `func.params.iter().map(|p| resolve_type_checked(&p.ty, ...))`. Change all 4 to include the name:

At lines ~145-155 (`collect_module_symbols`):
```rust
let params: Vec<(String, Type)> = func
    .params
    .iter()
    .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, &tmp_resolver)?)))
    .collect::<Result<Vec<_>>>()?;
```

Same pattern at lines ~425-438 (`analyze_post_mono` phase 1), ~969-982 (`analyze_pre_mono` phase 1), and ~1216-1229 (`analyze_post_mono_single` phase 1).

- [ ] **Step 3: Update FuncSig usage sites**

Sites that consume `sig.params`:

1. `sig.params.is_empty()` (lines ~514, ~1304) — unchanged, `.is_empty()` works on `Vec<(String, Type)>`.

2. `sig.params.len()` (lines ~2249, ~2253) — unchanged.

3. `args.iter().zip(sig.params.iter())` (line ~2289) — change iteration variable:

```rust
for (arg, (_param_name, expected_ty)) in args.iter().zip(sig.params.iter()) {
    let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut())?;
    let effective_ty = substitute_type(expected_ty, &subst);
    // ... rest unchanged
```

**Note:** Use `_param_name` (prefixed underscore) here to suppress unused variable warnings from clippy. Task 7 will later replace this with `param_name` when provenance calls are added.

4. Protocol method sig `method_sig.params` (lines ~2679, ~2683, ~2687) — these are `ProtocolMethodSig` which already uses `Vec<(String, Type)>`, so no change needed.

- [ ] **Step 4: Run `cargo fmt`, `cargo clippy`, and tests**

Run: `cargo fmt && cargo clippy 2>&1 | head -30 && cargo test 2>&1 | tail -20`
Expected: Clean compile, all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/semantic/resolver.rs src/semantic/mod.rs
git commit -m "Extend FuncSig.params to Vec<(String, Type)> for arg name tracking"
```

---

### Task 3: Add `VarProvenance` and provenance methods to InferenceContext

Add provenance tracking infrastructure. No behavior change yet.

**Files:**
- Modify: `src/semantic/infer.rs`

- [ ] **Step 1: Write unit tests for provenance methods**

Add to the `#[cfg(test)] mod tests` block in `src/semantic/infer.rs`:

```rust
#[test]
fn fresh_var_with_provenance_records() {
    let mut ctx = InferenceContext::new();
    let id = ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "T".into(),
        def_name: "foo".into(),
        arg_name: None,
        span: Span { start: 10, end: 20 },
    });
    let prov = ctx.get_provenance(id).unwrap();
    assert_eq!(prov.type_param_name, "T");
    assert_eq!(prov.def_name, "foo");
    assert!(prov.arg_name.is_none());
}

#[test]
fn set_provenance_replaces() {
    let mut ctx = InferenceContext::new();
    let id = ctx.fresh_var();
    assert!(ctx.get_provenance(id).is_none());
    ctx.set_provenance(id, VarProvenance {
        type_param_name: "U".into(),
        def_name: "bar".into(),
        arg_name: Some("x".into()),
        span: Span { start: 0, end: 5 },
    });
    assert_eq!(ctx.get_provenance(id).unwrap().type_param_name, "U");
}

#[test]
fn update_arg_name_sets_name() {
    let mut ctx = InferenceContext::new();
    let id = ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "T".into(),
        def_name: "f".into(),
        arg_name: None,
        span: Span { start: 0, end: 0 },
    });
    ctx.update_arg_name(id, "x".into());
    assert_eq!(
        ctx.get_provenance(id).unwrap().arg_name.as_deref(),
        Some("x")
    );
}

#[test]
fn propagate_provenance_copies_to_empty() {
    let mut ctx = InferenceContext::new();
    let a = ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "T".into(),
        def_name: "f".into(),
        arg_name: Some("a".into()),
        span: Span { start: 0, end: 5 },
    });
    let b = ctx.fresh_var();
    assert!(ctx.get_provenance(b).is_none());
    ctx.propagate_provenance(a, b);
    assert!(ctx.get_provenance(b).is_some());
    assert_eq!(ctx.get_provenance(b).unwrap().type_param_name, "T");
}

#[test]
fn propagate_provenance_does_not_overwrite() {
    let mut ctx = InferenceContext::new();
    let a = ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "T".into(),
        def_name: "f".into(),
        arg_name: None,
        span: Span { start: 0, end: 0 },
    });
    let b = ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "U".into(),
        def_name: "g".into(),
        arg_name: None,
        span: Span { start: 0, end: 0 },
    });
    ctx.propagate_provenance(a, b);
    assert_eq!(ctx.get_provenance(b).unwrap().type_param_name, "U");
}

#[test]
fn reset_clears_provenance() {
    let mut ctx = InferenceContext::new();
    ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "T".into(),
        def_name: "f".into(),
        arg_name: None,
        span: Span { start: 0, end: 0 },
    });
    ctx.reset();
    let id = ctx.fresh_var();
    assert!(ctx.get_provenance(id).is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bengal infer::tests -- --nocapture 2>&1 | tail -20`
Expected: Compilation error — `VarProvenance` and methods don't exist yet.

- [ ] **Step 3: Add `VarProvenance` struct and implement methods**

In `src/semantic/infer.rs`, add the struct after `InferVarId`:

```rust
/// Provenance metadata for an inference variable — tracks where it came from.
#[derive(Debug, Clone)]
pub struct VarProvenance {
    pub type_param_name: String,
    pub def_name: String,
    pub arg_name: Option<String>,
    pub span: Span,
}
```

Add `var_provenance` to `InferenceContext`:

```rust
#[derive(Debug)]
pub struct InferenceContext {
    var_states: Vec<VarState>,
    var_kinds: Vec<VarKind>,
    var_provenance: Vec<Option<VarProvenance>>,
    pub pending_type_args: Vec<(NodeId, Vec<InferVarId>, Vec<TypeParam>, String)>,
    pending_int_range_checks: Vec<(InferVarId, i64)>,
}
```

Update `new()`:

```rust
pub fn new() -> Self {
    Self {
        var_states: Vec::new(),
        var_kinds: Vec::new(),
        var_provenance: Vec::new(),
        pending_type_args: Vec::new(),
        pending_int_range_checks: Vec::new(),
    }
}
```

Update `fresh_var()`, `fresh_integer()`, `fresh_float()` to push `None` to `var_provenance`:

```rust
pub fn fresh_var(&mut self) -> InferVarId {
    let id = self.var_states.len() as InferVarId;
    self.var_states.push(VarState::Unbound);
    self.var_kinds.push(VarKind::General);
    self.var_provenance.push(None);
    id
}

pub fn fresh_integer(&mut self) -> InferVarId {
    let id = self.var_states.len() as InferVarId;
    self.var_states.push(VarState::Unbound);
    self.var_kinds.push(VarKind::IntegerLiteral);
    self.var_provenance.push(None);
    id
}

pub fn fresh_float(&mut self) -> InferVarId {
    let id = self.var_states.len() as InferVarId;
    self.var_states.push(VarState::Unbound);
    self.var_kinds.push(VarKind::FloatLiteral);
    self.var_provenance.push(None);
    id
}
```

Add new methods:

```rust
pub fn fresh_var_with_provenance(&mut self, prov: VarProvenance) -> InferVarId {
    let id = self.fresh_var();
    self.var_provenance[id as usize] = Some(prov);
    id
}

pub fn set_provenance(&mut self, id: InferVarId, prov: VarProvenance) {
    self.var_provenance[id as usize] = Some(prov);
}

pub fn update_arg_name(&mut self, id: InferVarId, name: String) {
    if let Some(prov) = &mut self.var_provenance[id as usize] {
        prov.arg_name = Some(name);
    }
}

pub fn propagate_provenance(&mut self, from: InferVarId, to: InferVarId) {
    let from_root = self.find(from);
    let to_root = self.find(to);
    if self.var_provenance[to_root as usize].is_none() {
        self.var_provenance[to_root as usize] = self.var_provenance[from_root as usize].clone();
    }
}

pub fn get_provenance(&self, id: InferVarId) -> Option<&VarProvenance> {
    self.var_provenance.get(id as usize).and_then(|p| p.as_ref())
}
```

Update `reset()`:

```rust
pub fn reset(&mut self) {
    self.var_states.clear();
    self.var_kinds.clear();
    self.var_provenance.clear();
    self.pending_type_args.clear();
    self.pending_int_range_checks.clear();
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p bengal infer::tests -- --nocapture 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/semantic/infer.rs
git commit -m "Add VarProvenance and provenance tracking methods to InferenceContext"
```

---

### Task 4: Change `apply_defaults` to collect errors with provenance

**Files:**
- Modify: `src/semantic/infer.rs:215-267`

- [ ] **Step 1: Write test for provenance-enhanced error**

Add to tests in `src/semantic/infer.rs`:

```rust
#[test]
fn apply_defaults_unresolved_with_provenance() {
    let mut ctx = InferenceContext::new();
    let _id = ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "T".into(),
        def_name: "foo".into(),
        arg_name: None,
        span: Span { start: 10, end: 20 },
    });
    let errors = ctx.apply_defaults();
    assert_eq!(errors.len(), 1);
    let msg = errors[0].to_string();
    assert!(msg.contains("'T'"), "expected type param name, got: {}", msg);
    assert!(msg.contains("'foo'"), "expected func name, got: {}", msg);
}

#[test]
fn apply_defaults_multiple_errors_collected() {
    let mut ctx = InferenceContext::new();
    ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "A".into(),
        def_name: "f".into(),
        arg_name: None,
        span: Span { start: 0, end: 5 },
    });
    ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "B".into(),
        def_name: "f".into(),
        arg_name: None,
        span: Span { start: 0, end: 5 },
    });
    let errors = ctx.apply_defaults();
    assert_eq!(errors.len(), 2);
}

#[test]
fn apply_defaults_integer_literal_returns_empty_errors() {
    let mut ctx = InferenceContext::new();
    ctx.fresh_integer();
    let errors = ctx.apply_defaults();
    assert!(errors.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bengal infer::tests::apply_defaults_unresolved_with_provenance -- --nocapture 2>&1`
Expected: Compilation error — `apply_defaults` still returns `Result`.

- [ ] **Step 3: Change `apply_defaults` signature and implementation**

In `src/semantic/infer.rs`, replace the `apply_defaults` method:

```rust
pub fn apply_defaults(&mut self) -> Vec<BengalError> {
    let mut errors = Vec::new();
    for id in 0..self.var_states.len() {
        let id = id as InferVarId;
        let resolved = self.deep_resolve(Type::InferVar(id));
        match resolved {
            Type::InferVar(_) => {
                match self.var_kinds[id as usize] {
                    VarKind::IntegerLiteral => {
                        self.set_resolved(id, Type::I32);
                    }
                    VarKind::FloatLiteral => {
                        self.set_resolved(id, Type::F64);
                    }
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
            Type::IntegerLiteral(_) => {
                self.set_resolved(id, Type::I32);
            }
            Type::FloatLiteral(_) => {
                self.set_resolved(id, Type::F64);
            }
            _ => {}
        }
    }

    let range_checks: Vec<_> = self.pending_int_range_checks.clone();
    for &(id, value) in &range_checks {
        let resolved = self.deep_resolve(Type::InferVar(id));
        match resolved {
            Type::I32 => {
                if value < i32::MIN as i64 || value > i32::MAX as i64 {
                    errors.push(unify_err(format!(
                        "integer literal `{}` is out of range for `Int32`",
                        value
                    )));
                }
            }
            Type::I64 => {}
            _ => {}
        }
    }

    errors
}
```

- [ ] **Step 4: Update existing tests that check `apply_defaults` return type**

Update these tests to use the new `Vec<BengalError>` return:

```rust
#[test]
fn apply_defaults_integer_literal_to_i32() {
    let mut ctx = InferenceContext::new();
    let a = ctx.fresh_integer();
    assert!(ctx.apply_defaults().is_empty());
    assert_eq!(ctx.resolve(a), Type::I32);
}

#[test]
fn apply_defaults_float_literal_to_f64() {
    let mut ctx = InferenceContext::new();
    let a = ctx.fresh_float();
    assert!(ctx.apply_defaults().is_empty());
    assert_eq!(ctx.resolve(a), Type::F64);
}

#[test]
fn apply_defaults_already_resolved() {
    let mut ctx = InferenceContext::new();
    let a = ctx.fresh_integer();
    ctx.set_resolved(a, Type::I64);
    assert!(ctx.apply_defaults().is_empty());
    assert_eq!(ctx.resolve(a), Type::I64);
}

#[test]
fn apply_defaults_unresolved_infer_var_error() {
    let mut ctx = InferenceContext::new();
    let _a = ctx.fresh_var();
    assert!(!ctx.apply_defaults().is_empty());
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p bengal infer::tests -- --nocapture 2>&1 | tail -20`
Expected: All infer tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/semantic/infer.rs
git commit -m "Change apply_defaults to collect errors with provenance messages"
```

---

### Task 5: Add provenance propagation in `unify()` and literal conflict error

**Files:**
- Modify: `src/semantic/infer.rs` (unify method)

- [ ] **Step 1: Write test for provenance-enhanced unification error**

Add to tests in `src/semantic/infer.rs`:

```rust
#[test]
fn unify_integer_float_literal_error_with_provenance() {
    let mut ctx = InferenceContext::new();
    // Simulate choose(42, 3.14) scenario:
    // InferVar(0) for type param T with provenance
    let t_var = ctx.fresh_var_with_provenance(VarProvenance {
        type_param_name: "T".into(),
        def_name: "choose".into(),
        arg_name: None,
        span: Span { start: 0, end: 10 },
    });
    // IntegerLiteral(1) for arg "a"
    let int_var = ctx.fresh_integer();
    ctx.set_provenance(int_var, VarProvenance {
        type_param_name: String::new(),
        def_name: "choose".into(),
        arg_name: Some("a".into()),
        span: Span { start: 0, end: 10 },
    });
    // FloatLiteral(2) for arg "b"
    let float_var = ctx.fresh_float();
    ctx.set_provenance(float_var, VarProvenance {
        type_param_name: String::new(),
        def_name: "choose".into(),
        arg_name: Some("b".into()),
        span: Span { start: 0, end: 10 },
    });
    // Unify IntegerLiteral with InferVar(T) -> propagates provenance
    assert!(ctx.unify(Type::IntegerLiteral(int_var), Type::InferVar(t_var)).is_ok());
    // Unify FloatLiteral with resolved T (now IntegerLiteral) -> conflict
    let result = ctx.unify(Type::FloatLiteral(float_var), Type::InferVar(t_var));
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("conflicting constraints"), "got: {}", msg);
    assert!(msg.contains("'T'"), "expected type param name, got: {}", msg);
    assert!(msg.contains("'choose'"), "expected func name, got: {}", msg);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p bengal infer::tests::unify_integer_float_literal_error_with_provenance -- --nocapture 2>&1`
Expected: FAIL — current error message is generic "cannot unify".

- [ ] **Step 3: Add provenance propagation to `unify()`**

In `src/semantic/infer.rs`, update the `unify` method. Add provenance propagation to `InferVar` binding and add the new IntegerLiteral vs FloatLiteral match arm:

```rust
pub fn unify(&mut self, ty1: Type, ty2: Type) -> Result<(), BengalError> {
    let ty1 = self.deep_resolve(ty1);
    let ty2 = self.deep_resolve(ty2);

    if ty1 == ty2 {
        return Ok(());
    }

    match (ty1, ty2) {
        (Type::TypeParam { .. }, _) | (_, Type::TypeParam { .. }) => Ok(()),

        // InferVar binds to anything — propagate provenance
        (Type::InferVar(a), ref other) | (ref other, Type::InferVar(a)) => {
            // If resolving to a literal type, propagate provenance to the literal var
            match other {
                Type::IntegerLiteral(lit_id) | Type::FloatLiteral(lit_id) => {
                    self.propagate_provenance(a, *lit_id);
                }
                _ => {}
            }
            self.set_resolved(a, other.clone());
            Ok(())
        }

        // IntegerLiteral vs FloatLiteral — conflict with provenance
        (Type::IntegerLiteral(id1), Type::FloatLiteral(id2))
        | (Type::FloatLiteral(id2), Type::IntegerLiteral(id1)) => {
            let prov1 = self.var_provenance.get(self.find(id1) as usize).cloned().flatten();
            let prov2 = self.var_provenance.get(self.find(id2) as usize).cloned().flatten();

            if let (Some(p1), Some(p2)) = (&prov1, &prov2) {
                // p1 is always the integer literal's provenance (id1), p2 is the float's (id2)
                // Use whichever has a type_param_name for the top-level context
                let tp_prov = if !p1.type_param_name.is_empty() { p1 } else { p2 };
                Err(BengalError::SemanticError {
                    message: format!(
                        "type parameter '{}' in function '{}' has conflicting constraints: \
                         integer literal (from argument '{}') vs float literal (from argument '{}')",
                        tp_prov.type_param_name, tp_prov.def_name,
                        p1.arg_name.as_deref().unwrap_or("?"),
                        p2.arg_name.as_deref().unwrap_or("?"),
                    ),
                    span: tp_prov.span,
                })
            } else {
                Err(unify_err("cannot unify integer literal with float literal"))
            }
        }

        // IntegerLiteral with integer concrete types
        (Type::IntegerLiteral(a), ref concrete @ Type::I32)
        | (Type::IntegerLiteral(a), ref concrete @ Type::I64)
        | (ref concrete @ Type::I32, Type::IntegerLiteral(a))
        | (ref concrete @ Type::I64, Type::IntegerLiteral(a)) => {
            self.set_resolved(a, concrete.clone());
            Ok(())
        }

        // IntegerLiteral with IntegerLiteral — propagate provenance
        (Type::IntegerLiteral(a), Type::IntegerLiteral(b)) => {
            self.propagate_provenance(a, b);
            self.link(a, b);
            Ok(())
        }

        // FloatLiteral with float concrete types
        (Type::FloatLiteral(a), ref concrete @ Type::F32)
        | (Type::FloatLiteral(a), ref concrete @ Type::F64)
        | (ref concrete @ Type::F32, Type::FloatLiteral(a))
        | (ref concrete @ Type::F64, Type::FloatLiteral(a)) => {
            self.set_resolved(a, concrete.clone());
            Ok(())
        }

        // FloatLiteral with FloatLiteral — propagate provenance
        (Type::FloatLiteral(a), Type::FloatLiteral(b)) => {
            self.propagate_provenance(a, b);
            self.link(a, b);
            Ok(())
        }

        // Array, Generic, catch-all — unchanged
        // ... (keep existing arms)
    }
}
```

**Important:** The new `IntegerLiteral` vs `FloatLiteral` arm MUST appear before the individual `IntegerLiteral`/`FloatLiteral` arms, because Rust matches top-to-bottom.

- [ ] **Step 4: Run tests**

Run: `cargo test -p bengal infer::tests -- --nocapture 2>&1 | tail -30`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/semantic/infer.rs
git commit -m "Add provenance propagation in unify and literal conflict error"
```

---

### Task 6: Add `try_type_to_annotation` defensive fallback

**Files:**
- Modify: `src/semantic/infer.rs:42-66`

- [ ] **Step 1: Add `try_type_to_annotation` function**

In `src/semantic/infer.rs`, rename existing `type_to_annotation` to `try_type_to_annotation` and add a wrapper:

```rust
pub fn try_type_to_annotation(ty: &Type) -> std::result::Result<TypeAnnotation, BengalError> {
    match ty {
        Type::I32 => Ok(TypeAnnotation::I32),
        Type::I64 => Ok(TypeAnnotation::I64),
        Type::F32 => Ok(TypeAnnotation::F32),
        Type::F64 => Ok(TypeAnnotation::F64),
        Type::Bool => Ok(TypeAnnotation::Bool),
        Type::Unit => Ok(TypeAnnotation::Unit),
        Type::Struct(name) => Ok(TypeAnnotation::Named(name.clone())),
        Type::TypeParam { name, .. } => Ok(TypeAnnotation::Named(name.clone())),
        Type::Generic { name, args } => {
            let converted: std::result::Result<Vec<_>, _> =
                args.iter().map(try_type_to_annotation).collect();
            Ok(TypeAnnotation::Generic {
                name: name.clone(),
                args: converted?,
            })
        }
        Type::Array { element, size } => Ok(TypeAnnotation::Array {
            element: Box::new(try_type_to_annotation(element)?),
            size: *size,
        }),
        // Defensive: should be unreachable if apply_defaults error collection works correctly
        Type::InferVar(_) | Type::IntegerLiteral(_) | Type::FloatLiteral(_) => {
            Err(unify_err("unresolved type variable in type_to_annotation"))
        }
    }
}

pub fn type_to_annotation(ty: &Type) -> TypeAnnotation {
    try_type_to_annotation(ty).expect("unresolved type variable in type_to_annotation")
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p bengal infer::tests -- --nocapture 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/semantic/infer.rs
git commit -m "Add try_type_to_annotation defensive fallback"
```

---

### Task 7: Update `analyze_pre_mono` error collection and call site provenance

**Files:**
- Modify: `src/semantic/mod.rs:1063-1083` (pre-mono loop)
- Modify: `src/semantic/mod.rs:2258-2301` (Call handler)
- Modify: `src/semantic/mod.rs:2434-2478` (StructInit handler)

- [ ] **Step 1: Update `analyze_pre_mono` to collect errors**

In `src/semantic/mod.rs`, replace the function loop at lines ~1063-1077:

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

// --- Stage 2: validate protocol constraints on inferred type args ---
if let Err(e) = validate_inferred_constraints(&inferred, program) {
    all_errors.push(e);
}

if !all_errors.is_empty() {
    return Err(all_errors.remove(0));
}

Ok(inferred)
```

- [ ] **Step 2: Add provenance registration to Call handler**

In the Call handler (`src/semantic/mod.rs`, around line 2266-2287), update the inference branch to use `fresh_var_with_provenance`:

```rust
} else if !sig.type_params.is_empty() {
    if let Some(ref mut c) = ctx {
        let var_ids: Vec<InferVarId> = sig
            .type_params
            .iter()
            .map(|tp| {
                c.fresh_var_with_provenance(infer::VarProvenance {
                    type_param_name: tp.name.clone(),
                    def_name: name.clone(),
                    arg_name: None,
                    span: expr.span,
                })
            })
            .collect();
        c.register_call_site(
            expr.id,
            var_ids.clone(),
            sig.type_params.clone(),
            name.clone(),
        );
        sig.type_params
            .iter()
            .zip(var_ids.iter())
            .map(|(tp, &id)| (tp.name.clone(), Type::InferVar(id)))
            .collect()
    } else {
        HashMap::new()
    }
```

- [ ] **Step 3: Add provenance to argument unification in Call handler**

Update the argument loop (around line 2289):

```rust
for (arg, (param_name, expected_ty)) in args.iter().zip(sig.params.iter()) {
    let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut())?;
    let effective_ty = substitute_type(expected_ty, &subst);
    if let Some(ref mut c) = ctx {
        if let Type::InferVar(id) = &effective_ty {
            c.update_arg_name(*id, param_name.clone());
        }
        if let Type::IntegerLiteral(id) | Type::FloatLiteral(id) = &arg_ty {
            c.set_provenance(
                *id,
                infer::VarProvenance {
                    type_param_name: String::new(),
                    def_name: name.clone(),
                    arg_name: Some(param_name.clone()),
                    span: arg.span,
                },
            );
        }
        c.unify(arg_ty.clone(), effective_ty)?;
    } else if !types_compatible(&arg_ty, &effective_ty) {
        return Err(sem_err(format!(
            "argument type mismatch: expected `{}`, found `{}`",
            effective_ty, arg_ty
        )));
    }
}
```

- [ ] **Step 4: Add provenance to StructInit handler**

Update the StructInit handler inference branch (around line 2434-2453):

```rust
if let Some(ref mut c) = ctx {
    let var_ids: Vec<InferVarId> = struct_info
        .type_params
        .iter()
        .map(|tp| {
            c.fresh_var_with_provenance(infer::VarProvenance {
                type_param_name: tp.name.clone(),
                def_name: name.clone(),
                arg_name: None,
                span: expr.span,
            })
        })
        .collect();
    c.register_call_site(
        expr.id,
        var_ids.clone(),
        struct_info.type_params.clone(),
        name.clone(),
    );
    // ... rest of subst map building unchanged
```

And add provenance to the StructInit argument loop (around line 2461-2477):

```rust
for ((label, arg_expr), (param_name, param_ty)) in args.iter().zip(init.params.iter()) {
    if label != param_name {
        return Err(sem_err(format!(
            "expected argument label `{}`, found `{}`",
            param_name, label
        )));
    }
    let arg_ty = analyze_expr(arg_expr, resolver, ctx.as_deref_mut())?;
    let effective_ty = substitute_type(param_ty, &subst);
    if let Some(ref mut c) = ctx {
        if let Type::InferVar(id) = &effective_ty {
            c.update_arg_name(*id, param_name.clone());
        }
        if let Type::IntegerLiteral(id) | Type::FloatLiteral(id) = &arg_ty {
            c.set_provenance(
                *id,
                infer::VarProvenance {
                    type_param_name: String::new(),
                    def_name: name.clone(),
                    arg_name: Some(param_name.clone()),
                    span: arg_expr.span,
                },
            );
        }
        c.unify(arg_ty.clone(), effective_ty)?;
    } else if !types_compatible(&arg_ty, &effective_ty) {
        return Err(sem_err(format!(
            "argument type mismatch: expected `{}`, found `{}`",
            effective_ty, arg_ty
        )));
    }
}
```

- [ ] **Step 5: Run `cargo fmt`, `cargo clippy`, then full test suite**

Run: `cargo fmt && cargo clippy 2>&1 | head -30 && cargo test 2>&1 | tail -30`
Expected: All tests pass except `error_unresolvable_type` (which still has `#[should_panic]`).

- [ ] **Step 6: Commit**

```bash
git add src/semantic/mod.rs
git commit -m "Update analyze_pre_mono error collection and add call site provenance"
```

---

### Task 8: Update integration tests

**Files:**
- Modify: `tests/type_inference.rs:410-454`

- [ ] **Step 1: Update `error_unresolvable_type` test**

Replace the `#[should_panic]` test:

```rust
#[test]
fn error_unresolvable_type() {
    let result = compile_should_fail(
        "func default_value<T>() -> T { return 0; }
         func main() -> Int32 { let x = default_value(); return 0; }",
    );
    assert!(
        result.contains("cannot infer type parameter 'T'")
            && result.contains("default_value"),
        "Expected detailed inference error, got: {}",
        result
    );
}
```

- [ ] **Step 2: Update `error_integer_float_mismatch` test**

Remove "undefined function" fallback:

```rust
#[test]
fn error_integer_float_mismatch() {
    let result = compile_should_fail(
        "func choose<T>(a: T, b: T) -> T { return a; }
         func main() -> Int32 { choose(42, 3.14); return 0; }",
    );
    assert!(
        result.contains("conflicting constraints")
            || result.contains("cannot unify"),
        "Expected type conflict error, got: {}",
        result
    );
}
```

- [ ] **Step 3: Add new test cases**

```rust
#[test]
fn error_partial_inference_failure() {
    let result = compile_should_fail(
        "func make<A, B>(a: A) -> B { return a; }
         func main() -> Int32 { let x = make(42); return 0; }",
    );
    assert!(
        result.contains("cannot infer type parameter 'B'"),
        "Expected inference error for B, got: {}",
        result
    );
}

#[test]
fn error_struct_init_inference_failure() {
    let result = compile_should_fail(
        "struct Holder<T> { var value: T; }
         func get_holder<T>() -> Holder<T> { return Holder(value: 0); }
         func main() -> Int32 { let h = get_holder(); return 0; }",
    );
    assert!(
        result.contains("cannot infer type parameter"),
        "Expected inference error, got: {}",
        result
    );
}

#[test]
fn error_multiple_inference_failures() {
    let result = compile_should_fail(
        "func default_a<T>() -> T { return 0; }
         func default_b<U>() -> U { return 0; }
         func main() -> Int32 {
             let x = default_a();
             let y = default_b();
             return 0;
         }",
    );
    assert!(
        result.contains("cannot infer type parameter"),
        "Expected inference error, got: {}",
        result
    );
}
```

- [ ] **Step 4: Run full test suite**

Run: `cargo fmt && cargo clippy 2>&1 | head -20 && cargo test 2>&1 | tail -30`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add tests/type_inference.rs
git commit -m "Update type inference error tests for improved diagnostics"
```

---

### Task 9: Final verification

- [ ] **Step 1: Run the complete test suite**

Run: `cargo test 2>&1`
Expected: All tests pass, zero failures.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1`
Expected: No warnings.

- [ ] **Step 3: Verify the two original bug scenarios**

Run the `error_unresolvable_type` and `error_integer_float_mismatch` tests in isolation:

Run: `cargo test error_unresolvable_type error_integer_float_mismatch -- --nocapture 2>&1`
Expected: Both pass with clean error messages.

- [ ] **Step 4: Commit (if any remaining changes)**

```bash
git status
# If clean: done. If not: commit remaining changes.
```
