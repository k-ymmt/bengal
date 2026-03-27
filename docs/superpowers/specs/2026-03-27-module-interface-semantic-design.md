# Module Interface Serialization — Semantic Info (Rust-style)

## Overview

Extend `.bengalmod` interface files to include semantic information (function signatures, struct definitions, protocol definitions) alongside the existing BIR data. This follows the Rust `.rmeta` model: store type signatures for consumer-side type checking, and BIR (equivalent to Rust's MIR) for generic function monomorphization.

## Background

The existing `interface.rs` serializes `BirModule` per module into `.bengalmod` files. However, consumers currently cannot perform type checking from these files alone — they lack function signatures, struct field types, and protocol requirements. This design adds that missing semantic layer.

### Why BIR-Centric (Approach A)

BIR-level monomorphization is already implemented (TODO §1.6). Since BIR is now the monomorphization input, we serialize BIR for generic function bodies — not AST. This matches Rust's approach of storing MIR in `.rmeta` files.

Benefits:
- BIR types already have `Serialize/Deserialize`
- No need to add serde to AST types (~15 types avoided)
- Consumer only needs codegen (no lowering step for generics)

## Data Model

### Assumptions

- Symbol names (functions, structs, protocols) are unique within a module. The parser enforces this by rejecting duplicate top-level definitions. The `visibilities` map relies on this assumption.
- Only locally-defined symbols are included in the interface. Imported symbols are not re-exported. Symbols without an entry in the `visibilities` map (i.e., imported symbols) are excluded during interface generation.

### Interface-Specific Types

Internal compiler types (`StructInfo`, `ComputedPropInfo`, etc.) contain `Block` (AST bodies) that are irrelevant for interface consumers. We define lightweight, body-free types for serialization.

```rust
// src/interface.rs

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleInterface {
    pub functions: Vec<InterfaceFuncEntry>,
    pub structs: Vec<InterfaceStructEntry>,
    pub protocols: Vec<InterfaceProtocolEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceFuncEntry {
    pub name: String,
    pub sig: InterfaceFuncSig,
    // Whether the function is generic is derived from sig.type_params.is_empty().
    // Generic functions have their BIR body stored in BirModule for monomorphization.
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceFuncSig {
    pub type_params: Vec<InterfaceTypeParam>,
    pub params: Vec<(String, InterfaceType)>,
    pub return_type: InterfaceType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceTypeParam {
    pub name: String,
    pub bound: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceStructEntry {
    pub name: String,
    pub type_params: Vec<InterfaceTypeParam>,
    pub conformances: Vec<String>,
    pub fields: Vec<(String, InterfaceType)>,
    pub methods: Vec<InterfaceMethodSig>,
    pub computed: Vec<InterfaceComputedProp>,
    pub init_params: Vec<(String, InterfaceType)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceMethodSig {
    pub name: String,
    pub params: Vec<(String, InterfaceType)>,
    pub return_type: InterfaceType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceComputedProp {
    pub name: String,
    pub ty: InterfaceType,
    pub has_setter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceProtocolEntry {
    pub name: String,
    pub methods: Vec<InterfaceMethodSig>,
    pub properties: Vec<InterfacePropertyReq>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfacePropertyReq {
    pub name: String,
    pub ty: InterfaceType,
    pub has_setter: bool,
}
```

### InterfaceType

A serialization-safe subset of the internal `Type` enum. Excludes inference-only variants (`InferVar`, `IntegerLiteral`, `FloatLiteral`, `Error`).

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InterfaceType {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Unit,
    Struct(String),
    TypeParam { name: String, bound: Option<String> },
    Generic { name: String, args: Vec<InterfaceType> },
    Array { element: Box<InterfaceType>, size: u64 },
}
```

### BengalModFile Update

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
    pub interfaces: HashMap<ModulePath, ModuleInterface>,  // NEW
}
```

## SemanticInfo Extension

Currently `SemanticInfo` lacks function signatures (they live in `Resolver`'s private fields). We add:

```rust
// semantic/mod.rs
pub struct SemanticInfo {
    pub struct_defs: HashMap<String, StructInfo>,
    pub struct_init_calls: HashSet<NodeId>,
    pub protocols: HashMap<String, ProtocolInfo>,
    pub functions: HashMap<String, FuncSig>,        // NEW
    pub visibilities: HashMap<String, Visibility>,   // NEW
}
```

`visibilities` maps symbol names (functions, structs, protocols) to their declared visibility. Symbol names are unique within a module because the parser rejects duplicate top-level definitions.

### Resolver Changes

Add a `take_functions()` method to `Resolver` (analogous to existing `take_struct_defs()` and `take_protocols()`):

```rust
// semantic/resolver.rs
impl Resolver {
    pub fn take_functions(&mut self) -> HashMap<String, FuncSig> {
        std::mem::take(&mut self.functions)
    }
}
```

Update all `SemanticInfo` construction sites to populate the new fields:
- `pre_mono.rs` — `analyze_pre_mono()` / `analyze_pre_mono_lenient()`
- `post_mono.rs` — `analyze_post_mono()`
- `single_module_analysis.rs` — `analyze_single_module()`

At each site, call `resolver.take_functions()` and collect visibility from the parsed AST (`Program.functions`, `Program.structs`, `Program.protocols`).

Note: `single_module_analysis.rs` uses `take_all_struct_defs()` / `take_all_protocols()` (which include imported symbols). The corresponding `take_functions()` should follow the same pattern. Imported symbols will be naturally excluded during interface generation because they lack entries in the `visibilities` map.

## Conversion Logic

### Type → InterfaceType

```rust
impl InterfaceType {
    pub fn from_type(ty: &Type) -> Self {
        match ty {
            Type::I32 => InterfaceType::I32,
            Type::I64 => InterfaceType::I64,
            Type::F32 => InterfaceType::F32,
            Type::F64 => InterfaceType::F64,
            Type::Bool => InterfaceType::Bool,
            Type::Unit => InterfaceType::Unit,
            Type::Struct(name) => InterfaceType::Struct(name.clone()),
            Type::TypeParam { name, bound } => InterfaceType::TypeParam {
                name: name.clone(),
                bound: bound.clone(),
            },
            Type::Generic { name, args } => InterfaceType::Generic {
                name: name.clone(),
                args: args.iter().map(InterfaceType::from_type).collect(),
            },
            Type::Array { element, size } => InterfaceType::Array {
                element: Box::new(InterfaceType::from_type(element)),
                size: *size,
            },
            Type::InferVar(_) | Type::IntegerLiteral(_)
            | Type::FloatLiteral(_) | Type::Error => {
                unreachable!("interface types must be fully resolved")
            }
        }
    }
}
```

### SemanticInfo → ModuleInterface

```rust
impl ModuleInterface {
    /// Build a ModuleInterface from semantic info and BIR.
    ///
    /// `conformance_map` is sourced from `BirModule.conformance_map`, which is
    /// already available in `LoweredPackage.modules[path].bir`.
    /// `init_params` is derived from `StructInfo.init.params`.
    pub fn from_semantic_info(sem: &SemanticInfo, conformance_map: &HashMap<String, Vec<String>>) -> Self {
        // 1. Filter: only symbols with Public/Package in sem.visibilities
        //    (symbols without a visibilities entry — i.e., imported — are excluded)
        // 2. Convert FuncSig → InterfaceFuncEntry
        // 3. Convert StructInfo → InterfaceStructEntry (strip Block bodies, add conformances)
        // 4. Convert ProtocolInfo → InterfaceProtocolEntry
    }
}
```

Visibility filtering rule: include symbols with `Visibility::Public` or `Visibility::Package`. Exclude `Internal`, `Fileprivate`, `Private`. This targets same-package separate compilation; cross-package consumption (where `Package` would be excluded) is deferred to future work.

## Pipeline Integration

```
parse → analyze → lower → optimize → monomorphize → codegen → link
              ↓           ↓
         SemanticInfo   BirModule
              ↓           ↓
         ModuleInterface  ↓
              └─────┬─────┘
            BengalModFile
```

1. `LoweredPackage` gains a `pkg_sem_info: PackageSemanticInfo` field to carry semantic info forward from the analyze stage.
2. The `lower()` function is updated to move `pkg_sem_info` from `AnalyzedPackage` into `LoweredPackage` (ownership transfer, no clone). Since `optimize()` passes `LoweredPackage` through, the field is automatically preserved.
3. `write_interface()` is updated to build `ModuleInterface` per module from `pkg_sem_info` and include it in `BengalModFile`.

## Testing

### Unit Tests (interface.rs)

- **Round-trip**: `ModuleInterface` → serialize → deserialize → equals original
- **InterfaceType conversion**: all `Type` variants map correctly, including recursive cases (`Generic`, `Array`)
- **Visibility filter**: Public/Package included, Internal/Private excluded
- **Generic struct types**: `Pair<T, U>` round-trips correctly with `InterfaceType::Generic`

### Integration Tests (tests/)

- Multi-module package: write `.bengalmod` → read back → verify semantic info preserved
- Generic functions: `type_params` non-empty and BIR body present in BirModule
- Struct conformances: protocol conformance info preserved in interface

### Out of Scope

- Consuming `.bengalmod` for separate compilation (future Step 2)
- Type checking from deserialized interfaces
- Text-based interface format (TODO §1.5)
