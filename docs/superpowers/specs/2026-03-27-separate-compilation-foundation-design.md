# Separate Compilation Foundation Design

## Overview

Implement the foundation for separate compilation: (1) load `ModuleInterface` data into the semantic analyzer's `Resolver`, and (2) automatically generate `.bengalmod` interface files during compilation. This enables future fully-separate per-module compilation (TODO 2, phase 3).

## Scope

- **Interface → Resolver injection**: Convert `ModuleInterface` to native semantic types (`FuncSig`, `StructInfo`, `ProtocolInfo`) and register them in the `Resolver`
- **Pipeline integration**: Automatically generate per-module `.bengalmod` files after the lower stage
- **Out of scope**: Actual separate compilation (compiling a module using only interface files without dependency ASTs), incremental builds, parallel compilation

## Success Criteria

Unit tests verify:
- `ModuleInterface` symbols injected into `Resolver` are resolvable for type checking
- `.bengalmod` files are automatically generated per module during compilation

## Design

### 1. `InterfaceType` → `Type` Reverse Conversion

Add `to_type()` to `InterfaceType` in `src/interface.rs`, symmetric to the existing `from_type()`.

```rust
impl InterfaceType {
    pub fn to_type(&self) -> Type {
        match self {
            InterfaceType::I32 => Type::I32,
            InterfaceType::I64 => Type::I64,
            InterfaceType::F32 => Type::F32,
            InterfaceType::F64 => Type::F64,
            InterfaceType::Bool => Type::Bool,
            InterfaceType::Unit => Type::Unit,
            InterfaceType::Struct(name) => Type::Struct(name.clone()),
            InterfaceType::TypeParam { name, bound } => Type::TypeParam {
                name: name.clone(),
                bound: bound.clone(),
            },
            InterfaceType::Generic { name, args } => Type::Generic {
                name: name.clone(),
                args: args.iter().map(|a| a.to_type()).collect(),
            },
            InterfaceType::Array { element, size } => Type::Array {
                element: Box::new(element.to_type()),
                size: *size,
            },
        }
    }
}
```

No `InferVar`/`IntegerLiteral`/`FloatLiteral`/`Error` variants exist in `InterfaceType`, so the reverse conversion is total.

### 2. Interface Entry → Semantic Type Conversion

Add conversion methods to `InterfaceTypeParam`, `InterfaceFuncEntry`, `InterfaceStructEntry`, and `InterfaceProtocolEntry` in `src/interface.rs`.

**`InterfaceTypeParam::to_type_param()`** → `TypeParam`

**`InterfaceFuncEntry::to_func_sig()`** → `FuncSig`
- Maps type_params, params, return_type via `to_type()`

**`InterfaceStructEntry::to_struct_info()`** → `StructInfo`
- Rebuilds `field_index`, `computed_index`, `method_index` HashMaps from the ordered lists
- `InitializerInfo.body` is `None` (no body available from interface — used for type checking only)
- `ComputedPropInfo.getter` uses a dummy empty `Block { stmts: vec![] }` (no getter body available from interface). `ComputedPropInfo.setter` uses `Some(Block { stmts: vec![] })` if `has_setter` is true, `None` otherwise. These dummy blocks are never executed — only `ty` and `has_setter` are used during type checking.
- Maps conformances, fields, computed properties, methods, init params

**`InterfaceProtocolEntry::to_protocol_info()`** → `ProtocolInfo`
- Maps methods to `ProtocolMethodSig`, properties to `ProtocolPropertyReq`

These conversions are the symmetric inverse of `from_semantic_info()`.

Note: `MethodInfo` and `InterfaceMethodSig` both lack `type_params` — generic methods on structs are not yet supported in the language. This is a known limitation consistent with the interface format.

### 3. `Resolver::register_interface`

There are two levels of injection:

**Level 1: `GlobalSymbolTable` injection (for Phase 1)**

`analyze_package` Phase 1 builds a `GlobalSymbolTable = HashMap<ModulePath, HashMap<String, GlobalSymbol>>` where `GlobalSymbol` contains `kind`, `visibility`, and `module`. A new function converts `ModuleInterface` into `GlobalSymbolTable` entries:

```rust
// src/semantic/package_analysis.rs
pub fn interface_to_global_symbols(
    iface: &ModuleInterface,
    module_path: &ModulePath,
) -> HashMap<String, GlobalSymbol> {
    let mut symbols = HashMap::new();
    for func in &iface.functions {
        symbols.insert(func.name.clone(), GlobalSymbol {
            kind: SymbolKind::Function(func.to_func_sig()),
            visibility: func.visibility,
            module: module_path.clone(),
        });
    }
    for s in &iface.structs {
        symbols.insert(s.name.clone(), GlobalSymbol {
            kind: SymbolKind::Struct(s.to_struct_info()),
            visibility: s.visibility,
            module: module_path.clone(),
        });
    }
    for p in &iface.protocols {
        symbols.insert(p.name.clone(), GlobalSymbol {
            kind: SymbolKind::Protocol(p.to_protocol_info()),
            visibility: p.visibility,
            module: module_path.clone(),
        });
    }
    symbols
}
```

This is injected into the `GlobalSymbolTable` alongside AST-derived symbols. Phase 2 (import resolution) then uses the existing `resolve_imports_for_module` which calls `resolver.import_func()`, `resolver.import_struct()`, `resolver.import_protocol()` — the same path as AST-derived symbols.

**Level 2: `Resolver` import methods (for direct injection)**

For testing and future use, add a convenience method to `Resolver`:

```rust
// src/semantic/resolver.rs
pub fn register_interface(&mut self, iface: &ModuleInterface) {
    for func in &iface.functions {
        self.import_func(func.name.clone(), func.to_func_sig());
    }
    for s in &iface.structs {
        self.import_struct(s.name.clone(), s.to_struct_info());
    }
    for p in &iface.protocols {
        self.import_protocol(p.name.clone(), p.to_protocol_info());
    }
}
```

This uses the existing `import_func`/`import_struct`/`import_protocol` methods which write to `imported_funcs`/`imported_structs`/`imported_protocols` HashMaps — the correct import maps, not the local definition maps.

**Design rationale (Swift/Rust precedent):**
- Both Swift and Rust convert serialized module data into the compiler's native internal types, making loaded symbols indistinguishable from source-parsed symbols
- Visibility is tracked in `GlobalSymbol.visibility`, matching the existing `analyze_package` Phase 1 convention
- `Resolver` has no `visibilities` field — visibility is handled at the `GlobalSymbolTable` level during import resolution

**Integration with `analyze_package`:**
- Phase 1 (`collect_global_symbols`): interface-derived symbols inserted into `GlobalSymbolTable` alongside AST-derived symbols
- Phase 2 (`resolve_imports_for_module`): unchanged — uses `GlobalSymbolTable` to resolve imports, calls `import_func`/`import_struct`/`import_protocol` on `Resolver`
- Phase 3 (per-module analysis): unchanged — `Resolver` already looks up both local and imported symbols transparently

### 4. Pipeline `.bengalmod` Auto-Generation

Add `emit_interfaces()` to `src/pipeline.rs`, called after the lower stage.

**Output location:** `.build/cache/` directory, with module paths mapped to subdirectories:

```
.build/
  cache/
    math.bengalmod
    utils/
      string.bengalmod
```

**Implementation:**

```rust
fn emit_interfaces(lowered: &LoweredPackage) -> Result<()> {
    let cache_dir = Path::new(".build/cache");
    std::fs::create_dir_all(cache_dir)?;

    for (module_path, module) in &lowered.modules {
        let sem_info = lowered.pkg_sem_info.module_infos.get(module_path);
        let Some(sem_info) = sem_info else { continue };
        let iface = ModuleInterface::from_semantic_info(sem_info);
        let mod_file = BengalModFile {
            package_name: lowered.package_name.clone(),
            modules: HashMap::from([(module_path.clone(), module.bir.clone())]),
            interfaces: HashMap::from([(module_path.clone(), iface)]),
        };

        let file_path = cache_dir.join(module_path.to_file_path("bengalmod"));
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_bengalmod_file(&mod_file, &file_path)?;
    }
    Ok(())
}
```

**`write_bengalmod_file`:** New function in `src/interface.rs` that takes `&BengalModFile` directly (the existing `write_interface` takes `&LoweredPackage`). The existing `write_interface` is refactored to internally call `write_bengalmod_file`.

**Invocation:** Between lower and optimize in `compile_to_executable`. The cached `.bengalmod` files contain unoptimized BIR. This is intentional — optimization is applied per-consumer during compilation, not at cache time. This matches Rust's `.rmeta` approach where MIR is stored pre-optimization.

**`ModulePath::to_file_path`:** New helper method that converts `math::utils` → `math/utils.bengalmod`. For root module (`ModulePath(vec![])`), returns `root.bengalmod`.

**Error handling:** Failures in `emit_interfaces` are non-fatal warnings — they should not prevent compilation from succeeding. The `.bengalmod` files are a cache, not a required output.

**`.build/` should be added to `.gitignore`.**

## Testing Strategy

### Unit tests (in `src/interface.rs` `#[cfg(test)]`)

**`InterfaceType::to_type` round-trip:**
- All variants: I32, I64, F32, F64, Bool, Unit, Struct, TypeParam, Generic, Array
- Verify `InterfaceType::from_type(&ty).to_type() == ty`

**Entry → semantic type round-trip:**
- `InterfaceFuncEntry::to_func_sig` — params, return type, type params
- `InterfaceStructEntry::to_struct_info` — fields, methods, computed, init_params, conformances, index maps
- `InterfaceProtocolEntry::to_protocol_info` — methods, properties

### Integration tests (in `tests/interface.rs`)

**`Resolver::register_interface`:**
- Construct `ModuleInterface` → call `register_interface` → verify functions, structs, protocols are lookup-able in `Resolver`
- Verify visibility is correctly registered
- Verify qualified name format (`module_path::func_name`)

**Pipeline `.bengalmod` generation:**
- Compile multi-module Bengal source → verify `.build/cache/` contains per-module `.bengalmod` files
- Read generated `.bengalmod` back with `read_interface` and verify contents
