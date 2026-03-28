# Sysroot / Library Search Path Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable the Bengal compiler to automatically discover pre-compiled libraries (`.bengalmod`) from a sysroot and user-specified search paths, eliminating the need for explicit `--dep name=path` for every dependency.

**Architecture:** Eager pre-scan approach — after parsing, scan all `import` statements for unknown module names, resolve them via search paths (`-L bengal=` then sysroot), load as `ExternalDep`, and pass to `analyze_with_deps`. Sysroot auto-detected from compiler binary path, overridable via `--sysroot`. Two `-L` kinds: `bengal` (`.bengalmod` search) and `native` (linker `-L`).

**Tech Stack:** Rust, clap (CLI), inkwell (LLVM target triple), existing `.bengalmod` format

**Spec:** `docs/superpowers/specs/2026-03-28-sysroot-library-search-design.md`

---

### Task 1: LibrarySearcher Core Types and Unit Tests

**Files:**
- Create: `src/sysroot.rs`
- Modify: `src/lib.rs:1-12` — add `pub mod sysroot;`

- [ ] **Step 1: Write failing tests for `LibrarySearcher`**

Create `src/sysroot.rs` with types and tests only (no implementation):

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct SearchPath {
    pub kind: SearchPathKind,
    pub path: PathBuf,
}

pub enum SearchPathKind {
    Bengal,
    Native,
}

pub struct LibrarySearcher {
    bengal_search_paths: Vec<PathBuf>,
    native_search_paths: Vec<PathBuf>,
}

impl LibrarySearcher {
    pub fn new(sysroot_override: Option<PathBuf>, search_paths: Vec<SearchPath>) -> Self {
        todo!()
    }

    pub fn find_bengalmod(&self, name: &str) -> Option<PathBuf> {
        todo!()
    }

    pub fn native_search_paths(&self) -> &[PathBuf] {
        &self.native_search_paths
    }

    pub fn bengal_search_paths(&self) -> &[PathBuf] {
        &self.bengal_search_paths
    }

    fn resolve_sysroot(override_path: Option<PathBuf>) -> Option<PathBuf> {
        todo!()
    }

    fn target_triple() -> String {
        todo!()
    }

    fn sysroot_lib_path(sysroot: &Path) -> PathBuf {
        let triple = Self::target_triple();
        sysroot.join("lib").join("bengallib").join(triple).join("lib")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_sysroot_with_lib(name: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        let triple = LibrarySearcher::target_triple();
        let lib_dir = dir.path().join("lib").join("bengallib").join(&triple).join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::write(lib_dir.join(format!("{}.bengalmod", name)), b"dummy").unwrap();
        dir
    }

    fn create_search_dir_with_lib(name: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(format!("{}.bengalmod", name)), b"dummy").unwrap();
        dir
    }

    #[test]
    fn sysroot_override_takes_priority() {
        let sysroot = create_sysroot_with_lib("Core");
        let searcher = LibrarySearcher::new(
            Some(sysroot.path().to_path_buf()),
            vec![],
        );
        assert!(searcher.find_bengalmod("Core").is_some());
    }

    #[test]
    fn find_bengalmod_in_bengal_search_path() {
        let search_dir = create_search_dir_with_lib("MyLib");
        let searcher = LibrarySearcher::new(
            None,
            vec![SearchPath { kind: SearchPathKind::Bengal, path: search_dir.path().to_path_buf() }],
        );
        assert!(searcher.find_bengalmod("MyLib").is_some());
    }

    #[test]
    fn bengal_search_path_before_sysroot() {
        let sysroot = create_sysroot_with_lib("Foo");
        let search_dir = create_search_dir_with_lib("Foo");
        let searcher = LibrarySearcher::new(
            Some(sysroot.path().to_path_buf()),
            vec![SearchPath { kind: SearchPathKind::Bengal, path: search_dir.path().to_path_buf() }],
        );
        let found = searcher.find_bengalmod("Foo").unwrap();
        // Should find in -L bengal= path, not sysroot
        assert_eq!(found, search_dir.path().join("Foo.bengalmod"));
    }

    #[test]
    fn returns_none_when_not_found() {
        let searcher = LibrarySearcher::new(None, vec![]);
        assert!(searcher.find_bengalmod("NonExistent").is_none());
    }

    #[test]
    fn native_search_paths_separated() {
        let searcher = LibrarySearcher::new(
            None,
            vec![
                SearchPath { kind: SearchPathKind::Native, path: PathBuf::from("/usr/lib") },
                SearchPath { kind: SearchPathKind::Bengal, path: PathBuf::from("/opt/bengal/lib") },
            ],
        );
        assert_eq!(searcher.native_search_paths(), &[PathBuf::from("/usr/lib")]);
        assert_eq!(searcher.bengal_search_paths(), &[PathBuf::from("/opt/bengal/lib")]);
    }

    #[test]
    fn sysroot_resolution_silent_fallback_on_missing_dir() {
        let dir = TempDir::new().unwrap();
        // sysroot exists but lib/bengallib/<target>/lib/ does not
        let searcher = LibrarySearcher::new(Some(dir.path().to_path_buf()), vec![]);
        // Should silently have no sysroot search path
        assert!(searcher.find_bengalmod("Core").is_none());
    }

    #[test]
    fn multiple_bengal_search_paths_first_wins() {
        let dir1 = create_search_dir_with_lib("Dup");
        let dir2 = create_search_dir_with_lib("Dup");
        let searcher = LibrarySearcher::new(
            None,
            vec![
                SearchPath { kind: SearchPathKind::Bengal, path: dir1.path().to_path_buf() },
                SearchPath { kind: SearchPathKind::Bengal, path: dir2.path().to_path_buf() },
            ],
        );
        let found = searcher.find_bengalmod("Dup").unwrap();
        assert_eq!(found, dir1.path().join("Dup.bengalmod"));
    }
}
```

- [ ] **Step 2: Register the module**

In `src/lib.rs`, add after line 11 (`pub mod suggest;`):

```rust
pub mod sysroot;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib sysroot::tests`
Expected: FAIL — all tests panic with `todo!()`

- [ ] **Step 4: Implement `target_triple`**

```rust
fn target_triple() -> String {
    inkwell::targets::TargetMachine::get_default_triple()
        .as_str()
        .to_string_lossy()
        .into_owned()
}
```

- [ ] **Step 5: Implement `resolve_sysroot`**

```rust
fn resolve_sysroot(override_path: Option<PathBuf>) -> Option<PathBuf> {
    let sysroot = match override_path {
        Some(path) => path,
        None => {
            let exe = std::env::current_exe().ok()?;
            // <sysroot>/bin/bengal → <sysroot>
            exe.parent()?.parent()?.to_path_buf()
        }
    };
    let lib_path = Self::sysroot_lib_path(&sysroot);
    if lib_path.is_dir() {
        Some(sysroot)
    } else {
        None
    }
}
```

- [ ] **Step 6: Implement `new` and `find_bengalmod`**

```rust
pub fn new(sysroot_override: Option<PathBuf>, search_paths: Vec<SearchPath>) -> Self {
    let mut bengal_search_paths = Vec::new();
    let mut native_search_paths = Vec::new();

    for sp in search_paths {
        match sp.kind {
            SearchPathKind::Bengal => bengal_search_paths.push(sp.path),
            SearchPathKind::Native => native_search_paths.push(sp.path),
        }
    }

    // Sysroot appended last (lowest priority)
    if let Some(sysroot) = Self::resolve_sysroot(sysroot_override) {
        bengal_search_paths.push(Self::sysroot_lib_path(&sysroot));
    }

    Self {
        bengal_search_paths,
        native_search_paths,
    }
}

pub fn find_bengalmod(&self, name: &str) -> Option<PathBuf> {
    let filename = format!("{}.bengalmod", name);
    for dir in &self.bengal_search_paths {
        let candidate = dir.join(&filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib sysroot::tests`
Expected: all 7 tests PASS

- [ ] **Step 8: Run clippy**

Run: `cargo clippy --lib`
Expected: no warnings

- [ ] **Step 9: Commit**

```bash
git add src/sysroot.rs src/lib.rs
git commit -m "feat(sysroot): add LibrarySearcher with sysroot resolution and search paths"
```

---

### Task 2: CLI Parsing — `--sysroot` and `-L` Flags

**Files:**
- Modify: `src/main.rs:7-45` — add parse_search_path, --sysroot, -L fields

- [ ] **Step 1: Add `-L` parser function**

In `src/main.rs`, add after the `parse_dep` function (after line 15):

```rust
fn parse_search_path(s: &str) -> std::result::Result<(String, PathBuf), String> {
    let (kind, path) = s
        .split_once('=')
        .ok_or_else(|| {
            format!(
                "unsupported -L form: expected '-L bengal=<path>' or '-L native=<path>', got '-L {}'",
                s
            )
        })?;
    match kind {
        "bengal" | "native" => Ok((kind.to_string(), PathBuf::from(path))),
        _ => Err(format!(
            "unsupported -L kind '{}': expected 'bengal' or 'native'",
            kind
        )),
    }
}
```

- [ ] **Step 2: Add `--sysroot` and `-L` to Compile variant**

In the `Command::Compile` variant (after the `deps` field, line 35), add:

```rust
        /// Sysroot path override
        #[arg(long)]
        sysroot: Option<PathBuf>,
        /// Library search path: -L bengal=<path> or -L native=<path>
        #[arg(short = 'L', value_parser = parse_search_path)]
        search_paths: Vec<(String, PathBuf)>,
```

- [ ] **Step 3: Update the match arm to destructure new fields**

Update `Command::Compile` destructuring at line 63-66 to include the new fields:

```rust
        Command::Compile {
            file,
            emit_bir,
            deps,
            sysroot,
            search_paths,
        } => {
```

- [ ] **Step 4: Construct `LibrarySearcher` after parsing**

After the `diag` construction (line 79), before parsing, add:

```rust
            // Build library searcher from --sysroot and -L flags
            let lib_search_paths: Vec<bengal::sysroot::SearchPath> = search_paths
                .into_iter()
                .map(|(kind, path)| {
                    let kind = match kind.as_str() {
                        "bengal" => bengal::sysroot::SearchPathKind::Bengal,
                        "native" => bengal::sysroot::SearchPathKind::Native,
                        _ => unreachable!(),
                    };
                    bengal::sysroot::SearchPath { kind, path }
                })
                .collect();
            let library_searcher = bengal::sysroot::LibrarySearcher::new(sysroot, lib_search_paths);
```

- [ ] **Step 5: Verify compilation**

Run: `cargo build`
Expected: compiles without errors. `library_searcher` will have an "unused" warning — this is expected and will be resolved in Task 3.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): add --sysroot and -L flags for library search paths"
```

---

### Task 3: Pipeline Integration — `pre_scan_imports` and Auto-Discovery

**Files:**
- Modify: `src/pipeline.rs` — add `pre_scan_imports` function
- Modify: `src/main.rs:83-93` — call `pre_scan_imports` after explicit dep loading

- [ ] **Step 1: Add `pre_scan_imports` to `src/pipeline.rs`**

Add after the `load_external_dep` function (after line 568):

```rust
/// Scan all parsed modules for `import` statements with `PathPrefix::Named` prefix,
/// and auto-discover unknown dependencies from the library searcher.
/// Returns additional `ExternalDep`s found via search paths (not including explicit --dep ones).
pub fn pre_scan_imports(
    graph: &crate::package::ModuleGraph,
    explicit_dep_names: &std::collections::HashSet<String>,
    searcher: &crate::sysroot::LibrarySearcher,
) -> Result<Vec<ExternalDep>, crate::error::PipelineError> {
    use crate::parser::ast::PathPrefix;

    let mut discovered_names = std::collections::HashSet::new();
    let mut discovered_deps = Vec::new();

    // Collect all top-level import names from PathPrefix::Named
    for (_mod_path, mod_info) in &graph.modules {
        for import_decl in &mod_info.ast.import_decls {
            if let PathPrefix::Named(name) = &import_decl.prefix {
                if !explicit_dep_names.contains(name) && !discovered_names.contains(name) {
                    if let Some(path) = searcher.find_bengalmod(name) {
                        let dep = load_external_dep(name, &path)?;
                        discovered_names.insert(name.clone());
                        discovered_deps.push(dep);
                    }
                }
            }
        }
    }

    Ok(discovered_deps)
}
```

- [ ] **Step 2: Integrate `pre_scan_imports` in `src/main.rs`**

In `src/main.rs`, after the explicit dep loading loop (after line 93, before `source_map`), add:

```rust
            // Auto-discover dependencies from search paths
            match bengal::pipeline::pre_scan_imports(
                &parsed.graph,
                &seen_dep_names,
                &library_searcher,
            ) {
                Ok(discovered) => external_deps.extend(discovered),
                Err(e) => return Err(Report::new(e.into_diagnostic())),
            }
```

- [ ] **Step 3: Verify compilation and tests**

Run: `cargo build && cargo test`
Expected: compiles, all existing tests pass. No behavioral change yet since tests don't use search paths.

- [ ] **Step 4: Commit**

```bash
git add src/pipeline.rs src/main.rs
git commit -m "feat(pipeline): add pre_scan_imports for auto-discovery via search paths"
```

---

### Task 4: `-L native=` Support in Linker

**Files:**
- Modify: `src/codegen/llvm.rs:311-322` — extend `link_objects` signature
- Modify: `src/pipeline.rs:458-518` — extend `link` to accept and pass native search paths
- Modify: `src/lib.rs:39` — pass `&[]` for native_search_paths
- Modify: `src/main.rs` — pass `library_searcher.native_search_paths()` to `link`

- [ ] **Step 1: Extend `link_objects` in `src/codegen/llvm.rs`**

Replace the `link_objects` function (lines 311-322):

```rust
pub fn link_objects(
    obj_files: &[std::path::PathBuf],
    output: &std::path::Path,
    native_search_paths: &[std::path::PathBuf],
) -> Result<()> {
    let mut cmd = std::process::Command::new("cc");
    cmd.args(obj_files.iter().map(|p| p.as_os_str()));
    for path in native_search_paths {
        cmd.arg("-L").arg(path);
    }
    cmd.arg("-o").arg(output);
    let status = cmd
        .status()
        .map_err(|e| codegen_err(format!("linker failed: {}", e)))?;
    if !status.success() {
        return Err(codegen_err("linker failed"));
    }
    Ok(())
}
```

- [ ] **Step 2: Extend `pipeline::link` signature**

Update the `link` function signature in `src/pipeline.rs` (line 458):

```rust
pub fn link(
    compiled: CompiledPackage,
    external_objects: &HashMap<ModulePath, Vec<u8>>,
    output_path: &Path,
    native_search_paths: &[PathBuf],
) -> Result<(), crate::error::PipelineError> {
```

Ensure `PathBuf` is imported — update the existing import at the top of `pipeline.rs` from `use std::path::Path;` to:

```rust
use std::path::{Path, PathBuf};
```

And update the `link_objects` call (at the `crate::codegen::link_objects` line):

```rust
    crate::codegen::link_objects(&obj_files, output_path, native_search_paths)
        .map_err(|e| crate::error::PipelineError::package("link", e))?;
```

Note: `codegen/mod.rs` re-exports `link_objects` from `llvm.rs` — the signature change propagates automatically, no changes needed in `mod.rs`.

- [ ] **Step 3: Update `src/lib.rs` call site**

In `src/lib.rs`, update line 39:

```rust
    pipeline::link(compiled, &ext_objects, output_path, &[])
```

- [ ] **Step 4: Update `src/main.rs` call site**

Update the `link` call in `src/main.rs` (line 157):

```rust
            bengal::pipeline::link(
                compiled,
                &ext_objects,
                &exe_path,
                library_searcher.native_search_paths(),
            )
            .map_err(|e| Report::new(e.into_diagnostic()))?;
```

- [ ] **Step 5: Update test helper `compile_and_run_with_deps`**

In `tests/separate_compilation.rs`, update the `link` call at line 39:

```rust
    bengal::pipeline::link(compiled, &ext_objects, &exe_path, &[]).unwrap();
```

- [ ] **Step 6: Verify compilation and all tests pass**

Run: `cargo build && cargo test`
Expected: compiles, all existing tests pass

- [ ] **Step 7: Commit**

```bash
git add src/codegen/llvm.rs src/pipeline.rs src/lib.rs src/main.rs tests/separate_compilation.rs
git commit -m "feat(linker): add native_search_paths support to link pipeline"
```

---

### Task 5: Validate `-L bengal=` Non-Existent Path

**Files:**
- Modify: `src/main.rs` — add path existence check for `-L bengal=` paths

- [ ] **Step 1: Add `user_bengal_search_paths` to `LibrarySearcher`**

The sysroot path is also in `bengal_search_paths`, so we need to distinguish user-provided paths from it.

In `src/sysroot.rs`, add a field to track count of user paths:

```rust
pub struct LibrarySearcher {
    bengal_search_paths: Vec<PathBuf>,
    native_search_paths: Vec<PathBuf>,
    user_bengal_path_count: usize,
}
```

Update `new`:

```rust
    let user_bengal_path_count = bengal_search_paths.len();

    // Sysroot appended last (lowest priority)
    if let Some(sysroot) = Self::resolve_sysroot(sysroot_override) {
        bengal_search_paths.push(Self::sysroot_lib_path(&sysroot));
    }

    Self {
        bengal_search_paths,
        native_search_paths,
        user_bengal_path_count,
    }
```

Add accessor:

```rust
    /// Returns only user-specified -L bengal= paths (excludes sysroot).
    pub fn user_bengal_search_paths(&self) -> &[PathBuf] {
        &self.bengal_search_paths[..self.user_bengal_path_count]
    }
```

- [ ] **Step 2: Add validation in `src/main.rs`**

In `src/main.rs`, after the `LibrarySearcher::new(...)` call, add:

```rust
            // Validate -L bengal= paths exist (user-provided only, not sysroot)
            for path in library_searcher.user_bengal_search_paths() {
                if !path.is_dir() {
                    return Err(miette::miette!(
                        "-L bengal= path '{}' does not exist or is not a directory",
                        path.display()
                    ));
                }
            }
```

- [ ] **Step 3: Run tests**

Run: `cargo build && cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/sysroot.rs src/main.rs
git commit -m "feat(cli): validate -L bengal= paths exist"
```

---

### Task 6: Integration Tests — Sysroot Auto-Discovery

**Files:**
- Create: `tests/sysroot.rs` — integration test file for sysroot and search path tests

This task uses the existing `compile_lib` pattern from `tests/separate_compilation.rs` to create `.bengalmod` files, then tests discovery through search paths.

- [ ] **Step 1: Create `tests/sysroot.rs` with test helper**

```rust
mod common;

use std::path::{Path, PathBuf};

/// Compile a library source into a .bengalmod file in the given directory.
fn compile_lib(name: &str, source: &str, dir: &Path) -> PathBuf {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(name, source).unwrap();
    let analyzed = bengal::pipeline::analyze(parsed, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    let optimized = bengal::pipeline::optimize(lowered);
    let emit_data = bengal::pipeline::EmitData::from_lowered(&optimized);
    bengal::pipeline::emit_package_bengalmod(&emit_data, dir);
    dir.join(format!("{}.bengalmod", name))
}

/// Get the LLVM target triple for constructing sysroot paths.
fn target_triple() -> String {
    inkwell::targets::TargetMachine::get_default_triple()
        .as_str()
        .to_string_lossy()
        .into_owned()
}

/// Create a sysroot directory structure and place a .bengalmod file in it.
fn create_test_sysroot(lib_name: &str, lib_source: &str) -> (tempfile::TempDir, PathBuf) {
    let sysroot_dir = tempfile::TempDir::new().unwrap();
    let triple = target_triple();
    let lib_dir = sysroot_dir
        .path()
        .join("lib")
        .join("bengallib")
        .join(&triple)
        .join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();

    // Compile the library into the sysroot lib directory
    let bengalmod_path = compile_lib(lib_name, lib_source, &lib_dir);
    (sysroot_dir, bengalmod_path)
}

/// Compile app source with a sysroot and search paths, link, run, return exit code.
fn compile_and_run_with_searcher(
    source: &str,
    explicit_deps: &[(&str, &Path)],
    sysroot: Option<PathBuf>,
    search_paths: Vec<bengal::sysroot::SearchPath>,
) -> i32 {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source("app", source).unwrap();

    // Load explicit deps
    let mut external_deps: Vec<bengal::pipeline::ExternalDep> = explicit_deps
        .iter()
        .map(|(name, path)| bengal::pipeline::load_external_dep(name, path).unwrap())
        .collect();
    let explicit_names: std::collections::HashSet<String> =
        explicit_deps.iter().map(|(n, _)| n.to_string()).collect();

    // Build searcher and auto-discover
    let searcher = bengal::sysroot::LibrarySearcher::new(sysroot, search_paths);
    let discovered =
        bengal::pipeline::pre_scan_imports(&parsed.graph, &explicit_names, &searcher).unwrap();
    external_deps.extend(discovered);

    let analyzed =
        bengal::pipeline::analyze_with_deps(parsed, &external_deps, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    let mut lowered = lowered;
    bengal::pipeline::merge_external_deps(&mut lowered, &external_deps);
    let optimized = bengal::pipeline::optimize(lowered);
    let mono = bengal::pipeline::monomorphize(optimized, &mut diag).unwrap();
    let compiled = bengal::pipeline::codegen(mono, &mut diag).unwrap();

    let ext_objects = bengal::pipeline::collect_external_objects(&external_deps);
    let link_dir = tempfile::TempDir::new().unwrap();
    let exe_path = link_dir.path().join("test_exe");
    bengal::pipeline::link(compiled, &ext_objects, &exe_path, searcher.native_search_paths())
        .unwrap();

    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run compiled binary");
    output.status.code().unwrap_or(-1)
}
```

- [ ] **Step 2: Add sysroot auto-discovery test**

```rust
#[test]
fn sysroot_auto_discovery() {
    let (sysroot, _) = create_test_sysroot(
        "core",
        r#"
        public func core_version() -> Int32 { return 1; }
        func main() -> Int32 { return 0; }
        "#,
    );

    let result = compile_and_run_with_searcher(
        r#"
        import core::core_version;
        func main() -> Int32 {
            return core_version();
        }
        "#,
        &[],
        Some(sysroot.path().to_path_buf()),
        vec![],
    );
    assert_eq!(result, 1);
}
```

- [ ] **Step 3: Add `-L bengal=` discovery test**

```rust
#[test]
fn bengal_search_path_discovery() {
    let dir = tempfile::TempDir::new().unwrap();
    compile_lib(
        "math",
        r#"
        public func triple(x: Int32) -> Int32 { return x + x + x; }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_searcher(
        r#"
        import math::triple;
        func main() -> Int32 {
            return triple(3);
        }
        "#,
        &[],
        None,
        vec![bengal::sysroot::SearchPath {
            kind: bengal::sysroot::SearchPathKind::Bengal,
            path: dir.path().to_path_buf(),
        }],
    );
    assert_eq!(result, 9);
}
```

- [ ] **Step 4: Add `-L bengal=` priority over sysroot test**

```rust
#[test]
fn bengal_search_path_priority_over_sysroot() {
    // sysroot version returns 1
    let (sysroot, _) = create_test_sysroot(
        "prio",
        r#"
        public func prio_val() -> Int32 { return 1; }
        func main() -> Int32 { return 0; }
        "#,
    );

    // -L bengal= version returns 2
    let search_dir = tempfile::TempDir::new().unwrap();
    compile_lib(
        "prio",
        r#"
        public func prio_val() -> Int32 { return 2; }
        func main() -> Int32 { return 0; }
        "#,
        search_dir.path(),
    );

    let result = compile_and_run_with_searcher(
        r#"
        import prio::prio_val;
        func main() -> Int32 {
            return prio_val();
        }
        "#,
        &[],
        Some(sysroot.path().to_path_buf()),
        vec![bengal::sysroot::SearchPath {
            kind: bengal::sysroot::SearchPathKind::Bengal,
            path: search_dir.path().to_path_buf(),
        }],
    );
    assert_eq!(result, 2, "-L bengal= should take priority over sysroot");
}
```

- [ ] **Step 5: Add explicit `--dep` coexistence test**

```rust
#[test]
fn explicit_dep_coexists_with_sysroot() {
    let (sysroot, _) = create_test_sysroot(
        "core",
        r#"
        public func core_val() -> Int32 { return 10; }
        func main() -> Int32 { return 0; }
        "#,
    );

    let dep_dir = tempfile::TempDir::new().unwrap();
    let dep_path = compile_lib(
        "extra",
        r#"
        public func extra_val() -> Int32 { return 20; }
        func main() -> Int32 { return 0; }
        "#,
        dep_dir.path(),
    );

    let result = compile_and_run_with_searcher(
        r#"
        import core::core_val;
        import extra::extra_val;
        func main() -> Int32 {
            return core_val() + extra_val();
        }
        "#,
        &[("extra", &dep_path)],
        Some(sysroot.path().to_path_buf()),
        vec![],
    );
    assert_eq!(result, 30);
}
```

- [ ] **Step 6: Add malformed sysroot fallback test**

```rust
#[test]
fn malformed_sysroot_falls_back_silently() {
    let dir = tempfile::TempDir::new().unwrap();
    // No lib/bengallib/<target>/lib/ structure — just an empty directory

    let searcher = bengal::sysroot::LibrarySearcher::new(
        Some(dir.path().to_path_buf()),
        vec![],
    );
    assert!(
        searcher.find_bengalmod("Core").is_none(),
        "malformed sysroot should silently produce no results"
    );
}
```

- [ ] **Step 7: Add nonexistent library import error test**

```rust
#[test]
fn import_nonexistent_library_fails() {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(
        "app",
        r#"
        import nonexistent::foo;
        func main() -> Int32 { return foo(); }
        "#,
    )
    .unwrap();

    let searcher = bengal::sysroot::LibrarySearcher::new(None, vec![]);
    let explicit_names = std::collections::HashSet::new();
    let discovered =
        bengal::pipeline::pre_scan_imports(&parsed.graph, &explicit_names, &searcher).unwrap();

    // No deps discovered — analysis should fail on unresolved import
    let result = bengal::pipeline::analyze_with_deps(parsed, &discovered, &mut diag);
    assert!(result.is_err(), "import of nonexistent library should fail");
}
```

- [ ] **Step 8: Add multiple `-L bengal=` first-match-wins test**

```rust
#[test]
fn multiple_bengal_search_paths_first_wins() {
    // dir1 version returns 1
    let dir1 = tempfile::TempDir::new().unwrap();
    compile_lib(
        "dup",
        r#"
        public func dup_val() -> Int32 { return 1; }
        func main() -> Int32 { return 0; }
        "#,
        dir1.path(),
    );

    // dir2 version returns 2
    let dir2 = tempfile::TempDir::new().unwrap();
    compile_lib(
        "dup",
        r#"
        public func dup_val() -> Int32 { return 2; }
        func main() -> Int32 { return 0; }
        "#,
        dir2.path(),
    );

    let result = compile_and_run_with_searcher(
        r#"
        import dup::dup_val;
        func main() -> Int32 {
            return dup_val();
        }
        "#,
        &[],
        None,
        vec![
            bengal::sysroot::SearchPath {
                kind: bengal::sysroot::SearchPathKind::Bengal,
                path: dir1.path().to_path_buf(),
            },
            bengal::sysroot::SearchPath {
                kind: bengal::sysroot::SearchPathKind::Bengal,
                path: dir2.path().to_path_buf(),
            },
        ],
    );
    assert_eq!(result, 1, "first -L bengal= path should win");
}
```

- [ ] **Step 9: Run all tests**

Run: `cargo test`
Expected: all tests PASS (existing + new)

- [ ] **Step 10: Commit**

```bash
git add tests/sysroot.rs
git commit -m "test(sysroot): add integration tests for sysroot and search path discovery"
```

---

### Task 7: TODO.md Updates

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Mark item 4 as implemented and update future items**

Update the "4. Sysroot / ライブラリ検索パス" section in `TODO.md` to reflect that the core implementation is done, keeping the "将来改善" subsection.

- [ ] **Step 2: Commit**

```bash
git add TODO.md
git commit -m "docs: update TODO.md for sysroot implementation"
```
