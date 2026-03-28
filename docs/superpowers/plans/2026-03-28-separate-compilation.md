# Separate Compilation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable compiling a Bengal package using only `.bengalmod` interface files from its dependencies, without needing the dependency source code.

**Architecture:** Package-level separate compilation following Rust's crate model. External deps specified via `--dep name=path.bengalmod` CLI flag. Consumer loads dependency interfaces into GlobalSymbolTable, merges dependency BIR for codegen, and links everything together.

**Tech Stack:** Rust, clap (CLI), rmp-serde (MessagePack serialization), inkwell (LLVM codegen)

**Spec:** `docs/superpowers/specs/2026-03-28-separate-compilation-design.md`

---

### Task 1: Data Types and Helpers

**Files:**
- Modify: `src/pipeline.rs` — add `ExternalDep` struct
- Modify: `src/semantic/mod.rs:49-55` — add `external_dep_names` to `PackageSemanticInfo`
- Modify: `src/semantic/package_analysis.rs` — add `dep_module_path` helper, update `PackageSemanticInfo` construction
- Test: `src/semantic/package_analysis.rs` (inline test)

- [ ] **Step 1: Write test for `dep_module_path`**

Add at the bottom of `src/semantic/package_analysis.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dep_module_path_root() {
        let result = dep_module_path("math", &ModulePath::root());
        assert_eq!(result, ModulePath(vec!["math".to_string()]));
    }

    #[test]
    fn dep_module_path_submodule() {
        let result = dep_module_path("math", &ModulePath(vec!["utils".to_string()]));
        assert_eq!(result, ModulePath(vec!["math".to_string(), "utils".to_string()]));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib dep_module_path`
Expected: FAIL — `dep_module_path` not found

- [ ] **Step 3: Implement data types and helpers**

In `src/pipeline.rs`, add after `BirOutput`:

```rust
use crate::bir::instruction::BirModule;
use crate::interface::ModuleInterface;

/// An external package dependency loaded from a `.bengalmod` file.
pub struct ExternalDep {
    /// Dependency name as specified in --dep (used in import path resolution)
    pub name: String,
    /// Package name from the .bengalmod file (used for symbol mangling)
    pub package_name: String,
    /// Per-module interface data
    pub interfaces: HashMap<ModulePath, ModuleInterface>,
    /// Per-module BIR (for codegen and monomorphization)
    pub bir_modules: HashMap<ModulePath, BirModule>,
}
```

In `src/semantic/mod.rs`, update `PackageSemanticInfo`:

```rust
pub struct PackageSemanticInfo {
    pub module_infos: HashMap<ModulePath, SemanticInfo>,
    pub import_sources: HashMap<(ModulePath, String), ModulePath>,
    /// Maps external dep module paths to their original package names (for name mangling).
    pub external_dep_names: HashMap<ModulePath, String>,
}
```

In `src/semantic/package_analysis.rs`, add `dep_module_path` (make it `pub(crate)`):

```rust
/// Build a module path for an external dependency module.
/// Prefixes the dep name to the internal module path to avoid collisions.
pub(crate) fn dep_module_path(dep_name: &str, internal_path: &ModulePath) -> ModulePath {
    let mut segments = vec![dep_name.to_string()];
    segments.extend(internal_path.0.iter().cloned());
    ModulePath(segments)
}
```

Update `analyze_package` return to include `external_dep_names`:

```rust
Ok(PackageSemanticInfo {
    module_infos,
    import_sources,
    external_dep_names: HashMap::new(),
})
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib dep_module_path`
Expected: PASS

Run: `cargo test --lib` (check nothing is broken)
Expected: PASS

- [ ] **Step 5: Commit**

```
feat(pipeline): add ExternalDep type and dep_module_path helper
```

---

### Task 2: Package-Level `.bengalmod` Emission

**Files:**
- Modify: `src/pipeline.rs` — add `emit_package_bengalmod`
- Modify: `src/lib.rs:21-34` — call `emit_package_bengalmod` in `compile_to_executable`
- Test: `src/pipeline.rs` (inline test)

- [ ] **Step 1: Write test for `emit_package_bengalmod`**

Add to `src/pipeline.rs` tests module:

```rust
#[test]
fn emit_package_bengalmod_creates_file() {
    let parsed = parse_source("testlib", "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap();
    let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
    let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    emit_package_bengalmod(&lowered, dir.path());

    let file_path = dir.path().join("testlib.bengalmod");
    assert!(file_path.exists(), "package .bengalmod should be created");

    let loaded = crate::interface::read_interface(&file_path).unwrap();
    assert_eq!(loaded.package_name, "testlib");
    assert!(!loaded.interfaces.is_empty());
    assert!(!loaded.modules.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib emit_package_bengalmod`
Expected: FAIL — `emit_package_bengalmod` not found

- [ ] **Step 3: Implement `emit_package_bengalmod`**

Add to `src/pipeline.rs` after `emit_interfaces`:

```rust
/// Emit a single `.bengalmod` containing all modules of the package.
/// This is consumed by `--dep` in other packages.
pub fn emit_package_bengalmod(lowered: &LoweredPackage, cache_dir: &std::path::Path) {
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("warning: failed to create cache directory: {}", e);
        return;
    }

    let mut all_modules = HashMap::new();
    let mut all_interfaces = HashMap::new();

    for (module_path, module) in &lowered.modules {
        let sem_info = match lowered.pkg_sem_info.module_infos.get(module_path) {
            Some(info) => info,
            None => continue,
        };
        let iface = crate::interface::ModuleInterface::from_semantic_info(sem_info);
        all_modules.insert(module_path.clone(), module.bir.clone());
        all_interfaces.insert(module_path.clone(), iface);
    }

    let mod_file = crate::interface::BengalModFile {
        package_name: lowered.package_name.clone(),
        modules: all_modules,
        interfaces: all_interfaces,
    };

    let file_path = cache_dir.join(format!("{}.bengalmod", lowered.package_name));
    if let Err(e) = crate::interface::write_bengalmod_file(&mod_file, &file_path) {
        eprintln!("warning: failed to write package interface: {}", e);
    }
}
```

Update `compile_to_executable` in `src/lib.rs` to call both:

```rust
pipeline::emit_interfaces(&lowered, std::path::Path::new(".build/cache"));
pipeline::emit_package_bengalmod(&lowered, std::path::Path::new(".build/cache"));
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib emit_package_bengalmod`
Expected: PASS

- [ ] **Step 5: Commit**

```
feat(pipeline): add emit_package_bengalmod for package-level .bengalmod
```

---

### Task 3: Load External Dependencies

**Files:**
- Modify: `src/pipeline.rs` — add `load_external_dep`
- Test: `src/pipeline.rs` (inline test)

- [ ] **Step 1: Write test for `load_external_dep`**

```rust
#[test]
fn load_external_dep_round_trip() {
    // Build a library
    let parsed = parse_source("mathlib", "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap();
    let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
    let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    emit_package_bengalmod(&lowered, dir.path());

    let dep = load_external_dep("math", &dir.path().join("mathlib.bengalmod")).unwrap();
    assert_eq!(dep.name, "math");
    assert_eq!(dep.package_name, "mathlib");
    assert!(!dep.interfaces.is_empty());
    assert!(!dep.bir_modules.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib load_external_dep`
Expected: FAIL — `load_external_dep` not found

- [ ] **Step 3: Implement `load_external_dep`**

Add to `src/pipeline.rs`:

```rust
/// Load an external dependency from a `.bengalmod` file.
pub fn load_external_dep(
    name: &str,
    path: &Path,
) -> Result<ExternalDep, crate::error::PipelineError> {
    let mod_file = crate::interface::read_interface(path).map_err(|e| {
        crate::error::PipelineError::package("load_dep", e)
    })?;
    Ok(ExternalDep {
        name: name.to_string(),
        package_name: mod_file.package_name,
        interfaces: mod_file.interfaces,
        bir_modules: mod_file.modules,
    })
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib load_external_dep`
Expected: PASS

- [ ] **Step 5: Commit**

```
feat(pipeline): add load_external_dep for reading .bengalmod files
```

---

### Task 4: Analyze Stage — GlobalSymbolTable Injection + Import Resolution Fallback

This task combines symbol injection and import fallback because they are mutually dependent — the test requires both to pass.

**Files:**
- Modify: `src/semantic/package_analysis.rs:44-98` — add `external_deps` param to `analyze_package`, inject into GlobalSymbolTable
- Modify: `src/semantic/package_analysis.rs:378-407` — update `resolve_import_module_path` to fallback to `global_symbols`
- Modify: `src/semantic/package_analysis.rs:275-331` — pass `global_symbols` through `resolve_imports_for_module`
- Modify: `src/semantic/mod.rs:19-21` — re-export `dep_module_path`
- Modify: `src/pipeline.rs:138-191` — update `analyze` call site to pass `&[]`
- Test: `src/semantic/package_analysis.rs` (inline tests)

- [ ] **Step 1: Write test for injection + import resolution**

Add to `src/semantic/package_analysis.rs` tests:

```rust
#[test]
fn analyze_package_injects_external_dep_symbols() {
    use crate::interface::ModuleInterface;
    use crate::pipeline::ExternalDep;

    // Build a minimal external interface
    let iface = ModuleInterface {
        functions: vec![crate::interface::InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "ext_add".to_string(),
            sig: crate::interface::InterfaceFuncSig {
                type_params: vec![],
                params: vec![
                    ("a".to_string(), crate::interface::InterfaceType::I32),
                    ("b".to_string(), crate::interface::InterfaceType::I32),
                ],
                return_type: crate::interface::InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };

    let dep = ExternalDep {
        name: "extlib".to_string(),
        package_name: "extlib".to_string(),
        interfaces: HashMap::from([(ModulePath::root(), iface)]),
        bir_modules: HashMap::new(),
    };

    // Build a consumer that imports from the external dep
    let graph = crate::package::ModuleGraph::from_source(
        "app",
        "import extlib::ext_add;\nfunc main() -> Int32 { return ext_add(1, 2); }",
    ).unwrap();

    let mut diag = crate::error::DiagCtxt::new();
    let result = analyze_package(&graph, "app", &[dep], &mut diag);
    assert!(result.is_ok(), "analyze_package should succeed with external dep: {:?}", result.err());

    let pkg_info = result.unwrap();
    // Verify import_sources maps the imported function to the external module path
    let source = pkg_info.import_sources.get(&(ModulePath::root(), "ext_add".to_string()));
    assert!(source.is_some(), "ext_add should be in import_sources");
    assert_eq!(source.unwrap(), &ModulePath(vec!["extlib".to_string()]));

    // Verify external_dep_names
    let dep_name = pkg_info.external_dep_names.get(&ModulePath(vec!["extlib".to_string()]));
    assert_eq!(dep_name, Some(&"extlib".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib analyze_package_injects_external`
Expected: FAIL — signature mismatch

- [ ] **Step 3: Update `analyze_package` + `resolve_import_module_path` together**

In `src/semantic/package_analysis.rs`, change `analyze_package`:

```rust
pub fn analyze_package(
    graph: &ModuleGraph,
    _package_name: &str,
    external_deps: &[crate::pipeline::ExternalDep],
    diag: &mut DiagCtxt,
) -> Result<PackageSemanticInfo> {
    // Phase 1: Collect all top-level symbols
    let mut global_symbols = collect_global_symbols(graph)?;

    // Inject external dep symbols into GlobalSymbolTable
    let mut external_dep_names: HashMap<ModulePath, String> = HashMap::new();
    for dep in external_deps {
        for (mod_path, iface) in &dep.interfaces {
            let ext_path = dep_module_path(&dep.name, mod_path);
            let symbols = interface_to_global_symbols(iface, &ext_path);
            global_symbols.insert(ext_path.clone(), symbols);
            external_dep_names.insert(ext_path, dep.package_name.clone());
        }
    }

    // Phase 2 + 3: unchanged ...

    Ok(PackageSemanticInfo {
        module_infos,
        import_sources,
        external_dep_names,
    })
}
```

Update `resolve_import_module_path` to accept `global_symbols` and fallback:

```rust
fn resolve_import_module_path(
    current_module: &ModulePath,
    prefix: &PathPrefix,
    path_segments: &[String],
    graph: &ModuleGraph,
    global_symbols: &GlobalSymbolTable,
) -> Result<ModulePath> {
    // ... existing base + segment logic ...

    // Local module — preferred
    if graph.modules.contains_key(&result) {
        return Ok(result);
    }
    // External dependency — fallback
    if global_symbols.contains_key(&result) {
        return Ok(result);
    }
    Err(pkg_err(format!(
        "unresolved import: module '{}' not found",
        result
    )))
}
```

Update the call site in `resolve_imports_for_module`:

```rust
let target_module = resolve_import_module_path(
    current_module, &import.prefix, &import.path, graph, global_symbols,
)?;
```

In `src/semantic/mod.rs`, add re-export:

```rust
pub use package_analysis::{
    GlobalSymbol, GlobalSymbolTable, SymbolKind, analyze_package, dep_module_path,
    interface_to_global_symbols,
};
```

Update all call sites of `analyze_package` to pass `&[]`:
- `src/pipeline.rs:182`: `crate::semantic::analyze_package(&parsed.graph, &parsed.package_name, &[], diag)`

- [ ] **Step 4: Run tests**

Run: `cargo test --lib analyze_package_injects_external`
Expected: PASS

Run: `cargo test --lib` (regression check)
Expected: PASS

- [ ] **Step 5: Commit**

```
feat(semantic): inject external deps into GlobalSymbolTable with import fallback
```

---

### Task 5: Name Mangling for External Dependencies

**Files:**
- Modify: `src/pipeline_helpers.rs:9-96` — add `resolve_mangle_context`, update `build_name_map`
- Test: `src/pipeline_helpers.rs` (inline test)

- [ ] **Step 1: Write test for external dep name mangling**

Add a test module at the bottom of `src/pipeline_helpers.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::ModulePath;

    #[test]
    fn resolve_mangle_context_local() {
        let external_dep_names = HashMap::new();
        let source = ModulePath(vec!["utils".to_string()]);
        let (pkg, segs) = resolve_mangle_context(&source, "myapp", &external_dep_names);
        assert_eq!(pkg, "myapp");
        assert_eq!(segs, &["utils".to_string()]);
    }

    #[test]
    fn resolve_mangle_context_external() {
        let mut external_dep_names = HashMap::new();
        external_dep_names.insert(
            ModulePath(vec!["math".to_string()]),
            "mathlib".to_string(),
        );
        let source = ModulePath(vec!["math".to_string()]);
        let (pkg, segs) = resolve_mangle_context(&source, "myapp", &external_dep_names);
        assert_eq!(pkg, "mathlib");
        assert!(segs.is_empty()); // "math" prefix stripped, root module has no segments
    }

    #[test]
    fn resolve_mangle_context_external_submodule() {
        let mut external_dep_names = HashMap::new();
        external_dep_names.insert(
            ModulePath(vec!["math".to_string(), "advanced".to_string()]),
            "mathlib".to_string(),
        );
        let source = ModulePath(vec!["math".to_string(), "advanced".to_string()]);
        let (pkg, segs) = resolve_mangle_context(&source, "myapp", &external_dep_names);
        assert_eq!(pkg, "mathlib");
        assert_eq!(segs, &["advanced".to_string()]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib resolve_mangle_context`
Expected: FAIL — function not found

- [ ] **Step 3: Implement `resolve_mangle_context`**

Add to `src/pipeline_helpers.rs` before `build_name_map`:

```rust
/// Determine the package name and module segments for name mangling.
/// For external deps, uses the dep's package_name and strips the dep_name prefix.
/// For local imports, uses the consumer's package_name.
fn resolve_mangle_context<'a>(
    source_module: &'a ModulePath,
    package_name: &'a str,
    external_dep_names: &'a HashMap<ModulePath, String>,
) -> (&'a str, &'a [String]) {
    if let Some(dep_pkg_name) = external_dep_names.get(source_module) {
        // Strip the dep_name prefix (first segment) added by dep_module_path().
        // Invariant: dep_module_path always prepends exactly one segment.
        (dep_pkg_name.as_str(), &source_module.0[1..])
    } else {
        (package_name, source_module.0.as_slice())
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib resolve_mangle_context`
Expected: PASS

- [ ] **Step 5: Update `build_name_map` for imported functions**

In `src/pipeline_helpers.rs`, update the "Imported functions" section (lines 78-93):

```rust
// Imported functions
for ((imp_mod, imp_name), source_module) in &pkg_sem_info.import_sources {
    if imp_mod != mod_path {
        continue;
    }
    if sem_info.struct_defs.contains_key(imp_name) {
        continue;
    }
    let (pkg_name, source_segments) =
        resolve_mangle_context(source_module, package_name, &pkg_sem_info.external_dep_names);
    let source_segs: Vec<&str> = if source_segments.is_empty() {
        vec![""]
    } else {
        source_segments.iter().map(|s| s.as_str()).collect()
    };
    let mangled = crate::mangle::mangle_function(pkg_name, &source_segs, imp_name, &[]);
    name_map.insert(imp_name.clone(), mangled);
}
```

- [ ] **Step 6: Update `build_name_map` for imported struct methods**

Update the "Local and imported methods" section (lines 36-76):

```rust
// Local and imported methods
for (struct_name, struct_info) in &sem_info.struct_defs {
    let is_imported = pkg_sem_info
        .import_sources
        .contains_key(&(mod_path.clone(), struct_name.clone()));
    if is_imported {
        if let Some(source_module) = pkg_sem_info
            .import_sources
            .get(&(mod_path.clone(), struct_name.clone()))
        {
            let (pkg_name, source_segments) = resolve_mangle_context(
                source_module,
                package_name,
                &pkg_sem_info.external_dep_names,
            );
            let source_segs: Vec<&str> = if source_segments.is_empty() {
                vec![""]
            } else {
                source_segments.iter().map(|s| s.as_str()).collect()
            };
            // Note: struct initializers use StructInit BIR instruction (not function calls),
            // so they don't need name_map entries. Only methods need mangling.
            for method in &struct_info.methods {
                let local_mangled = format!("{}_{}", struct_name, method.name);
                let mangled = crate::mangle::mangle_method(
                    pkg_name,
                    &source_segs,
                    struct_name,
                    &method.name,
                    &[],
                );
                name_map.insert(local_mangled, mangled);
            }
        }
    } else {
        for method in &struct_info.methods {
            let local_mangled = format!("{}_{}", struct_name, method.name);
            let mangled = crate::mangle::mangle_method(
                package_name,
                &module_segments,
                struct_name,
                &method.name,
                &[],
            );
            name_map.insert(local_mangled, mangled);
        }
    }
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test --lib`
Expected: PASS

Run: `cargo test` (full test suite including integration tests)
Expected: PASS

- [ ] **Step 8: Commit**

```
feat(pipeline): add external dep name mangling in build_name_map
```

---

### Task 6: BIR Merge and Pipeline Wiring

**Files:**
- Modify: `src/pipeline.rs` — add `merge_external_deps`
- Modify: `src/lib.rs` — wire external deps into `compile_to_executable`
- Test: `src/pipeline.rs` (inline test)

- [ ] **Step 1: Write test for `merge_external_deps`**

```rust
#[test]
fn merge_external_deps_adds_modules() {
    let parsed = parse_source("app", "func main() -> Int32 { return 1; }").unwrap();
    let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
    let mut lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();

    assert_eq!(lowered.modules.len(), 1); // just root

    let lib_parsed = parse_source("mathlib", "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap();
    let lib_analyzed = analyze(lib_parsed, &mut DiagCtxt::new()).unwrap();
    let lib_lowered = lower(lib_analyzed, &mut DiagCtxt::new()).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    emit_package_bengalmod(&lib_lowered, dir.path());
    let dep = load_external_dep("math", &dir.path().join("mathlib.bengalmod")).unwrap();

    merge_external_deps(&mut lowered, &[dep]);

    assert!(lowered.modules.len() > 1, "external BIR should be merged");
    let ext_path = crate::semantic::dep_module_path("math", &ModulePath::root());
    let ext_module = lowered.modules.get(&ext_path);
    assert!(ext_module.is_some(), "external module should exist at dep_module_path");
    assert!(!ext_module.unwrap().is_entry, "external module should not be entry");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib merge_external_deps`
Expected: FAIL — function not found

- [ ] **Step 3: Implement `merge_external_deps`**

Add to `src/pipeline.rs`:

```rust
/// Merge external dependency BIR modules into the lowered package.
/// Must be called AFTER emit_interfaces/emit_package_bengalmod
/// and BEFORE optimize.
pub fn merge_external_deps(lowered: &mut LoweredPackage, external_deps: &[ExternalDep]) {
    for dep in external_deps {
        for (mod_path, bir_module) in &dep.bir_modules {
            let ext_path = crate::semantic::dep_module_path(&dep.name, mod_path);
            lowered.modules.insert(
                ext_path,
                LoweredModule {
                    bir: bir_module.clone(),
                    is_entry: false,
                },
            );
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib merge_external_deps`
Expected: PASS

- [ ] **Step 5: Update `compile_to_executable` to accept external deps**

In `src/lib.rs`, update signature:

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
    pipeline::emit_package_bengalmod(&lowered, std::path::Path::new(".build/cache"));
    let mut lowered = lowered;
    pipeline::merge_external_deps(&mut lowered, external_deps);
    let optimized = pipeline::optimize(lowered);
    let mono = pipeline::monomorphize(optimized, &mut diag)?;
    let compiled = pipeline::codegen(mono, &mut diag)?;
    pipeline::link(compiled, output_path)
}
```

Add `analyze_with_deps` to `src/pipeline.rs`. The existing `analyze()` delegates to it:

```rust
/// Semantic analysis with external dependencies.
pub fn analyze_with_deps(
    parsed: ParsedPackage,
    external_deps: &[ExternalDep],
    diag: &mut DiagCtxt,
) -> Result<AnalyzedPackage, crate::error::PipelineError> {
    // Validate generics for all modules
    for mod_info in parsed.graph.modules.values() {
        if let Err(e) = crate::semantic::validate_generics(&mod_info.ast) {
            diag.emit(e);
            continue;
        }
    }

    // Run pre-mono type inference per module
    let mut inferred_maps: HashMap<ModulePath, HashMap<NodeId, Vec<TypeAnnotation>>> =
        HashMap::new();
    for (mod_path, mod_info) in &parsed.graph.modules {
        let (inferred, _pre_mono_sem_info) =
            match crate::semantic::analyze_pre_mono_lenient(&mod_info.ast) {
                Ok(result) => result,
                Err(e) => {
                    diag.emit(e);
                    continue;
                }
            };
        let inferred_map: HashMap<NodeId, Vec<TypeAnnotation>> = inferred
            .map
            .into_iter()
            .map(|(id, site)| (id, site.type_args))
            .collect();
        inferred_maps.insert(mod_path.clone(), inferred_map);
    }

    if diag.has_errors() {
        return Err(crate::error::PipelineError::package(
            "analyze",
            BengalError::PackageError {
                message: "analysis failed due to module errors".to_string(),
            },
        ));
    }

    // Cross-module semantic analysis — passes external_deps
    let pkg_sem_info = crate::semantic::analyze_package(
        &parsed.graph, &parsed.package_name, external_deps, diag,
    ).map_err(|e| crate::error::PipelineError::package("analyze", e))?;

    Ok(AnalyzedPackage {
        package_name: parsed.package_name,
        graph: parsed.graph,
        inferred_maps,
        pkg_sem_info,
    })
}

/// Semantic analysis (no external deps). Delegates to `analyze_with_deps`.
pub fn analyze(
    parsed: ParsedPackage,
    diag: &mut DiagCtxt,
) -> Result<AnalyzedPackage, crate::error::PipelineError> {
    analyze_with_deps(parsed, &[], diag)
}
```

Update all existing callers of `compile_to_executable` to pass `&[]`:
- `src/main.rs`: the CLI handler — `bengal::compile_to_executable(&entry_path, &exe_path, &[])`
  (Note: this will be further updated in Task 8 to pass actual deps)
- `tests/common/mod.rs:91`: `bengal::compile_to_executable(&entry_path, &exe_path, &[])`
- `tests/common/mod.rs:118`: `bengal::compile_to_executable(&entry_path, &exe_path, &[])`

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: PASS (all existing tests still work with `&[]`)

- [ ] **Step 7: Commit**

```
feat(pipeline): add merge_external_deps and wire into compilation
```

---

### Task 7: CLI `--dep` Flag

**Files:**
- Modify: `src/main.rs` — add `deps` to Compile subcommand, wire loading

- [ ] **Step 1: Add `--dep` flag to CLI**

In `src/main.rs`, update the `Compile` variant:

```rust
Compile {
    file: PathBuf,
    #[arg(long)]
    emit_bir: bool,
    /// External dependency: --dep name=path.bengalmod
    #[arg(long = "dep", value_parser = parse_dep)]
    deps: Vec<(String, PathBuf)>,
},
```

Add the parser function:

```rust
fn parse_dep(s: &str) -> std::result::Result<(String, PathBuf), String> {
    let (name, path) = s
        .split_once('=')
        .ok_or_else(|| format!("invalid dep format '{}', expected name=path.bengalmod", s))?;
    if name.is_empty() {
        return Err("dep name cannot be empty".to_string());
    }
    Ok((name.to_string(), PathBuf::from(path)))
}
```

- [ ] **Step 2: Wire deps into compilation pipeline**

In the `Command::Compile` handler, after parsing:

```rust
// Load external dependencies
let mut external_deps = Vec::new();
let mut seen_dep_names = std::collections::HashSet::new();
for (name, path) in &deps {
    if !seen_dep_names.insert(name.clone()) {
        return Err(miette::miette!("--dep '{}' specified multiple times", name));
    }
    let dep = bengal::pipeline::load_external_dep(name, path)
        .map_err(|e| Report::new(e.into_diagnostic()))?;
    external_deps.push(dep);
}
```

Then update the inline pipeline in the Compile handler. Replace `bengal::pipeline::analyze(parsed, &mut diag)` with `bengal::pipeline::analyze_with_deps(parsed, &external_deps, &mut diag)`, and add the merge + emit steps after lower:

```rust
let lowered = lowered.map_err(|e| Report::new(e.into_diagnostic()))?;

// Emit local package .bengalmod (before merging external deps)
bengal::pipeline::emit_interfaces(&lowered, std::path::Path::new(".build/cache"));
bengal::pipeline::emit_package_bengalmod(&lowered, std::path::Path::new(".build/cache"));

// Merge external dep BIR into lowered package
let mut lowered = lowered;
bengal::pipeline::merge_external_deps(&mut lowered, &external_deps);

let optimized = bengal::pipeline::optimize(lowered);
```

- [ ] **Step 3: Run build check**

Run: `cargo build`
Expected: PASS (compiles cleanly)

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```
feat(cli): add --dep flag for external package dependencies
```

---

### Task 8: Integration Test — Basic Function Call

**Files:**
- Create: `tests/separate_compilation.rs`

- [ ] **Step 1: Write test helper and basic function test**

Create `tests/separate_compilation.rs`:

```rust
mod common;

use std::path::Path;

/// Compile a library source into a .bengalmod file, return path.
fn compile_lib(name: &str, source: &str, dir: &Path) -> std::path::PathBuf {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(name, source).unwrap();
    let analyzed = bengal::pipeline::analyze(parsed, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    bengal::pipeline::emit_package_bengalmod(&lowered, dir);
    dir.join(format!("{}.bengalmod", name))
}

/// Compile app with external deps, link, run, return exit code.
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

    // Link all object files
    let link_dir = tempfile::TempDir::new().unwrap();
    let exe_path = link_dir.path().join("test_exe");
    bengal::pipeline::link(compiled, &exe_path).unwrap();

    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run compiled binary");
    output.status.code().unwrap_or(-1)
}

#[test]
fn separate_compilation_basic_function_call() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "mathlib",
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import math::add;
        func main() -> Int32 {
            return add(1, 2);
        }
        "#,
        &[("math", &lib_path)],
    );
    assert_eq!(result, 3);
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --test separate_compilation separate_compilation_basic_function_call`
Expected: PASS

- [ ] **Step 3: Commit**

```
test: add separate compilation basic function call test
```

---

### Task 9: Integration Test — Struct Usage

**Files:**
- Modify: `tests/separate_compilation.rs`

- [ ] **Step 1: Write struct test**

```rust
#[test]
fn separate_compilation_struct_usage() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "geomlib",
        r#"
        public struct Point {
            var x: Int32;
            var y: Int32;
            func sum() -> Int32 {
                return self.x + self.y;
            }
        }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import geom::Point;
        func main() -> Int32 {
            let p = Point(x: 10, y: 20);
            return p.sum();
        }
        "#,
        &[("geom", &lib_path)],
    );
    assert_eq!(result, 30);
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --test separate_compilation separate_compilation_struct_usage`
Expected: PASS

- [ ] **Step 3: Commit**

```
test: add separate compilation struct usage test
```

---

### Task 10: Integration Test — Generic Function

**Files:**
- Modify: `tests/separate_compilation.rs`

- [ ] **Step 1: Write generic function test**

```rust
#[test]
fn separate_compilation_generic_function() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "utillib",
        r#"
        public func identity<T>(x: T) -> T { return x; }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import util::identity;
        func main() -> Int32 {
            return identity<Int32>(42);
        }
        "#,
        &[("util", &lib_path)],
    );
    assert_eq!(result, 42);
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --test separate_compilation separate_compilation_generic_function`
Expected: PASS

- [ ] **Step 3: Commit**

```
test: add separate compilation generic function test
```

---

### Task 11: Integration Test — Protocol

**Files:**
- Modify: `tests/separate_compilation.rs`

- [ ] **Step 1: Write protocol test**

```rust
#[test]
fn separate_compilation_protocol() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "protolib",
        r#"
        public protocol Summable {
            func sum() -> Int32;
        }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import proto::Summable;
        public struct Pair: Summable {
            var a: Int32;
            var b: Int32;
            func sum() -> Int32 {
                return self.a + self.b;
            }
        }
        func main() -> Int32 {
            let p = Pair(a: 7, b: 8);
            return p.sum();
        }
        "#,
        &[("proto", &lib_path)],
    );
    assert_eq!(result, 15);
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --test separate_compilation separate_compilation_protocol`
Expected: PASS

- [ ] **Step 3: Commit**

```
test: add separate compilation protocol test
```

---

### Task 12: Integration Test — Multiple Dependencies

**Files:**
- Modify: `tests/separate_compilation.rs`

- [ ] **Step 1: Write multiple deps test**

```rust
#[test]
fn separate_compilation_multiple_deps() {
    let dir = tempfile::TempDir::new().unwrap();

    let math_path = compile_lib(
        "mathlib",
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
        dir.path(),
    );
    let str_path = compile_lib(
        "strlib",
        "public func double(x: Int32) -> Int32 { return x + x; }",
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import math::add;
        import str::double;
        func main() -> Int32 {
            return add(double(5), 3);
        }
        "#,
        &[("math", &math_path), ("str", &str_path)],
    );
    assert_eq!(result, 13);
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --test separate_compilation separate_compilation_multiple_deps`
Expected: PASS

- [ ] **Step 3: Commit**

```
test: add separate compilation multiple deps test
```

---

### Task 13: Integration Test — Error Cases

**Files:**
- Modify: `tests/separate_compilation.rs`

- [ ] **Step 1: Write error case tests**

```rust
#[test]
fn separate_compilation_visibility_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "privlib",
        r#"
        public func pub_fn() -> Int32 { return 1; }
        func internal_fn() -> Int32 { return 2; }
        "#,
        dir.path(),
    );

    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(
        "app",
        r#"
        import priv::internal_fn;
        func main() -> Int32 { return internal_fn(); }
        "#,
    ).unwrap();

    let dep = bengal::pipeline::load_external_dep("priv", &lib_path).unwrap();
    let result = bengal::pipeline::analyze_with_deps(
        parsed, &[dep], &mut diag,
    );
    assert!(result.is_err(), "should fail: internal function not accessible");
}

#[test]
fn separate_compilation_missing_file_error() {
    let result = bengal::pipeline::load_external_dep(
        "nonexistent",
        std::path::Path::new("/tmp/nonexistent.bengalmod"),
    );
    assert!(result.is_err(), "should fail: file not found");
}

#[test]
fn separate_compilation_version_mismatch_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let bad_file = dir.path().join("bad.bengalmod");
    // Write a file with correct magic but wrong version
    let mut data = Vec::new();
    data.extend_from_slice(b"BGMD");
    data.extend_from_slice(&999u32.to_le_bytes());
    data.extend_from_slice(&[0; 10]); // garbage payload
    std::fs::write(&bad_file, &data).unwrap();

    let result = bengal::pipeline::load_external_dep("bad", &bad_file);
    assert!(result.is_err(), "should fail: version mismatch");
    let err_msg = format!("{}", result.unwrap_err().source_error);
    assert!(
        err_msg.contains("incompatible") || err_msg.contains("version"),
        "error should mention version: {}",
        err_msg
    );
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test separate_compilation`
Expected: PASS (all tests in the file)

- [ ] **Step 3: Commit**

```
test: add separate compilation error case tests
```

---

### Task 14: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: No warnings

- [ ] **Step 3: Final commit if any fixups needed**

Only if clippy or test fixes were required.
