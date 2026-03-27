# Separate Compilation Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable interface-based symbol injection into the Resolver and automatic `.bengalmod` generation during compilation.

**Architecture:** Add reverse conversions (`InterfaceType` → `Type`, interface entries → semantic types), a `Resolver::register_interface` method for symbol injection, and a `emit_interfaces` pipeline step that writes per-module `.bengalmod` files to `.build/cache/`.

**Tech Stack:** Rust, existing Bengal compiler infrastructure (`src/interface.rs`, `src/semantic/resolver.rs`, `src/pipeline.rs`)

---

### Task 1: `InterfaceType::to_type` reverse conversion

**Files:**
- Modify: `src/interface.rs` (add `to_type` method to `InterfaceType`)
- Test: `src/interface.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write failing round-trip tests**

Add to the existing `#[cfg(test)] mod tests` in `src/interface.rs`:

```rust
use crate::semantic::types::Type;

#[test]
fn interface_type_to_type_round_trip_primitives() {
    let types = vec![Type::I32, Type::I64, Type::F32, Type::F64, Type::Bool, Type::Unit];
    for ty in types {
        let iface_ty = InterfaceType::from_type(&ty);
        assert_eq!(iface_ty.to_type(), ty);
    }
}

#[test]
fn interface_type_to_type_round_trip_struct() {
    let ty = Type::Struct("Point".to_string());
    assert_eq!(InterfaceType::from_type(&ty).to_type(), ty);
}

#[test]
fn interface_type_to_type_round_trip_type_param() {
    let ty = Type::TypeParam { name: "T".to_string(), bound: Some("Summable".to_string()) };
    assert_eq!(InterfaceType::from_type(&ty).to_type(), ty);
}

#[test]
fn interface_type_to_type_round_trip_generic() {
    let ty = Type::Generic {
        name: "Pair".to_string(),
        args: vec![Type::I32, Type::Bool],
    };
    assert_eq!(InterfaceType::from_type(&ty).to_type(), ty);
}

#[test]
fn interface_type_to_type_round_trip_array() {
    let ty = Type::Array { element: Box::new(Type::I32), size: 4 };
    assert_eq!(InterfaceType::from_type(&ty).to_type(), ty);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib -- interface_type_to_type`
Expected: Compilation error — `to_type` method doesn't exist.

- [ ] **Step 3: Implement `to_type`**

Add to `impl InterfaceType` in `src/interface.rs` (next to the existing `from_type` at line 44):

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib -- interface_type_to_type && cargo clippy`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/interface.rs
git commit -m "feat(interface): add InterfaceType::to_type reverse conversion"
```

---

### Task 2: Interface entry → semantic type conversions

**Files:**
- Modify: `src/interface.rs` (add `to_type_param`, `to_func_sig`, `to_struct_info`, `to_protocol_info`)
- Test: `src/interface.rs` (inline `#[cfg(test)]`)

These methods need imports for semantic types. Add at the top of `src/interface.rs`:
```rust
use crate::semantic::resolver::{
    ComputedPropInfo, InitializerInfo, MethodInfo, ProtocolMethodSig, ProtocolPropertyReq,
    StructInfo, FuncSig, ProtocolInfo,
};
use crate::parser::ast::Block;
```

Check existing imports — some may already be present (e.g., `FuncSig` might already be imported for `from_semantic_info`). Only add what's missing.

- [ ] **Step 1: Write failing tests for `to_type_param`**

```rust
#[test]
fn interface_type_param_to_type_param_round_trip() {
    let tp = TypeParam { name: "T".to_string(), bound: Some("Summable".to_string()) };
    let iface_tp = InterfaceTypeParam::from_type_param(&tp);
    let restored = iface_tp.to_type_param();
    assert_eq!(restored.name, tp.name);
    assert_eq!(restored.bound, tp.bound);
}
```

- [ ] **Step 2: Implement `to_type_param`**

Add to `impl InterfaceTypeParam` (next to `from_type_param` at line 111):

```rust
pub fn to_type_param(&self) -> TypeParam {
    TypeParam {
        name: self.name.clone(),
        bound: self.bound.clone(),
    }
}
```

- [ ] **Step 3: Write failing tests for `to_func_sig`**

```rust
#[test]
fn interface_func_entry_to_func_sig() {
    let entry = InterfaceFuncEntry {
        visibility: Visibility::Public,
        name: "add".to_string(),
        sig: InterfaceFuncSig {
            type_params: vec![],
            params: vec![("a".to_string(), InterfaceType::I32), ("b".to_string(), InterfaceType::I32)],
            return_type: InterfaceType::I32,
        },
    };
    let sig = entry.to_func_sig();
    assert_eq!(sig.params.len(), 2);
    assert_eq!(sig.params[0], ("a".to_string(), Type::I32));
    assert_eq!(sig.return_type, Type::I32);
    assert!(sig.type_params.is_empty());
}

#[test]
fn interface_func_entry_to_func_sig_generic() {
    let entry = InterfaceFuncEntry {
        visibility: Visibility::Public,
        name: "identity".to_string(),
        sig: InterfaceFuncSig {
            type_params: vec![InterfaceTypeParam { name: "T".to_string(), bound: None }],
            params: vec![("x".to_string(), InterfaceType::TypeParam { name: "T".to_string(), bound: None })],
            return_type: InterfaceType::TypeParam { name: "T".to_string(), bound: None },
        },
    };
    let sig = entry.to_func_sig();
    assert_eq!(sig.type_params.len(), 1);
    assert_eq!(sig.type_params[0].name, "T");
    assert_eq!(sig.return_type, Type::TypeParam { name: "T".to_string(), bound: None });
}
```

- [ ] **Step 4: Implement `to_func_sig`**

Add to `impl InterfaceFuncEntry`:

```rust
pub fn to_func_sig(&self) -> FuncSig {
    FuncSig {
        type_params: self.sig.type_params.iter().map(|tp| tp.to_type_param()).collect(),
        params: self.sig.params.iter().map(|(n, t)| (n.clone(), t.to_type())).collect(),
        return_type: self.sig.return_type.to_type(),
    }
}
```

- [ ] **Step 5: Write failing tests for `to_struct_info`**

```rust
#[test]
fn interface_struct_entry_to_struct_info() {
    let entry = InterfaceStructEntry {
        visibility: Visibility::Public,
        name: "Point".to_string(),
        type_params: vec![],
        conformances: vec!["Summable".to_string()],
        fields: vec![("x".to_string(), InterfaceType::I32), ("y".to_string(), InterfaceType::I32)],
        methods: vec![InterfaceMethodSig {
            name: "sum".to_string(),
            params: vec![],
            return_type: InterfaceType::I32,
        }],
        computed: vec![InterfaceComputedProp {
            name: "total".to_string(),
            ty: InterfaceType::I32,
            has_setter: false,
        }],
        init_params: vec![("x".to_string(), InterfaceType::I32), ("y".to_string(), InterfaceType::I32)],
    };
    let info = entry.to_struct_info();

    // Fields
    assert_eq!(info.fields.len(), 2);
    assert_eq!(info.fields[0], ("x".to_string(), Type::I32));
    assert_eq!(*info.field_index.get("x").unwrap(), 0);
    assert_eq!(*info.field_index.get("y").unwrap(), 1);

    // Methods
    assert_eq!(info.methods.len(), 1);
    assert_eq!(info.methods[0].name, "sum");
    assert_eq!(*info.method_index.get("sum").unwrap(), 0);

    // Computed
    assert_eq!(info.computed.len(), 1);
    assert_eq!(info.computed[0].name, "total");
    assert!(!info.computed[0].has_setter);
    assert_eq!(*info.computed_index.get("total").unwrap(), 0);

    // Init
    assert_eq!(info.init.params.len(), 2);
    assert!(info.init.body.is_none());

    // Conformances
    assert_eq!(info.conformances, vec!["Summable".to_string()]);
}
```

- [ ] **Step 6: Implement `to_struct_info`**

Add to `impl InterfaceStructEntry`:

```rust
pub fn to_struct_info(&self) -> StructInfo {
    StructInfo {
        type_params: self.type_params.iter().map(|tp| tp.to_type_param()).collect(),
        conformances: self.conformances.clone(),
        fields: self.fields.iter().map(|(n, t)| (n.clone(), t.to_type())).collect(),
        field_index: self.fields.iter().enumerate().map(|(i, (n, _))| (n.clone(), i)).collect(),
        computed: self.computed.iter().map(|c| ComputedPropInfo {
            name: c.name.clone(),
            ty: c.ty.to_type(),
            has_setter: c.has_setter,
            getter: Block { stmts: vec![] },
            setter: if c.has_setter { Some(Block { stmts: vec![] }) } else { None },
        }).collect(),
        computed_index: self.computed.iter().enumerate().map(|(i, c)| (c.name.clone(), i)).collect(),
        init: InitializerInfo {
            params: self.init_params.iter().map(|(n, t)| (n.clone(), t.to_type())).collect(),
            body: None,
        },
        methods: self.methods.iter().map(|m| MethodInfo {
            name: m.name.clone(),
            params: m.params.iter().map(|(n, t)| (n.clone(), t.to_type())).collect(),
            return_type: m.return_type.to_type(),
        }).collect(),
        method_index: self.methods.iter().enumerate().map(|(i, m)| (m.name.clone(), i)).collect(),
    }
}
```

- [ ] **Step 7: Write failing tests for `to_protocol_info`**

```rust
#[test]
fn interface_protocol_entry_to_protocol_info() {
    let entry = InterfaceProtocolEntry {
        visibility: Visibility::Public,
        name: "Summable".to_string(),
        methods: vec![InterfaceMethodSig {
            name: "sum".to_string(),
            params: vec![],
            return_type: InterfaceType::I32,
        }],
        properties: vec![InterfacePropertyReq {
            name: "value".to_string(),
            ty: InterfaceType::I32,
            has_setter: true,
        }],
    };
    let info = entry.to_protocol_info();
    assert_eq!(info.name, "Summable");
    assert_eq!(info.methods.len(), 1);
    assert_eq!(info.methods[0].name, "sum");
    assert_eq!(info.methods[0].return_type, Type::I32);
    assert_eq!(info.properties.len(), 1);
    assert_eq!(info.properties[0].name, "value");
    assert!(info.properties[0].has_setter);
}
```

- [ ] **Step 8: Implement `to_protocol_info`**

Add to `impl InterfaceProtocolEntry`:

```rust
pub fn to_protocol_info(&self) -> ProtocolInfo {
    ProtocolInfo {
        name: self.name.clone(),
        methods: self.methods.iter().map(|m| ProtocolMethodSig {
            name: m.name.clone(),
            params: m.params.iter().map(|(n, t)| (n.clone(), t.to_type())).collect(),
            return_type: m.return_type.to_type(),
        }).collect(),
        properties: self.properties.iter().map(|p| ProtocolPropertyReq {
            name: p.name.clone(),
            ty: p.ty.to_type(),
            has_setter: p.has_setter,
        }).collect(),
    }
}
```

- [ ] **Step 9: Run all tests**

Run: `cargo test --lib && cargo clippy`
Expected: All pass.

- [ ] **Step 10: Commit**

```bash
git add src/interface.rs
git commit -m "feat(interface): add interface entry to semantic type conversions"
```

---

### Task 3: `Resolver::register_interface`

**Files:**
- Modify: `src/semantic/resolver.rs` (add `register_interface` method)
- Test: `tests/interface.rs` (integration tests)

- [ ] **Step 1: Write failing integration tests**

Add to `tests/interface.rs`:

```rust
use bengal::semantic::resolver::Resolver;

#[test]
fn register_interface_functions() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("a".to_string(), InterfaceType::I32), ("b".to_string(), InterfaceType::I32)],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let mut resolver = Resolver::default();
    resolver.register_interface(&iface);
    let sig = resolver.lookup_func("add").expect("function should be registered");
    assert_eq!(sig.params.len(), 2);
    assert_eq!(sig.return_type, bengal::semantic::types::Type::I32);
}

#[test]
fn register_interface_structs() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec![],
            fields: vec![("x".to_string(), InterfaceType::I32)],
            methods: vec![],
            computed: vec![],
            init_params: vec![("x".to_string(), InterfaceType::I32)],
        }],
        protocols: vec![],
    };
    let mut resolver = Resolver::default();
    resolver.register_interface(&iface);
    let info = resolver.lookup_struct("Point").expect("struct should be registered");
    assert_eq!(info.fields.len(), 1);
}

#[test]
fn register_interface_protocols() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Runnable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "run".to_string(),
                params: vec![],
                return_type: InterfaceType::Unit,
            }],
            properties: vec![],
        }],
    };
    let mut resolver = Resolver::default();
    resolver.register_interface(&iface);
    let info = resolver.lookup_protocol("Runnable").expect("protocol should be registered");
    assert_eq!(info.methods.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test interface -- register_interface`
Expected: Compilation error — `register_interface` doesn't exist.

- [ ] **Step 3: Implement `register_interface`**

Add to `impl Resolver` in `src/semantic/resolver.rs`:

```rust
pub fn register_interface(&mut self, iface: &crate::interface::ModuleInterface) {
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

- [ ] **Step 4: Run tests**

Run: `cargo test --test interface -- register_interface && cargo clippy`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/semantic/resolver.rs tests/interface.rs
git commit -m "feat(resolver): add register_interface for interface-based symbol injection"
```

---

### Task 4: `ModulePath::to_file_path` and `write_bengalmod_file`

**Files:**
- Modify: `src/package.rs` (add `to_file_path`)
- Modify: `src/interface.rs` (add `write_bengalmod_file`, refactor `write_interface`)
- Test: `src/package.rs` (inline test), `tests/interface.rs`

- [ ] **Step 1: Write failing tests for `to_file_path`**

Add inline test in `src/package.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_file_path_root() {
        let path = ModulePath::root();
        assert_eq!(path.to_file_path("bengalmod"), std::path::PathBuf::from("root.bengalmod"));
    }

    #[test]
    fn to_file_path_single() {
        let path = ModulePath(vec!["math".to_string()]);
        assert_eq!(path.to_file_path("bengalmod"), std::path::PathBuf::from("math.bengalmod"));
    }

    #[test]
    fn to_file_path_nested() {
        let path = ModulePath(vec!["utils".to_string(), "string".to_string()]);
        assert_eq!(path.to_file_path("bengalmod"), std::path::PathBuf::from("utils/string.bengalmod"));
    }
}
```

- [ ] **Step 2: Implement `to_file_path`**

Add to `impl ModulePath` in `src/package.rs`:

```rust
pub fn to_file_path(&self, extension: &str) -> std::path::PathBuf {
    if self.0.is_empty() {
        std::path::PathBuf::from(format!("root.{}", extension))
    } else {
        let mut path = std::path::PathBuf::new();
        for (i, segment) in self.0.iter().enumerate() {
            if i == self.0.len() - 1 {
                path.push(format!("{}.{}", segment, extension));
            } else {
                path.push(segment);
            }
        }
        path
    }
}
```

- [ ] **Step 3: Run `to_file_path` tests**

Run: `cargo test --lib -- package::tests && cargo clippy`
Expected: All pass.

- [ ] **Step 4: Add `write_bengalmod_file` and refactor `write_interface`**

Add new function in `src/interface.rs`:

```rust
pub fn write_bengalmod_file(file: &BengalModFile, path: &Path) -> Result<()> {
    let payload = rmp_serde::to_vec(file).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to serialize interface: {}", e),
    })?;

    let mut out = std::fs::File::create(path).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to create file '{}': {}", path.display(), e),
    })?;

    out.write_all(MAGIC)
        .and_then(|()| out.write_all(&FORMAT_VERSION.to_le_bytes()))
        .and_then(|()| out.write_all(&payload))
        .map_err(|e| BengalError::InterfaceError {
            message: format!("failed to write interface file: {}", e),
        })?;

    Ok(())
}
```

Then refactor `write_interface` to call it:

```rust
pub fn write_interface(package: &LoweredPackage, path: &Path) -> Result<()> {
    let mut modules: HashMap<ModulePath, BirModule> = HashMap::new();
    let mut interfaces: HashMap<ModulePath, ModuleInterface> = HashMap::new();

    for (mod_path, lowered_mod) in &package.modules {
        modules.insert(mod_path.clone(), lowered_mod.bir.clone());
        if let Some(sem_info) = package.pkg_sem_info.module_infos.get(mod_path) {
            interfaces.insert(mod_path.clone(), ModuleInterface::from_semantic_info(sem_info));
        }
    }

    let file = BengalModFile {
        package_name: package.package_name.clone(),
        modules,
        interfaces,
    };

    write_bengalmod_file(&file, path)
}
```

- [ ] **Step 5: Run existing tests to verify refactor**

Run: `cargo test --test interface && cargo clippy`
Expected: All existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/package.rs src/interface.rs
git commit -m "feat(interface): add write_bengalmod_file and ModulePath::to_file_path"
```

---

### Task 5: Pipeline `.bengalmod` auto-generation

**Files:**
- Modify: `src/pipeline.rs` (add `emit_interfaces`)
- Modify: `src/lib.rs` (call `emit_interfaces` in `compile_to_executable`)
- Modify: `.gitignore` (add `.build/`)
- Test: `tests/interface.rs`

- [ ] **Step 1: Implement `emit_interfaces` in `src/pipeline.rs`**

The function takes a `cache_dir` parameter for testability (not a hardcoded path):

```rust
use crate::interface::{ModuleInterface, BengalModFile, write_bengalmod_file};

pub fn emit_interfaces(lowered: &LoweredPackage, cache_dir: &std::path::Path) {
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("warning: failed to create cache directory: {}", e);
        return;
    }

    for (module_path, module) in &lowered.modules {
        let sem_info = match lowered.pkg_sem_info.module_infos.get(module_path) {
            Some(info) => info,
            None => continue,
        };
        let iface = ModuleInterface::from_semantic_info(sem_info);
        let mod_file = BengalModFile {
            package_name: lowered.package_name.clone(),
            modules: std::collections::HashMap::from([(module_path.clone(), module.bir.clone())]),
            interfaces: std::collections::HashMap::from([(module_path.clone(), iface)]),
        };

        let file_path = cache_dir.join(module_path.to_file_path("bengalmod"));
        if let Some(parent) = file_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("warning: failed to create cache subdirectory: {}", e);
                continue;
            }
        }
        if let Err(e) = write_bengalmod_file(&mod_file, &file_path) {
            eprintln!("warning: failed to write interface cache: {}", e);
        }
    }
}
```

Note: Errors are non-fatal warnings, matching the spec.

- [ ] **Step 2: Call `emit_interfaces` in `compile_to_executable`**

In `src/lib.rs`, add the call between `lower` and `optimize`:

```rust
let lowered = pipeline::lower(analyzed, &mut diag)?;
pipeline::emit_interfaces(&lowered, std::path::Path::new(".build/cache"));  // NEW
let optimized = pipeline::optimize(lowered);
```

- [ ] **Step 3: Add `.build/` to `.gitignore`**

Append to `.gitignore`:

```
.build/
```

- [ ] **Step 4: Write integration test for `emit_interfaces`**

Add to `tests/interface.rs`. This exercises `emit_interfaces` end-to-end with a temp directory:

```rust
#[test]
fn emit_interfaces_creates_cache_files() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");

    let lowered = source_to_lowered("public func add(a: Int32, b: Int32) -> Int32 { a + b; }");
    bengal::pipeline::emit_interfaces(&lowered, &cache_dir);

    // Verify file exists for root module
    let root_path = cache_dir.join("root.bengalmod");
    assert!(root_path.exists(), "root.bengalmod should be created");

    // Verify file is readable and contains expected data
    let restored = bengal::interface::read_interface(&root_path).unwrap();
    assert_eq!(restored.package_name, lowered.package_name);
    let root_mod = bengal::package::ModulePath::root();
    assert!(restored.interfaces.contains_key(&root_mod));
    let iface = &restored.interfaces[&root_mod];
    assert!(!iface.functions.is_empty());
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test && cargo clippy`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/pipeline.rs src/lib.rs .gitignore tests/interface.rs
git commit -m "feat(pipeline): auto-generate .bengalmod files after lowering"
```

---

### Task 6: `interface_to_global_symbols` (GlobalSymbolTable injection)

**Files:**
- Modify: `src/semantic/package_analysis.rs` (add `interface_to_global_symbols`, make types accessible)
- Test: `tests/interface.rs`

Note: `GlobalSymbol`, `SymbolKind`, and `GlobalSymbolTable` are currently private in `package_analysis.rs`, and the module itself is private (`mod package_analysis` in `src/semantic/mod.rs`). Integration tests cannot access private modules. Changes needed:

1. Make `GlobalSymbol` (and its fields), `SymbolKind`, `GlobalSymbolTable` `pub`
2. In `src/semantic/mod.rs`, add re-export: `pub use package_analysis::{interface_to_global_symbols, GlobalSymbol, SymbolKind, GlobalSymbolTable};`

- [ ] **Step 1: Make types public and add re-exports**

In `src/semantic/package_analysis.rs`, change:
- `enum SymbolKind` → `pub enum SymbolKind`
- `struct GlobalSymbol` → `pub struct GlobalSymbol` (and all fields `pub`)
- `type GlobalSymbolTable` → `pub type GlobalSymbolTable`

In `src/semantic/mod.rs`, add alongside the existing `pub use package_analysis::analyze_package;`:
```rust
pub use package_analysis::{interface_to_global_symbols, GlobalSymbol, SymbolKind, GlobalSymbolTable};
```

- [ ] **Step 2: Write failing test**

Add to `tests/interface.rs`:

```rust
#[test]
fn interface_to_global_symbols_all_types() {
    use bengal::package::ModulePath;
    use bengal::semantic::interface_to_global_symbols;

    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("a".to_string(), InterfaceType::I32)],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Package,
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec![],
            fields: vec![("x".to_string(), InterfaceType::I32)],
            methods: vec![],
            computed: vec![],
            init_params: vec![("x".to_string(), InterfaceType::I32)],
        }],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Runnable".to_string(),
            methods: vec![],
            properties: vec![],
        }],
    };

    let mod_path = ModulePath(vec!["math".to_string()]);
    let symbols = interface_to_global_symbols(&iface, &mod_path);

    assert_eq!(symbols.len(), 3);
    assert!(symbols.contains_key("add"));
    assert!(symbols.contains_key("Point"));
    assert!(symbols.contains_key("Runnable"));

    // Verify visibility
    assert_eq!(symbols["add"].visibility, Visibility::Public);
    assert_eq!(symbols["Point"].visibility, Visibility::Package);

    // Verify module path
    assert_eq!(symbols["add"].module, mod_path);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --test interface -- interface_to_global_symbols`
Expected: Compilation error — function doesn't exist.

- [ ] **Step 4: Implement `interface_to_global_symbols`**

Add to `src/semantic/package_analysis.rs`:

```rust
use crate::interface::ModuleInterface;

pub fn interface_to_global_symbols(
    iface: &ModuleInterface,
    module_path: &ModulePath,
) -> HashMap<String, GlobalSymbol> {
    let mut symbols = HashMap::new();
    for func in &iface.functions {
        symbols.insert(func.name.clone(), GlobalSymbol {
            kind: SymbolKind::Func(func.to_func_sig()),
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

- [ ] **Step 5: Run tests**

Run: `cargo test --test interface -- interface_to_global_symbols && cargo clippy`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/semantic/package_analysis.rs tests/interface.rs
git commit -m "feat(semantic): add interface_to_global_symbols for GlobalSymbolTable injection"
```
