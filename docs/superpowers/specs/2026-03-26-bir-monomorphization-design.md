# Design: BIR-Level Monomorphization

## Overview

Migrate monomorphization from AST level to BIR level. Currently, generic functions
are specialized by cloning and substituting AST nodes before BIR lowering. The new
architecture keeps BIR generic (with type parameters) and performs type substitution
on-the-fly during codegen, following the same model as Rust's MIR-based
monomorphization.

### Motivation

- **Separate compilation efficiency**: When generic BIR is serialized in interface
  files (`.bengalmod`), consumers only need codegen — no re-parsing, re-analysis,
  or re-lowering of generic function bodies.
- **Architecture improvement**: Cleaner pipeline with a single analysis phase,
  BIR-level optimizations that apply uniformly to generic and non-generic code,
  and a foundation for future optimizations (inlining, constant propagation at
  BIR level).

### Approach: Rust-Style On-The-Fly Substitution

- BIR retains generic type parameters (`BirType::TypeParam`).
- No BIR cloning for monomorphization.
- A mono collector traverses BIR to discover required concrete instances.
- Codegen substitutes type parameters with concrete types per-instruction as it
  generates LLVM IR.
- This follows Rust's MIR approach where `Instance<'tcx>` pairs a function with
  concrete type arguments and codegen reads generic MIR while applying
  substitutions on-the-fly.

## Data Structure Changes

### BirType

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]  // Hash added for Instance dedup
pub enum BirType {
    Unit, I32, I64, F32, F64, Bool,
    Struct { name: String, type_args: Vec<BirType> },  // CHANGED: type_args for generic structs
    Array { element: Box<BirType>, size: u64 },
    TypeParam(String),  // NEW: "T", "U", etc.
}
```

The `Struct` variant changes from `Struct(String)` to include `type_args`. For
non-generic structs, `type_args` is empty. For `Pair<Int32, T>`, it would be
`Struct { name: "Pair", type_args: [I32, TypeParam("T")] }`. `Hash` is added
to support `Instance` deduplication in the mono collector.

### BirFunction

```rust
pub struct BirFunction {
    pub name: String,
    pub type_params: Vec<String>,  // NEW: ["T", "U"], empty for non-generic
    pub params: Vec<(Value, BirType)>,
    pub return_type: BirType,
    pub blocks: Vec<BasicBlock>,
    pub body: Vec<CfgRegion>,
}
```

### Call Instruction

```rust
Call {
    result: Value,
    func_name: String,
    args: Vec<Value>,
    type_args: Vec<BirType>,  // NEW: empty for non-generic calls
    ty: BirType,
}
```

**`Call.ty` semantics**: The `ty` field stores the **resolved return type at the
call site**, not the callee's raw generic return type. During lowering, when the
call site provides concrete `type_args`, the callee's generic return type is
substituted immediately. For example, calling `identity<Int32>(42)` produces
`Call { ty: I32, ... }` not `Call { ty: TypeParam("T"), ... }`. When the caller
is itself generic and the type args contain `TypeParam`, the `ty` field uses the
**caller's** type parameter names. This ensures codegen only needs the current
function's substitution map.

### StructInit Instruction

```rust
StructInit {
    result: Value,
    struct_name: String,
    fields: Vec<(String, Value)>,
    type_args: Vec<BirType>,  // NEW: empty for non-generic structs
    ty: BirType,
}
```

## Pipeline Changes

### Current Pipeline

```
parse -> validate_generics -> analyze_pre_mono -> monomorphize(AST)
       -> analyze_post_mono -> lower_program -> optimize -> codegen
```

### Target Pipeline

```
parse -> validate_generics -> analyze(unified) -> lower_program(generic-aware)
       -> optimize -> mono_collect(BIR scan) -> codegen(with substitution)
```

### Key Changes

1. **Unified analysis**: Extend `analyze_pre_mono` to also build `SemanticInfo`
   (struct definitions, function signatures) and type-check generic function
   bodies with type parameter constraints. Replaces `analyze_post_mono`.

2. **Generic-aware lowering**: `lower_program` handles type parameters in
   function signatures, local variables, and expressions. AST `TypeAnnotation`
   with type parameters maps to `BirType::TypeParam`.

3. **Mono collector** (new): Traverses BIR starting from entry points (e.g.,
   `main`), collects all required concrete instances by examining `Call`
   instructions' `type_args`. Recursive discovery with deduplication.

4. **Codegen with substitution**: For each `Instance` (function name + concrete
   type args), reads the generic BIR and substitutes `TypeParam` with concrete
   types while generating LLVM IR. Name mangling follows existing convention
   (e.g., `identity` + `[I32]` -> `identity_Int32`).

## Mono Collector

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Instance {
    pub func_name: String,
    pub type_args: Vec<BirType>,  // always concrete (no TypeParam)
}

pub struct MonoCollectResult {
    pub func_instances: Vec<Instance>,
    pub struct_instances: HashSet<(String, Vec<BirType>)>,  // (struct_name, concrete_type_args)
}

pub fn mono_collect(bir: &BirModule, entry: &str) -> MonoCollectResult
```

### Algorithm

1. Add entry point functions (non-generic) to worklist.
2. Pop an `Instance` from worklist.
3. Build the substitution map from the instance's type args.
4. For each `Call` with non-empty `type_args`, resolve `TypeParam` references
   using the substitution map, creating a new concrete `Instance`. Add to
   worklist if not already seen.
5. Scan **all** `BirType` occurrences in the function for struct instances:
   - `BirFunction.params` and `BirFunction.return_type` (function signature)
   - All `BirType` fields in every instruction (`Literal.ty`, `BinaryOp.ty`,
     `Call.ty`, `Cast.from_ty`/`to_ty`, `FieldGet.ty`/`object_ty`,
     `FieldSet.ty`, `StructInit.ty`, `ArrayInit.ty`, `ArrayGet.ty`,
     `ArraySet.ty`)
   - Terminator argument types (`Br.args`, `BrBreak.args`/`value`,
     `BrContinue.args`)
   - Basic block parameter types (`BasicBlock.params`)
   Apply `resolve_bir_type` to each, and for every resolved `BirType::Struct`
   with non-empty `type_args`, record in `struct_instances`. This covers
   nested types like `Array { element: Struct { ... } }` as well as structs
   that appear **only** in function signatures (e.g.,
   `func idPair<T, U>(p: Pair<T, U>) -> Pair<T, U> { return p; }`).
6. Repeat until worklist is empty.
7. Return all discovered function and struct instances.

### Generic Struct Layout Resolution

`BirModule.struct_layouts` stores generic struct layouts with `TypeParam` in
field types:

```rust
// Generic layout for Pair<T, U>
struct_layouts: {
    "Pair" => [("first", TypeParam("T")), ("second", TypeParam("U"))]
}
```

During codegen, for each concrete struct instance (e.g., `("Pair", [I32, Bool])`),
the substitution map is applied to the generic layout to produce a concrete
layout:

```rust
// Concrete layout for Pair<Int32, Bool>
[("first", I32), ("second", Bool)]
```

Codegen's `build_struct_types` is extended to iterate `struct_instances` from
the mono collector result. For each `(name, concrete_type_args)`, it applies
the substitution to the generic layout, creates an LLVM struct type under the
mangled name (e.g., `Pair_Int32_Bool`), and registers it for field index
computation (GEP instructions).

## Codegen Substitution

```rust
fn codegen_instance(instance: &Instance, bir: &BirModule, ...) {
    let func = bir.lookup(&instance.func_name);
    let subst: HashMap<String, BirType> =
        func.type_params.iter().zip(&instance.type_args)
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();

    // For each instruction, resolve types:
    //   BirType::TypeParam("T") -> subst["T"] -> BirType::I32
    // Panic if TypeParam not found in subst (indicates a bug).
}
```

Non-generic functions have empty `type_params` and empty substitution maps,
so no overhead for the common case.

### Type Resolution Utility

A single `resolve_bir_type` function ensures consistent substitution across all
BIR locations — instructions, terminators, block parameters, and struct layouts:

```rust
fn resolve_bir_type(ty: &BirType, subst: &HashMap<String, BirType>) -> BirType {
    match ty {
        BirType::TypeParam(name) => subst.get(name)
            .unwrap_or_else(|| panic!("unresolved TypeParam: {name}"))
            .clone(),
        BirType::Array { element, size } => BirType::Array {
            element: Box::new(resolve_bir_type(element, subst)),
            size: *size,
        },
        BirType::Struct { name, type_args } => BirType::Struct {
            name: name.clone(),
            type_args: type_args.iter().map(|t| resolve_bir_type(t, subst)).collect(),
        },
        other => other.clone(),
    }
}
```

This function is applied to:
- All `BirType` fields in instructions (`Literal.ty`, `BinaryOp.ty`,
  `Call.ty`, `Cast.from_ty`, `Cast.to_ty`, `FieldGet.ty`, etc.)
- Terminator argument types (`Br.args`, `BrBreak.args`, `BrContinue.args`,
  `BrBreak.value`)
- Basic block parameter types (`BasicBlock.params`)
- Struct layouts (for concrete layout computation)

### Protocol Method Call Lowering

Current BIR lowering (`lowering.rs`) only handles `ExprKind::MethodCall` when
the receiver is `BirType::Struct`. For BIR mono, lowering must also handle
method calls on constrained type parameters (`T: Summable`).

#### Cases to Handle

1. **Direct call on type param**: `item.sum()` where `item: T` and `T: Summable`
2. **Chained field + method**: `self.value.sum()` where `self.value: T` and
   `T: Summable` (as in `Wrapper<T: Summable>`)

#### Lowering Strategy

When `ExprKind::MethodCall` encounters a receiver with `BirType::TypeParam`:

1. Look up the type parameter's protocol constraint from `SemanticInfo`
   (derived from `Type::TypeParam { bound: Some(proto) }` in semantic analysis).
2. Look up the method signature in the protocol definition to determine the
   return type.
3. Emit a **protocol method call** in BIR:

```
Call {
    func: "{Protocol}_{method}",  // e.g., "Summable_sum"
    args: [receiver, ...],
    type_args: [TypeParam("T")],  // the receiver's type param
    ty: <return type>,
}
```

The `func_name` format `{Protocol}_{method}` is a placeholder name that does
not correspond to any real function. It is resolved at codegen time via the
conformance map.

For case 2 (`self.value.sum()`), `self.value` is lowered via `FieldGet` which
produces a value of `BirType::TypeParam("T")`. The subsequent `.sum()` call
sees `TypeParam("T")` as the receiver and follows the same protocol method
call path above.

#### Conformance Map and Codegen Resolution

During codegen with `T = Int32`, protocol method resolution proceeds as:

1. Resolve `type_args`: `[TypeParam("T")]` -> `[I32]` via substitution map.
2. Look up the concrete implementation using a **conformance map** stored in
   `BirModule`:

```rust
// BirModule addition
pub conformance_map: HashMap<(String, BirType), String>,
// (protocol_method, concrete_type) -> implementation_name
// e.g., ("Summable_sum", BirType::Struct { name: "Point", type_args: [] }) -> "Point_sum"
// Using BirType (which derives Hash+Eq) as key handles generic struct types
// like Pair<Int32, Bool> without ambiguous string serialization.
```

3. Replace the `Call` target with the concrete function name (e.g., `Point_sum`).

The conformance map is populated during BIR lowering from the semantic analysis
results (protocol conformance declarations). Implementation names follow the
existing BIR lowering convention (`StructName_methodName`).

#### Example: Protocol Method on Constrained TypeParam

```bengal
protocol Summable { func sum() -> Int32; }
struct Point: Summable {
    var x: Int32; var y: Int32;
    func sum() -> Int32 { return self.x + self.y; }
}
struct Wrapper<T: Summable> {
    var value: T;
    func getSum() -> Int32 { return self.value.sum(); }
}
func main() -> Int32 {
    let w = Wrapper<Point>(value: Point(x: 3, y: 4));
    return w.getSum();
}
```

Generic BIR for `Wrapper_getSum` (after mono of `Wrapper` struct but with
`T` still generic in the method):

```
@Wrapper_getSum<T>(%0: Struct("Wrapper", [TypeParam("T")])) -> I32 {
  bb0:
    %1 = field_get %0, "value" : Struct("Wrapper", [TypeParam("T")]) -> TypeParam("T")
    %2 = call @Summable_sum(%1) type_args=[TypeParam("T")] : I32
    return %2
}
```

Conformance map entries:
```
("Summable_sum", Struct("Point", [])) -> "Point_sum"
```

Codegen with `T = Point`:
- `%1` resolves to `BirType::Struct { name: "Point", type_args: [] }`
- `Call @Summable_sum type_args=[Point]` -> conformance map lookup ->
  `Call @Point_sum`

## Example

### Source

```bengal
func identity<T>(value: T) -> T {
    return value;
}
func main() -> Int32 {
    return identity<Int32>(42);
}
```

### Generic BIR (after lowering)

```
@identity<T>(%0: TypeParam("T")) -> TypeParam("T") {
  bb0:
    return %0
}

@main() -> I32 {
  bb0:
    %0 = literal 42 : I32
    %1 = call @identity(%0) type_args=[I32] : I32  // resolved at lowering time
    return %1
}
```

### Mono Collection

Starting from `main`:
- `main` has no type params -> walk its BIR
- Found `Call @identity type_args=[I32]` -> add `Instance("identity", [I32])`
- Walk `identity` with `T=I32` -> no further generic calls
- Result: `[Instance("identity", [I32])]`

### Codegen

Generate `identity_Int32`:
- Substitution: `T -> I32`
- `%0: TypeParam("T")` -> `%0: I32`
- return type `TypeParam("T")` -> `I32`
- Emit LLVM function `@identity_Int32(i32) -> i32`

### Example 2: Generic Struct

#### Source

```bengal
struct Pair<T, U> {
    let first: T
    let second: U
}
func getFirst<T, U>(p: Pair<T, U>) -> T {
    return p.first;
}
func main() -> Int32 {
    let p = Pair<Int32, Bool>(first: 10, second: true);
    return getFirst<Int32, Bool>(p);
}
```

#### Generic BIR

```
struct_layouts: { "Pair" => [("first", TypeParam("T")), ("second", TypeParam("U"))] }

@getFirst<T, U>(%0: Struct("Pair", [TypeParam("T"), TypeParam("U")])) -> TypeParam("T") {
  bb0:
    %1 = field_get %0, "first" : Struct("Pair", [TypeParam("T"), TypeParam("U")]) -> TypeParam("T")
    return %1
}

@main() -> I32 {
  bb0:
    %0 = literal 10 : I32
    %1 = literal 1 : Bool
    %2 = struct_init @Pair { first: %0, second: %1 } type_args=[I32, Bool] : Struct("Pair", [I32, Bool])
    %3 = call @getFirst(%2) type_args=[I32, Bool] : I32  // resolved at lowering
    return %3
}
```

#### Mono Collection

- Walk `main` -> `StructInit @Pair type_args=[I32, Bool]` -> record struct `("Pair", [I32, Bool])`
- Walk `main` -> `Call @getFirst type_args=[I32, Bool]` -> record `Instance("getFirst", [I32, Bool])`
- Walk `getFirst` with `T=I32, U=Bool` -> no further generic calls
- Struct instances: `{("Pair", [I32, Bool])}`
- Func instances: `[Instance("getFirst", [I32, Bool])]`

#### Codegen

- Build concrete layout for `Pair_Int32_Bool`: `[("first", I32), ("second", Bool)]`
- Generate `getFirst_Int32_Bool`: resolve `Struct("Pair", [TypeParam("T"), TypeParam("U")])` -> `Struct("Pair", [I32, Bool])`

## Multi-Module Considerations

### Current Pipeline

`compile_package` runs per-module: `analyze_pre_mono` + `monomorphize` per
module, then `analyze_package` for cross-module post-mono analysis, then
per-module `lower_module` + `compile_module`, and finally `link_objects`.

Each module currently monomorphizes independently — if module A imports generic
`foo` from B and calls `foo<Int32>`, the AST mono creates a specialized
`foo_Int32` in A's AST. This is instantiation-site ownership.

### New Pipeline: Per-Module with Instantiation-Site Ownership

The BIR mono migration preserves the per-module compilation model using
**instantiation-site ownership** with **`linkonce_odr` LLVM linkage**. This
matches how C++ templates and Rust generics handle cross-module instantiation.

#### How It Works

1. **BIR lowering** runs per-module as before. Each module's BIR contains its
   own function definitions. Generic functions imported from other modules are
   accessible via a **package-wide BIR registry** (all modules' BIR is retained
   in memory after lowering).

2. **Mono collector** runs per-module, but has read access to the package-wide
   BIR registry. When module A's collector finds `Call @B_foo type_args=[I32]`,
   it looks up `B_foo`'s generic BIR from B's module and continues recursive
   discovery. The resulting `Instance("B_foo", [I32])` is owned by module A.

3. **Codegen** runs per-module. Each module emits LLVM IR for:
   - Its own non-generic functions (with `external` linkage as before)
   - All generic instances discovered by its mono collector, using
     **`linkonce_odr` linkage** for generic instantiations

4. **Linking**: If both module A and C instantiate `B_foo<Int32>`, both object
   files contain `B_foo_Int32` with `linkonce_odr` linkage. The linker
   deduplicates, keeping only one copy in the final binary.

#### Symbol Ownership Rules

| Symbol type | Linkage | Emitted by |
|------------|---------|------------|
| Non-generic function | `external` | Defining module only |
| Generic instantiation | `linkonce_odr` | Each module that needs it |
| Non-generic struct | N/A (type only) | Defining module's codegen |
| Generic struct instantiation | N/A (type only) | Each module that needs it |

#### Extern Declarations

The existing extern declaration collection (`lib.rs:198`) continues to work for
non-generic cross-module calls. For generic instantiations, no extern
declaration is needed — each module emits its own copy with `linkonce_odr`.

#### Name Mangling for Cross-Module Instances

Generic instantiation names are mangled using the **defining module's path**
(not the calling module's), ensuring all copies of the same instance have the
same symbol name for linker deduplication:

```
B::foo<Int32>  ->  pkgname_B_foo_Int32  (same name whether emitted from A or C)
```

#### Why This Approach

- **Preserves per-module compilation**: Each module compiles independently,
  enabling future parallel codegen.
- **Future-compatible with separate compilation** (§1.6.2): When `.bengalmod`
  files provide generic BIR, consuming modules instantiate locally with
  `linkonce_odr` — the exact same model.
- **Matches industry practice**: C++ uses `linkonce_odr` for template
  instantiations; Rust uses a similar scheme across codegen units.
- **No whole-package codegen bottleneck**: Avoids single-threaded codegen for
  the entire package.

#### Changes to `compile_package_to_executable`

- After all modules are lowered to BIR, build a package-wide BIR registry
  (read-only lookup of generic functions by mangled name).
- Mono collector per module receives a reference to this registry.
- Codegen emits `linkonce_odr` linkage for all generic instantiation functions.
- Struct type construction: generic struct layouts are available from the
  package-wide registry; each module's codegen builds concrete LLVM struct
  types for the struct instances its collector discovered.

## Incremental Migration Strategy

### Phase 1: BIR Data Structure Changes

- Add `BirType::TypeParam(String)` and `Hash` derive to `BirType`
- Change `BirType::Struct(String)` to `BirType::Struct { name, type_args }`
- Add `BirFunction.type_params: Vec<String>`
- Add `type_args: Vec<BirType>` to `Call` and `StructInit`
- Add `conformance_map` to `BirModule`
- Update all existing code for the `Struct` variant change (mechanical)
- Existing code uses `type_params: vec![]`, `type_args: vec![]`
- Update BIR printer for `TypeParam`, `type_args`, and new `Struct` format
- **All existing tests pass unchanged.**

### Phase 2: Generic BIR Lowering

- Extend `lower_program` to handle type parameters in function signatures,
  parameters, return types, and local variables.
- Map AST `TypeAnnotation` with type params to `BirType::TypeParam`.
- Set `BirFunction.type_params` for generic functions.
- Set `Call.type_args` for generic call sites.
- **Extend `ExprKind::MethodCall` lowering** to handle receivers with
  `BirType::TypeParam`: look up the protocol constraint, emit
  `Call { func: "{Protocol}_{method}", type_args: [TypeParam(...)], ... }`.
  This covers `item.sum()` (direct call on type param) and `self.value.sum()`
  (field access producing TypeParam, then method call).
- Populate `conformance_map` during lowering from semantic analysis results.
- **Validate via dedicated test helpers (main pipeline still uses AST mono).**
- **Key test cases**: `generic_constraint_violation`, `generic_struct_with_constraint`
  (`tests/generics.rs`) must be reproducible with the new lowering path.

### Phase 3: Codegen On-The-Fly Substitution

- Introduce `Instance` type.
- Implement substitution logic in codegen: resolve `BirType::TypeParam` using
  `Instance.type_args`.
- Handle `BirType -> LLVM type` conversion for `TypeParam`.
- Name mangling: `func_name` + `type_args` -> mangled name.
- Implement **protocol method resolution** in codegen: for `Call` targets
  matching `{Protocol}_{method}` pattern, look up conformance map with the
  resolved concrete type to find the real function name.
- **Unit tests: manually construct `Instance`, verify correct LLVM IR from
  generic BIR, including protocol method calls.**

### Phase 4: Mono Collector

- Implement `mono_collect(bir: &BirModule, entry: &str) -> MonoCollectResult`.
- Collect both function instances and struct instances.
- Recursive discovery from entry points with deduplication.
- **Cross-validation: compare AST mono's specialized function set against BIR
  mono collector's `Instance` set for the same program.**

### Phase 5: Unified Analysis

- Extend `analyze_pre_mono` to build `SemanticInfo`.
- Type-check generic function bodies with type parameter constraints.
- Remove `analyze_post_mono` calls.
- **Pipeline becomes: `analyze -> lower_program -> optimize -> mono_collect ->
  codegen`.**

### Phase 6: Cutover and Cleanup

- Remove `monomorphize.rs` (AST mono).
- Remove all `monomorphize()` call sites.
- Clean up `InferredTypeArgs` (keep type inference results, remove mono-specific
  parts).
- Update `compile_package_to_executable`:
  - Build package-wide BIR registry after all modules are lowered.
  - Run mono collector per-module with registry access.
  - Emit generic instantiations with `linkonce_odr` LLVM linkage.
  - Adjust extern declaration collection (generic instantiations don't need
    extern declarations — each module emits its own copy).
- **All existing tests pass on the new BIR mono pipeline.**

### Phase Dependencies

```
Phase 1 -> Phase 2 -> Phase 3 -> Phase 4 -> Phase 5 -> Phase 6
```

Phase 5 could theoretically run in parallel with Phase 3-4, but sequential
execution is safer.

## Risks and Mitigations

### Risk 1: Generic BIR Lowering Complexity

Current lowering assumes concrete types. Generic functions introduce `TypeParam`
in field access results, method call targets, and struct layouts.

**Mitigation:** Phase 2 starts with simple cases (`identity<T>`) and
incrementally adds field access, method calls, and nested generics. Each case
gets dedicated tests.

### Risk 2: Analysis Unification Impact

Removing `analyze_post_mono` is a significant change to `semantic/mod.rs`.

**Mitigation:** In Phase 5, first add `SemanticInfo` construction to pre-mono
analysis alongside the existing post-mono path. Verify identical results before
removing post-mono.

### Risk 3: Codegen Substitution Gaps

Every codegen path must handle `TypeParam` correctly. A missed substitution
causes crashes or incorrect code.

**Mitigation:** `TypeParam` in the LLVM type conversion panics if not in the
substitution map. Phase 3 tests cover all BIR instruction types with type
parameters.

## Out of Scope

- Pipeline restructuring beyond what is needed for BIR mono
- Serializing generic BIR in interface files (`.bengalmod`)
- Additional BIR-level optimizations (inlining, constant propagation)
- Witness table / dynamic dispatch for protocol existentials

These items will be tracked in TODO.md for future work.
