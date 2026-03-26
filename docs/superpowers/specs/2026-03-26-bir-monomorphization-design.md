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
3. Walk the BIR function's instructions, applying the current substitution map.
4. For each `Call` with non-empty `type_args`, resolve `TypeParam` references
   using the current instance's substitution map, creating a new concrete
   `Instance`. Add to worklist if not already seen.
5. For each `StructInit` with non-empty `type_args`, resolve and record the
   concrete struct instance in `struct_instances`.
6. For each `BirType::Struct` with non-empty `type_args` encountered in any
   instruction (FieldGet, FieldSet, etc.), also record the concrete struct
   instance.
7. Repeat until worklist is empty.
8. Return all discovered function and struct instances.

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

This concrete layout is used to create the LLVM struct type and compute field
indices for GEP instructions.

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

### Protocol Method Resolution

For generic functions with protocol constraints (e.g., `T: Summable`), method
calls are represented in BIR as:

```
Call { func: "Summable.add", type_args: [TypeParam("T")], ... }
```

During codegen with `T = Int32`, protocol method resolution proceeds as:

1. Resolve `type_args`: `[TypeParam("T")]` -> `[I32]` via substitution map.
2. Look up the concrete implementation using a **conformance map** stored in
   `BirModule`:

```rust
// BirModule addition
pub conformance_map: HashMap<(String, String), String>,
// (protocol_method, concrete_type) -> implementation_name
// e.g., ("Summable.add", "Int32") -> "Int32_add"
```

3. Replace the `Call` target with the concrete function name `Int32_add`.

The conformance map is populated during BIR lowering from the semantic analysis
results (protocol conformance declarations).

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
    %1 = call @identity(%0) type_args=[I32] : TypeParam("T")  // resolved at codegen
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
    %3 = call @getFirst(%2) type_args=[I32, Bool] : TypeParam("T")
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

The multi-module pipeline (`compile_package`) currently runs `analyze_pre_mono`
+ `monomorphize` per module, then `analyze_package` for cross-module analysis.

For this migration, the mono collector runs on the **entire package's BIR** (all
modules combined), so cross-module generic instantiations are naturally
discovered. Module A calling a generic function from module B is handled by the
collector finding the `Call` in A's BIR and adding the `Instance` for B's
function.

Full cross-module separate compilation (where module B's BIR is loaded from an
interface file rather than compiled in the same session) is out of scope and
tracked in TODO.md §1.6.2.

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
- **Validate via dedicated test helpers (main pipeline still uses AST mono).**

### Phase 3: Codegen On-The-Fly Substitution

- Introduce `Instance` type.
- Implement substitution logic in codegen: resolve `BirType::TypeParam` using
  `Instance.type_args`.
- Handle `BirType -> LLVM type` conversion for `TypeParam`.
- Name mangling: `func_name` + `type_args` -> mangled name.
- **Unit tests: manually construct `Instance`, verify correct LLVM IR from
  generic BIR.**

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
