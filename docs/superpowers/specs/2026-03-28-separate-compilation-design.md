# Separate Compilation Design

## Overview

Enable compiling a Bengal package using only `.bengalmod` interface files from its dependencies, without needing the dependency source code. This is the Rust-style approach: the unit of separate compilation is the **package** (analogous to Rust's crate), not individual modules.

## Scope

**In scope:**
- `--dep name=path.bengalmod` CLI flag for specifying external package dependencies
- Package-level `.bengalmod` emission (all modules in one file)
- Loading `.bengalmod` files and injecting symbols into `GlobalSymbolTable`
- `import` fallback resolution: local modules first, then external dependencies
- Merging dependency BIR into consumer's compilation for codegen
- Consumer-side monomorphization of dependency generics (Rust model)
- Format version check on `.bengalmod` load
- Integration tests covering functions, structs, generics, protocols, and error cases

**Out of scope (future):**
- `.a` static library archives for pre-compiled object code reuse (TODO step 3)
- Sysroot / library search paths (TODO step 4)
- Source hash-based cache invalidation
- Parallel builds
- `Package.toml` manifest file
- Re-exports of dependency symbols (a package's `.bengalmod` contains only its own symbols and BIR, not those of its dependencies)
- Diamond dependencies (A depends on B and C, both depend on D) — will be addressed with `.a` archives where the linker handles duplicate symbols

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Compilation unit | Package (not module) | Matches Rust's crate model; `analyze_package` stays unchanged for intra-package modules |
| Dependency specification | CLI flag `--dep name=path` | Simplest approach; Rust uses `--extern`; build system can compose flags later |
| Dependency codegen | Re-codegen from BIR each time | `.bengalmod` is self-contained; optimized later with `.a` archives (TODO step 3) |
| Generic monomorphization | Consumer-side, BIR merge | Matches Rust's downstream monomorphization model; reuses existing `monomorphize` stage |
| Import resolution | Fallback (local then external) | No new syntax needed; Rust Edition 2018 uses the same implicit model |
| Cache validation | Format version check only | No build system yet; user is responsible for rebuilding deps |
| Cyclic package dependencies | Structurally impossible | A package must be compiled before its `.bengalmod` can be consumed, so cycles cannot form |

**`name` vs `package_name` distinction:** `ExternalDep.name` is the CLI-specified dep name used for import path resolution (what the consumer writes in `import` statements). `ExternalDep.package_name` is the original package name from the `.bengalmod` file, used for symbol mangling (must match the dependency's own mangling). These may differ: `--dep mymath=libmath.bengalmod` means imports use `mymath` but mangled names use `libmath`'s package name.

## Data Types

### `ExternalDep`

```rust
// src/pipeline.rs
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

### CLI Flag

```rust
// src/main.rs — added to Compile subcommand
/// External dependency: --dep name=path.bengalmod
#[arg(long = "dep", value_parser = parse_dep)]
deps: Vec<(String, PathBuf)>,
```

Parser function splits on `=` to extract `(name, path)`.

## Pipeline Changes

### Overall Flow

```
parse
  → load_external_deps (read .bengalmod files → Vec<ExternalDep>)
  → analyze (inject external interfaces into GlobalSymbolTable)
  → lower (name_map uses dep package names for mangling)
  → emit_interfaces (unchanged — per-module .bengalmod for local package)
  → emit_package_bengalmod (new — single .bengalmod for entire package)
  → merge_external_bir (merge dep BIR into LoweredPackage)
  → optimize
  → monomorphize
  → codegen
  → link
```

**Critical ordering:** `emit_interfaces` and `emit_package_bengalmod` MUST run BEFORE `merge_external_bir`. The local package's `.bengalmod` must contain only its own modules, not merged dependency BIR.

### 1. Package-Level `.bengalmod` Emission

Add `emit_package_bengalmod()` alongside the existing `emit_interfaces()`. Both coexist:

- **`emit_interfaces()`** (existing): Per-module `.bengalmod` files in `.build/cache/`. Retained for future incremental compilation.
- **`emit_package_bengalmod()`** (new): Single `.bengalmod` for the entire package. This is what `--dep` consumes.

`compile_to_executable` calls both.

```rust
pub fn emit_package_bengalmod(lowered: &LoweredPackage, cache_dir: &Path) {
    let mut all_modules = HashMap::new();
    let mut all_interfaces = HashMap::new();

    for (module_path, module) in &lowered.modules {
        let sem_info = match lowered.pkg_sem_info.module_infos.get(module_path) {
            Some(info) => info,
            None => continue,
        };
        let iface = ModuleInterface::from_semantic_info(sem_info);
        all_modules.insert(module_path.clone(), module.bir.clone());
        all_interfaces.insert(module_path.clone(), iface);
    }

    let mod_file = BengalModFile {
        package_name: lowered.package_name.clone(),
        modules: all_modules,
        interfaces: all_interfaces,
    };

    let file_path = cache_dir.join(format!("{}.bengalmod", lowered.package_name));
    // write with error handling (non-fatal, same as emit_interfaces)
}
```

Output: `.build/cache/<package_name>.bengalmod`

### 2. Loading External Dependencies

```rust
pub fn load_external_dep(name: &str, path: &Path) -> Result<ExternalDep, PipelineError> {
    let mod_file = read_interface(path)?;
    // Format version check (already in read_interface)
    Ok(ExternalDep {
        name: name.to_string(),
        package_name: mod_file.package_name,
        interfaces: mod_file.interfaces,
        bir_modules: mod_file.modules,
    })
}
```

### 3. Analyze Stage — GlobalSymbolTable Injection

**New signature for `analyze_package`:**

```rust
pub fn analyze_package(
    graph: &ModuleGraph,
    _package_name: &str,
    external_deps: &[ExternalDep],  // NEW
    diag: &mut DiagCtxt,
) -> Result<PackageSemanticInfo>
```

When no external deps are present (backward compat), pass `&[]`.

**Phase 1 addition — after `collect_global_symbols`:**

External dependency module paths are prefixed with the dep name to avoid collisions with local modules:

```rust
// Defined in src/semantic/package_analysis.rs (used by both analyze and merge stages)
fn dep_module_path(dep_name: &str, internal_path: &ModulePath) -> ModulePath {
    let mut segments = vec![dep_name.to_string()];
    segments.extend(internal_path.0.iter().cloned());
    ModulePath(segments)
}
```

Mapping examples:
- dep name `"math"`, internal `ModulePath::root()` → `ModulePath(["math"])`
- dep name `"math"`, internal `ModulePath(["utils"])` → `ModulePath(["math", "utils"])`

```rust
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
```

The existing `resolve_imports_for_module` already receives `&global_symbols`, so injected external symbols participate in import resolution automatically.

**Phase 2 change — import resolution fallback:**

**New signature for `resolve_import_module_path`:**

```rust
fn resolve_import_module_path(
    current_module: &ModulePath,
    prefix: &PathPrefix,
    path_segments: &[String],
    graph: &ModuleGraph,
    global_symbols: &GlobalSymbolTable,  // NEW
) -> Result<ModulePath>
```

After building the target `ModulePath`:

```rust
// Local module — preferred
if graph.modules.contains_key(&result) {
    return Ok(result);
}
// External dependency — fallback
if global_symbols.contains_key(&result) {
    return Ok(result);
}
Err(pkg_err("unresolved import: module '...' not found"))
```

**Import path alignment walkthrough:**

For `import math::add` (single symbol):
- Parser: `PathPrefix::Named("math")`, path = `[]`, tail = `Single("add")`
- `resolve_import_module_path` builds `ModulePath(["math"])`
- External dep registered at `dep_module_path("math", root)` = `ModulePath(["math"])` ✓

For `import math::utils::foo` (sub-module):
- Parser: `PathPrefix::Named("math")`, path = `["utils"]`, tail = `Single("foo")`
- `resolve_import_module_path` builds `ModulePath(["math", "utils"])`
- External dep registered at `dep_module_path("math", ["utils"])` = `ModulePath(["math", "utils"])` ✓

**`PackageSemanticInfo` extension:**

```rust
pub struct PackageSemanticInfo {
    pub module_infos: HashMap<ModulePath, SemanticInfo>,
    pub import_sources: HashMap<(ModulePath, String), ModulePath>,
    /// Maps external dep module paths to their package names (for name mangling)
    pub external_dep_names: HashMap<ModulePath, String>,
}
```

Construction in `analyze_package` updated:
```rust
Ok(PackageSemanticInfo {
    module_infos,
    import_sources,
    external_dep_names,  // populated during Phase 1 above
})
```

### 4. Lower Stage — Name Mangling

`build_name_map` uses `external_dep_names` to determine whether an imported symbol comes from an external package. Uses `mangle_function` for functions, `mangle_method` for methods, and `mangle_initializer` for struct initializers (note: `mangle_initializer` produces `_BGI...` tags, distinct from `mangle_method`'s `_BGM...` tags). Helper to resolve package name and source segments:

```rust
/// Returns (package_name, source_module_segments) for mangling.
/// For external deps, uses the dep's package_name and strips the dep_name prefix.
/// For local imports, uses the consumer's package_name.
fn resolve_mangle_context<'a>(
    source_module: &'a ModulePath,
    package_name: &'a str,
    external_dep_names: &'a HashMap<ModulePath, String>,
) -> (&'a str, &'a [String]) {
    if let Some(dep_pkg_name) = external_dep_names.get(source_module) {
        (dep_pkg_name.as_str(), &source_module.0[1..])
    } else {
        (package_name, source_module.0.as_slice())
    }
}
```

**Imported functions** (existing section in `build_name_map`, lines 78-93):

```rust
for ((imp_mod, imp_name), source_module) in &pkg_sem_info.import_sources {
    if imp_mod != mod_path { continue; }
    // Skip structs — they are handled in the method mangling section
    if sem_info.struct_defs.contains_key(imp_name) { continue; }

    let (pkg_name, source_segments) = resolve_mangle_context(
        source_module, package_name, &pkg_sem_info.external_dep_names
    );
    let source_segs: Vec<&str> = if source_segments.is_empty() {
        vec![""]
    } else {
        source_segments.iter().map(|s| s.as_str()).collect()
    };
    let mangled = mangle_function(pkg_name, &source_segs, imp_name, &[]);
    name_map.insert(imp_name.clone(), mangled);
}
```

**Imported struct methods** (existing section, lines 36-75):

```rust
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
                source_module, package_name, &pkg_sem_info.external_dep_names
            );
            let source_segs: Vec<&str> = if source_segments.is_empty() {
                vec![""]
            } else {
                source_segments.iter().map(|s| s.as_str()).collect()
            };

            // Methods
            for method in &struct_info.methods {
                let local_mangled = format!("{}_{}", struct_name, method.name);
                let mangled = mangle_method(pkg_name, &source_segs, struct_name, &method.name, &[]);
                name_map.insert(local_mangled, mangled);
            }

            // Initializer (if struct has init params)
            if !struct_info.init.params.is_empty() {
                let local_mangled = format!("{}_init", struct_name);
                let mangled = mangle_initializer(pkg_name, &source_segs, struct_name);
                name_map.insert(local_mangled, mangled);
            }
        }
    } else {
        // Local struct: existing logic with package_name and module_segments
    }
}
```

### 5. BIR Merge

**Ordering:** Runs AFTER `emit_interfaces`/`emit_package_bengalmod`, BEFORE `optimize`. This ensures the local package's `.bengalmod` does not contain external dependency BIR.

```rust
fn merge_external_deps(lowered: &mut LoweredPackage, external_deps: &[ExternalDep]) {
    for dep in external_deps {
        for (mod_path, bir_module) in &dep.bir_modules {
            let ext_path = dep_module_path(&dep.name, mod_path);
            lowered.modules.insert(ext_path, LoweredModule {
                bir: bir_module.clone(),
                is_entry: false,
            });
        }
    }
}
```

**Downstream stages operate purely on BIR data:**
- **optimize**: `bir::optimize_module(&mut module.bir)` — BIR only, no `pkg_sem_info` access
- **monomorphize**: `bir::mono::mono_collect(&module.bir, "main")` — BIR only
- **codegen**: `codegen::compile_module_with_mono(&module.bir, ...)` — BIR only
- **link**: operates on `object_bytes` — no semantic info needed

External BIR modules will not have entries in `pkg_sem_info.module_infos`, but this is safe because no post-lower stage consults `pkg_sem_info`. The `emit_interfaces`/`emit_package_bengalmod` functions skip modules with no `sem_info` entry (`None => continue`), which is the correct behavior.

## Error Handling

| Condition | Behavior |
|-----------|----------|
| `.bengalmod` file not found | Fatal error with path in message |
| Format version mismatch | Fatal error: "incompatible .bengalmod version: expected N, got M" |
| Import targets non-existent module | Existing error: "unresolved import: module '...' not found" |
| Import targets internal symbol | Existing error: "'...' is not accessible from module '...'" |
| Duplicate dep names | Fatal error: "--dep 'math' specified multiple times" |
| Dep name conflicts with local module | Local module wins (fallback semantics); emit warning: "dependency 'math' is shadowed by local module 'math'" |

## Testing Strategy

### Integration Tests (`tests/separate_compilation.rs`)

Tests use the pipeline API directly — no CLI subprocess needed.

**Pattern:** compile library source → emit `.bengalmod` → compile app with dep → link → execute → check exit code.

```rust
// Helper: compile lib, write .bengalmod, return path
fn compile_lib(name: &str, source: &str, dir: &Path) -> PathBuf { ... }
// Helper: compile app with deps, link, execute, return exit code
fn compile_and_run(source: &str, deps: &[(&str, &Path)]) -> i32 { ... }
```

**Test cases:**

1. **Basic function call**: lib exports `func add(a: Int32, b: Int32) -> Int32`, app calls `add(1, 2)` — returns 3
2. **Struct usage**: lib exports struct `Point` with fields and method `sum()`, app creates instance and calls method
3. **Generic function**: lib exports `func identity<T>(x: T) -> T`, app calls `identity(42)` — monomorphized in consumer
4. **Protocol**: lib exports protocol `Summable`, app's struct conforms and uses it
5. **Multiple deps**: `--dep a=... --dep b=...`, app uses symbols from both
6. **Visibility**: lib has `internal func secret()`, app tries to import — error
7. **Error: missing file**: `--dep math=nonexistent.bengalmod` — error message
8. **Error: version mismatch**: corrupted/old `.bengalmod` — error message
