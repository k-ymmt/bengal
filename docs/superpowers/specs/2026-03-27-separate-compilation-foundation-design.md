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
- Maps conformances, fields, computed properties, methods, init params

**`InterfaceProtocolEntry::to_protocol_info()`** → `ProtocolInfo`
- Maps methods to `ProtocolMethodSig`, properties to `ProtocolPropertyReq`

These conversions are the symmetric inverse of `from_semantic_info()`.

### 3. `Resolver::register_interface`

Add a new method to `Resolver` in `src/semantic/resolver.rs`:

```rust
pub fn register_interface(
    &mut self,
    iface: &ModuleInterface,
    module_path: &ModulePath,
) {
    for func in &iface.functions {
        let qualified_name = format!("{}::{}", module_path, func.name);
        self.functions.insert(qualified_name.clone(), func.to_func_sig());
        self.visibilities.insert(qualified_name, func.visibility);
    }
    for s in &iface.structs {
        self.struct_defs.insert(s.name.clone(), s.to_struct_info());
        self.visibilities.insert(s.name.clone(), s.visibility);
    }
    for p in &iface.protocols {
        self.protocols.insert(p.name.clone(), p.to_protocol_info());
        self.visibilities.insert(p.name.clone(), p.visibility);
    }
}
```

**Design rationale (Swift/Rust precedent):**
- Both Swift and Rust convert serialized module data into the compiler's native internal types, making loaded symbols indistinguishable from source-parsed symbols
- Functions use qualified names (`module_path::func_name`) matching the existing `analyze_package` Phase 1 convention
- Structs and protocols use unqualified names (they are package-global in Bengal)
- Visibility is registered for import-time access checks

**Integration with `analyze_package`:**
- Phase 1 (global symbol collection) currently walks ASTs and populates `Resolver.functions`, `struct_defs`, `protocols`, `visibilities`
- `register_interface` writes to the same HashMaps in the same format
- Phase 2 (import resolution) and Phase 3 (per-module analysis) see no difference between AST-derived and interface-derived symbols

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
        let sem_info = lowered.pkg_sem_info.get(module_path);
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
        write_interface(&mod_file, &file_path)?;
    }
    Ok(())
}
```

**Invocation:** Between lower and optimize in `compile_to_executable`.

**`ModulePath::to_file_path`:** New helper method that converts `math::utils` → `math/utils.bengalmod`.

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
