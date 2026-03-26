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
pub enum BirType {
    Unit, I32, I64, F32, F64, Bool,
    Struct(String),
    Array { element: Box<BirType>, size: u64 },
    TypeParam(String),  // NEW: "T", "U", etc.
}
```

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
pub struct Instance {
    pub func_name: String,
    pub type_args: Vec<BirType>,  // always concrete (no TypeParam)
}

pub fn mono_collect(bir: &BirModule, entry: &str) -> Vec<Instance>
```

### Algorithm

1. Add entry point functions (non-generic) to worklist.
2. Pop an `Instance` from worklist.
3. Walk the BIR function's instructions.
4. For each `Call` with non-empty `type_args`, resolve `TypeParam` references
   using the current instance's substitution map, creating a new concrete
   `Instance`.
5. If the new `Instance` is not already seen, add to worklist.
6. Repeat until worklist is empty.
7. Return all discovered instances.

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

### Protocol Method Resolution

For generic functions with protocol constraints (e.g., `T: Summable`), method
calls are represented in BIR as:

```
Call { func: "Summable.add", type_args: [TypeParam("T")], ... }
```

During codegen with `T = Int32`, the substitution resolves this to the concrete
method `Int32_add` (following existing name mangling rules).

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

## Incremental Migration Strategy

### Phase 1: BIR Data Structure Changes

- Add `BirType::TypeParam(String)`
- Add `BirFunction.type_params: Vec<String>`
- Add `type_args: Vec<BirType>` to `Call` and `StructInit`
- Existing code uses `type_params: vec![]`, `type_args: vec![]`
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

- Implement `mono_collect(bir: &BirModule, entry: &str) -> Vec<Instance>`.
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
- Update BIR printer for `TypeParam` and `type_args` display.
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
