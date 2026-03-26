# Pipeline Restructuring Design

## Overview

Restructure the Bengal compiler pipeline after the BIR monomorphization migration.
The goals are:

1. **Responsibility separation** — decompose the monolithic `compile_package_to_executable` (~240 lines) into discrete stage functions with explicit intermediate data types.
2. **Unified pipeline** — treat single-file compilation as a "package with one module," eliminating code duplication between the two paths.
3. **Error context improvement** — attach module path and source code to pipeline errors so diagnostics display correct location information in package mode.
4. **Redundant recomputation elimination** — preserve pre-mono analysis results in `AnalyzedPackage` so the lower stage does not re-derive them.

## Approach

**Functional pipeline (Approach B):** Each stage is an independent function that takes the previous stage's output and returns a new intermediate type. No shared mutable state.

```
parse → analyze → lower → optimize → monomorphize → codegen → link
```

This mirrors rustc's query-based architecture. Benefits: each stage is independently testable, inputs/outputs are enforced at the type level, and there is no lifetime complexity from a stateful pipeline struct.

## Intermediate Data Types

All types live in `src/pipeline.rs`.

### ParsedPackage

Output of the `parse` stage.

```rust
pub struct ParsedPackage {
    pub package_name: String,
    pub graph: ModuleGraph,
}
```

For single-file input, `ModuleGraph::single(path, source)` constructs a graph with one module. `ModuleInfo` gains a `source: String` field for error reporting.

### AnalyzedPackage

Output of the `analyze` stage.

```rust
pub struct AnalyzedPackage {
    pub package_name: String,
    pub graph: ModuleGraph,
    pub inferred_maps: HashMap<ModulePath, HashMap<NodeId, Vec<TypeAnnotation>>>,
    pub pkg_sem_info: PackageSemanticInfo,
    pub pre_mono_infos: HashMap<ModulePath, SemanticInfo>,
}
```

`pre_mono_infos` retains per-module `SemanticInfo` that is currently discarded (`_pre_mono_sem_info` in `lib.rs:103`). The `lower` stage reuses this instead of recomputing it.

### LoweredPackage / LoweredModule

Output of the `lower` stage.

```rust
pub struct LoweredPackage {
    pub package_name: String,
    pub modules: HashMap<ModulePath, LoweredModule>,
}

pub struct LoweredModule {
    pub bir: BirModule,
    pub is_entry: bool,
}
```

### MonomorphizedPackage / MonomorphizedModule

Output of the `monomorphize` stage.

```rust
pub struct MonomorphizedPackage {
    pub package_name: String,
    pub modules: HashMap<ModulePath, MonomorphizedModule>,
}

pub struct MonomorphizedModule {
    pub bir: BirModule,
    pub mono_result: MonoCollectResult,
    pub external_functions: Vec<(String, Vec<BirType>, BirType)>,
    pub is_entry: bool,
}
```

`external_functions` collection (currently inline in `lib.rs`) moves into the `monomorphize` stage.

### CompiledPackage

Output of the `codegen` stage.

```rust
pub struct CompiledPackage {
    pub object_bytes: Vec<(ModulePath, Vec<u8>)>,
}
```

## Stage Functions

### parse

```rust
pub fn parse(entry_path: &Path) -> Result<ParsedPackage, PipelineError>;
```

Responsibilities:
- Detect single-file vs package mode (presence of `Bengal.toml`).
- Single-file: read source, lex, parse, construct `ModuleGraph::single()`.
- Package: load config, call `build_module_graph()`.
- Derive `package_name` from config or directory/file name.

### analyze

```rust
pub fn analyze(parsed: ParsedPackage) -> Result<AnalyzedPackage, PipelineError>;
```

Responsibilities:
- `validate_generics` for all modules.
- `validate_main` for the entry module.
- `analyze_pre_mono_lenient` per module (retain results in `pre_mono_infos`).
- `analyze_package` for cross-module resolution.

### lower

```rust
pub fn lower(analyzed: AnalyzedPackage) -> Result<LoweredPackage, PipelineError>;
```

Responsibilities:
- Build `name_map` per module via `build_name_map()` helper.
- Call `lower_module_with_inferred()` per module.

#### Helper: build_name_map

Extracted from the ~70-line inline block in `lib.rs`.

```rust
fn build_name_map(
    package_name: &str,
    mod_path: &ModulePath,
    mod_info: &ModuleInfo,
    sem_info: &SemanticInfo,
    pkg_sem_info: &PackageSemanticInfo,
) -> HashMap<String, String>;
```

### optimize

```rust
pub fn optimize(package: &mut LoweredPackage);
```

Calls `bir::optimize_module` on each module's BIR. In-place mutation (no new type).

### monomorphize

```rust
pub fn monomorphize(lowered: LoweredPackage) -> MonomorphizedPackage;
```

Responsibilities:
- `mono_collect` per module.
- `collect_external_functions` per module (extracted helper).

#### Helper: collect_external_functions

Extracted from the ~40-line inline block in `lib.rs`.

```rust
fn collect_external_functions(
    bir: &BirModule,
    mono_result: &MonoCollectResult,
) -> Vec<(String, Vec<BirType>, BirType)>;
```

### codegen

```rust
pub fn codegen(mono: MonomorphizedPackage) -> Result<CompiledPackage, PipelineError>;
```

Calls `codegen::compile_module_with_mono()` per module, collecting object bytes.

### link

```rust
pub fn link(compiled: CompiledPackage, output_path: &Path) -> Result<(), PipelineError>;
```

Writes object bytes to temp files, calls `codegen::link_objects()`, cleans up.

## Public API Changes

### src/lib.rs

```rust
// Main API — source file to executable
pub fn compile_to_executable(entry_path: &Path, output_path: &Path) -> Result<()>;

// BIR output (debug/test)
pub fn compile_to_bir(entry_path: &Path) -> Result<BirOutput>;

// Object bytes only (test, no linking)
pub fn compile_to_objects(entry_path: &Path) -> Result<CompiledPackage>;

// Test helper — compile from source string
pub(crate) fn compile_source_to_objects(source: &str) -> Result<Vec<u8>>;
```

Removed:
- `compile_source(&str) -> Result<Vec<u8>>` — replaced by `compile_to_objects(&Path)`.
- `compile_package_to_executable` — renamed to `compile_to_executable`.

`compile_to_bir` changes from `&str` to `&Path` input, supporting packages.

`BirOutput`:
```rust
pub struct BirOutput {
    pub modules: HashMap<ModulePath, LoweredModule>,
    pub texts: HashMap<ModulePath, String>,
}
```

Existing re-exports (`compile_to_module`, etc.) are maintained for codegen test compatibility.

### src/main.rs

- `Command::Compile`: remove single-file vs package branching; always call `compile_to_executable`.
- Convert `PipelineError` to `BengalDiagnostic` using the attached module path and source code.
- `Command::Eval`: unchanged (JIT path stays as-is).

## Error Handling

### PipelineError

```rust
#[derive(Debug, Error)]
#[error("{phase} error in {module}: {source_error}")]
pub struct PipelineError {
    pub phase: &'static str,
    pub module: String,
    pub source_code: Option<String>,
    pub source_error: BengalError,
}
```

Each stage wraps `BengalError` into `PipelineError` with the module path and source code, so `main.rs` can produce correct `BengalDiagnostic` output with proper file names and source locations.

## Analysis Result Caching

`AnalyzedPackage::pre_mono_infos` holds per-module `SemanticInfo` from `analyze_pre_mono_lenient`. Currently this value is discarded (`_pre_mono_sem_info` in `lib.rs:103`). Retaining it avoids redundant recomputation in the `lower` stage.

## Changes to package.rs

- `ModuleInfo` gains `pub source: String` field (source code for error reporting).
- `ModuleGraph::single(path: &Path, source: String) -> Result<ModuleGraph>` constructor added for single-file-as-package unification.

## File Layout

```
src/
  pipeline.rs    (new)  Stage functions + intermediate types + helpers
  lib.rs         (mod)  Public API rewritten over pipeline
  main.rs        (mod)  Unified compile path, PipelineError handling
  error.rs       (mod)  PipelineError added
  package.rs     (mod)  ModuleGraph::single(), source field
  mangle.rs      (-)    No changes
  bir/           (-)    No changes
  codegen/       (-)    No changes
  semantic/      (-)    No changes
```

Estimated size of `pipeline.rs`: ~300 lines (fits in a single file).

## Out of Scope (deferred to TODO.md)

### Mangling improvements
- Entity kind markers (function/method/initializer distinction)
- Function/method collision prevention
- Unified generic type encoding (integrate `mangle.rs` and `Instance::mangled_name()`)

### Error handling enhancements
- `Span` for `LoweringError` / `CodegenError`
- "Did you mean ...?" suggestions
- Multi-error batch reporting

### Caching enhancements
- File-change-detection-based incremental analysis cache
- Disk-persisted cache for rebuild acceleration
- Per-module parallel analysis

### Other
- `eval` subcommand pipeline integration
