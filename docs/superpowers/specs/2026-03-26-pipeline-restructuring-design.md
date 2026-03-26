# Pipeline Restructuring Design

## Overview

Restructure the Bengal compiler pipeline after the BIR monomorphization migration.
The goals are:

1. **Responsibility separation** — decompose the monolithic `compile_package_to_executable` (~240 lines) into discrete stage functions with explicit intermediate data types.
2. **Unified pipeline** — treat single-file compilation as a "package with one module," eliminating code duplication between the two paths.
3. **Error context improvement** — attach module path and source code to pipeline errors so diagnostics display correct location information in package mode.
4. **Redundant recomputation elimination** — preserve `InferredTypeArgs` per module in `AnalyzedPackage` for reuse in the `lower` stage, and note further caching improvements as future work.

## Approach

**Functional pipeline (Approach B):** Each stage is an independent function that consumes the previous stage's output and returns a new intermediate type. No shared mutable state.

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

For single-file input, `ModuleGraph::from_source(name, source)` constructs a graph with one module by lexing, parsing, and wrapping the AST in a `ModuleGraph` with a root `ModulePath`. For in-memory strings (test/eval), the same constructor is used. `ModuleInfo` already has a `source: String` field (populated during graph building).

### AnalyzedPackage

Output of the `analyze` stage.

```rust
pub struct AnalyzedPackage {
    pub package_name: String,
    pub graph: ModuleGraph,
    pub inferred_maps: HashMap<ModulePath, HashMap<NodeId, Vec<TypeAnnotation>>>,
    pub pkg_sem_info: PackageSemanticInfo,
}
```

Note: `analyze_pre_mono_lenient` produces per-module `SemanticInfo` that is currently discarded (`_pre_mono_sem_info` in `lib.rs:103`). This is intentional — the `lower` stage uses `pkg_sem_info.module_infos` (cross-module-resolved `SemanticInfo` from `analyze_package`), not the pre-mono results. Only `InferredTypeArgs` from `analyze_pre_mono_lenient` is retained (via `inferred_maps`). Refactoring `analyze_package` to reuse pre-mono `SemanticInfo` is a potential future optimization but out of scope.

### LoweredPackage / LoweredModule

Output of the `lower` stage.

```rust
pub struct LoweredPackage {
    pub package_name: String,
    pub modules: HashMap<ModulePath, LoweredModule>,
    pub sources: HashMap<ModulePath, String>,  // for error reporting in later stages
}

pub struct LoweredModule {
    pub bir: BirModule,
    pub is_entry: bool,
}
```

`sources` carries per-module source code forward so that `codegen` stage errors can produce proper diagnostics with file names and source locations.

### MonomorphizedPackage / MonomorphizedModule

Output of the `monomorphize` stage.

```rust
pub struct MonomorphizedPackage {
    pub package_name: String,
    pub modules: HashMap<ModulePath, MonomorphizedModule>,
    pub sources: HashMap<ModulePath, String>,
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
    pub object_bytes: HashMap<ModulePath, Vec<u8>>,
}
```

## Stage Functions

### parse

```rust
pub fn parse(entry_path: &Path) -> Result<ParsedPackage, PipelineError>;
```

Responsibilities:
- Detect single-file vs package mode (presence of `Bengal.toml`).
- Single-file: read source, call `ModuleGraph::from_source()` which lexes, parses, and wraps AST.
- Package: load config, call `build_module_graph()`.
- Derive `package_name` from config or directory/file name.

### analyze

```rust
pub fn analyze(parsed: ParsedPackage) -> Result<AnalyzedPackage, PipelineError>;
```

Responsibilities:
- `validate_generics` for all modules.
- `analyze_pre_mono_lenient` per module (retain `InferredTypeArgs` in `inferred_maps`).
- `analyze_package` for cross-module resolution (includes `validate_main` check for the entry module via `analyze_single_module(require_main=true)`).

Note: `validate_main` is not called separately. `analyze_package` internally calls `analyze_single_module(require_main=is_root)`, which checks the entry module for a valid `main()` function. This matches the existing package pipeline behavior.

### lower

```rust
pub fn lower(analyzed: AnalyzedPackage) -> Result<LoweredPackage, PipelineError>;
```

Responsibilities:
- Build `name_map` per module via `build_name_map()` helper.
- Call `lower_module_with_inferred()` per module.
- Carry `sources` forward from `graph` for later error reporting.

Note: The single-file path currently uses `lower_program_with_inferred`. After unification through the package path, `lower_program_with_inferred` becomes dead code and can be removed.

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
pub fn optimize(lowered: LoweredPackage) -> LoweredPackage;
```

Consumes and returns `LoweredPackage` (maintains functional pipeline contract). Calls `bir::optimize_module` on each module's BIR internally.

### monomorphize

```rust
pub fn monomorphize(optimized: LoweredPackage) -> Result<MonomorphizedPackage, PipelineError>;
```

Returns `Result` for pipeline consistency, even though current `mono_collect` is infallible. This future-proofs the API and keeps error handling uniform across all stages.

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

Calls `codegen::compile_module_with_mono()` per module, collecting object bytes. On error, wraps `BengalError` with the module's source code from `mono.sources`.

### link

```rust
pub fn link(compiled: CompiledPackage, output_path: &Path) -> Result<(), PipelineError>;
```

Writes object bytes to temp files, calls `codegen::link_objects()`, cleans up. `BUILD_COUNTER` atomic moves from `lib.rs` to `pipeline.rs` near this function.

## Public API Changes

### src/lib.rs

```rust
// Main API — source file to executable
pub fn compile_to_executable(entry_path: &Path, output_path: &Path) -> Result<()>;

// BIR output (debug/test)
pub fn compile_to_bir(entry_path: &Path) -> Result<BirOutput>;

// BIR output from source string (eval subcommand)
pub fn compile_source_to_bir(source: &str) -> Result<BirOutput>;

// Object bytes only (test, no linking)
pub fn compile_to_objects(entry_path: &Path) -> Result<CompiledPackage>;

// Compile from source string (public for integration tests)
pub fn compile_source_to_objects(source: &str) -> Result<Vec<u8>>;
```

Removed:
- `compile_source(&str) -> Result<Vec<u8>>` — replaced by `compile_source_to_objects`.
- `compile_package_to_executable` — renamed to `compile_to_executable`.

`compile_to_bir` changes from `&str` to `&Path` input, supporting packages. A `compile_source_to_bir(&str)` variant is added for the `eval` subcommand.

`compile_source_to_objects` is `pub` (not `pub(crate)`) because integration tests in `tests/` call it via test helpers (`compile_to_native_and_run`, `compile_source_should_fail`). Internally, it uses `ModuleGraph::from_source()` to construct a single-module graph from an in-memory string.

`BirOutput`:
```rust
pub struct BirOutput {
    pub modules: HashMap<ModulePath, LoweredModule>,
    pub bir_texts: HashMap<ModulePath, String>,
}
```

Existing re-exports (`compile_to_module`, etc.) are maintained for codegen test compatibility. The JIT test path (`compile_and_run` using `compile_to_module_with_mono`) remains unchanged.

### src/main.rs

- `Command::Compile`: remove single-file vs package branching; always call `compile_to_executable`.
- Convert `PipelineError` to `BengalDiagnostic` using the attached module path and source code.
- `Command::Eval`: uses `compile_source_to_bir` (new `&str` variant) instead of the removed `compile_to_bir(&str)`. The eval path extracts the single module's `BirModule` from `BirOutput.modules` (root `ModulePath`) for JIT execution. Full pipeline integration of eval is deferred to future work.
- `--emit-bir` in `Command::Compile`: iterates `BirOutput.bir_texts` and prints each module's BIR text prefixed with a `=== module: <path> ===` header line. For single-file input (one module), this produces the same output as before.

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

For errors that occur before any module is identified (e.g., `find_package_root`, `load_package`, `build_module_graph` in the `parse` stage), `module` is set to `"<package>"`.

## Changes to package.rs

- `ModuleGraph::from_source(name: &str, source: &str) -> Result<ModuleGraph>` constructor added. This lexes, parses, and wraps the AST in a single-module `ModuleGraph` with a root `ModulePath`. Used for single-file compilation and in-memory source compilation (tests, eval).
- `ModuleInfo` already has `pub source: String` — no change needed.

## File Layout

```
src/
  pipeline.rs    (new)  Stage functions + intermediate types + helpers
  lib.rs         (mod)  Public API rewritten over pipeline
  main.rs        (mod)  Unified compile path, PipelineError handling
  error.rs       (mod)  PipelineError added
  package.rs     (mod)  ModuleGraph::from_source()
  mangle.rs      (-)    No changes
  bir/           (-)    No changes (lower_program_with_inferred becomes dead code, removable)
  codegen/       (-)    No changes
  semantic/      (-)    No changes
```

Estimated size of `pipeline.rs`: ~400-500 lines (single file, no need for directory).

## Test Migration

Existing tests that call `bengal::compile_source(source)` will be updated to call `bengal::compile_source_to_objects(source)`. Affected test helpers in `tests/common/mod.rs`:
- `compile_to_native_and_run()` — change `compile_source` to `compile_source_to_objects`
- `compile_source_should_fail()` — change `compile_source` to `compile_source_to_objects`
- `compile_and_run_package()` — change `compile_package_to_executable` to `compile_to_executable`
- `compile_package_should_fail()` — change `compile_package_to_executable` to `compile_to_executable`

Tests in `lib.rs` that call `compile_to_bir(source)` will be updated to use `compile_source_to_bir(source)`.

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
- Refactor `analyze_package` to reuse pre-mono `SemanticInfo` (avoid redundant work)
- File-change-detection-based incremental analysis cache
- Disk-persisted cache for rebuild acceleration
- Per-module parallel analysis

### Other
- `eval` subcommand full pipeline integration (currently uses JIT path directly)
- Dead code removal: `lower_program_with_inferred` (after unification confirmed working)
