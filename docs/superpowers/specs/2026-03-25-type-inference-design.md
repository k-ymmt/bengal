# Type Inference Design

## Overview

Implement bidirectional type checking with local unification for the Bengal compiler,
inspired by Rust's type inference approach. This enables generic type argument inference,
bidirectional numeric literal inference, and contextual type propagation.

## Goals

1. **Generic type argument inference** — omit explicit type arguments when inferable from context
   - From arguments: `identity(42)` infers `<Int32>`
   - From return type context: `let x: Int32 = default_value()` infers `<Int32>`
   - Nested generics: `Box(value: Pair(first: 1, second: true))` infers `Box<Pair<Int32, Bool>>`
   - Multiple type params from different sources: `make_pair(42, true)` infers `<Int32, Bool>`
2. **Bidirectional numeric literal inference** — numeric literals adapt to expected type
   - Variable annotation: `let x: Int64 = 42`
   - Function argument: `takes_i64(42)`
   - Assignment target: `var x: Int64 = 0; x = 42;`
   - Binary operand: `let x: Int64 = 100; let y = x + 42;`
3. **Bidirectional struct/function inference** — expected type flows into initializers and calls
   - `let b: Box<Int64> = Box(value: 42)` infers type args from the left-hand side

## Non-Goals

- Function overloading (decided against — protocols + generics cover the same use cases)
- Closure type inference (closures are not yet implemented)
- Function return type inference from body
- Cross-function type inference (inference is local to each function body)

## Approach: Bidirectional Type Checking + Local Unification

### Why This Approach

- Covers all required use cases without the complexity of a full constraint solver
- No backtracking needed (Bengal has no overloading)
- Predictable performance (linear in expression size)
- Naturally extensible for future features (closures, enums)
- Same approach used by Rust (modified Hindley-Milner with bidirectional flow)

### Alternatives Considered

**A. Constraint-based solver (Swift-style):** Most expressive but over-engineered for Bengal.
Swift's solver handles overloading via backtracking, which Bengal doesn't need. Adds significant
implementation complexity and debugging difficulty for no practical benefit.

**C. Multi-pass inference:** Simpler to understand but awkward for bidirectional information flow.
Nested generics and return-type-only inference don't work cleanly across separate passes.

## Detailed Design

### 1. Core Infrastructure — Type Variables and Union-Find

#### Type Variables

Three new variants added to the `Type` enum:

```rust
pub enum Type {
    // ... existing variants ...
    InferVar(InferVarId),        // unconstrained type variable
    IntegerLiteral(InferVarId),  // resolves to I32 or I64 (default: I32)
    FloatLiteral(InferVarId),    // resolves to F32 or F64 (default: F64)
}
```

- `InferVar` — general-purpose type variable for generic type parameter inference
- `IntegerLiteral` — integer literal whose concrete type is not yet known
- `FloatLiteral` — float literal whose concrete type is not yet known

#### Union-Find (InferenceContext)

```rust
pub struct InferenceContext {
    var_states: Vec<VarState>,
    /// Tracks which NodeIds have inferred type args and their corresponding InferVarIds.
    /// Populated during expression checking when a generic call site has empty type_args.
    pending_type_args: Vec<(NodeId, Vec<InferVarId>)>,
}

enum VarState {
    Unbound,
    Linked(InferVarId),
    Resolved(Type),
}
```

Operations:
- `fresh_var()` — create a new `InferVar`
- `fresh_integer()` — create a new `IntegerLiteral`
- `fresh_float()` — create a new `FloatLiteral`
- `resolve(id)` — follow Union-Find chain to current type (with path compression)
- `unify(ty1, ty2)` — unify two types, fail on conflict
- `register_call_site(node_id, infer_var_ids)` — record that a call site needs inferred type args
- `record_inferred_type_args(inferred)` — after `apply_defaults`, resolve all pending call sites
  through Union-Find, convert to `TypeAnnotation` via `type_to_annotation`, and populate the
  side table

#### Unification Rules

Unification is **symmetric**: `unify(A, B)` and `unify(B, A)` always produce the same result.
The implementation must resolve both sides through Union-Find before applying rules.

| ty1 | ty2 | Result |
|---|---|---|
| `InferVar(a)` | any type `T` | `a → T` |
| `IntegerLiteral(a)` | `I32` or `I64` | `a → I32/I64` |
| `IntegerLiteral(a)` | `InferVar(b)` | `b → IntegerLiteral(a)` (preserves literal flexibility) |
| `IntegerLiteral(a)` | `IntegerLiteral(b)` | link `a` and `b` |
| `IntegerLiteral(a)` | `FloatLiteral(b)` | error: cannot unify integer literal with float literal |
| `IntegerLiteral(a)` | `Bool`, `Unit`, etc. | error |
| `FloatLiteral(a)` | `F32` or `F64` | `a → F32/F64` |
| `FloatLiteral(a)` | `InferVar(b)` | `b → FloatLiteral(a)` |
| `FloatLiteral(a)` | `FloatLiteral(b)` | link `a` and `b` |
| `FloatLiteral(a)` | `I32`, `Bool`, etc. | error |
| `Struct(name1)` | `Struct(name2)` | success if name1 == name2, else error |
| `Struct(name)` | `Generic { .. }` | error (arity mismatch: non-generic vs generic) |
| `Array { elem: T1, size: N }` | `Array { elem: T2, size: N }` | recursively unify T1, T2 |
| `Array { size: N1 }` | `Array { size: N2 }` | error if N1 != N2 |
| `Generic { name, args1 }` | `Generic { name, args2 }` | pairwise unify args (same name, same arity) |
| `Generic { name1 }` | `Generic { name2 }` | error if name1 != name2 |
| `TypeParam { name }` | `TypeParam { name }` (same name) | success |
| `TypeParam` | different type | error (TypeParam is opaque in pre-mono pass) |
| same concrete types | success | |
| different concrete types | error | |

### 2. Bidirectional Type Checking — check/infer Modes

#### Core API

```rust
enum Expectation {
    None,              // infer mode
    ExpectType(Type),  // check mode
}

fn check_expr(
    expr: &Expr,
    expected: Expectation,
    ctx: &mut InferenceContext,
    resolver: &Resolver,
) -> Result<Type>
```

#### Behavior Per Expression

| Expression | infer mode | check mode |
|---|---|---|
| `42` | return `IntegerLiteral(fresh)` | unify with expected type |
| `3.14` | return `FloatLiteral(fresh)` | unify with expected type |
| `true`/`false` | return `Bool` | unify `Bool` with expected |
| `x` (variable) | return type from scope | unify with expected |
| `self` | return self context type | unify with expected |
| `a + b` | unify operands, return result type | unify result with expected |
| `!a` | check `a` as `Bool`, return `Bool` | unify `Bool` with expected |
| `foo(args)` | see generic inference section | unify return type with expected |
| `obj.method(args)` | see method call resolution section | unify return type with expected |
| `Foo(fields)` | see generic inference section | unify struct type with expected |
| `if`/`block` | infer from branches | propagate expected into branches |
| `while` | see loop type inference section | propagate expected into break/nobreak |
| `expr as T` | infer `expr`, return `T` | return `T` (cast target is explicit) |
| `obj.field` | see field access resolution section | unify with expected |
| `obj[index]` | see index access resolution section | unify with expected |
| `[a, b, c]` | infer from first element, check rest | propagate expected element type |

#### Expected Type Propagation in Statements

**let with annotation (check mode):**
```
let x: Int64 = 42;
```
1. Check `42` with expected type `I64`
2. `42` → `IntegerLiteral(v0)`, unify with `I64` → `v0 = I64`

**let without annotation (infer mode):**
```
let x = 42;
```
1. Infer `42` → `IntegerLiteral(v0)`
2. After function body completes, `v0` falls back to `I32`

**Assignment / FieldAssign / IndexAssign (check mode from target type):**
```
var x: Int64 = 0;
x = 42;              // check 42 against x's type (I64)
obj.field = 42;      // check 42 against field's type
arr[0] = 42;         // check 42 against array element type
```

**return (check mode from function return type):**
```
func foo() -> Int64 { return 42; }
```
1. Function return type `I64` is the expected type for the return expression
2. Check `42` with expected `I64` → resolved

**yield (check mode from enclosing block's expected type):**
```
let x: Int64 = { yield 42; };
```
1. Block is checked with expected `I64`
2. `yield 42` propagates expected `I64` into `42`

**break with value (check mode from loop's InferVar):**
```
let x: Int64 = while cond { break 42; } nobreak { yield 0; };
```
1. Loop expression is checked with expected `I64`
2. Loop result InferVar `v0` is unified with expected `I64`
3. `break 42` and `nobreak yield 0` both unify against `v0`

### 3. Numeric Literals and Binary Operations

#### Binary Operation Inference

Both operands must have the same numeric type. Expected type propagates through.

```
let x: Int64 = 100;
let y = x + 42;
```
1. `x` → `I64`, `42` → `IntegerLiteral(v0)`
2. Unify operands: `unify(I64, IntegerLiteral(v0))` → `v0 = I64`
3. Result type: `I64`

```
let x: Int64 = 1 + 2;
```
1. Expected `I64` for `1 + 2`
2. `1` → `IntegerLiteral(v0)`, `2` → `IntegerLiteral(v1)`
3. Unify operands → `v0` and `v1` linked
4. Unify result `IntegerLiteral(v0)` with expected `I64` → resolved

#### Comparison operators

Result type is always `Bool`. Expected type does NOT propagate to operands
(since the result is `Bool`, not the operand type).

#### Default Fallback

After checking each function/struct-member body, resolve remaining variables:

```rust
fn apply_defaults(ctx: &mut InferenceContext) -> Result<()> {
    for id in 0..ctx.var_states.len() {
        match ctx.resolve(id) {
            IntegerLiteral(_) => ctx.set_resolved(id, Type::I32),
            FloatLiteral(_)   => ctx.set_resolved(id, Type::F64),
            InferVar(_)       => return Err("cannot infer type"),
            _                 => {} // already resolved
        }
    }
    Ok(())
}
```

#### Float Literal Precision Note

The AST currently stores float literals as `f64` (`ExprKind::Float(f64)`). When a float literal
resolves to `F32`, there may be precision loss since the value was already parsed as `f64`.
This is acceptable for now — the same behavior exists in Rust (`3.14f32` is parsed as `f64` then
narrowed). If this becomes a problem in the future, the AST can be changed to store the literal
as a string and defer parsing to after type resolution.

### 4. Generic Type Argument Inference

#### Mechanism

When type arguments are omitted at a generic call site, assign a fresh `InferVar` to each
type parameter and resolve via unification with argument types and expected type.

#### Generic Function Call

```
func identity<T>(value: T) -> T { return value; }
let x = identity(42);
```
1. Signature: `<T>(value: T) -> T`
2. Type args omitted → `T` = `InferVar(v0)`
3. `ctx.register_call_site(expr.id, vec![v0])` — record for side table
4. Substitution map: `{T → InferVar(v0)}`
5. Parameter type after substitution: `value: InferVar(v0)`
6. Check argument `42` with expected `InferVar(v0)` → `unify(IntegerLiteral(v1), InferVar(v0))` → `v0 = IntegerLiteral(v1)`
7. Return type: `InferVar(v0)` → `IntegerLiteral(v1)` → fallback `I32`

#### Return-Type-Only Inference

```
func default_value<T>() -> T { ... }
let x: Int32 = default_value();
```
1. `T` → `InferVar(v0)`
2. No arguments → no info from args
3. Return type `InferVar(v0)` unified with expected `I32` → `v0 = I32`

#### Struct Initialization

```
struct Box<T> { var value: T; }
let b = Box(value: 42);
```
1. `T` → `InferVar(v0)`
2. Field `value` type: `InferVar(v0)`
3. Check `42` → `unify(IntegerLiteral(v1), InferVar(v0))` → `v0 = IntegerLiteral(v1)`
4. Result: `Generic { "Box", [IntegerLiteral(v1)] }` → fallback `Box<Int32>`

#### Bidirectional Struct Initialization

```
let b: Box<Int64> = Box(value: 42);
```
1. `T` → `InferVar(v0)`
2. Result type: `Generic { "Box", [InferVar(v0)] }`
3. Unify with expected `Generic { "Box", [I64] }` → `v0 = I64`
4. Field `value` type is now `I64` (v0 resolved)
5. Check `42` with `I64` → resolved

**Key:** unify result type with expected type BEFORE checking arguments, so expected type
information flows into argument checking.

#### Method Call Resolution on Generic and TypeParam Receivers

The current method call analysis only handles `Type::Struct`. The pre-mono pass introduces
`Type::Generic` and `Type::TypeParam` receivers that also need method resolution.

**Type::Generic receiver** (affects all code with generic struct instances):

```
struct Wrapper<T> {
    var value: T;
    func get() -> T { return self.value; }
}
let w = Wrapper(value: 42);
w.get();  // receiver type is Generic { "Wrapper", [IntegerLiteral(v0)] }
```

Resolution:
1. Receiver has type `Generic { name, args }`
2. Look up base struct `name` in `struct_defs`
3. Build substitution map from struct's `type_params` to `args`
4. Look up method in struct info
5. Substitute method signature (param types and return type) through the map
6. Check arguments against substituted param types
7. Return substituted return type

```rust
Type::Generic { name, args } => {
    let struct_info = resolver.lookup_struct(&name)?.clone();
    let subst: HashMap<String, Type> = struct_info.type_params.iter()
        .zip(args.iter())
        .map(|(tp, arg)| (tp.name.clone(), arg.clone()))
        .collect();
    let method_info = struct_info.lookup_method(method)?;
    let return_type = substitute_type(&method_info.return_type, &subst);
    // ... check args with substituted param types ...
    Ok(return_type)
}
```

**Type::TypeParam receiver with protocol bound** (affects generic function bodies):

```
func call_sum<T: Summable>(item: T) -> Int32 {
    return item.sum();  // receiver type is TypeParam { name: "T", bound: Some("Summable") }
}
```

Resolution:
1. Receiver has type `TypeParam { name, bound: Some(proto) }`
2. Look up protocol `proto` in `protocol_defs`
3. Find method in protocol's method signatures
4. Use protocol method signature as-is (return type may be a concrete type or Self)
5. If return type is `Self`, substitute with `TypeParam { name, bound }`

```rust
Type::TypeParam { name, bound: Some(proto) } => {
    let proto_info = resolver.lookup_protocol(&proto)?.clone();
    let method_sig = proto_info.lookup_method(method)?;
    // If return type references Self, substitute with the TypeParam
    let return_type = substitute_self(&method_sig.return_type,
        &Type::TypeParam { name: name.clone(), bound: Some(proto.clone()) });
    // ... check args ...
    Ok(return_type)
}
```

**Type::TypeParam without bound:**
```
Type::TypeParam { bound: None, .. } => {
    Err("method call on unconstrained type parameter")
}
```

#### Field Access and Index Access on Generic Types

Similar to method calls, field access and index access need to handle `Type::Generic`:

**Field access on Generic receiver:**
```
let w = Wrapper(value: 42);
let v = w.value;  // receiver type is Generic { "Wrapper", [...] }
```
1. Look up base struct from `Generic { name, .. }`
2. Build substitution map from type params to type args
3. Look up field type and substitute

**Index access on Generic/InferVar array:**
Works as before — the array type `Array { element, size }` is resolved through unification,
and element type may be an `InferVar` that resolves later.

#### Loop Type Inference

The current resolver tracks break types via `loop_break_types: Vec<Option<Type>>` with direct
equality checking (`*existing != ty`). This is incompatible with inference because
`IntegerLiteral(v0) != IntegerLiteral(v1)` even when they should unify.

**Change:** Replace `loop_break_types` with `InferVar`-based tracking.

```rust
// In resolver.rs
loop_result_vars: Vec<Option<InferVarId>>,  // was: loop_break_types: Vec<Option<Type>>
```

**enter_loop:**
```rust
pub fn enter_loop(&mut self, ctx: &mut InferenceContext) -> InferVarId {
    let result_var = ctx.fresh_var();
    self.loop_result_vars.push(Some(result_var));
    self.loop_depth += 1;
    result_var
}
```

**Processing break:**
```rust
// In check_stmt for Stmt::Break(Some(expr)):
let break_ty = check_expr(expr, Expectation::None, ctx, resolver)?;
let loop_var = resolver.current_loop_var();
ctx.unify(break_ty, Type::InferVar(loop_var))?;
```

**Processing nobreak yield:**
```rust
// nobreak block's yield also unifies against the loop var
let nobreak_ty = check_block(nobreak_block, Expectation::None, ctx, resolver)?;
let loop_var = resolver.current_loop_var();
ctx.unify(nobreak_ty, Type::InferVar(loop_var))?;
```

**With expected type from context:**
```rust
// When while expression has expected type:
let loop_var = resolver.enter_loop(ctx);
// Unify expected type with loop result var FIRST
if let Expectation::ExpectType(expected) = expected {
    ctx.unify(Type::InferVar(loop_var), expected)?;
}
// Then analyze body — break/nobreak will unify against loop_var
analyze_loop_body(...);
```

This allows:
```
let x: Int64 = while cond { break 42; } nobreak { yield 0; };
```
1. `loop_var = InferVar(v0)`, unify with expected `I64` → `v0 = I64`
2. `break 42`: `IntegerLiteral(v1)` unified with `v0` → `v1 = I64`
3. `nobreak yield 0`: `IntegerLiteral(v2)` unified with `v0` → `v2 = I64`

And:
```
let x = while cond { break 42; } nobreak { yield 0; };
```
1. `loop_var = InferVar(v0)`, no expected type
2. `break 42`: `unify(IntegerLiteral(v1), InferVar(v0))` → `v0 = IntegerLiteral(v1)`
3. `nobreak yield 0`: `unify(IntegerLiteral(v2), IntegerLiteral(v1))` → linked
4. Fallback → `I32`

#### Nested Generics

```
struct Pair<A, B> { var first: A; var second: B; }
struct Box<T> { var value: T; }
let x = Box(value: Pair(first: 1, second: true));
```
1. `Box.T` → `InferVar(v0)`
2. Field `value` expected: `InferVar(v0)`
3. Inner `Pair`: `A` → `InferVar(v1)`, `B` → `InferVar(v2)`
4. `first: 1` → `v1 = IntegerLiteral(v3)`
5. `second: true` → `v2 = Bool`
6. Pair result: `Generic { "Pair", [IntegerLiteral(v3), Bool] }`
7. Unify with `InferVar(v0)` → `v0` resolved
8. Fallback → `Box<Pair<Int32, Bool>>`

#### Generic Function Bodies (Pre-Monomorphization)

During the pre-mono analysis pass, generic function bodies ARE analyzed. `TypeParam` types are
treated as opaque concrete types — they unify only with themselves (same name). This allows
inference within generic bodies:

```
func wrap<T>(value: T) -> Box<T> {
    return Box(value: value);  // infers Box's type arg as T
}
```
1. `Box.U` → `InferVar(v0)` (fresh var for Box's type param)
2. `value` has type `TypeParam("T")`
3. `unify(InferVar(v0), TypeParam("T"))` → `v0 = TypeParam("T")`
4. Result: `Box<T>` — correct, will be specialized after monomorphization

When `v0` resolves to `TypeParam("T")`, `type_to_annotation` converts it to
`TypeAnnotation::Named("T")`. The monomorphizer's `substitute_type` already handles `Named`
names that match substitution keys, so this works correctly during monomorphization.

Method calls on bounded type parameters within generic bodies are resolved via protocol
lookup (see "Method Call Resolution" above):

```
func call_sum<T: Summable>(item: T) -> Int32 {
    return item.sum();  // resolved via Summable protocol
}
```

#### validate_generics Changes

- Omitted type arguments at generic call sites → **no longer an error** (inference will handle it)
- Partial type argument specification (not 0, not all) → still an error
- Type arguments on non-generic functions/structs → still an error

#### Protocol Constraint Validation for Inferred Type Args

The current `validate_generics` checks protocol constraints when explicit type arguments are
present. For inferred type arguments, constraint validation moves to after `apply_defaults`
in the analysis pass:

```rust
fn validate_inferred_constraints(
    inferred: &InferredTypeArgs,
    resolver: &Resolver,
) -> Result<()> {
    for (node_id, type_args) in &inferred.map {
        // Look up the type params with bounds for this call site
        // Check each inferred type arg satisfies its protocol bound
        // SKIP TypeParam args — their constraints are checked post-mono
        // when concrete types are substituted in
    }
    Ok(())
}
```

**TypeParam args are skipped** in this validation. When inference within a generic body resolves
a type argument to `TypeParam("T")`, the constraint cannot be validated until monomorphization
substitutes a concrete type. The post-mono `analyze_post_mono` pass handles this case because
after monomorphization, all `TypeParam`s are replaced with concrete types and the existing
constraint checking works as-is.

Example:
```
struct Wrapper<T: Summable> { var value: T; }
func forward<U: Summable>(v: U) -> Wrapper<U> {
    return Wrapper(value: v);  // inferred type arg for Wrapper is TypeParam("U")
}
```
- Pre-mono: inference resolves Wrapper's type arg to `TypeParam("U")` → skip constraint check
- Monomorphize: `forward<Int32>` → body becomes `Wrapper<Int32>(value: v)` with concrete arg
- Post-mono: `validate_generics` checks `Int32: Summable` → success (or failure if not conforming)

### 5. Pipeline Integration

#### Module Changes

```
src/semantic/types.rs      — add InferVar/IntegerLiteral/FloatLiteral to Type enum
src/semantic/mod.rs        — replace analyze_expr with check_expr, introduce InferenceContext
src/semantic/resolver.rs   — change loop_break_types to loop_result_vars (InferVarId-based)
src/monomorphize.rs        — accept InferredTypeArgs as fallback lookup
```

#### New Module

```
src/semantic/infer.rs      — InferenceContext, Union-Find, unify, apply_defaults
```

#### Side Table (not AST rewrite)

The AST remains immutable. Inferred type arguments are stored in a side table:

```rust
pub struct InferredTypeArgs {
    map: HashMap<NodeId, Vec<TypeAnnotation>>,
}
```

The side table stores `Vec<TypeAnnotation>` (not `Vec<Type>`) because `monomorphize` operates
on AST-level types (`TypeAnnotation`). After inference resolves a type variable to a concrete
`Type`, it is converted back to `TypeAnnotation` before storing:

```rust
fn type_to_annotation(ty: &Type) -> TypeAnnotation {
    match ty {
        Type::I32 => TypeAnnotation::I32,
        Type::I64 => TypeAnnotation::I64,
        Type::F32 => TypeAnnotation::F32,
        Type::F64 => TypeAnnotation::F64,
        Type::Bool => TypeAnnotation::Bool,
        Type::Unit => TypeAnnotation::Unit,
        Type::Struct(name) => TypeAnnotation::Named(name.clone()),
        Type::TypeParam { name, .. } => TypeAnnotation::Named(name.clone()),
        Type::Generic { name, args } => TypeAnnotation::Generic {
            name: name.clone(),
            args: args.iter().map(type_to_annotation).collect(),
        },
        Type::Array { element, size } => TypeAnnotation::Array {
            element: Box::new(type_to_annotation(element)),
            size: *size,
        },
        // InferVar/IntegerLiteral/FloatLiteral must be resolved before calling this
        Type::InferVar(_) | Type::IntegerLiteral(_) | Type::FloatLiteral(_) => {
            unreachable!("unresolved type variable in side table")
        }
    }
}
```

Monomorphize checks AST `type_args` first; if empty, falls back to the side table.
This keeps "what the user wrote" separate from "what was inferred."

#### Pipeline Order

```
Current:  validate_generics → monomorphize → analyze → BIR → LLVM
New:      validate_generics(relaxed) → analyze(with inference) → monomorphize(with side table) → analyze_post_mono → BIR → LLVM
```

Key change: `monomorphize` moves AFTER `analyze` because type inference must resolve
generic type arguments before monomorphization can specialize them.

#### analyze Function (Pre-Mono, With Inference)

```rust
pub fn analyze(program: &Program) -> Result<InferredTypeArgs> {
    let mut ctx = InferenceContext::new();
    let mut inferred = InferredTypeArgs::new();
    let mut resolver = Resolver::new();
    // Phase 1a, 1b, 2: unchanged (register symbols, resolve types, validate main)
    // Phase 3a: struct member bodies (initializers, methods, computed properties)
    for struct_def in &program.structs {
        analyze_struct_member_bodies(struct_def, &mut resolver, &mut ctx)?;
        ctx.apply_defaults()?;
        ctx.record_inferred_type_args(&mut inferred);
        ctx.reset();
    }
    // Phase 3b: function bodies
    for func in &program.functions {
        analyze_function_body(func, &mut resolver, &mut ctx)?;
        ctx.apply_defaults()?;
        ctx.record_inferred_type_args(&mut inferred);
        ctx.reset(); // reset per function
    }
    // Phase 4: validate protocol constraints for inferred type args (skip TypeParam args)
    validate_inferred_constraints(&inferred, &resolver)?;
    Ok(inferred)
}
```

#### analyze_post_mono Function

`analyze_post_mono` is the current `analyze` function — full semantic checking on the
monomorphized program. After monomorphization, all generic definitions have been replaced
with specialized versions, so this pass sees only concrete types. It produces the `SemanticInfo`
needed by BIR lowering.

Responsibilities:
- Symbol registration and type resolution (same as current Phase 1)
- main function validation (same as current Phase 2)
- Full body analysis with type checking (same as current Phase 3)
- Protocol conformance checking (same as current Phase 3b)
- Produce `SemanticInfo` (node type map, struct layouts, etc.) for BIR

This function does NOT use `InferenceContext` — all types are concrete at this point.

#### lib.rs

```rust
pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    semantic::validate_generics(&program)?;
    let inferred = semantic::analyze(&program)?;
    let program = monomorphize::monomorphize(&program, &inferred);
    let sem_info = semantic::analyze_post_mono(&program)?;
    let mut bir = bir::lower_program(&program, &sem_info)?;
    bir::optimize_module(&mut bir);
    let obj_bytes = codegen::compile(&bir)?;
    Ok(obj_bytes)
}
```

#### Other Compilation Paths

The same pipeline change applies to all compilation entry points:

- `compile_source` — single-file compilation (shown above)
- `compile_to_bir` — compile to BIR for testing; same pipeline change
- `compile_package_to_executable` — multi-module compilation; each module gets
  `analyze` (with inference) before `monomorphize`, and `analyze_package` is updated
  to use the new pipeline order
- Inline tests (e.g., `test_compile_to_module_reexport`) that call `analyze` directly
  must be updated to the new pipeline order

The multi-module path in `analyze_package` follows the same pattern: global symbol
collection → per-module inference analysis → per-module monomorphization →
per-module post-mono analysis.

### 6. Error Messages and Edge Cases

#### Error Messages

**Unification failure:**
```
let x: Bool = 42;
→ type mismatch: expected 'Bool', found integer literal
```

**Unresolvable type parameter:**
```
let x = default_value();
→ cannot infer type parameter 'T' for function 'default_value'; add explicit type annotation
```

**Integer literal out of range:**
```
let x: Int32 = 9999999999;
→ integer literal '9999999999' is out of range for 'Int32'
```

Range checking happens when unification resolves a literal to a concrete integer type.

**Integer/float literal mismatch:**
```
func choose<T>(a: T, b: T) -> T { ... }
let x = choose(42, 3.14);
→ type mismatch: cannot unify integer literal with float literal
```

**Method call on unconstrained type parameter:**
```
func bad<T>(item: T) -> Int32 { return item.sum(); }
→ method call on unconstrained type parameter 'T'
```

#### Edge Cases

**Partial type argument specification is forbidden:**
```
make_pair<Int32>(42, true);  // error
```
Type arguments must be all-or-nothing. Consistent with Swift and Rust.

**Explicit type arguments take precedence:**
```
let x = identity<Int64>(42);  // T=Int64, 42 resolves to Int64
```
Side table is not populated; AST `type_args` is used directly by monomorphize.

**Empty array literals:**
```
let arr: [Int32; 0] = [];   // OK: element type from expected type
let arr = [];               // error: cannot infer type of empty array literal
```

**Protocol constraint validation:**
```
struct Wrapper<T: Summable> { var value: T; }
let w = Wrapper(value: true);  // error: 'Bool' does not conform to 'Summable'
```
After `apply_defaults`, `validate_inferred_constraints` checks that inferred type args
satisfy their protocol bounds (skipping TypeParam args, which are validated post-mono).

### 7. Test Strategy

#### Numeric Literal Inference
- Variable annotation: `let x: Int64 = 42`
- Function argument: `takes_i64(42)`
- Assignment: `var x: Int64 = 0; x = 42;`
- Field assignment: `obj.field = 42;` (where field is Int64)
- Index assignment: `arr[0] = 42;` (where arr is [Int64; N])
- Binary operand: `let x: Int64 = 100; let y = x + 42;`
- Default fallback: `let a = 42;` → Int32, `let b = 3.14;` → Float64
- Return statement: `func foo() -> Int64 { return 42; }`
- Yield in block: `let x: Int64 = { yield 42; };`
- Break with value: `let x: Int64 = while cond { break 42; } nobreak { yield 0; };`
- Multiple breaks with literals: `while cond { if x { break 1; } break 2; }`

#### Generic Function Inference
- From arguments: `identity(42)`
- Multiple type params: `make_pair(42, true)`
- Return-type-only: `let x: Int32 = default_value()`

#### Struct Initialization Inference
- From fields: `Box(value: 42)`
- From expected type: `let b: Box<Int64> = Box(value: 42)`
- Nested: `Box(value: Pair(first: 1, second: true))`

#### Method Calls on Generic Structs
- Method on inferred generic struct: `let w = Wrapper(value: 42); w.get();`
- Method with expected type: `let v: Int64 = Wrapper(value: 42).get();`
- Method call in generic body with protocol bound: `item.sum()` where `T: Summable`

#### Array Literal Inference
- Element type from annotation: `let arr: [Int64; 3] = [1, 2, 3];`
- Empty array with annotation: `let arr: [Int32; 0] = [];`

#### Coexistence With Explicit Type Arguments
- Explicit still works: `identity<Int64>(42)`
- Explicit takes precedence over context: `let x: Int32 = identity<Int64>(42)` → error

#### Error Cases
- Unresolvable type variable: `let x = default_value();`
- Partial type args: `make_pair<Int32>(42, true);`
- Literal out of range: `let x: Int32 = 9999999999;`
- Constraint violation: `Wrapper(value: true)` when `T: Summable`
- Type mismatch: `let x: Bool = 42;`
- Integer/float mismatch: `choose(42, 3.14)` where T must be one type
- Method call on unconstrained type param: `item.foo()` where T has no bound

#### Regression
- All existing tests continue to pass unchanged
