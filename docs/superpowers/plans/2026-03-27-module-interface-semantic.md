# Module Interface Semantic Serialization — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `.bengalmod` interface files with semantic information (function signatures, struct definitions, protocol definitions) following the Rust `.rmeta` model.

**Architecture:** Add interface-specific lightweight types (no AST `Block` bodies) to `src/interface.rs`, extend `SemanticInfo` with function signatures and visibility info, then convert semantic info → interface types during `write_interface()`. The existing BIR serialization is preserved alongside the new semantic layer.

**Tech Stack:** Rust, serde (already in Cargo.toml), rmp-serde (already in Cargo.toml)

**Spec:** `docs/superpowers/specs/2026-03-27-module-interface-semantic-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/interface.rs` | Modify | Add `InterfaceType`, `ModuleInterface`, conversion logic, update `BengalModFile` / `write_interface` |
| `src/semantic/resolver.rs` | Modify | Add `take_functions()`, add `conformances` to `StructInfo` |
| `src/semantic/mod.rs` | Modify | Add `functions` and `visibilities` fields to `SemanticInfo` |
| `src/semantic/pre_mono.rs` | Modify | Populate new `SemanticInfo` fields at construction site (line 350) |
| `src/semantic/post_mono.rs` | Modify | Populate new `SemanticInfo` fields at construction site (line 301) |
| `src/semantic/single_module_analysis.rs` | Modify | Populate new `SemanticInfo` fields at construction site (line 307) |
| `src/pipeline.rs` | Modify | Add `pkg_sem_info` to `LoweredPackage`, thread through `lower()` |
| `tests/interface.rs` | Modify | Add semantic interface round-trip tests |

---

### Task 1: Add `InterfaceType` and `InterfaceTypeParam`

**Files:**
- Modify: `src/interface.rs`

- [ ] **Step 1: Write failing test for `InterfaceType::from_type`**

Add at the bottom of `src/interface.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::types::Type;

    #[test]
    fn interface_type_from_primitives() {
        assert_eq!(InterfaceType::from_type(&Type::I32), InterfaceType::I32);
        assert_eq!(InterfaceType::from_type(&Type::I64), InterfaceType::I64);
        assert_eq!(InterfaceType::from_type(&Type::F32), InterfaceType::F32);
        assert_eq!(InterfaceType::from_type(&Type::F64), InterfaceType::F64);
        assert_eq!(InterfaceType::from_type(&Type::Bool), InterfaceType::Bool);
        assert_eq!(InterfaceType::from_type(&Type::Unit), InterfaceType::Unit);
    }

    #[test]
    fn interface_type_from_struct() {
        assert_eq!(
            InterfaceType::from_type(&Type::Struct("Point".to_string())),
            InterfaceType::Struct("Point".to_string()),
        );
    }

    #[test]
    fn interface_type_from_type_param() {
        assert_eq!(
            InterfaceType::from_type(&Type::TypeParam {
                name: "T".to_string(),
                bound: Some("Summable".to_string()),
            }),
            InterfaceType::TypeParam {
                name: "T".to_string(),
                bound: Some("Summable".to_string()),
            },
        );
    }

    #[test]
    fn interface_type_from_generic_recursive() {
        let ty = Type::Generic {
            name: "Pair".to_string(),
            args: vec![Type::I32, Type::Struct("Point".to_string())],
        };
        assert_eq!(
            InterfaceType::from_type(&ty),
            InterfaceType::Generic {
                name: "Pair".to_string(),
                args: vec![InterfaceType::I32, InterfaceType::Struct("Point".to_string())],
            },
        );
    }

    #[test]
    fn interface_type_from_array_recursive() {
        let ty = Type::Array {
            element: Box::new(Type::Generic {
                name: "Box".to_string(),
                args: vec![Type::I64],
            }),
            size: 5,
        };
        assert_eq!(
            InterfaceType::from_type(&ty),
            InterfaceType::Array {
                element: Box::new(InterfaceType::Generic {
                    name: "Box".to_string(),
                    args: vec![InterfaceType::I64],
                }),
                size: 5,
            },
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib interface::tests`
Expected: FAIL — `InterfaceType` not found

- [ ] **Step 3: Implement `InterfaceType`, `InterfaceTypeParam`, and `from_type`**

Add to `src/interface.rs` (before the existing `BengalModFile`):

```rust
use crate::semantic::types::Type;

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
            Type::InferVar(_) | Type::IntegerLiteral(_) | Type::FloatLiteral(_) | Type::Error => {
                unreachable!("interface types must be fully resolved")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceTypeParam {
    pub name: String,
    pub bound: Option<String>,
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib interface::tests`
Expected: All 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/interface.rs
git commit -m "feat(interface): add InterfaceType and from_type conversion"
```

---

### Task 2: Add `ModuleInterface` and all interface struct types

**Files:**
- Modify: `src/interface.rs`

- [ ] **Step 1: Write failing test for `ModuleInterface` round-trip**

Add to the `tests` module in `src/interface.rs`:

```rust
#[test]
fn module_interface_round_trip() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![
                    ("a".to_string(), InterfaceType::I32),
                    ("b".to_string(), InterfaceType::I32),
                ],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![InterfaceStructEntry {
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec!["Summable".to_string()],
            fields: vec![
                ("x".to_string(), InterfaceType::I32),
                ("y".to_string(), InterfaceType::I32),
            ],
            methods: vec![InterfaceMethodSig {
                name: "sum".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
            computed: vec![InterfaceComputedProp {
                name: "magnitude".to_string(),
                ty: InterfaceType::I32,
                has_setter: false,
            }],
            init_params: vec![
                ("x".to_string(), InterfaceType::I32),
                ("y".to_string(), InterfaceType::I32),
            ],
        }],
        protocols: vec![InterfaceProtocolEntry {
            name: "Summable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "sum".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
            properties: vec![InterfacePropertyReq {
                name: "count".to_string(),
                ty: InterfaceType::I32,
                has_setter: false,
            }],
        }],
    };
    let bytes = rmp_serde::to_vec(&iface).unwrap();
    let loaded: ModuleInterface = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(iface, loaded);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib interface::tests::module_interface_round_trip`
Expected: FAIL — `ModuleInterface` not found

- [ ] **Step 3: Add all interface struct types**

Add to `src/interface.rs` (after `InterfaceTypeParam`):

```rust
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceFuncSig {
    pub type_params: Vec<InterfaceTypeParam>,
    pub params: Vec<(String, InterfaceType)>,
    pub return_type: InterfaceType,
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

- [ ] **Step 4: Run tests**

Run: `cargo test --lib interface::tests`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/interface.rs
git commit -m "feat(interface): add ModuleInterface and all interface struct types"
```

---

### Task 3: Extend `SemanticInfo` — add `functions`, `visibilities`, `conformances`

**Files:**
- Modify: `src/semantic/resolver.rs` — add `take_functions()`, add `conformances` to `StructInfo`
- Modify: `src/semantic/mod.rs` — add fields to `SemanticInfo`, update `resolve_struct_members()`
- Modify: `src/semantic/pre_mono.rs` — populate new fields (line 350)
- Modify: `src/semantic/post_mono.rs` — populate new fields (line 301)
- Modify: `src/semantic/single_module_analysis.rs` — populate new fields (line 307)

- [ ] **Step 1: Add `conformances` field to `StructInfo`**

In `src/semantic/resolver.rs`, add after the `type_params` field (line 30):

```rust
#[derive(Debug, Clone)]
pub struct StructInfo {
    pub type_params: Vec<TypeParam>,
    pub conformances: Vec<String>,  // NEW
    pub fields: Vec<(String, Type)>,
    // ... rest unchanged
}
```

- [ ] **Step 2: Fix all `StructInfo` construction sites to include `conformances`**

In `src/semantic/resolver.rs`, update `reserve_struct()` (line 160):
```rust
conformances: vec![],
```

In `src/semantic/mod.rs`, update `resolve_struct_members()` (around line 322-334) to accept `conformances` and pass it through:

Change the function signature to take a `&StructDef` (it already does), and when building `StructInfo`:
```rust
resolver.define_struct(
    name.clone(),
    resolver::StructInfo {
        type_params: struct_def.type_params.clone(),
        conformances: struct_def.conformances.clone(),  // NEW
        fields,
        field_index,
        computed,
        computed_index,
        init,
        methods,
        method_index,
    },
);
```

- [ ] **Step 3: Add `take_functions()` and `take_all_functions()` to `Resolver`**

In `src/semantic/resolver.rs`, add after `take_all_protocols()` (line 210):

```rust
pub fn take_functions(&mut self) -> HashMap<String, FuncSig> {
    std::mem::take(&mut self.functions)
}

/// Take all function signatures (local + imported) for use in BIR lowering.
pub fn take_all_functions(&mut self) -> HashMap<String, FuncSig> {
    let mut all = std::mem::take(&mut self.functions);
    for (name, sig) in std::mem::take(&mut self.imported_funcs) {
        all.entry(name).or_insert(sig);
    }
    all
}
```

- [ ] **Step 4: Add new fields to `SemanticInfo`**

In `src/semantic/mod.rs`, update the `SemanticInfo` struct (line 38-43):

```rust
#[derive(Debug)]
pub struct SemanticInfo {
    pub struct_defs: HashMap<String, resolver::StructInfo>,
    pub struct_init_calls: std::collections::HashSet<NodeId>,
    pub protocols: HashMap<String, resolver::ProtocolInfo>,
    pub functions: HashMap<String, resolver::FuncSig>,
    pub visibilities: HashMap<String, Visibility>,
}
```

- [ ] **Step 5: Add `collect_visibilities` helper to `mod.rs`**

In `src/semantic/mod.rs`, add a shared helper (before the `#[cfg(test)]` section):

```rust
pub(super) fn collect_visibilities(program: &Program) -> HashMap<String, Visibility> {
    let mut vis = HashMap::new();
    for f in &program.functions {
        vis.insert(f.name.clone(), f.visibility);
    }
    for s in &program.structs {
        vis.insert(s.name.clone(), s.visibility);
    }
    for p in &program.protocols {
        vis.insert(p.name.clone(), p.visibility);
    }
    vis
}
```

- [ ] **Step 6: Update `SemanticInfo` construction in `pre_mono.rs`**

In `src/semantic/pre_mono.rs`, update the `SemanticInfo` construction (line 350-354):

```rust
let sem_info = SemanticInfo {
    struct_defs: resolver.take_struct_defs(),
    struct_init_calls: resolver.take_struct_init_calls(),
    protocols: resolver.take_protocols(),
    functions: resolver.take_functions(),
    visibilities: collect_visibilities(program),
};
```

Add import at top: `use super::collect_visibilities;`

- [ ] **Step 7: Update `SemanticInfo` construction in `post_mono.rs`**

In `src/semantic/post_mono.rs`, update (line 301-305):

```rust
Ok(SemanticInfo {
    struct_defs: resolver.take_struct_defs(),
    struct_init_calls: resolver.take_struct_init_calls(),
    protocols: resolver.take_protocols(),
    functions: resolver.take_functions(),
    visibilities: collect_visibilities(program),
})
```

Add import at top: `use super::collect_visibilities;`

- [ ] **Step 8: Update `SemanticInfo` construction in `single_module_analysis.rs`**

In `src/semantic/single_module_analysis.rs`, update (line 307-311). Note: uses `take_all_*` variants (includes imports). `take_all_functions()` mirrors this pattern. Imported symbols are excluded during interface generation because they lack `visibilities` entries.

```rust
Ok(SemanticInfo {
    struct_defs: resolver.take_all_struct_defs(),
    struct_init_calls: resolver.take_struct_init_calls(),
    protocols: resolver.take_all_protocols(),
    functions: resolver.take_all_functions(),
    visibilities: collect_visibilities(program),
})
```

Add import at top: `use super::collect_visibilities;`

- [ ] **Step 9: Run full test suite**

Run: `cargo test`
Expected: All existing tests PASS (no behavior change, just new fields)

- [ ] **Step 10: Commit**

```bash
git add src/semantic/resolver.rs src/semantic/mod.rs src/semantic/pre_mono.rs src/semantic/post_mono.rs src/semantic/single_module_analysis.rs
git commit -m "feat(semantic): add functions, visibilities, conformances to SemanticInfo"
```

---

### Task 4: Implement `ModuleInterface::from_semantic_info`

**Files:**
- Modify: `src/interface.rs`

- [ ] **Step 1: Write failing test for visibility filtering**

Add to tests module in `src/interface.rs`:

```rust
use crate::parser::ast::{TypeParam, Visibility};
use crate::semantic::resolver::{
    ComputedPropInfo, FuncSig, InitializerInfo, MethodInfo, ProtocolInfo,
    ProtocolMethodSig, ProtocolPropertyReq, StructInfo,
};
use crate::semantic::SemanticInfo;
use std::collections::{HashMap, HashSet};

fn make_test_semantic_info() -> SemanticInfo {
    let mut functions = HashMap::new();
    functions.insert(
        "public_add".to_string(),
        FuncSig {
            type_params: vec![],
            params: vec![("a".to_string(), Type::I32), ("b".to_string(), Type::I32)],
            return_type: Type::I32,
        },
    );
    functions.insert(
        "internal_helper".to_string(),
        FuncSig {
            type_params: vec![],
            params: vec![],
            return_type: Type::Unit,
        },
    );
    functions.insert(
        "fileprivate_fn".to_string(),
        FuncSig {
            type_params: vec![],
            params: vec![],
            return_type: Type::Unit,
        },
    );
    functions.insert(
        "generic_pub".to_string(),
        FuncSig {
            type_params: vec![TypeParam {
                name: "T".to_string(),
                bound: Some("Summable".to_string()),
            }],
            params: vec![("x".to_string(), Type::TypeParam {
                name: "T".to_string(),
                bound: Some("Summable".to_string()),
            })],
            return_type: Type::I32,
        },
    );

    let mut visibilities = HashMap::new();
    visibilities.insert("public_add".to_string(), Visibility::Public);
    visibilities.insert("internal_helper".to_string(), Visibility::Internal);
    visibilities.insert("fileprivate_fn".to_string(), Visibility::Fileprivate);
    visibilities.insert("generic_pub".to_string(), Visibility::Public);
    visibilities.insert("MyStruct".to_string(), Visibility::Package);
    visibilities.insert("MyProto".to_string(), Visibility::Private);

    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "MyStruct".to_string(),
        StructInfo {
            type_params: vec![],
            conformances: vec!["Proto".to_string()],
            fields: vec![("x".to_string(), Type::I32)],
            field_index: [("x".to_string(), 0)].into_iter().collect(),
            computed: vec![],
            computed_index: HashMap::new(),
            init: InitializerInfo {
                params: vec![("x".to_string(), Type::I32)],
                body: None,
            },
            methods: vec![],
            method_index: HashMap::new(),
        },
    );

    let mut protocols = HashMap::new();
    protocols.insert(
        "MyProto".to_string(),
        ProtocolInfo {
            name: "MyProto".to_string(),
            methods: vec![],
            properties: vec![],
        },
    );

    SemanticInfo {
        struct_defs,
        struct_init_calls: HashSet::new(),
        protocols,
        functions,
        visibilities,
    }
}

#[test]
fn from_semantic_info_filters_visibility() {
    let sem = make_test_semantic_info();
    let iface = ModuleInterface::from_semantic_info(&sem);

    // Public + Package included; Internal, Fileprivate, Private excluded
    assert_eq!(iface.functions.len(), 2);
    let func_names: Vec<&str> = iface.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(func_names.contains(&"public_add"));
    assert!(func_names.contains(&"generic_pub"));
    assert!(!func_names.contains(&"internal_helper"));
    assert!(!func_names.contains(&"fileprivate_fn"));

    // Verify generic function has type_params with bound
    let generic = iface.functions.iter().find(|f| f.name == "generic_pub").unwrap();
    assert_eq!(generic.sig.type_params.len(), 1);
    assert_eq!(generic.sig.type_params[0].name, "T");
    assert_eq!(generic.sig.type_params[0].bound, Some("Summable".to_string()));

    // Package struct included
    assert_eq!(iface.structs.len(), 1);
    assert_eq!(iface.structs[0].name, "MyStruct");
    assert_eq!(iface.structs[0].conformances, vec!["Proto".to_string()]);

    // Private protocol excluded
    assert_eq!(iface.protocols.len(), 0);
}

#[test]
fn from_semantic_info_empty() {
    let sem = SemanticInfo {
        struct_defs: HashMap::new(),
        struct_init_calls: HashSet::new(),
        protocols: HashMap::new(),
        functions: HashMap::new(),
        visibilities: HashMap::new(),
    };
    let iface = ModuleInterface::from_semantic_info(&sem);
    assert!(iface.functions.is_empty());
    assert!(iface.structs.is_empty());
    assert!(iface.protocols.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib interface::tests::from_semantic_info_filters_visibility`
Expected: FAIL — `from_semantic_info` not found

- [ ] **Step 3: Implement `ModuleInterface::from_semantic_info`**

Add to `src/interface.rs`:

```rust
use crate::parser::ast::{TypeParam, Visibility};
use crate::semantic::resolver::{FuncSig, MethodInfo, ProtocolInfo, StructInfo};
use crate::semantic::SemanticInfo;

impl InterfaceTypeParam {
    pub fn from_type_param(tp: &TypeParam) -> Self {
        InterfaceTypeParam {
            name: tp.name.clone(),
            bound: tp.bound.clone(),
        }
    }
}

fn is_exported(vis: Visibility) -> bool {
    matches!(vis, Visibility::Public | Visibility::Package)
}

impl ModuleInterface {
    pub fn from_semantic_info(sem: &SemanticInfo) -> Self {
        let functions: Vec<InterfaceFuncEntry> = sem
            .functions
            .iter()
            .filter(|(name, _)| {
                sem.visibilities
                    .get(*name)
                    .copied()
                    .map_or(false, is_exported)
            })
            .map(|(name, sig)| InterfaceFuncEntry {
                name: name.clone(),
                sig: InterfaceFuncSig {
                    type_params: sig
                        .type_params
                        .iter()
                        .map(InterfaceTypeParam::from_type_param)
                        .collect(),
                    params: sig
                        .params
                        .iter()
                        .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                        .collect(),
                    return_type: InterfaceType::from_type(&sig.return_type),
                },
            })
            .collect();

        let structs: Vec<InterfaceStructEntry> = sem
            .struct_defs
            .iter()
            .filter(|(name, _)| {
                sem.visibilities
                    .get(*name)
                    .copied()
                    .map_or(false, is_exported)
            })
            .map(|(name, info)| InterfaceStructEntry {
                name: name.clone(),
                type_params: info
                    .type_params
                    .iter()
                    .map(InterfaceTypeParam::from_type_param)
                    .collect(),
                conformances: info.conformances.clone(),
                fields: info
                    .fields
                    .iter()
                    .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                    .collect(),
                methods: info
                    .methods
                    .iter()
                    .map(|m| InterfaceMethodSig {
                        name: m.name.clone(),
                        params: m
                            .params
                            .iter()
                            .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                            .collect(),
                        return_type: InterfaceType::from_type(&m.return_type),
                    })
                    .collect(),
                computed: info
                    .computed
                    .iter()
                    .map(|c| InterfaceComputedProp {
                        name: c.name.clone(),
                        ty: InterfaceType::from_type(&c.ty),
                        has_setter: c.has_setter,
                    })
                    .collect(),
                init_params: info
                    .init
                    .params
                    .iter()
                    .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                    .collect(),
            })
            .collect();

        let protocols: Vec<InterfaceProtocolEntry> = sem
            .protocols
            .iter()
            .filter(|(name, _)| {
                sem.visibilities
                    .get(*name)
                    .copied()
                    .map_or(false, is_exported)
            })
            .map(|(name, info)| InterfaceProtocolEntry {
                name: name.clone(),
                methods: info
                    .methods
                    .iter()
                    .map(|m| InterfaceMethodSig {
                        name: m.name.clone(),
                        params: m
                            .params
                            .iter()
                            .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                            .collect(),
                        return_type: InterfaceType::from_type(&m.return_type),
                    })
                    .collect(),
                properties: info
                    .properties
                    .iter()
                    .map(|p| InterfacePropertyReq {
                        name: p.name.clone(),
                        ty: InterfaceType::from_type(&p.ty),
                        has_setter: p.has_setter,
                    })
                    .collect(),
            })
            .collect();

        // Sort for deterministic output (HashMap iteration order is random)
        let mut functions = functions;
        functions.sort_by(|a, b| a.name.cmp(&b.name));
        let mut structs = structs;
        structs.sort_by(|a, b| a.name.cmp(&b.name));
        let mut protocols = protocols;
        protocols.sort_by(|a, b| a.name.cmp(&b.name));

        ModuleInterface {
            functions,
            structs,
            protocols,
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib interface::tests`
Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/interface.rs
git commit -m "feat(interface): implement ModuleInterface::from_semantic_info with visibility filtering"
```

---

### Task 5: Pipeline integration — thread `pkg_sem_info` through `LoweredPackage`

**Files:**
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Add `pkg_sem_info` field to `LoweredPackage`**

In `src/pipeline.rs`, update `LoweredPackage` (line 29-33):

```rust
pub struct LoweredPackage {
    pub package_name: String,
    pub modules: HashMap<ModulePath, LoweredModule>,
    pub sources: HashMap<ModulePath, String>,
    pub pkg_sem_info: PackageSemanticInfo,
}
```

Add the import at the top:
```rust
use crate::semantic::PackageSemanticInfo;
```

- [ ] **Step 2: Update `lower()` to move `pkg_sem_info`**

In `src/pipeline.rs`, update the `Ok(LoweredPackage { ... })` at the end of `lower()` (line 254-258):

```rust
Ok(LoweredPackage {
    package_name: analyzed.package_name,
    modules,
    sources,
    pkg_sem_info: analyzed.pkg_sem_info,
})
```

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/pipeline.rs
git commit -m "feat(pipeline): thread pkg_sem_info through LoweredPackage"
```

---

### Task 6: Update `BengalModFile` and `write_interface` to include semantic info

**Files:**
- Modify: `src/interface.rs`

- [ ] **Step 1: Update `BengalModFile` struct**

In `src/interface.rs`, update `BengalModFile` (line 15-19):

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
    pub interfaces: HashMap<ModulePath, ModuleInterface>,
}
```

- [ ] **Step 2: Update `write_interface` to build and include `ModuleInterface`**

Update the `write_interface` function:

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

    let payload = rmp_serde::to_vec(&file).map_err(|e| BengalError::InterfaceError {
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

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All existing tests PASS (the existing integration tests in `tests/interface.rs` will automatically test the new field because `BengalModFile` serialization must work)

- [ ] **Step 4: Commit**

```bash
git add src/interface.rs
git commit -m "feat(interface): include ModuleInterface in BengalModFile and write_interface"
```

---

### Task 7: Integration tests — semantic info round-trip

**Files:**
- Modify: `tests/interface.rs`

- [ ] **Step 1: Add test for semantic info in single-module round-trip**

Add to `tests/interface.rs`:

```rust
use bengal::interface::ModuleInterface;

#[test]
fn round_trip_semantic_info_functions() {
    let lowered = source_to_lowered(
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
         func internal_helper() -> Int32 { return 0; }
         func main() -> Int32 { return add(1, 2); }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let iface = loaded.interfaces.get(&ModulePath::root()).unwrap();
    // Only public function in interface (main and internal_helper are Internal)
    assert_eq!(iface.functions.len(), 1);
    assert_eq!(iface.functions[0].name, "add");
    assert_eq!(iface.functions[0].sig.params.len(), 2);
}

#[test]
fn round_trip_semantic_info_generic_function() {
    let lowered = source_to_lowered(
        "public func identity<T>(x: T) -> T { return x; }
         func main() -> Int32 { return identity<Int32>(42); }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let iface = loaded.interfaces.get(&ModulePath::root()).unwrap();
    assert_eq!(iface.functions.len(), 1);
    assert_eq!(iface.functions[0].name, "identity");
    assert!(!iface.functions[0].sig.type_params.is_empty());
    assert_eq!(iface.functions[0].sig.type_params[0].name, "T");
}

#[test]
fn round_trip_semantic_info_struct() {
    let lowered = source_to_lowered(
        "protocol Summable { func sum() -> Int32; }
         public struct Point: Summable {
            var x: Int32;
            var y: Int32;
            func sum() -> Int32 { return self.x + self.y; }
         }
         func main() -> Int32 {
            let p = Point(x: 1, y: 2);
            return p.sum();
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let iface = loaded.interfaces.get(&ModulePath::root()).unwrap();
    assert_eq!(iface.structs.len(), 1);
    assert_eq!(iface.structs[0].name, "Point");
    assert_eq!(iface.structs[0].conformances, vec!["Summable".to_string()]);
    assert_eq!(iface.structs[0].fields.len(), 2);
    assert_eq!(iface.structs[0].methods.len(), 1);
    assert_eq!(iface.structs[0].init_params.len(), 2);
}

#[test]
fn round_trip_semantic_info_protocol() {
    let lowered = source_to_lowered(
        "public protocol Drawable {
            func draw() -> Int32;
            var visible: Bool { get };
         }
         struct Canvas: Drawable {
            var visible: Bool { get { return true; } }
            func draw() -> Int32 { return 1; }
         }
         func main() -> Int32 { return 0; }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let iface = loaded.interfaces.get(&ModulePath::root()).unwrap();
    assert_eq!(iface.protocols.len(), 1);
    assert_eq!(iface.protocols[0].name, "Drawable");
    assert_eq!(iface.protocols[0].methods.len(), 1);
    assert_eq!(iface.protocols[0].properties.len(), 1);
    assert!(iface.protocols[0].properties[0].has_setter == false);
}
```

- [ ] **Step 2: Add multi-module semantic info test**

Add to `tests/interface.rs`:

```rust
#[test]
fn round_trip_multi_module_semantic_info() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("Bengal.toml"),
        "[package]\nname = \"mypkg\"\nentry = \"main.bengal\"",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.bengal"),
        "module math;\nimport math::add;\nfunc main() -> Int32 { return add(1, 2); }",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("math.bengal"),
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
    )
    .unwrap();

    let parsed = pipeline::parse(&dir.path().join("main.bengal")).unwrap();
    let analyzed = pipeline::analyze(parsed, &mut bengal::error::DiagCtxt::new()).unwrap();
    let lowered = pipeline::lower(analyzed, &mut bengal::error::DiagCtxt::new()).unwrap();
    let optimized = pipeline::optimize(lowered);

    let interface_file = dir.path().join("mypkg.bengalmod");
    write_interface(&optimized, &interface_file).unwrap();
    let loaded = read_interface(&interface_file).unwrap();

    // Math module should have `add` in its interface
    let math_path = ModulePath::new(vec!["math".to_string()]);
    let math_iface = loaded.interfaces.get(&math_path).unwrap();
    assert!(
        math_iface.functions.iter().any(|f| f.name == "add"),
        "math module interface should contain public func add"
    );
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test --test interface`
Expected: All tests PASS

- [ ] **Step 4: Run full test suite to confirm no regressions**

Run: `cargo test`
Expected: All tests PASS

- [ ] **Step 5: Run clippy**

Run: `cargo clippy`
Expected: No warnings

- [ ] **Step 6: Commit**

```bash
git add tests/interface.rs
git commit -m "test(interface): add semantic info round-trip integration tests"
```
