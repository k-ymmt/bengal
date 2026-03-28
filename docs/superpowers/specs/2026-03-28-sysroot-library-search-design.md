# Sysroot / Library Search Path Design

## Overview

Implement a sysroot mechanism and library search paths for the Bengal compiler, enabling automatic discovery of pre-compiled libraries (`.bengalmod` files) without explicit `--dep name=path` for every dependency. This is a prerequisite for a pre-compiled standard library (Core).

## Design Decisions

### Approach

**Approach A: Pipeline-integrated** — Resolve sysroot and `-L` search paths early in the pipeline. When `import Foo` encounters an unknown module, search paths are consulted to find and load `Foo.bengalmod` on demand. Explicit `--dep name=path` continues to work and takes priority over search.

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

### Search Path Kinds

Two kinds, with remaining Rust-style kinds (`dependency`, `crate`, `framework`, `all`) as future work (tracked in TODO.md):

| Kind | Flag | Purpose |
|------|------|---------|
| `bengal` | `-L bengal=path` | Search for `.bengalmod` files |
| `native` | `-L native=path` | Passed to linker as `-L` |

`-L path` (without kind) is reserved for future use and produces an error in the current implementation.

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
    → analyze_with_deps          # On unresolved import, search paths → find & load .bengalmod
    → ...
```

### Import Auto-Discovery

During `analyze_package()` import resolution, when `import Foo` is not satisfied by an explicit `--dep`:

1. Iterate search paths in order
2. If `Foo.bengalmod` is found, call `load_external_dep("Foo", found_path)` to load it
3. If not found in any path, produce the existing error

### `-L native=` Handling

Passed to `cc` as `-L` during the link phase. Not used for `.bengalmod` search.

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
| `src/lib.rs` | Thread `LibrarySearcher` into the pipeline |
| `src/pipeline.rs` | Add auto-discovery during `analyze_with_deps` |
| `src/semantic/package_analysis.rs` | Call `LibrarySearcher::find_bengalmod` for unresolved imports |

### Dummy Core Library

A test helper generates `Core.bengalmod` with a simple public function (e.g., `pub func core_version() -> Int`) for integration testing. Placed in a test fixture sysroot at `tests/fixtures/sysroot/lib/bengallib/<target>/lib/Core.bengalmod`.

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
