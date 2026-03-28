# Sysroot / Library Search Path Design

## Overview

Implement a sysroot mechanism and library search paths for the Bengal compiler, enabling automatic discovery of pre-compiled libraries (`.bengalmod` files) without explicit `--dep name=path` for every dependency. This is a prerequisite for a pre-compiled standard library (Core).

## Design Decisions

### Approach

**Approach A: Pipeline-integrated with eager pre-scan** — Resolve sysroot and `-L` search paths early in the pipeline. Before semantic analysis Phase 1, scan all `import` statements across all source modules to collect unknown module names. Resolve them via search paths, load as `ExternalDep`, and inject into the global symbol table before Phase 2 (import resolution). This avoids mid-iteration mutation of the symbol table. Explicit `--dep name=path` continues to work and takes priority over search.

Future consideration: **Approach C (two-phase indexing)** — Build an index of available `.bengalmod` files at startup (path existence only), then load on demand during import resolution. More scalable but more complex. Migrate to this when the number of libraries grows. (Tracked in TODO.md)

### Sysroot Directory Structure (Rust-style)

```
<sysroot>/
  bin/
    bengal                              # Compiler binary
  lib/
    bengallib/
      <llvm-target-triple>/            # e.g. aarch64-apple-darwin
        lib/
          Core.bengalmod               # Standard library
```

The target triple is the LLVM default target triple, obtained via `TargetMachine::get_default_triple()` (inkwell).

### Sysroot Resolution (priority order)

1. **`--sysroot PATH`** CLI flag (highest priority)
2. **Auto-detection from compiler binary** — `std::env::current_exe()` → `../../` as sysroot, verify `lib/bengallib/<target>/lib/` exists

If auto-detection fails (e.g., `current_exe()` error, sysroot directory not found), silently fall back to no-sysroot mode (matching Rust's behavior). No warning or error is emitted — the compiler simply has no sysroot search path.

### Search Path Kinds

Two kinds, with remaining Rust-style kinds (`dependency`, `crate`, `framework`, `all`) as future work (tracked in TODO.md):

| Kind | Flag | Purpose |
|------|------|---------|
| `bengal` | `-L bengal=path` | Search for `.bengalmod` files |
| `native` | `-L native=path` | Passed to linker as `-L` |

`-L path` (without kind) is reserved for future use and produces an error: `"unsupported -L form: expected '-L bengal=<path>' or '-L native=<path>'"`.

### Library Search Order (for `.bengalmod` discovery)

1. **`--dep name=path`** — Explicit path, bypasses search entirely
2. **`-L bengal=path`** — User-specified search directories, in order of specification
3. **Sysroot** — `<sysroot>/lib/bengallib/<target>/lib/` (always last)

Within each directory, look for `<name>.bengalmod`.

## CLI Interface

### New Flags

```bash
bengal compile main.bengal --sysroot /path/to/sysroot
bengal compile main.bengal -L bengal=/path/to/libs
bengal compile main.bengal -L native=/path/to/native/libs
```

### Existing (unchanged)

```bash
bengal compile main.bengal --dep name=path.bengalmod
```

### Combined Example

```bash
bengal compile main.bengal \
  --sysroot /opt/bengal \
  -L bengal=./vendor \
  --dep myutil=./build/myutil.bengalmod
```

`-L` can be specified multiple times.

## Pipeline Integration

### Current Flow

```
CLI → load_external_dep(name, path) → analyze_with_deps → ...
```

### New Flow

```
CLI → resolve_sysroot()          # Determine sysroot path
    → collect_search_paths()     # Build -L bengal= list, append sysroot at end
    → load_external_dep(...)     # --dep name=path loaded directly (unchanged)
    → pre_scan_imports()         # Scan all import statements, discover & load missing deps
    → analyze_with_deps          # All deps already loaded, normal analysis proceeds
    → ...
```

### Import Auto-Discovery (Eager Pre-Scan)

Auto-discovery happens **before** `analyze_package()` Phase 1, not during import resolution. This avoids mutating the global symbol table mid-iteration.

1. Parse all source modules and collect all `import` top-level names (e.g., `import Foo::bar` → `Foo`)
2. For each name not already provided by `--dep`, call `LibrarySearcher::find_bengalmod(name)`
3. If found, call `load_external_dep(name, found_path)` and add to the `ExternalDep` list
4. If not found, skip (will produce the existing error during import resolution in Phase 2)
5. Pass the augmented `ExternalDep` list to `analyze_with_deps` as usual

This runs in `pipeline.rs`, between explicit dep loading and `analyze_with_deps`.

### Name Collision: Local Module vs Auto-Discovered Library

If a local module and an auto-discovered library share the same name (e.g., local `Core` module and sysroot `Core.bengalmod`), the local module takes priority. This is consistent with the existing behavior where local modules shadow external deps in import resolution (`resolve_import_module_path` checks `graph.modules` first).

### `-L native=` Handling

Passed to `cc` as `-L` during the link phase. Not used for `.bengalmod` search. Requires modifying `link_objects()` in `src/codegen/llvm.rs` to accept and forward native search paths.

### Transitive Dependencies

Auto-discovered libraries must be self-contained (interfaces + BIR + object code in a single `.bengalmod`). Transitive dependency resolution (library A depending on library B) is not in scope for this iteration.

## Implementation Components

### New Module: `src/sysroot.rs`

```rust
pub struct SearchPath {
    pub kind: SearchPathKind,
    pub path: PathBuf,
}

pub enum SearchPathKind {
    Bengal,
    Native,
}

pub struct LibrarySearcher {
    /// Explicitly specified deps via --dep name=path
    explicit_deps: HashMap<String, PathBuf>,
    /// -L bengal= paths + sysroot (appended last)
    bengal_search_paths: Vec<PathBuf>,
    /// -L native= paths
    native_search_paths: Vec<PathBuf>,
}

impl LibrarySearcher {
    /// Construct from sysroot + CLI flags
    pub fn new(
        sysroot_override: Option<PathBuf>,
        search_paths: Vec<SearchPath>,
        explicit_deps: Vec<(String, PathBuf)>,
    ) -> Self;

    /// Find .bengalmod by name: explicit_deps → bengal_search_paths
    pub fn find_bengalmod(&self, name: &str) -> Option<PathBuf>;

    /// Auto-detect sysroot from compiler binary path
    fn resolve_sysroot(override_path: Option<PathBuf>) -> Option<PathBuf>;

    /// Get LLVM default target triple
    fn target_triple() -> String;
}
```

### Files to Modify

| File | Change |
|------|--------|
| `src/main.rs` | Parse `--sysroot` and `-L` CLI flags |
| `src/lib.rs` | Thread `LibrarySearcher` into the pipeline. Public API functions (`compile_to_executable`, etc.) continue to accept `&[ExternalDep]` only — auto-discovery is CLI-only behavior |
| `src/pipeline.rs` | Add `pre_scan_imports()` between explicit dep loading and `analyze_with_deps`. Pass `native_search_paths` through to `link()` |
| `src/codegen/llvm.rs` | Extend `link_objects()` to accept `native_search_paths: &[PathBuf]` and pass them as `-L` flags to `cc` |

### Dummy Core Library

A test helper generates `Core.bengalmod` with a simple public function (e.g., `pub func core_version() -> Int`) for integration testing. Test fixtures are generated at test time (not checked into the repository) because the sysroot path includes the LLVM target triple which varies per machine. The test helper creates a temporary sysroot at `<tmpdir>/sysroot/lib/bengallib/<target>/lib/Core.bengalmod` using `TargetMachine::get_default_triple()`.

## Test Strategy

### Unit Tests (`src/sysroot.rs`)

- Sysroot auto-detection from binary path
- `--sysroot` override takes priority over auto-detection
- Search order: `--dep` → `-L bengal=` → sysroot
- Returns `None` when `.bengalmod` not found in any path

### Integration Tests (`tests/`)

- **Sysroot auto-discovery**: Build test sysroot, verify `import Core` resolves without `--dep`
- **`-L bengal=` discovery**: Place `.bengalmod` in search path, verify name-only resolution
- **Priority**: Same-name `.bengalmod` in both `-L` and sysroot; `-L` wins
- **Coexistence with `--dep`**: Explicit `--dep` and sysroot auto-discovery work simultaneously
- **`-L native=`**: Correctly passed to linker
- **Error case**: `import` of nonexistent library produces clear error message
- **Local module shadows sysroot**: Local module named `Core` takes priority over sysroot `Core.bengalmod`
- **Malformed sysroot**: Missing `lib/bengallib/` subdirectory gracefully falls back to no-sysroot
