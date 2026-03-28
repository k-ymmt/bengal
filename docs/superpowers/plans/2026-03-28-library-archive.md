# Library Archive Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Embed pre-compiled object code in `.bengalmod` files so consumers skip codegen for non-generic dependency functions.

**Architecture:** Library-side: after codegen, save object bytes + generic-only BIR + interface into `.bengalmod`. Consumer-side: load pre-compiled objects directly for linking, merge only generic BIR for monomorphization. Pipeline reordering: `emit_package_bengalmod` moves after `codegen`.

**Tech Stack:** Rust, rmp-serde (MessagePack), inkwell (LLVM codegen)

**Spec:** `docs/superpowers/specs/2026-03-28-library-archive-design.md`

---

### Task 1: Add `object_bytes` to `BengalModFile` and bump FORMAT_VERSION

**Files:**
- Modify: `src/interface.rs:21-23` — bump `FORMAT_VERSION` to 3
- Modify: `src/interface.rs:646-651` — add `object_bytes` field to `BengalModFile`
- Modify: `src/interface.rs:674-696` — update `write_interface` to include empty `object_bytes`
- Modify: `src/pipeline.rs:478-482` — update `emit_interfaces` to include empty `object_bytes`
- Modify: `src/pipeline.rs:555-559` — update `emit_package_bengalmod` to include empty `object_bytes`

- [ ] **Step 1: Add `object_bytes` to `BengalModFile`**

In `src/interface.rs`, add the field:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
    pub interfaces: HashMap<ModulePath, ModuleInterface>,
    /// Pre-compiled object code per module (empty for old-format or generic-only packages).
    pub object_bytes: HashMap<ModulePath, Vec<u8>>,
}
```

Bump `FORMAT_VERSION`:
```rust
pub const FORMAT_VERSION: u32 = 3;
```

- [ ] **Step 2: Fix all compilation errors**

Every construction of `BengalModFile` needs `object_bytes: HashMap::new()`. Search the codebase for `BengalModFile {` and add the field. Known locations:
- `src/interface.rs` — `write_interface` function
- `src/pipeline.rs` — `emit_interfaces` function
- `src/pipeline.rs` — `emit_package_bengalmod` function

- [ ] **Step 3: Run tests**

Run: `cargo test --lib`
Expected: PASS (all existing tests work with empty `object_bytes`)

Note: Integration tests in `tests/separate_compilation.rs` will FAIL because they write v3 `.bengalmod` files from the library side but the `compile_lib` helper doesn't go through codegen yet. This is expected — they will be fixed in later tasks.

- [ ] **Step 4: Commit**

```
feat(interface): add object_bytes to BengalModFile and bump to v3
```

---

### Task 2: `filter_generic_functions` helper

**Files:**
- Modify: `src/pipeline.rs` — add `filter_generic_functions`
- Test: `src/pipeline.rs` (inline test)

- [ ] **Step 1: Write test**

Add to `src/pipeline.rs` tests module:

```rust
#[test]
fn filter_generic_functions_mixed() {
    use crate::bir::instruction::{BirFunction, BirType, BasicBlock, CfgRegion};

    let generic_fn = BirFunction {
        name: "identity".to_string(),
        type_params: vec!["T".to_string()],
        params: vec![],
        return_type: BirType::TypeParam("T".to_string()),
        blocks: vec![],
        body: vec![],
    };
    let non_generic_fn = BirFunction {
        name: "add".to_string(),
        type_params: vec![],
        params: vec![],
        return_type: BirType::I32,
        blocks: vec![],
        body: vec![],
    };
    let bir = BirModule {
        functions: vec![generic_fn.clone(), non_generic_fn],
        struct_layouts: HashMap::from([("Point".to_string(), vec![("x".to_string(), BirType::I32)])]),
        struct_type_params: HashMap::from([("Box".to_string(), vec!["T".to_string()])]),
        conformance_map: HashMap::new(),
    };

    let filtered = filter_generic_functions(&bir);
    assert_eq!(filtered.functions.len(), 1);
    assert_eq!(filtered.functions[0].name, "identity");
    assert_eq!(filtered.struct_layouts.len(), 1);
    assert_eq!(filtered.struct_type_params.len(), 1);
}

#[test]
fn filter_generic_functions_no_generics() {
    let bir = BirModule {
        functions: vec![BirFunction {
            name: "add".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: BirType::I32,
            blocks: vec![],
            body: vec![],
        }],
        struct_layouts: HashMap::from([("Point".to_string(), vec![])]),
        struct_type_params: HashMap::new(),
        conformance_map: HashMap::new(),
    };

    let filtered = filter_generic_functions(&bir);
    assert!(filtered.functions.is_empty());
    assert_eq!(filtered.struct_layouts.len(), 1, "struct_layouts preserved");
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --lib filter_generic_functions`
Expected: FAIL

- [ ] **Step 3: Implement `filter_generic_functions`**

Add to `src/pipeline.rs` (private helper, before `emit_package_bengalmod`):

```rust
/// Filter a BIR module to retain only generic functions.
/// Non-function data (struct_layouts, struct_type_params, conformance_map) is preserved.
fn filter_generic_functions(bir: &BirModule) -> BirModule {
    BirModule {
        functions: bir
            .functions
            .iter()
            .filter(|f| !f.type_params.is_empty())
            .cloned()
            .collect(),
        struct_layouts: bir.struct_layouts.clone(),
        struct_type_params: bir.struct_type_params.clone(),
        conformance_map: bir.conformance_map.clone(),
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib filter_generic_functions`
Expected: PASS

- [ ] **Step 5: Commit**

```
feat(pipeline): add filter_generic_functions helper
```

---

### Task 3: `EmitData` and updated `emit_package_bengalmod`

**Files:**
- Modify: `src/pipeline.rs` — add `EmitData`, rewrite `emit_package_bengalmod`

- [ ] **Step 1: Write test for updated `emit_package_bengalmod`**

Replace the existing `emit_package_bengalmod_creates_file` test with one that verifies object code and generic-only BIR:

```rust
#[test]
fn emit_package_bengalmod_with_object_code() {
    let source = r#"
        public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        public func identity<T>(x: T) -> T { return x; }
        func main() -> Int32 { return add(1, 2); }
    "#;
    let parsed = parse_source("testlib", source).unwrap();
    let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
    let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();
    let optimized = optimize(lowered);
    let emit_data = EmitData::from_lowered(&optimized);
    let mono = monomorphize(optimized, &mut DiagCtxt::new()).unwrap();
    let compiled = codegen(mono, &mut DiagCtxt::new()).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    emit_package_bengalmod(&emit_data, &compiled, dir.path());

    let file_path = dir.path().join("testlib.bengalmod");
    assert!(file_path.exists());

    let loaded = crate::interface::read_interface(&file_path).unwrap();
    assert_eq!(loaded.package_name, "testlib");
    assert!(!loaded.interfaces.is_empty());

    // Object code should be present
    assert!(!loaded.object_bytes.is_empty(), "object_bytes should be non-empty");

    // BIR should contain only generic functions
    let root_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    for func in &root_bir.functions {
        assert!(
            !func.type_params.is_empty(),
            "BIR should only contain generic functions, found '{}'",
            func.name
        );
    }
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --lib emit_package_bengalmod_with_object_code`
Expected: FAIL — `EmitData` not found

- [ ] **Step 3: Implement `EmitData`**

Add to `src/pipeline.rs`:

```rust
/// Data saved from LoweredPackage for emit_package_bengalmod.
/// Extracted after optimize() but before monomorphize() consumes the package.
pub struct EmitData {
    pub package_name: String,
    pub pkg_sem_info: PackageSemanticInfo,
    pub modules_bir: HashMap<ModulePath, BirModule>,
}

impl EmitData {
    pub fn from_lowered(lowered: &LoweredPackage) -> Self {
        EmitData {
            package_name: lowered.package_name.clone(),
            pkg_sem_info: lowered.pkg_sem_info.clone(),
            modules_bir: lowered
                .modules
                .iter()
                .map(|(path, m)| (path.clone(), m.bir.clone()))
                .collect(),
        }
    }
}
```

Note: `PackageSemanticInfo` needs `Clone`. Check if it already derives `Clone`; if not, add `#[derive(Clone)]` to `PackageSemanticInfo` in `src/semantic/mod.rs`. Also check `SemanticInfo` — it needs `Clone` too since it's in `PackageSemanticInfo.module_infos`. Add derives as needed (to `SemanticInfo`, `FuncSig`, `StructInfo`, etc. if they don't already have `Clone`).

- [ ] **Step 4: Rewrite `emit_package_bengalmod`**

Change the signature and body:

```rust
/// Emit a single `.bengalmod` containing all modules of the package.
/// Uses EmitData (saved pre-monomorphize) for BIR/interface and CompiledPackage for object code.
pub fn emit_package_bengalmod(
    emit_data: &EmitData,
    compiled: &CompiledPackage,
    cache_dir: &std::path::Path,
) {
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("warning: failed to create cache directory: {}", e);
        return;
    }

    let mut all_modules = HashMap::new();
    let mut all_interfaces = HashMap::new();
    let mut all_object_bytes = HashMap::new();

    for (module_path, bir) in &emit_data.modules_bir {
        let sem_info = match emit_data.pkg_sem_info.module_infos.get(module_path) {
            Some(info) => info,
            None => continue,
        };
        let iface = crate::interface::ModuleInterface::from_semantic_info(sem_info);
        all_modules.insert(module_path.clone(), filter_generic_functions(bir));
        all_interfaces.insert(module_path.clone(), iface);

        if let Some(obj) = compiled.object_bytes.get(module_path) {
            all_object_bytes.insert(module_path.clone(), obj.clone());
        }
    }

    let mod_file = crate::interface::BengalModFile {
        package_name: emit_data.package_name.clone(),
        modules: all_modules,
        interfaces: all_interfaces,
        object_bytes: all_object_bytes,
    };

    let file_path = cache_dir.join(format!("{}.bengalmod", emit_data.package_name));
    if let Err(e) = crate::interface::write_bengalmod_file(&mod_file, &file_path) {
        eprintln!("warning: failed to write package interface: {}", e);
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib emit_package_bengalmod`
Expected: PASS

- [ ] **Step 6: Commit**

```
feat(pipeline): add EmitData and embed object code in emit_package_bengalmod
```

---

### Task 4: Update `ExternalDep` and `load_external_dep`

**Files:**
- Modify: `src/pipeline.rs:22-31` — add `object_bytes` to `ExternalDep`
- Modify: `src/pipeline.rs:497-510` — update `load_external_dep`

- [ ] **Step 1: Add `object_bytes` to `ExternalDep`**

```rust
pub struct ExternalDep {
    pub name: String,
    pub package_name: String,
    pub interfaces: HashMap<ModulePath, ModuleInterface>,
    pub bir_modules: HashMap<ModulePath, BirModule>,
    /// Pre-compiled object code per module.
    pub object_bytes: HashMap<ModulePath, Vec<u8>>,
}
```

- [ ] **Step 2: Fix compilation errors**

Update `load_external_dep` to map `object_bytes`:
```rust
Ok(ExternalDep {
    name: name.to_string(),
    package_name: mod_file.package_name,
    interfaces: mod_file.interfaces,
    bir_modules: mod_file.modules,
    object_bytes: mod_file.object_bytes,
})
```

Search for all `ExternalDep {` constructions (including in tests like `src/semantic/package_analysis.rs`) and add `object_bytes: HashMap::new()`.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib`
Expected: PASS

- [ ] **Step 4: Commit**

```
feat(pipeline): add object_bytes to ExternalDep
```

---

### Task 5: `collect_external_objects` and update `link`

**Files:**
- Modify: `src/pipeline.rs` — add `collect_external_objects`, update `link` signature

- [ ] **Step 1: Write test for `collect_external_objects`**

```rust
#[test]
fn collect_external_objects_maps_correctly() {
    let dep = ExternalDep {
        name: "math".to_string(),
        package_name: "mathlib".to_string(),
        interfaces: HashMap::new(),
        bir_modules: HashMap::new(),
        object_bytes: HashMap::from([(ModulePath::root(), vec![1, 2, 3])]),
    };

    let objects = collect_external_objects(&[dep]);
    let ext_path = crate::semantic::dep_module_path("math", &ModulePath::root());
    assert!(objects.contains_key(&ext_path));
    assert_eq!(objects[&ext_path], vec![1, 2, 3]);
}
```

- [ ] **Step 2: Implement `collect_external_objects`**

```rust
/// Collect pre-compiled object bytes from external dependencies for linking.
pub fn collect_external_objects(
    external_deps: &[ExternalDep],
) -> HashMap<ModulePath, Vec<u8>> {
    let mut objects = HashMap::new();
    for dep in external_deps {
        for (mod_path, bytes) in &dep.object_bytes {
            let ext_path = crate::semantic::dep_module_path(&dep.name, mod_path);
            objects.insert(ext_path, bytes.clone());
        }
    }
    objects
}
```

- [ ] **Step 3: Update `link` to accept external objects**

New signature:
```rust
pub fn link(
    compiled: CompiledPackage,
    external_objects: &HashMap<ModulePath, Vec<u8>>,
    output_path: &Path,
) -> Result<(), crate::error::PipelineError>
```

Inside, after writing local `.o` files, also write external objects with `ext_` prefix:
```rust
for (mod_path, bytes) in external_objects {
    let obj_name = if mod_path.0.is_empty() {
        "ext_root.o".to_string()
    } else {
        format!("ext_{}.o", mod_path.0.join("_"))
    };
    let obj_path = temp_dir.join(&obj_name);
    std::fs::write(&obj_path, bytes).map_err(|e| {
        crate::error::PipelineError::package(
            "link",
            BengalError::PackageError {
                message: format!("failed to write external object file: {}", e),
            },
        )
    })?;
    obj_files.push(obj_path);
}
```

- [ ] **Step 4: Fix all callers of `link`**

Search for all calls to `pipeline::link(` and `link(compiled`:
- `src/lib.rs` — `compile_to_executable`: pass `&HashMap::new()` for now (will update in Task 6)
- `src/main.rs` — Compile handler: pass `&HashMap::new()` for now
- `tests/separate_compilation.rs` — `compile_and_run_with_deps`: pass `&HashMap::new()` for now

- [ ] **Step 5: Run tests**

Run: `cargo test --lib collect_external_objects`
Expected: PASS

Run: `cargo test`
Expected: PASS (all tests with empty external objects)

- [ ] **Step 6: Commit**

```
feat(pipeline): add collect_external_objects and update link for external objects
```

---

### Task 6: Wire pipeline — `compile_to_executable`, `main.rs`, test helpers

**Files:**
- Modify: `src/lib.rs` — reorder pipeline with EmitData
- Modify: `src/main.rs` — reorder inline pipeline with EmitData
- Modify: `tests/separate_compilation.rs` — update `compile_lib` and `compile_and_run_with_deps`

- [ ] **Step 1: Update `compile_to_executable` in `src/lib.rs`**

```rust
pub fn compile_to_executable(
    entry_path: &Path,
    output_path: &Path,
    external_deps: &[pipeline::ExternalDep],
) -> std::result::Result<(), error::PipelineError> {
    let mut diag = DiagCtxt::new();
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze_with_deps(parsed, external_deps, &mut diag)?;
    let lowered = pipeline::lower(analyzed, &mut diag)?;
    pipeline::emit_interfaces(&lowered, std::path::Path::new(".build/cache"));
    let mut lowered = lowered;
    pipeline::merge_external_deps(&mut lowered, external_deps);
    let optimized = pipeline::optimize(lowered);
    let emit_data = pipeline::EmitData::from_lowered(&optimized);
    let mono = pipeline::monomorphize(optimized, &mut diag)?;
    let compiled = pipeline::codegen(mono, &mut diag)?;
    pipeline::emit_package_bengalmod(&emit_data, &compiled, std::path::Path::new(".build/cache"));
    let ext_objects = pipeline::collect_external_objects(external_deps);
    pipeline::link(compiled, &ext_objects, output_path)
}
```

- [ ] **Step 2: Update `main.rs` inline pipeline**

In the Compile handler, move `emit_package_bengalmod` after codegen:
1. Remove the current `emit_package_bengalmod` call (lines 123-126)
2. After `let optimized = ...`, add `let emit_data = bengal::pipeline::EmitData::from_lowered(&optimized);`
3. After `let compiled = ...`, add `bengal::pipeline::emit_package_bengalmod(&emit_data, &compiled, ...);`
4. Before `link`, add `let ext_objects = bengal::pipeline::collect_external_objects(&external_deps);`
5. Update `link` call to pass `&ext_objects`

- [ ] **Step 3: Update `tests/separate_compilation.rs` helpers**

Update `compile_lib` to run the full pipeline (through codegen) so the `.bengalmod` includes object code:

```rust
fn compile_lib(name: &str, source: &str, dir: &Path) -> std::path::PathBuf {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(name, source).unwrap();
    let analyzed = bengal::pipeline::analyze(parsed, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    let optimized = bengal::pipeline::optimize(lowered);
    let emit_data = bengal::pipeline::EmitData::from_lowered(&optimized);
    let mono = bengal::pipeline::monomorphize(optimized, &mut diag).unwrap();
    let compiled = bengal::pipeline::codegen(mono, &mut diag).unwrap();
    bengal::pipeline::emit_package_bengalmod(&emit_data, &compiled, dir);
    dir.join(format!("{}.bengalmod", name))
}
```

Update `compile_and_run_with_deps` to pass external objects to link:

```rust
fn compile_and_run_with_deps(source: &str, deps: &[(&str, &Path)]) -> i32 {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source("app", source).unwrap();

    let external_deps: Vec<bengal::pipeline::ExternalDep> = deps
        .iter()
        .map(|(name, path)| bengal::pipeline::load_external_dep(name, path).unwrap())
        .collect();

    let analyzed = bengal::pipeline::analyze_with_deps(parsed, &external_deps, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    let mut lowered = lowered;
    bengal::pipeline::merge_external_deps(&mut lowered, &external_deps);
    let optimized = bengal::pipeline::optimize(lowered);
    let mono = bengal::pipeline::monomorphize(optimized, &mut diag).unwrap();
    let compiled = bengal::pipeline::codegen(mono, &mut diag).unwrap();

    let ext_objects = bengal::pipeline::collect_external_objects(&external_deps);
    let link_dir = tempfile::TempDir::new().unwrap();
    let exe_path = link_dir.path().join("test_exe");
    bengal::pipeline::link(compiled, &ext_objects, &exe_path).unwrap();

    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run compiled binary");
    output.status.code().unwrap_or(-1)
}
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: ALL PASS (all 7 separate_compilation tests + all lib tests)

This is the critical milestone — all existing tests must pass with the new pipeline.

- [ ] **Step 5: Commit**

```
feat(pipeline): wire object code embedding into full compilation pipeline
```

---

### Task 7: Final verification and cleanup

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No new warnings

- [ ] **Step 3: Verify `.bengalmod` content**

Add a verification test if not already covered:

```rust
#[test]
fn bengalmod_contains_object_code_and_generic_bir_only() {
    // Compile lib with both generic and non-generic functions
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "mixedlib",
        r#"
        public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        public func identity<T>(x: T) -> T { return x; }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let loaded = bengal::interface::read_interface(&lib_path).unwrap();

    // Object bytes present
    assert!(!loaded.object_bytes.is_empty(), "should have object code");

    // BIR contains only generic functions
    for (_path, bir) in &loaded.modules {
        for func in &bir.functions {
            assert!(
                !func.type_params.is_empty(),
                "non-generic function '{}' should not be in BIR",
                func.name
            );
        }
    }

    // Interface contains both generic and non-generic
    let root_iface = loaded.interfaces.values().next().unwrap();
    let func_names: Vec<&str> = root_iface.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(func_names.contains(&"add"), "interface should contain non-generic 'add'");
    assert!(func_names.contains(&"identity"), "interface should contain generic 'identity'");
}
```

- [ ] **Step 4: Update TODO.md**

Mark step 3 as done in TODO.md.

- [ ] **Step 5: Commit**

```
test: add library archive verification tests
```
