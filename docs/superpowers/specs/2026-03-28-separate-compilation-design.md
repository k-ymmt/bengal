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

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Compilation unit | Package (not module) | Matches Rust's crate model; `analyze_package` stays unchanged for intra-package modules |
| Dependency specification | CLI flag `--dep name=path` | Simplest approach; Rust uses `--extern`; build system can compose flags later |
| Dependency codegen | Re-codegen from BIR each time | `.bengalmod` is self-contained; optimized later with `.a` archives (TODO step 3) |
| Generic monomorphization | Consumer-side, BIR merge | Matches Rust's downstream monomorphization model; reuses existing `monomorphize` stage |
| Import resolution | Fallback (local then external) | No new syntax needed; Rust Edition 2018 uses the same implicit model |
| Cache validation | Format version check only | No build system yet; user is responsible for rebuilding deps |

## Data Types

### `ExternalDep`

```rust
// src/pipeline.rs
pub struct ExternalDep {
    /// Dependency name as specified in --dep (used in import statements)
    pub name: String,
    /// Package name from the .bengalmod file
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
  → merge_external_bir (merge dep BIR into LoweredPackage)
  → emit_interfaces (unchanged — emits local package's .bengalmod)
  → optimize
  → monomorphize
  → codegen
  → link
```

### 1. Package-Level `.bengalmod` Emission

Add `emit_package_bengalmod()` alongside existing `emit_interfaces()`. Produces a single `.bengalmod` containing all modules of the package:

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
    // write with error handling (non-fatal)
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

`analyze_package` receives `external_deps: &[ExternalDep]`.

**Phase 1 addition — after `collect_global_symbols`:**

External dependency module paths are prefixed with the dep name to avoid collisions with local modules:

```rust
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
for dep in external_deps {
    for (mod_path, iface) in &dep.interfaces {
        let ext_path = dep_module_path(&dep.name, mod_path);
        let symbols = interface_to_global_symbols(iface, &ext_path);
        global_symbols.insert(ext_path, symbols);
    }
}
```

**Phase 2 change — import resolution fallback:**

`resolve_import_module_path` gains a `global_symbols` parameter:

```rust
// After building the target ModulePath:
if graph.modules.contains_key(&result) {
    return Ok(result);  // Local module — preferred
}
if global_symbols.contains_key(&result) {
    return Ok(result);  // External dependency — fallback
}
Err(pkg_err("unresolved import: module '...' not found"))
```

This means `import math::add` first checks local modules, then external deps — no new syntax needed.

**`PackageSemanticInfo` extension:**

```rust
pub struct PackageSemanticInfo {
    pub module_infos: HashMap<ModulePath, SemanticInfo>,
    pub import_sources: HashMap<(ModulePath, String), ModulePath>,
    /// Maps external dep module paths to their package names (for name mangling)
    pub external_dep_names: HashMap<ModulePath, String>,
}
```

Populated during Phase 1:
```rust
for dep in external_deps {
    for mod_path in dep.interfaces.keys() {
        let ext_path = dep_module_path(&dep.name, mod_path);
        pkg_sem_info.external_dep_names.insert(ext_path, dep.package_name.clone());
    }
}
```

### 4. Lower Stage — Name Mangling

`build_name_map` uses `external_dep_names` to determine whether an imported symbol comes from an external package:

```rust
for ((imp_mod, imp_name), source_module) in &pkg_sem_info.import_sources {
    if imp_mod != mod_path { continue; }

    let (pkg_name, source_segments) = if let Some(dep_pkg_name) =
        pkg_sem_info.external_dep_names.get(source_module)
    {
        // External: use dep's package name, strip dep_name prefix from path
        let internal_segments = &source_module.0[1..];
        (dep_pkg_name.as_str(), internal_segments)
    } else {
        // Local: use own package name
        (package_name, source_module.0.as_slice())
    };

    let mangled = mangle_function(pkg_name, source_segments, imp_name, &[]);
    name_map.insert(imp_name.clone(), mangled);
}
```

Same logic applies to method mangling (`mangle_method`).

### 5. BIR Merge

After lower, before optimize:

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

The merged BIR modules flow through the rest of the pipeline unchanged:
- **optimize**: optimization passes run on all modules (local + external)
- **monomorphize**: consumer-side monomorphization of external generics; concrete instantiations end up in consumer's object code
- **codegen**: all modules compiled to object bytes
- **link**: all object files linked into the final executable

## Error Handling

| Condition | Behavior |
|-----------|----------|
| `.bengalmod` file not found | Fatal error with path in message |
| Format version mismatch | Fatal error: "incompatible .bengalmod version: expected N, got M" |
| Import targets non-existent module | Existing error: "unresolved import: module '...' not found" |
| Import targets internal symbol | Existing error: "'...' is not accessible from module '...'" |
| Duplicate dep names | Fatal error: "--dep 'math' specified multiple times" |
| Dep name conflicts with local module | Local module wins (fallback semantics) |

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
