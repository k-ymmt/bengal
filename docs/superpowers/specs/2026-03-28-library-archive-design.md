# Library Archive Design

## Overview

Embed pre-compiled object code in `.bengalmod` files so consumers skip codegen for non-generic dependency functions. Generic functions retain BIR for consumer-side monomorphization. This is an internal optimization — the `--dep` CLI interface is unchanged.

## Scope

**In scope:**
- Add `object_bytes: HashMap<ModulePath, Vec<u8>>` to `BengalModFile`
- Move `emit_package_bengalmod` after codegen to receive `CompiledPackage`
- Filter BIR in `.bengalmod` to generic functions only
- Consumer-side: merge only generic BIR, link pre-compiled object code directly
- Update `link` to accept external dep object bytes
- All existing `tests/separate_compilation.rs` tests continue to pass

**Out of scope (future):**
- `.bengallib` / `.rlib`-style format split (metadata-only vs metadata+objects) — documented in TODO.md
- `check` command (codegen skip)
- Cross-compilation considerations
- `ar`-based `.a` file generation

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Object code location | Embedded in `.bengalmod` | One file per dep; existing `--dep` CLI unchanged; split to `.bengallib` later |
| BIR in `.bengalmod` | Generic functions only | Non-generic functions have object code; BIR unnecessary |
| Format version | Keep v2 | Project is pre-release; no compatibility needed |
| Pipeline position for emit | After codegen | Object code must exist before it can be embedded |
| Future extensibility | Code structured for `.bengallib` split | `object_bytes` is a separate field, easily moved to another file |

## Data Structure Changes

### `BengalModFile`

```rust
// src/interface.rs
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,          // Generic functions only
    pub interfaces: HashMap<ModulePath, ModuleInterface>,  // Unchanged
    pub object_bytes: HashMap<ModulePath, Vec<u8>>,       // NEW: pre-compiled object code
}
```

### `ExternalDep`

```rust
// src/pipeline.rs
pub struct ExternalDep {
    pub name: String,
    pub package_name: String,
    pub interfaces: HashMap<ModulePath, ModuleInterface>,
    pub bir_modules: HashMap<ModulePath, BirModule>,  // Generic BIR only
    pub object_bytes: HashMap<ModulePath, Vec<u8>>,   // NEW
}
```

## Pipeline Changes

### Library-side (emit)

Current flow:
```
parse → analyze → lower → emit_package_bengalmod → optimize → mono → codegen → link
```

New flow:
```
parse → analyze → lower → optimize → mono → codegen → emit_package_bengalmod → link
```

`emit_package_bengalmod` moves after `codegen` and receives the compiled output:

```rust
pub fn emit_package_bengalmod(
    lowered: &LoweredPackage,
    compiled: &CompiledPackage,  // NEW: provides object_bytes
    cache_dir: &Path,
)
```

Inside, for each module:
1. Generate `ModuleInterface` from `SemanticInfo` (unchanged)
2. Filter `BirModule` to generic functions only via `filter_generic_functions`
3. Take object bytes from `compiled.object_bytes`
4. Package into `BengalModFile` with all three components

**`filter_generic_functions`:**

```rust
fn filter_generic_functions(bir: &BirModule) -> BirModule {
    BirModule {
        functions: bir.functions.iter()
            .filter(|f| !f.type_params.is_empty())
            .cloned()
            .collect(),
        struct_layouts: bir.struct_layouts.clone(),
    }
}
```

This function is placed in `src/pipeline.rs` as a private helper. It preserves `struct_layouts` because consumer-side monomorphization of generic functions may need struct layout information.

**`compile_to_executable` update:**

```rust
pub fn compile_to_executable(
    entry_path: &Path,
    output_path: &Path,
    external_deps: &[pipeline::ExternalDep],
) -> Result<()> {
    let mut diag = DiagCtxt::new();
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze_with_deps(parsed, external_deps, &mut diag)?;
    let lowered = pipeline::lower(analyzed, &mut diag)?;
    pipeline::emit_interfaces(&lowered, Path::new(".build/cache"));
    let mut lowered = lowered;
    pipeline::merge_external_deps(&mut lowered, external_deps);
    let optimized = pipeline::optimize(lowered);
    let mono = pipeline::monomorphize(optimized, &mut diag)?;
    let compiled = pipeline::codegen(mono, &mut diag)?;
    // emit AFTER codegen (needs object_bytes), BEFORE link
    pipeline::emit_package_bengalmod(&optimized_ref, &compiled, Path::new(".build/cache"));
    let ext_objects = pipeline::collect_external_objects(external_deps);
    pipeline::link(compiled, &ext_objects, output_path)
}
```

Note: `emit_package_bengalmod` needs both `LoweredPackage` (for SemanticInfo/BIR) and `CompiledPackage` (for object_bytes). Since `optimize` consumes `LoweredPackage`, we need to retain the necessary data. The actual implementation should pass the pre-optimize `LoweredPackage` data that was saved, or restructure to keep `pkg_sem_info` and BIR accessible after codegen. The simplest approach: save a reference to `pkg_sem_info` and the BIR before `optimize` consumes them, or change `optimize`/`monomorphize` to preserve this data in their output types.

**Recommended approach:** Add `pkg_sem_info` to `MonomorphizedPackage` and `CompiledPackage` (or pass it separately). The implementer should choose the cleanest option that avoids cloning large structures unnecessarily.

### Consumer-side (load + merge + link)

**`load_external_dep`:** Trivially updated — `BengalModFile` now has `object_bytes`, which maps directly to `ExternalDep.object_bytes`.

**`merge_external_deps`:** Unchanged in logic. The BIR in `dep.bir_modules` now contains only generic functions (since that's what `.bengalmod` stores). The existing merge code works as-is.

**`link` stage:**

New signature:
```rust
pub fn link(
    compiled: CompiledPackage,
    external_objects: &HashMap<ModulePath, Vec<u8>>,
    output_path: &Path,
) -> Result<(), PipelineError>
```

Writes both local and external object bytes to temp `.o` files, then passes all to the system linker.

**`collect_external_objects` helper:**

```rust
pub fn collect_external_objects(
    external_deps: &[ExternalDep],
) -> HashMap<ModulePath, Vec<u8>> {
    let mut objects = HashMap::new();
    for dep in external_deps {
        for (mod_path, bytes) in &dep.object_bytes {
            let ext_path = dep_module_path(&dep.name, mod_path);
            objects.insert(ext_path, bytes.clone());
        }
    }
    objects
}
```

### `main.rs` inline pipeline

Same changes as `compile_to_executable`: move `emit_package_bengalmod` after codegen, pass `compiled` to it, update `link` call to include external objects.

## Error Handling

| Condition | Behavior |
|-----------|----------|
| `.bengalmod` with empty `object_bytes` | Valid — old-format or library-only package; fall back to BIR codegen (existing path) |
| Object code for wrong target architecture | Linker error (not our responsibility to detect) |
| Serialization of large object bytes | MessagePack handles binary data efficiently; non-fatal warning on write failure |

## Testing Strategy

### Existing tests (must pass)

All 7 tests in `tests/separate_compilation.rs` continue to pass. These are the primary validation that the optimization doesn't change behavior.

### New unit tests

**`filter_generic_functions`:**
- BirModule with mixed generic/non-generic functions → only generic functions remain
- BirModule with only non-generic functions → empty functions list, struct_layouts preserved
- BirModule with only generic functions → all functions preserved

**`emit_package_bengalmod` with object code:**
- Compile a library with both generic and non-generic functions
- Emit `.bengalmod`
- Read it back and verify:
  - `object_bytes` is non-empty
  - `modules` (BIR) contains only generic functions
  - `interfaces` contains all public functions (both generic and non-generic)
