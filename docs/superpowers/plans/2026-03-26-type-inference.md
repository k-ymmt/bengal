# Type Inference Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add bidirectional type checking with local unification to the Bengal compiler, enabling generic type argument inference and numeric literal inference.

**Architecture:** New `InferenceContext` with Union-Find manages type variables. `analyze_expr` and `analyze_stmt` gain an `Option<&mut InferenceContext>` parameter — when `Some`, bidirectional inference is active (pre-mono); when `None`, current behavior is preserved (post-mono). Pipeline reordered: `analyze_pre_mono` (with inference) runs before `monomorphize`, producing a side table (`InferredTypeArgs`) of inferred type arguments indexed by `NodeId`.

**Dual-mode strategy:** The same `analyze_expr`/`analyze_stmt` code serves both pre-mono and post-mono passes. When `ctx: Option<&mut InferenceContext>` is `None`, numeric literals return `I32`/`F64` directly, type comparisons use equality, and loop break types use direct comparison (current behavior). When `Some`, numeric literals return `IntegerLiteral`/`FloatLiteral`, type comparisons use unification, and loop break types use `InferVar`. This avoids duplicating ~550 lines of analysis code.

**Tech Stack:** Rust, LLVM (codegen unchanged)

**Spec:** `docs/superpowers/specs/2026-03-25-type-inference-design.md`

---

### Task 1: InferenceContext and Union-Find

Build the core inference engine in a new module. Isolated from existing code — no behavior changes.

**Files:**
- Create: `src/semantic/infer.rs`
- Modify: `src/semantic/mod.rs` (add `mod infer;` declaration)

- [ ] **Step 1: Write unit tests for Union-Find operations**

Create `src/semantic/infer.rs` with a `#[cfg(test)]` module testing:
- `fresh_var()` returns incrementing IDs
- `resolve()` on unbound var returns `InferVar(id)`
- `resolve()` follows linked chain (path compression)
- `resolve()` on resolved var returns the concrete type
- `reset()` clears all state

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bengal infer::tests -- --nocapture`
Expected: compilation errors (module and types don't exist yet)

- [ ] **Step 3: Implement InferenceContext**

```rust
use crate::parser::ast::NodeId;
use crate::semantic::types::Type;

pub type InferVarId = u32;

#[derive(Debug, Clone)]
enum VarState {
    Unbound,
    Linked(InferVarId),
    Resolved(Type),
}

pub struct InferenceContext {
    var_states: Vec<VarState>,
    pending_type_args: Vec<(NodeId, Vec<InferVarId>)>,
}

impl InferenceContext {
    pub fn new() -> Self { ... }
    pub fn fresh_var(&mut self) -> InferVarId { ... }
    pub fn fresh_integer(&mut self) -> InferVarId { ... }
    pub fn fresh_float(&mut self) -> InferVarId { ... }
    pub fn resolve(&mut self, id: InferVarId) -> Type { ... }  // with path compression
    pub fn reset(&mut self) { ... }
}
```

The distinction between InferVar/IntegerLiteral/FloatLiteral is in the `Type` enum, not in `VarState`. All three start as `Unbound` in `VarState`. The caller wraps the id in the appropriate `Type` variant. `resolve()` follows the Union-Find chain: if unbound, returns the original type (InferVar/IntegerLiteral/FloatLiteral); if linked, follows the chain; if resolved, returns the concrete type.

- [ ] **Step 4: Add `pub mod infer;` to `src/semantic/mod.rs`**

Add near the top alongside existing module declarations.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p bengal infer::tests -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full test suite to verify no regressions**

Run: `cargo test`
Expected: all existing tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/semantic/infer.rs src/semantic/mod.rs
git commit -m "Add InferenceContext with Union-Find for type inference"
```

---

### Task 2: Type Enum Extension

Add `InferVar`, `IntegerLiteral`, `FloatLiteral` variants to the `Type` enum. Update `Display` and helper methods.

**Files:**
- Modify: `src/semantic/types.rs:6-54`

- [ ] **Step 1: Write a unit test that constructs the new type variants**

In `src/semantic/infer.rs` tests, add tests that create `Type::InferVar(0)`, `Type::IntegerLiteral(0)`, `Type::FloatLiteral(0)` and verify Display output.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p bengal infer::tests -- --nocapture`
Expected: FAIL — variants don't exist

- [ ] **Step 3: Add variants to Type enum**

In `src/semantic/types.rs`, add to the `Type` enum (after `Array`):

```rust
InferVar(u32),
IntegerLiteral(u32),
FloatLiteral(u32),
```

Update `Display` impl:
- `InferVar(id)` → `"?{id}"`
- `IntegerLiteral(_)` → `"integer literal"`
- `FloatLiteral(_)` → `"float literal"`

Update `is_numeric()` — `IntegerLiteral` and `FloatLiteral` return `true`.
Update `is_integer()` — `IntegerLiteral` returns `true`.
Update `is_float()` — `FloatLiteral` returns `true`.

- [ ] **Step 4: Build to find all exhaustive match errors, add arms**

Run: `cargo build`

The Rust compiler's exhaustiveness checking will flag every `match` on `Type` that needs updating. Add `InferVar(_) | IntegerLiteral(_) | FloatLiteral(_)` arms. In post-mono code (BIR lowering in `src/bir/mod.rs`, LLVM codegen in `src/codegen/llvm.rs`), these should be `unreachable!("inference type in post-mono pass")`. In semantic analysis code, they need case-by-case handling (most can be unreachable for now, will be filled in during Tasks 8-9).

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all tests PASS (no behavior change)

- [ ] **Step 6: Commit**

```bash
git add src/semantic/types.rs src/semantic/mod.rs src/bir/ src/codegen/
git commit -m "Add InferVar, IntegerLiteral, FloatLiteral type variants"
```

---

### Task 3: Unification

Implement `unify()` with all rules from the spec.

**Files:**
- Modify: `src/semantic/infer.rs`

- [ ] **Step 1: Write unit tests for each unification rule**

Test cases (one per rule in the spec's unification table):
- `InferVar(a)` + concrete type → `a` resolved
- `IntegerLiteral(a)` + `I32` → resolved to I32
- `IntegerLiteral(a)` + `I64` → resolved to I64
- `IntegerLiteral(a)` + `InferVar(b)` → `b` resolved to `IntegerLiteral(a)`
- `IntegerLiteral(a)` + `IntegerLiteral(b)` → linked
- `IntegerLiteral(a)` + `FloatLiteral(b)` → error
- `IntegerLiteral(a)` + `Bool` → error
- `FloatLiteral(a)` + `F32` / `F64` → resolved
- `Array` recursive unify on element types
- `Generic` pairwise unify on args
- `Struct(a)` + `Struct(a)` → success; `Struct(a)` + `Struct(b)` → error
- `Struct` + `Generic` → error
- `TypeParam(name)` + `TypeParam(name)` → success
- `TypeParam(a)` + different type → error
- Symmetry: `unify(a, b)` == `unify(b, a)` for all above

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bengal infer::tests::unify -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement unify()**

```rust
impl InferenceContext {
    pub fn unify(&mut self, ty1: Type, ty2: Type) -> Result<()> {
        let ty1 = self.deep_resolve(ty1);
        let ty2 = self.deep_resolve(ty2);
        match (ty1, ty2) {
            (a, b) if a == b => Ok(()),
            (Type::InferVar(a), other) | (other, Type::InferVar(a)) => {
                self.set_resolved(a, other); Ok(())
            }
            (Type::IntegerLiteral(a), ty @ (Type::I32 | Type::I64))
            | (ty @ (Type::I32 | Type::I64), Type::IntegerLiteral(a)) => {
                self.set_resolved(a, ty); Ok(())
            }
            (Type::IntegerLiteral(a), Type::IntegerLiteral(b)) => {
                self.link(a, b); Ok(())
            }
            // ... all other rules per spec ...
            (a, b) => Err(sem_err(format!("type mismatch: '{}' vs '{}'", a, b)))
        }
    }
    fn deep_resolve(&mut self, ty: Type) -> Type { ... }
}
```

`deep_resolve` follows union-find chains for InferVar/IntegerLiteral/FloatLiteral and recurses into Array/Generic to resolve nested type variables.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p bengal infer::tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/semantic/infer.rs
git commit -m "Implement unification with all type rules"
```

---

### Task 4: Side Table, Defaults, and type_to_annotation

**Files:**
- Modify: `src/semantic/infer.rs`

- [ ] **Step 1: Write unit tests**

Test cases:
- `apply_defaults` resolves `IntegerLiteral` → `I32`, `FloatLiteral` → `F64`
- `apply_defaults` errors on unresolved `InferVar`
- `type_to_annotation` converts each Type variant correctly including `TypeParam` → `Named`
- `register_call_site` + `record_inferred_type_args` populates side table correctly

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement**

```rust
use crate::parser::ast::{NodeId, TypeAnnotation, TypeParam};
use std::collections::HashMap;

/// Stores inferred type arguments along with enough info to validate constraints.
pub struct InferredTypeArgs {
    pub map: HashMap<NodeId, InferredCallSite>,
}

/// One call site's inferred type args plus the definition's type params (for constraint checking).
pub struct InferredCallSite {
    pub type_args: Vec<TypeAnnotation>,
    pub type_params: Vec<TypeParam>,  // from the definition, carries bound info
    pub def_name: String,             // function or struct name for error messages
}

pub fn type_to_annotation(ty: &Type) -> TypeAnnotation { ... }

impl InferenceContext {
    pub fn apply_defaults(&mut self) -> Result<()> { ... }
    pub fn register_call_site(
        &mut self, node_id: NodeId, var_ids: Vec<InferVarId>,
        type_params: Vec<TypeParam>, def_name: String,
    ) { ... }
    pub fn record_inferred_type_args(&mut self, inferred: &mut InferredTypeArgs) { ... }
}
```

`InferredCallSite` stores both the inferred type args AND the original definition's type params (with bounds). This allows `validate_inferred_constraints` to check constraints without a separate lookup mechanism.

The `monomorphize` function accesses `inferred.map.get(&node_id).map(|site| &site.type_args)` to get the type args for monomorphization.

- [ ] **Step 4: Run tests to verify they pass**

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/semantic/infer.rs
git commit -m "Add InferredTypeArgs side table and default resolution"
```

---

### Task 5: Relax validate_generics

Allow omitted type arguments at generic call sites. Currently this is an error.

**Files:**
- Modify: `src/semantic/mod.rs:645-917` (validate_generics)
- Modify: `tests/generics.rs`

- [ ] **Step 1: Write a test that verifies the old error no longer triggers**

In `tests/generics.rs`:
```rust
#[test]
fn generic_omit_type_args_parse_ok() {
    let result = compile_should_fail(
        "func identity<T>(value: T) -> T { return value; }
         func main() -> Int32 { return identity(42); }"
    );
    // Should fail at analyze, NOT at validate_generics
    assert!(!result.contains("requires explicit type arguments"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — current error IS "requires explicit type arguments"

- [ ] **Step 3: Modify validate_generics**

In `src/semantic/mod.rs`, find the check (~line 745-764) that errors when `type_args.is_empty()` for a generic function/struct. Change to skip (allow) when `type_args.is_empty()`. Keep errors for:
- Partial type args (not 0, not all)
- Type args on non-generic
- Wrong number of type args

- [ ] **Step 4: Run test to verify it passes**

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all existing tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/semantic/mod.rs tests/generics.rs
git commit -m "Relax validate_generics to allow omitted type arguments"
```

---

### Task 6: Generic Receiver Resolution

Add `Type::Generic` and `Type::TypeParam` handling to method calls, field access, and field assignment. These branches will be exercised after the pipeline reorder (Task 7). After monomorphization, all `Type::Generic` are replaced by `Type::Struct`, so these branches are dead code in the post-mono path — but they compile in both paths since the code is shared.

**Files:**
- Modify: `src/semantic/mod.rs` (MethodCall ~2013, FieldAccess ~1956, FieldAssign ~1676)

- [ ] **Step 1: Add helper function `resolve_generic_struct`**

```rust
fn resolve_generic_struct<'a>(
    ty: &Type, resolver: &'a Resolver,
) -> Option<(String, &'a StructInfo, HashMap<String, Type>)> {
    if let Type::Generic { name, args } = ty {
        let si = resolver.lookup_struct(name)?;
        let subst = si.type_params.iter().zip(args).map(|(tp, a)| (tp.name.clone(), a.clone())).collect();
        Some((name.clone(), si, subst))
    } else { None }
}
```

- [ ] **Step 2: Add `Type::Generic` branch to MethodCall**

Look up base struct, substitute type params in method signature, check args, return substituted return type.

- [ ] **Step 3: Add `Type::TypeParam` with protocol bound branch to MethodCall**

Look up protocol, find method, substitute Self if needed. Add error for `TypeParam { bound: None }`.

- [ ] **Step 4: Add `Type::Generic` branch to FieldAccess**

Same pattern: resolve generic struct, look up field, substitute type.

- [ ] **Step 5: Add `Type::Generic` branch to FieldAssign**

Same pattern, plus computed property setter check. Use unification when `ctx` is available, equality when not.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/semantic/mod.rs
git commit -m "Add Generic and TypeParam receiver resolution for method calls and field access"
```

---

### Task 7: Pipeline Reorder

The critical refactoring: move `analyze` before `monomorphize`. Split current `analyze` into pre-mono and post-mono. Wire up the side table.

**Dual-mode approach:** `analyze_expr` and `analyze_stmt` gain `ctx: Option<&mut InferenceContext>` parameter. When `None`, behavior is identical to current code. When `Some`, inference features are active. This keeps one copy of the analysis code.

**Protocol conformance checking** (the existing Phase 3b that checks struct conformances declarations) stays in `analyze_post_mono` only — it does not depend on inference and is simpler to keep in the post-mono path.

**Files:**
- Modify: `src/semantic/mod.rs:919-1163` (split analyze, add ctx parameter to analyze_expr/analyze_stmt)
- Modify: `src/monomorphize.rs:13,101` (accept InferredTypeArgs)
- Modify: `src/lib.rs:19-45` (compile_source, compile_to_bir pipeline)
- Review: `tests/common/mod.rs` (verify test helpers call compile_source, not analyze directly)

- [ ] **Step 1: Add `ctx: Option<&mut InferenceContext>` parameter to `analyze_expr` and `analyze_stmt`**

This is a mechanical change: add the parameter, pass it through all recursive calls. When `ctx` is `None`, all existing behavior is preserved (no inference-related code paths are hit yet since numeric literals still return I32/F64 when ctx is None). When `ctx` is `Some`, the same code runs (for now — inference features will be added in Tasks 8-10).

**Important:** Do NOT change any behavior in this step. Just thread the parameter.

- [ ] **Step 2: Create `analyze_pre_mono` function**

```rust
pub fn analyze_pre_mono(program: &Program) -> Result<InferredTypeArgs> {
    let mut ctx = InferenceContext::new();
    let mut inferred = InferredTypeArgs::new();
    // Phase 1a, 1b, 2: same as current
    // Phase 3: analyze function/struct bodies with Some(&mut ctx)
    // (skip protocol conformance checking — handled in post-mono)
    // Per body: apply_defaults + record_inferred_type_args + reset
    Ok(inferred)
}
```

- [ ] **Step 3: Rename current `analyze` to `analyze_post_mono`**

The existing `analyze` function becomes `analyze_post_mono`. It calls `analyze_expr`/`analyze_stmt` with `ctx: None`. Returns `SemanticInfo` as before. Includes protocol conformance checking (Phase 3b).

- [ ] **Step 4: Update `monomorphize` signature**

In `src/monomorphize.rs`, change to accept `&InferredTypeArgs`:

```rust
pub fn monomorphize(program: &Program, inferred: &InferredTypeArgs) -> Program
```

In `collect_from_expr`, when `type_args.is_empty()`, check `inferred.map.get(&expr.id)` for inferred type args. If found, use `site.type_args`.

- [ ] **Step 5: Update `compile_source` and `compile_to_bir` pipelines**

```rust
let inferred = semantic::analyze_pre_mono(&program)?;
let program = monomorphize::monomorphize(&program, &inferred);
let sem_info = semantic::analyze_post_mono(&program)?;
```

- [ ] **Step 6: Update inline tests and verify test helpers**

Check `tests/common/mod.rs` — it should call `compile_source` not `analyze` directly. Update any inline tests in `src/lib.rs` that call `analyze` to call the new function names.

- [ ] **Step 7: Run full test suite**

Run: `cargo test`
Expected: **ALL existing tests PASS**. Pre-mono analyze returns empty InferredTypeArgs, monomorphize works as before with explicit type args.

**This is the critical milestone. Debug any failures before proceeding.**

- [ ] **Step 8: Commit**

```bash
git add src/semantic/mod.rs src/monomorphize.rs src/lib.rs tests/common/
git commit -m "Reorder pipeline: analyze (pre-mono) before monomorphize"
```

---

### Task 8: Bidirectional Numeric Literal Inference

Replace hardcoded `Type::I32`/`Type::F64` for numeric literals with `IntegerLiteral`/`FloatLiteral` when inference context is active. Add expected type propagation.

**Files:**
- Modify: `src/semantic/mod.rs` (analyze_expr for Number/Float, analyze_stmt for Let/Var/Assign/Return/Yield)
- Create: `tests/type_inference.rs`

- [ ] **Step 1: Write integration tests**

Create `tests/type_inference.rs`:

```rust
mod common;
use common::{compile_and_run, compile_should_fail};

#[test]
fn infer_i64_from_annotation() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { let x: Int64 = 42; return 0; }"
    ), 0);
}

#[test]
fn infer_i64_from_return() {
    assert_eq!(compile_and_run(
        "func to_i64() -> Int64 { return 42; }
         func main() -> Int32 { to_i64(); return 0; }"
    ), 0);
}

#[test]
fn infer_i64_from_function_arg() {
    assert_eq!(compile_and_run(
        "func takes_i64(x: Int64) -> Int64 { return x; }
         func main() -> Int32 { takes_i64(42); return 0; }"
    ), 0);
}

#[test]
fn infer_i64_from_binary_op() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 {
            let x: Int64 = 100;
            let y = x + 42;
            return 0;
        }"
    ), 0);
}

#[test]
fn infer_i64_from_assignment() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { var x: Int64 = 0; x = 42; return 0; }"
    ), 0);
}

#[test]
fn infer_default_i32() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { let x = 42; return x; }"
    ), 42);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bengal type_inference -- --nocapture`
Expected: FAIL — `let x: Int64 = 42` errors because 42 is always I32

- [ ] **Step 3: Add `Expectation` type**

```rust
#[derive(Clone)]
pub(crate) enum Expectation {
    None,
    ExpectType(Type),
}
```

- [ ] **Step 4: Modify Number/Float literal handling in analyze_expr**

When `ctx` is `Some`:
- `ExprKind::Number` → `Type::IntegerLiteral(ctx.fresh_integer())`
- `ExprKind::Float` → `Type::FloatLiteral(ctx.fresh_float())`
- If expected type is available, unify with it

When `ctx` is `None`: existing behavior (return `I32`/`F64` directly).

Note: `ExprKind::Cast` source expressions are analyzed in infer mode (no expected type propagation to the source). The literal type resolves via `apply_defaults` if not constrained elsewhere.

- [ ] **Step 5: Add expected type propagation for statements**

Thread `Expectation` through `analyze_stmt` (add parameter or use a struct):

- **Let/Var**: annotation present → `ExpectType(declared)`, absent → `None`
- **Return**: use `resolver.current_return_type` as expectation.
  *Exception:* if return type is `TypeParam`, use `Expectation::None` (TypeParam cannot unify with IntegerLiteral — proper checking happens post-mono on specialized bodies)
- **Assign**: variable type as expectation
- **FieldAssign**: resolved field type as expectation (uses Generic receiver handling from Task 6)
- **IndexAssign**: array element type as expectation
- **Yield**: enclosing block's expected type
- **Binary ops**: unify both operands' types, then unify result with expected

When `ctx` is `None`: expected type propagation is skipped (no `Expectation` parameter passed, or always `None`).

- [ ] **Step 6: Call apply_defaults and handle range checks**

In `analyze_pre_mono`, after each function body: `ctx.apply_defaults()`. After defaults resolve `IntegerLiteral` to `I32`, verify range (literal value fits in i32). This can be a post-pass on the recorded node types, or deferred to `analyze_post_mono` where range checks already exist.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p bengal type_inference -- --nocapture`
Expected: PASS

- [ ] **Step 8: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 9: Commit**

```bash
git add src/semantic/mod.rs tests/type_inference.rs
git commit -m "Implement bidirectional numeric literal inference"
```

---

### Task 9: Generic Type Argument Inference

Enable omitting type arguments at generic call sites.

**Files:**
- Modify: `src/semantic/mod.rs` (Call and StructInit in analyze_expr)
- Modify: `tests/type_inference.rs`

- [ ] **Step 1: Write integration tests**

Add to `tests/type_inference.rs`:

```rust
#[test]
fn infer_generic_identity() {
    assert_eq!(compile_and_run(
        "func identity<T>(value: T) -> T { return value; }
         func main() -> Int32 { return identity(42); }"
    ), 42);
}

#[test]
fn infer_generic_struct_init() {
    assert_eq!(compile_and_run(
        "struct Box<T> { var value: T; }
         func main() -> Int32 {
            let b = Box(value: 42);
            return b.value;
         }"
    ), 42);
}

#[test]
fn infer_generic_from_expected_type() {
    assert_eq!(compile_and_run(
        "struct Box<T> { var value: T; }
         func main() -> Int32 {
            let b: Box<Int64> = Box(value: 42);
            return 0;
         }"
    ), 0);
}

#[test]
fn infer_generic_multiple_type_params() {
    assert_eq!(compile_and_run(
        "struct Pair<A, B> { var first: A; var second: B; }
         func main() -> Int32 {
            let p = Pair(first: 42, second: true);
            return p.first;
         }"
    ), 42);
}

#[test]
fn infer_generic_return_type_only() {
    assert_eq!(compile_and_run(
        "func make_val<T>(x: T) -> T { return x; }
         func main() -> Int32 {
            let x: Int64 = make_val(42);
            return 0;
         }"
    ), 0);
}

#[test]
fn infer_nested_generics() {
    assert_eq!(compile_and_run(
        "struct Pair<A, B> { var first: A; var second: B; }
         struct Box<T> { var value: T; }
         func main() -> Int32 {
            let x = Box(value: Pair(first: 1, second: true));
            return x.value.first;
         }"
    ), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p bengal infer_generic -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement generic function call inference**

In `analyze_expr` for `ExprKind::Call`, when `ctx` is `Some` AND `type_args.is_empty()` AND function has type params:
1. Create `ctx.fresh_var()` per type param → `InferVar(id)`
2. `ctx.register_call_site(expr.id, var_ids, func.type_params, func_name)`
3. Build substitution map `{param_name → InferVar(id)}`
4. Compute result type (substitute return type through map)
5. **Unify result with expected type FIRST** (bidirectional flow)
6. Check each argument against substituted param type (unification)
7. Return result type

- [ ] **Step 4: Implement generic struct init inference**

Same pattern for `ExprKind::StructInit` when `type_args.is_empty()` AND struct has type params:
1. Fresh vars, register call site
2. Result type: `Generic { name, args: [InferVar(id), ...] }`
3. **Unify with expected type FIRST**
4. Check fields against substituted field types
5. Return result type

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p bengal infer_generic -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/semantic/mod.rs tests/type_inference.rs
git commit -m "Implement generic type argument inference from args and expected type"
```

---

### Task 10: Loop Inference

Replace resolver's break type tracking with InferVar-based unification when inference context is active.

**Dual-mode approach:** The resolver keeps BOTH tracking mechanisms:
- `loop_break_types: Vec<Option<Type>>` — used when `ctx` is `None` (post-mono, current behavior)
- `loop_result_vars: Vec<Option<InferVarId>>` — used when `ctx` is `Some` (pre-mono with inference)

`enter_loop` / `exit_loop` / break handling dispatch to the appropriate mechanism based on whether `ctx` is available.

**Files:**
- Modify: `src/semantic/resolver.rs:87-88,199-232`
- Modify: `src/semantic/mod.rs` (While expression, Break statement)
- Modify: `tests/type_inference.rs`

- [ ] **Step 1: Write integration tests**

```rust
#[test]
fn loop_break_unit() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { while true { break; } return 0; }"
    ), 0);
}

#[test]
fn loop_no_break_unit() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 {
            var i: Int32 = 0;
            while i < 3 { i = i + 1; }
            return i;
         }"
    ), 3);
}

#[test]
fn loop_break_infer_i64() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 {
            let x: Int64 = while true { break 42; } nobreak { yield 0; };
            return 0;
         }"
    ), 0);
}
```

- [ ] **Step 2: Run tests — existing loop tests should still pass**

- [ ] **Step 3: Add `loop_result_vars` to resolver**

In `src/semantic/resolver.rs`, add `loop_result_vars: Vec<Option<InferVarId>>` alongside existing `loop_break_types`. Add methods:
- `enter_loop_infer(&mut self, ctx: &mut InferenceContext) -> InferVarId`
- `current_loop_var(&self) -> Option<InferVarId>`
- `exit_loop_infer(&mut self) -> Option<InferVarId>`

Keep existing `enter_loop`, `exit_loop`, `set_break_type` unchanged for the `ctx: None` path.

- [ ] **Step 4: Update While expression and Break analysis**

In `analyze_expr` for While, when `ctx` is `Some`:
1. `let loop_var = resolver.enter_loop_infer(ctx);`
2. If expected type: `ctx.unify(InferVar(loop_var), expected)?;`
3. Analyze body
4. After body: if loop_var unresolved, unify with `Unit`
5. Nobreak yield: unify with loop_var

In `analyze_stmt` for `Break(Some(expr))` with `ctx` `Some`:
- Unify break value type with `InferVar(loop_var)`

For `Break(None)` with `ctx` `Some`:
- Unify `Unit` with `InferVar(loop_var)`

When `ctx` is `None`: use existing `set_break_type` path.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/semantic/resolver.rs src/semantic/mod.rs tests/type_inference.rs
git commit -m "Implement loop type inference with InferVar-based break tracking"
```

---

### Task 11: Protocol Constraint Validation (Three Stages)

**Files:**
- Modify: `src/semantic/mod.rs` (add validate_inferred_constraints)
- Modify: `src/monomorphize.rs` (add check_specialization_constraints)
- Modify: `tests/type_inference.rs`

- [ ] **Step 1: Write tests for constraint violations**

```rust
#[test]
fn infer_constraint_violation() {
    let result = compile_should_fail(
        "protocol Summable { func sum() -> Int32; }
         struct Wrapper<T: Summable> { var value: T; }
         func main() -> Int32 {
            let w = Wrapper(value: true);
            return 0;
         }"
    );
    assert!(result.contains("does not conform"));
}
```

- [ ] **Step 2: Implement Stage 2 — validate_inferred_constraints**

In `src/semantic/mod.rs`:

```rust
fn validate_inferred_constraints(inferred: &InferredTypeArgs, resolver: &Resolver) -> Result<()> {
    for (_, site) in &inferred.map {
        for (param, arg) in site.type_params.iter().zip(&site.type_args) {
            if is_type_param_annotation(arg) { continue; }  // defer to Stage 3
            if let Some(bound) = &param.bound {
                // Check arg conforms to protocol bound (reuse existing logic)
            }
        }
    }
    Ok(())
}
```

Call at the end of `analyze_pre_mono`.

- [ ] **Step 3: Implement Stage 3 — monomorphize constraint check**

In `src/monomorphize.rs`, after substituting type args in `generate_specializations`:
Check each concrete arg satisfies its protocol bound.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/semantic/mod.rs src/monomorphize.rs tests/type_inference.rs
git commit -m "Add three-stage protocol constraint validation for inferred type args"
```

---

### Task 12: Package Compilation Path

**Files:**
- Modify: `src/lib.rs:50-105` (compile_package_to_executable)
- Modify: `src/semantic/mod.rs` (analyze_package_pre_mono)

- [ ] **Step 1: Create `analyze_package_pre_mono`**

Adapt `analyze_package` (line 68) to use `InferenceContext`. Key design:
- **One InferenceContext per function/struct body** (reset between bodies, same as single-file)
- **One unified InferredTypeArgs** across all modules
- Cross-module symbols (imported functions/structs) are already in the resolver via the existing import mechanism, so generic call sites in module A referencing module B's generics will have the function signature available

```rust
pub fn analyze_package_pre_mono(graph: &ModuleGraph, pkg: &str) -> Result<InferredTypeArgs> {
    // Phase 1: Global symbol collection across all modules (existing logic)
    // Phase 2: Per-module analysis with InferenceContext
    //   - For each module: analyze bodies with ctx=Some(&mut ctx)
    //   - apply_defaults + record per body
    // Phase 3: validate_inferred_constraints
    Ok(unified_inferred)
}
```

- [ ] **Step 2: Update compile_package_to_executable**

```rust
// 1-2: Parse, build module graph (unchanged)
// 3: validate_generics per module (relaxed)
for mod_info in graph.modules.values() {
    semantic::validate_generics(&mod_info.ast)?;
}
// 4: Unified pre-mono analysis
let inferred = semantic::analyze_package_pre_mono(&graph, &package_name)?;
// 5: Monomorphize per module with unified side table
for mod_info in graph.modules.values_mut() {
    mod_info.ast = monomorphize::monomorphize(&mod_info.ast, &inferred);
}
// 6: Post-mono analysis (existing analyze_package)
let pkg_sem_info = semantic::analyze_package(&graph, &package_name)?;
// 7: BIR → LLVM → link (unchanged)
```

- [ ] **Step 3: Run module tests**

Run: `cargo test -p bengal modules -- --nocapture`
Expected: all tests PASS

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/semantic/mod.rs
git commit -m "Update package compilation path for type inference pipeline"
```

---

### Task 13: Comprehensive End-to-End Test Suite

Add remaining tests from the spec's test strategy not covered by earlier tasks.

**Files:**
- Modify: `tests/type_inference.rs`

- [ ] **Step 1: Add numeric literal edge case tests**

```rust
#[test]
fn infer_f32_from_annotation() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { let x: Float32 = 3.14; return 0; }"
    ), 0);
}

#[test]
fn infer_yield_in_block() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { let x: Int64 = { yield 42; }; return 0; }"
    ), 0);
}

#[test]
fn infer_array_element_type() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { let arr: [Int64; 3] = [1, 2, 3]; return 0; }"
    ), 0);
}
```

- [ ] **Step 2: Add method/field tests on generic structs**

```rust
#[test]
fn method_on_inferred_generic_struct() {
    assert_eq!(compile_and_run(
        "struct Box<T> { var value: T;
            func get() -> T { return self.value; }
         }
         func main() -> Int32 {
            let b = Box(value: 42);
            return b.get();
         }"
    ), 42);
}

#[test]
fn field_assign_on_generic_struct() {
    assert_eq!(compile_and_run(
        "struct Box<T> { var value: T; }
         func main() -> Int32 {
            var b = Box(value: 0);
            b.value = 42;
            return b.value;
         }"
    ), 42);
}
```

- [ ] **Step 3: Add error case tests**

```rust
#[test]
fn error_unresolvable_type() {
    let result = compile_should_fail(
        "func default_value<T>() -> T { return 0; }
         func main() -> Int32 { let x = default_value(); return 0; }"
    );
    assert!(result.contains("cannot infer"));
}

#[test]
fn error_partial_type_args() {
    let result = compile_should_fail(
        "func pair<A, B>(a: A, b: B) -> Int32 { return 0; }
         func main() -> Int32 { return pair<Int32>(42, true); }"
    );
    assert!(result.contains("type argument"));
}

#[test]
fn error_literal_type_mismatch() {
    let result = compile_should_fail(
        "func main() -> Int32 { let x: Bool = 42; return 0; }"
    );
    assert!(result.contains("type mismatch"));
}

#[test]
fn error_integer_float_mismatch() {
    let result = compile_should_fail(
        "func choose<T>(a: T, b: T) -> T { return a; }
         func main() -> Int32 { choose(42, 3.14); return 0; }"
    );
    assert!(result.contains("type mismatch") || result.contains("cannot unify"));
}
```

- [ ] **Step 4: Add explicit type arg coexistence tests**

```rust
#[test]
fn explicit_type_args_still_work() {
    assert_eq!(compile_and_run(
        "func identity<T>(value: T) -> T { return value; }
         func main() -> Int32 { return identity<Int32>(42); }"
    ), 42);
}
```

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 6: Commit**

```bash
git add tests/type_inference.rs
git commit -m "Add comprehensive type inference test suite"
```

---

### Task 14: Cleanup and Final Verification

**Files:**
- All modified files

- [ ] **Step 1: Run cargo clippy**

Run: `cargo clippy -- -W warnings`
Fix any warnings.

- [ ] **Step 2: Run cargo fmt**

Run: `cargo fmt`

- [ ] **Step 3: Run full test suite one final time**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 4: Commit any cleanup changes**

```bash
git add -A
git commit -m "Clean up type inference implementation (clippy + fmt)"
```
