# Pipeline Restructuring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decompose the monolithic compilation pipeline into discrete stage functions with explicit intermediate types, unifying single-file and package compilation.

**Architecture:** Functional pipeline — each stage is an independent function consuming the previous stage's output. New `src/pipeline.rs` module holds all stage functions and intermediate data types. Single-file compilation is unified as "package with one module."

**Tech Stack:** Rust, thiserror, existing Bengal compiler infrastructure (lexer, parser, semantic, bir, codegen)

**Spec:** `docs/superpowers/specs/2026-03-26-pipeline-restructuring-design.md`

---

### Task 1: Add PipelineError to error.rs

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Add PipelineError struct**

Add after the existing `BengalError` enum (after line 31):

```rust
#[derive(Debug, Error)]
#[error("{phase} error in {module}: {source_error}")]
pub struct PipelineError {
    pub phase: &'static str,
    pub module: String,
    pub source_code: Option<String>,
    pub source_error: BengalError,
}

impl PipelineError {
    pub fn new(phase: &'static str, module: &str, source: Option<&str>, err: BengalError) -> Self {
        PipelineError {
            phase,
            module: module.to_string(),
            source_code: source.map(|s| s.to_string()),
            source_error: err,
        }
    }

    pub fn package(phase: &'static str, err: BengalError) -> Self {
        Self::new(phase, "<package>", None, err)
    }

    pub fn into_diagnostic(self) -> BengalDiagnostic {
        let filename = self.module.clone();
        let source = self.source_code.unwrap_or_default();
        self.source_error.into_diagnostic(&filename, &source)
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: no errors (PipelineError is defined but not yet used)

- [ ] **Step 3: Commit**

```bash
git add src/error.rs
git commit -m "Add PipelineError struct for pipeline error context"
```

---

### Task 2: Add ModuleGraph::from_source() to package.rs

**Files:**
- Modify: `src/package.rs`

- [ ] **Step 1: Write the test**

Add to the existing `#[cfg(test)] mod tests` block in `package.rs`:

```rust
#[test]
fn module_graph_from_source() {
    let source = "func main() -> Int32 { return 42; }";
    let graph = ModuleGraph::from_source("test", source).unwrap();
    assert_eq!(graph.modules.len(), 1);
    let root = graph.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(root.source, source);
    assert!(root.path.is_root());
    assert_eq!(root.ast.functions.len(), 1);
    assert_eq!(root.ast.functions[0].name, "main");
}

#[test]
fn module_graph_from_source_lex_error() {
    let result = ModuleGraph::from_source("test", "func @@@");
    assert!(result.is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib package::tests::module_graph_from_source -- --exact 2>&1 | tail -5`
Expected: FAIL — `from_source` method does not exist

- [ ] **Step 3: Implement ModuleGraph::from_source**

Add this `impl` block to `ModuleGraph` in `package.rs` (after the struct definition at line 96):

```rust
impl ModuleGraph {
    /// Create a single-module graph from in-memory source code.
    /// Lexes, parses, and wraps the AST in a root module.
    pub fn from_source(name: &str, source: &str) -> Result<ModuleGraph> {
        let tokens = crate::lexer::tokenize(source)?;
        let ast = crate::parser::parse(tokens)?;
        let mut modules = HashMap::new();
        modules.insert(
            ModulePath::root(),
            ModuleInfo {
                path: ModulePath::root(),
                file_path: std::path::PathBuf::from(format!("{}.bengal", name)),
                source: source.to_string(),
                ast,
            },
        );
        Ok(ModuleGraph { modules })
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib package::tests::module_graph_from_source 2>&1 | tail -5`
Expected: PASS

Run: `cargo test --lib package::tests::module_graph_from_source_lex_error 2>&1 | tail -5`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/package.rs
git commit -m "Add ModuleGraph::from_source() for single-file and in-memory compilation"
```

---

### Task 3: Create pipeline.rs with intermediate data types

**Files:**
- Create: `src/pipeline.rs`
- Modify: `src/lib.rs` (add `pub mod pipeline;`)

- [ ] **Step 1: Create pipeline.rs with type definitions**

Create `src/pipeline.rs`:

```rust
use std::collections::HashMap;
use std::path::Path;

use crate::bir::instruction::{BirModule, BirType};
use crate::bir::mono::MonoCollectResult;
use crate::error::{BengalError, PipelineError};
use crate::package::{ModuleGraph, ModuleInfo, ModulePath};
use crate::parser::ast::{NodeId, TypeAnnotation};
use crate::semantic::{PackageSemanticInfo, SemanticInfo};

/// Output of the `parse` stage.
pub struct ParsedPackage {
    pub package_name: String,
    pub graph: ModuleGraph,
}

/// Output of the `analyze` stage.
pub struct AnalyzedPackage {
    pub package_name: String,
    pub graph: ModuleGraph,
    pub inferred_maps: HashMap<ModulePath, HashMap<NodeId, Vec<TypeAnnotation>>>,
    pub pkg_sem_info: PackageSemanticInfo,
}

/// Output of the `lower` stage.
pub struct LoweredPackage {
    pub package_name: String,
    pub modules: HashMap<ModulePath, LoweredModule>,
    pub sources: HashMap<ModulePath, String>,
}

pub struct LoweredModule {
    pub bir: BirModule,
    pub is_entry: bool,
}

/// Output of the `monomorphize` stage.
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

/// Output of the `codegen` stage.
pub struct CompiledPackage {
    pub object_bytes: HashMap<ModulePath, Vec<u8>>,
}

/// Output of `compile_to_bir` / `compile_source_to_bir`.
pub struct BirOutput {
    pub modules: HashMap<ModulePath, LoweredModule>,
    pub bir_texts: HashMap<ModulePath, String>,
}
```

- [ ] **Step 2: Add module declaration to lib.rs**

Add `pub mod pipeline;` after the existing module declarations in `src/lib.rs` (after line 8):

```rust
pub mod pipeline;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: compiles (possibly warnings about unused fields — that's fine)

- [ ] **Step 4: Commit**

```bash
git add src/pipeline.rs src/lib.rs
git commit -m "Add pipeline module with intermediate data types"
```

---

### Task 4: Implement parse stage

**Files:**
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Write the test**

Add at the bottom of `src/pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("main.bengal");
        std::fs::write(&path, "func main() -> Int32 { return 1; }").unwrap();
        let result = parse(&path).unwrap();
        assert_eq!(result.graph.modules.len(), 1);
        assert!(!result.package_name.is_empty());
    }

    #[test]
    fn parse_from_source() {
        let result = parse_source("test", "func main() -> Int32 { return 1; }").unwrap();
        assert_eq!(result.package_name, "test");
        assert_eq!(result.graph.modules.len(), 1);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pipeline::tests::parse_single_file -- --exact 2>&1 | tail -5`
Expected: FAIL — `parse` function not found

- [ ] **Step 3: Implement parse and parse_source**

Add to `src/pipeline.rs` (before the `#[cfg(test)]` block):

```rust
use std::sync::atomic::{AtomicU64, Ordering};

static BUILD_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Parse from a file path. Detects single-file vs package mode.
pub fn parse(entry_path: &Path) -> Result<ParsedPackage, PipelineError> {
    let entry_path = entry_path
        .canonicalize()
        .map_err(|e| PipelineError::package("parse", BengalError::PackageError {
            message: format!("failed to resolve path '{}': {}", entry_path.display(), e),
        }))?;
    let entry_dir = entry_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    match crate::package::find_package_root(entry_dir)
        .map_err(|e| PipelineError::package("parse", e))?
    {
        Some(root) => {
            let config = crate::package::load_package(&root)
                .map_err(|e| PipelineError::package("parse", e))?;
            let graph = crate::package::build_module_graph(&root.join(&config.package.entry))
                .map_err(|e| PipelineError::package("parse", e))?;
            Ok(ParsedPackage {
                package_name: config.package.name,
                graph,
            })
        }
        None => {
            // Single-file mode: read, lex, parse, wrap in ModuleGraph
            let source = std::fs::read_to_string(&entry_path)
                .map_err(|e| PipelineError::package("parse", BengalError::PackageError {
                    message: format!("failed to read '{}': {}", entry_path.display(), e),
                }))?;
            let name = entry_path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("bengal");
            let graph = ModuleGraph::from_source(name, &source)
                .map_err(|e| PipelineError::new(
                    "parse",
                    &entry_path.display().to_string(),
                    Some(&source),
                    e,
                ))?;
            let package_name = entry_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("bengal")
                .to_string();
            Ok(ParsedPackage {
                package_name,
                graph,
            })
        }
    }
}

/// Parse from an in-memory source string (for tests and eval).
pub fn parse_source(name: &str, source: &str) -> Result<ParsedPackage, PipelineError> {
    let graph = ModuleGraph::from_source(name, source)
        .map_err(|e| PipelineError::new("parse", name, Some(source), e))?;
    Ok(ParsedPackage {
        package_name: name.to_string(),
        graph,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib pipeline::tests -- 2>&1 | tail -10`
Expected: 2 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/pipeline.rs
git commit -m "Implement parse and parse_source pipeline stages"
```

---

### Task 5: Implement analyze stage

**Files:**
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Write the test**

Add to the `tests` module in `pipeline.rs`:

```rust
#[test]
fn analyze_single_file() {
    let parsed = parse_source("test", "func main() -> Int32 { return 1; }").unwrap();
    let analyzed = analyze(parsed).unwrap();
    assert_eq!(analyzed.inferred_maps.len(), 1);
}

#[test]
fn analyze_generic_function() {
    let source = r#"
        func identity<T>(x: T) -> T { return x; }
        func main() -> Int32 { return identity<Int32>(42); }
    "#;
    let parsed = parse_source("test", source).unwrap();
    let analyzed = analyze(parsed).unwrap();
    // Inferred maps should contain entries for the generic call site
    let root_map = analyzed.inferred_maps.get(&ModulePath::root()).unwrap();
    assert!(!root_map.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pipeline::tests::analyze_single_file -- --exact 2>&1 | tail -5`
Expected: FAIL — `analyze` function not found

- [ ] **Step 3: Implement analyze**

Add to `src/pipeline.rs`:

```rust
/// Semantic analysis: validate generics, run pre-mono inference, cross-module resolution.
pub fn analyze(parsed: ParsedPackage) -> Result<AnalyzedPackage, PipelineError> {
    // Validate generics for all modules
    for (mod_path, mod_info) in &parsed.graph.modules {
        crate::semantic::validate_generics(&mod_info.ast)
            .map_err(|e| PipelineError::new(
                "analyze",
                &mod_path.to_string(),
                Some(&mod_info.source),
                e,
            ))?;
    }

    // Run pre-mono type inference per module
    let mut inferred_maps: HashMap<ModulePath, HashMap<NodeId, Vec<TypeAnnotation>>> =
        HashMap::new();
    for (mod_path, mod_info) in &parsed.graph.modules {
        let (inferred, _pre_mono_sem_info) =
            crate::semantic::analyze_pre_mono_lenient(&mod_info.ast)
                .map_err(|e| PipelineError::new(
                    "analyze",
                    &mod_path.to_string(),
                    Some(&mod_info.source),
                    e,
                ))?;
        let inferred_map: HashMap<NodeId, Vec<TypeAnnotation>> = inferred
            .map
            .into_iter()
            .map(|(id, site)| (id, site.type_args))
            .collect();
        inferred_maps.insert(mod_path.clone(), inferred_map);
    }

    // Cross-module semantic analysis
    let pkg_sem_info = crate::semantic::analyze_package(&parsed.graph, &parsed.package_name)
        .map_err(|e| PipelineError::package("analyze", e))?;

    Ok(AnalyzedPackage {
        package_name: parsed.package_name,
        graph: parsed.graph,
        inferred_maps,
        pkg_sem_info,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib pipeline::tests::analyze_ -- 2>&1 | tail -10`
Expected: 2 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/pipeline.rs
git commit -m "Implement analyze pipeline stage"
```

---

### Task 6: Implement lower stage with build_name_map helper

**Files:**
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Write the test**

Add to the `tests` module in `pipeline.rs`:

```rust
#[test]
fn lower_single_file() {
    let parsed = parse_source("test", "func main() -> Int32 { return 1; }").unwrap();
    let analyzed = analyze(parsed).unwrap();
    let lowered = lower(analyzed).unwrap();
    assert_eq!(lowered.modules.len(), 1);
    let root = lowered.modules.get(&ModulePath::root()).unwrap();
    assert!(root.is_entry);
    assert!(!root.bir.functions.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pipeline::tests::lower_single_file -- --exact 2>&1 | tail -5`
Expected: FAIL — `lower` function not found

- [ ] **Step 3: Implement build_name_map helper**

Add to `src/pipeline.rs`:

```rust
/// Build name_map: local function/method name -> mangled name.
/// Extracted from the inline block in the old compile_package_to_executable.
fn build_name_map(
    package_name: &str,
    mod_path: &ModulePath,
    mod_info: &ModuleInfo,
    sem_info: &SemanticInfo,
    pkg_sem_info: &PackageSemanticInfo,
) -> HashMap<String, String> {
    let is_entry = mod_path.is_root();
    let module_segments: Vec<&str> = if mod_path.0.is_empty() {
        vec![""]
    } else {
        mod_path.0.iter().map(|s| s.as_str()).collect()
    };

    let mut name_map: HashMap<String, String> = HashMap::new();

    // Local functions
    for func in &mod_info.ast.functions {
        if is_entry && func.name == "main" {
            name_map.insert("main".to_string(), "main".to_string());
        } else {
            let mangled =
                crate::mangle::mangle_function(package_name, &module_segments, &func.name);
            name_map.insert(func.name.clone(), mangled);
        }
    }

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
                let source_segments: Vec<&str> = if source_module.0.is_empty() {
                    vec![""]
                } else {
                    source_module.0.iter().map(|s| s.as_str()).collect()
                };
                for method in &struct_info.methods {
                    let local_mangled = format!("{}_{}", struct_name, method.name);
                    let mangled = crate::mangle::mangle_method(
                        package_name,
                        &source_segments,
                        struct_name,
                        &method.name,
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
                );
                name_map.insert(local_mangled, mangled);
            }
        }
    }

    // Imported functions
    for ((imp_mod, imp_name), source_module) in &pkg_sem_info.import_sources {
        if imp_mod != mod_path {
            continue;
        }
        if sem_info.struct_defs.contains_key(imp_name) {
            continue;
        }
        let source_segments: Vec<&str> = if source_module.0.is_empty() {
            vec![""]
        } else {
            source_module.0.iter().map(|s| s.as_str()).collect()
        };
        let mangled =
            crate::mangle::mangle_function(package_name, &source_segments, imp_name);
        name_map.insert(imp_name.clone(), mangled);
    }

    name_map
}
```

- [ ] **Step 4: Implement lower**

Add to `src/pipeline.rs`:

```rust
/// BIR lowering: build name maps, lower each module's AST to BIR.
pub fn lower(analyzed: AnalyzedPackage) -> Result<LoweredPackage, PipelineError> {
    let mut modules = HashMap::new();
    let mut sources = HashMap::new();

    for (mod_path, mod_info) in &analyzed.graph.modules {
        let sem_info = analyzed
            .pkg_sem_info
            .module_infos
            .get(mod_path)
            .ok_or_else(|| PipelineError::package("lower", BengalError::PackageError {
                message: format!("missing semantic info for module '{}'", mod_path),
            }))?;

        let name_map = build_name_map(
            &analyzed.package_name,
            mod_path,
            mod_info,
            sem_info,
            &analyzed.pkg_sem_info,
        );

        let empty_inferred = HashMap::new();
        let inferred_map = analyzed.inferred_maps.get(mod_path).unwrap_or(&empty_inferred);

        let bir_module = crate::bir::lowering::lower_module_with_inferred(
            &mod_info.ast,
            sem_info,
            &name_map,
            inferred_map,
        )
        .map_err(|e| PipelineError::new(
            "lower",
            &mod_path.to_string(),
            Some(&mod_info.source),
            e,
        ))?;

        sources.insert(mod_path.clone(), mod_info.source.clone());
        modules.insert(
            mod_path.clone(),
            LoweredModule {
                bir: bir_module,
                is_entry: mod_path.is_root(),
            },
        );
    }

    Ok(LoweredPackage {
        package_name: analyzed.package_name,
        modules,
        sources,
    })
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib pipeline::tests::lower_single_file -- --exact 2>&1 | tail -5`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/pipeline.rs
git commit -m "Implement lower pipeline stage with build_name_map helper"
```

---

### Task 7: Implement optimize, monomorphize, codegen, link stages

**Files:**
- Modify: `src/pipeline.rs`

- [ ] **Step 1: Write end-to-end test**

Add to the `tests` module in `pipeline.rs`:

```rust
#[test]
fn full_pipeline_single_file() {
    let parsed = parse_source("test", "func main() -> Int32 { return 42; }").unwrap();
    let analyzed = analyze(parsed).unwrap();
    let lowered = lower(analyzed).unwrap();
    let optimized = optimize(lowered);
    let mono = monomorphize(optimized).unwrap();
    let compiled = codegen(mono).unwrap();
    assert_eq!(compiled.object_bytes.len(), 1);
    let obj = compiled.object_bytes.get(&ModulePath::root()).unwrap();
    assert!(!obj.is_empty());
}

#[test]
fn full_pipeline_with_generics() {
    let source = r#"
        func identity<T>(x: T) -> T { return x; }
        func main() -> Int32 { return identity<Int32>(42); }
    "#;
    let parsed = parse_source("test", source).unwrap();
    let analyzed = analyze(parsed).unwrap();
    let lowered = lower(analyzed).unwrap();
    let optimized = optimize(lowered);
    let mono = monomorphize(optimized).unwrap();
    let compiled = codegen(mono).unwrap();
    assert!(!compiled.object_bytes.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pipeline::tests::full_pipeline_single_file -- --exact 2>&1 | tail -5`
Expected: FAIL — `optimize`/`monomorphize`/`codegen` functions not found

- [ ] **Step 3: Implement optimize**

Add to `src/pipeline.rs`:

```rust
/// BIR optimization: run optimization passes on each module's BIR.
pub fn optimize(mut lowered: LoweredPackage) -> LoweredPackage {
    for module in lowered.modules.values_mut() {
        crate::bir::optimize_module(&mut module.bir);
    }
    lowered
}
```

- [ ] **Step 4: Implement collect_external_functions helper and monomorphize**

Add to `src/pipeline.rs`:

```rust
/// Collect functions called but not defined in a BIR module.
fn collect_external_functions(
    bir: &BirModule,
    mono_result: &MonoCollectResult,
) -> Vec<(String, Vec<BirType>, BirType)> {
    use std::collections::HashSet;
    use crate::bir::instruction::Instruction;

    let defined_funcs: HashSet<String> = bir
        .functions
        .iter()
        .map(|f| f.name.clone())
        .collect();
    let resolved_instance_names: HashSet<String> = mono_result
        .func_instances
        .iter()
        .filter(|inst| defined_funcs.contains(&inst.func_name))
        .map(|inst| inst.mangled_name())
        .collect();

    let mut external_functions = Vec::new();
    let mut seen_externals = HashSet::new();

    for func in &bir.functions {
        // Build value -> type map for this function
        let mut value_types: HashMap<crate::bir::instruction::Value, BirType> = HashMap::new();
        for (val, ty) in &func.params {
            value_types.insert(*val, ty.clone());
        }
        for block in &func.blocks {
            for (val, ty) in &block.params {
                value_types.insert(*val, ty.clone());
            }
            for inst in &block.instructions {
                let (result, ty) = match inst {
                    Instruction::Literal { result, ty, .. } => (*result, ty.clone()),
                    Instruction::BinaryOp { result, ty, .. } => (*result, ty.clone()),
                    Instruction::Compare { result, .. } => (*result, BirType::Bool),
                    Instruction::Not { result, .. } => (*result, BirType::Bool),
                    Instruction::Cast { result, to_ty, .. } => (*result, to_ty.clone()),
                    Instruction::Call { result, ty, .. } => (*result, ty.clone()),
                    Instruction::StructInit { result, ty, .. } => (*result, ty.clone()),
                    Instruction::FieldGet { result, ty, .. } => (*result, ty.clone()),
                    Instruction::FieldSet { result, ty, .. } => (*result, ty.clone()),
                    Instruction::ArrayInit { result, ty, .. } => (*result, ty.clone()),
                    Instruction::ArrayGet { result, ty, .. } => (*result, ty.clone()),
                    Instruction::ArraySet { result, ty, .. } => (*result, ty.clone()),
                };
                value_types.insert(result, ty);
            }
        }

        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::Call {
                    func_name,
                    args,
                    ty,
                    ..
                } = inst
                    && !defined_funcs.contains(func_name)
                    && !resolved_instance_names.contains(func_name)
                    && !seen_externals.contains(func_name)
                {
                    let arg_types: Vec<BirType> = args
                        .iter()
                        .map(|arg| {
                            value_types
                                .get(arg)
                                .cloned()
                                .unwrap_or(BirType::I32)
                        })
                        .collect();
                    external_functions.push((func_name.clone(), arg_types, ty.clone()));
                    seen_externals.insert(func_name.clone());
                }
            }
        }
    }

    external_functions
}

/// Monomorphization collection: find all concrete instantiations needed.
pub fn monomorphize(
    lowered: LoweredPackage,
) -> Result<MonomorphizedPackage, PipelineError> {
    let mut modules = HashMap::new();

    for (mod_path, module) in lowered.modules {
        let mono_result = crate::bir::mono::mono_collect(&module.bir, "main");
        let external_functions = collect_external_functions(&module.bir, &mono_result);

        modules.insert(
            mod_path,
            MonomorphizedModule {
                bir: module.bir,
                mono_result,
                external_functions,
                is_entry: module.is_entry,
            },
        );
    }

    Ok(MonomorphizedPackage {
        package_name: lowered.package_name,
        modules,
        sources: lowered.sources,
    })
}
```

- [ ] **Step 5: Implement codegen**

Add to `src/pipeline.rs`:

```rust
/// Code generation: compile each module's BIR to native object code.
pub fn codegen(
    mono: MonomorphizedPackage,
) -> Result<CompiledPackage, PipelineError> {
    let mut object_bytes = HashMap::new();

    for (mod_path, module) in &mono.modules {
        let obj = crate::codegen::compile_module_with_mono(
            &module.bir,
            &module.mono_result,
            &module.external_functions,
        )
        .map_err(|e| PipelineError::new(
            "codegen",
            &mod_path.to_string(),
            mono.sources.get(mod_path).map(|s| s.as_str()),
            e,
        ))?;
        object_bytes.insert(mod_path.clone(), obj);
    }

    Ok(CompiledPackage { object_bytes })
}
```

- [ ] **Step 6: Implement link**

Add to `src/pipeline.rs`:

```rust
/// Link object files into an executable.
pub fn link(compiled: CompiledPackage, output_path: &Path) -> Result<(), PipelineError> {
    let build_id = BUILD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_dir =
        std::env::temp_dir().join(format!("bengal_build_{}_{}", std::process::id(), build_id));
    std::fs::create_dir_all(&temp_dir).map_err(|e| {
        PipelineError::package(
            "link",
            BengalError::PackageError {
                message: format!("failed to create temp dir: {}", e),
            },
        )
    })?;

    let mut obj_files = Vec::new();
    for (mod_path, bytes) in &compiled.object_bytes {
        let obj_name = if mod_path.0.is_empty() {
            "root.o".to_string()
        } else {
            format!("{}.o", mod_path.0.join("_"))
        };
        let obj_path = temp_dir.join(&obj_name);
        std::fs::write(&obj_path, bytes).map_err(|e| {
            PipelineError::package(
                "link",
                BengalError::PackageError {
                    message: format!("failed to write object file: {}", e),
                },
            )
        })?;
        obj_files.push(obj_path);
    }

    crate::codegen::link_objects(&obj_files, output_path)
        .map_err(|e| PipelineError::package("link", e))?;

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib pipeline::tests -- 2>&1 | tail -15`
Expected: all pipeline tests PASS

- [ ] **Step 8: Commit**

```bash
git add src/pipeline.rs
git commit -m "Implement optimize, monomorphize, codegen, link pipeline stages"
```

---

### Task 8: Rewrite lib.rs public API over pipeline

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Rewrite lib.rs**

Replace the contents of `src/lib.rs` (keeping module declarations and tests):

```rust
pub mod bir;
pub mod codegen;
pub mod error;
pub mod lexer;
pub mod mangle;
pub mod package;
pub mod parser;
pub mod pipeline;
pub mod semantic;

use std::collections::HashMap;
use std::path::Path;

use error::Result;
use package::ModulePath;
use pipeline::BirOutput;

/// Compile a Bengal source file (or package) to an executable.
pub fn compile_to_executable(entry_path: &Path, output_path: &Path) -> std::result::Result<(), error::PipelineError> {
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze(parsed)?;
    let lowered = pipeline::lower(analyzed)?;
    let optimized = pipeline::optimize(lowered);
    let mono = pipeline::monomorphize(optimized)?;
    let compiled = pipeline::codegen(mono)?;
    pipeline::link(compiled, output_path)
}

/// Compile a Bengal source file (or package) to BIR output.
pub fn compile_to_bir(entry_path: &Path) -> std::result::Result<BirOutput, error::PipelineError> {
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze(parsed)?;
    let lowered = pipeline::lower(analyzed)?;
    let optimized = pipeline::optimize(lowered);
    let mut bir_texts = HashMap::new();
    let mut modules = HashMap::new();
    for (path, module) in optimized.modules {
        bir_texts.insert(path.clone(), bir::print_module(&module.bir));
        modules.insert(path, module);
    }
    Ok(BirOutput {
        modules,
        bir_texts,
    })
}

/// Compile BIR from an in-memory source string (for eval subcommand).
pub fn compile_source_to_bir(source: &str) -> std::result::Result<BirOutput, error::PipelineError> {
    let parsed = pipeline::parse_source("<eval>", source)?;
    let analyzed = pipeline::analyze(parsed)?;
    let lowered = pipeline::lower(analyzed)?;
    let optimized = pipeline::optimize(lowered);
    let mut bir_texts = HashMap::new();
    let mut modules = HashMap::new();
    for (path, module) in optimized.modules {
        bir_texts.insert(path.clone(), bir::print_module(&module.bir));
        modules.insert(path, module);
    }
    Ok(BirOutput {
        modules,
        bir_texts,
    })
}

/// Compile a file/package to object bytes (no linking).
pub fn compile_to_objects(entry_path: &Path) -> std::result::Result<pipeline::CompiledPackage, error::PipelineError> {
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze(parsed)?;
    let lowered = pipeline::lower(analyzed)?;
    let optimized = pipeline::optimize(lowered);
    let mono = pipeline::monomorphize(optimized)?;
    pipeline::codegen(mono)
}

/// Compile from a source string to object bytes (for integration tests).
pub fn compile_source_to_objects(source: &str) -> Result<Vec<u8>> {
    let parsed = pipeline::parse_source("test", source)
        .map_err(|e| e.source_error)?;
    let analyzed = pipeline::analyze(parsed)
        .map_err(|e| e.source_error)?;
    let lowered = pipeline::lower(analyzed)
        .map_err(|e| e.source_error)?;
    let optimized = pipeline::optimize(lowered);
    let mono = pipeline::monomorphize(optimized)
        .map_err(|e| e.source_error)?;
    let compiled = pipeline::codegen(mono)
        .map_err(|e| e.source_error)?;
    // Return the single module's object bytes
    compiled
        .object_bytes
        .into_values()
        .next()
        .ok_or_else(|| error::BengalError::CodegenError {
            message: "no object code produced".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_source_returns_object() {
        let obj = compile_source_to_objects("func main() -> Int32 { return 1; }").unwrap();
        assert!(
            !obj.is_empty(),
            "compile_source_to_objects must return non-empty object bytes"
        );
    }

    #[test]
    fn test_bir_generic_function_has_type_params() {
        let source = r#"
            func identity<T>(x: T) -> T { return x; }
            func main() -> Int32 { return identity<Int32>(42); }
        "#;
        let output = compile_source_to_bir(source).unwrap();
        let root_text = output.bir_texts.get(&ModulePath::root()).unwrap();
        assert!(
            root_text.contains("identity"),
            "BIR must contain the generic function 'identity'"
        );
        assert!(
            root_text.contains("T"),
            "BIR must contain TypeParam 'T' for the generic function"
        );
    }

    #[test]
    fn test_compile_to_module_reexport() {
        let source = "func main() -> Int32 { return 1; }";
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(tokens).unwrap();
        let (_inferred, sem_info) = semantic::analyze_pre_mono(&program).unwrap();
        let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
        bir::optimize_module(&mut bir_module);

        let context = inkwell::context::Context::create();
        let _module = codegen::compile_to_module(&context, &bir_module).unwrap();
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | head -30`
Expected: compiles (old `compile_source`, `compile_to_bir`, `compile_package_to_executable` are removed)

- [ ] **Step 3: Run lib tests**

Run: `cargo test --lib tests:: -- 2>&1 | tail -10`
Expected: all 3 lib tests PASS

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs
git commit -m "Rewrite lib.rs public API over pipeline stages"
```

---

**Note:** After Task 8, `cargo test` (full suite) will not compile because `tests/common/mod.rs` still references the old API names. Only `cargo test --lib` works until Task 10 updates the test helpers. This is expected.

### Task 9: Update main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Rewrite main.rs**

Replace the contents of `src/main.rs`:

```rust
use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use miette::Report;

#[derive(Parser)]
#[command(name = "bengal", about = "The Bengal compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compile a .bengal file to a native executable
    Compile {
        /// Source file path
        file: PathBuf,
        /// Print BIR text representation
        #[arg(long)]
        emit_bir: bool,
    },
    /// Evaluate a Bengal program and print the result
    Eval {
        /// Program or expression to evaluate
        expr: String,
        /// Print BIR text representation
        #[arg(long)]
        emit_bir: bool,
    },
}

fn run() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Compile { file, emit_bir } => {
            let exe_path = file.with_extension("");
            if exe_path == file {
                return Err(miette::miette!(
                    "input file '{}' has no extension; cannot determine output path",
                    file.display()
                ));
            }

            // Run pipeline once, optionally printing BIR
            let parsed = bengal::pipeline::parse(&file)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let analyzed = bengal::pipeline::analyze(parsed)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let lowered = bengal::pipeline::lower(analyzed)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let optimized = bengal::pipeline::optimize(lowered);

            if emit_bir {
                for (mod_path, module) in &optimized.modules {
                    if optimized.modules.len() > 1 {
                        println!("=== module: {} ===", mod_path);
                    }
                    println!("{}", bengal::bir::print_module(&module.bir));
                }
            }

            let mono = bengal::pipeline::monomorphize(optimized)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            let compiled = bengal::pipeline::codegen(mono)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            bengal::pipeline::link(compiled, &exe_path)
                .map_err(|e| Report::new(e.into_diagnostic()))?;
            eprintln!("Wrote {}", exe_path.display());
        }
        Command::Eval { expr, emit_bir } => {
            let source = &expr;
            let filename = "<eval>";

            let bir_output = bengal::compile_source_to_bir(source)
                .map_err(|e| Report::new(e.into_diagnostic()))?;

            if emit_bir {
                for text in bir_output.bir_texts.values() {
                    println!("{text}");
                }
            }

            // Extract the single module's BIR for JIT execution
            let root_path = bengal::package::ModulePath::root();
            let root_module = bir_output
                .modules
                .get(&root_path)
                .ok_or_else(|| miette::miette!("no root module found"))?;

            let context = inkwell::context::Context::create();
            let module =
                bengal::codegen::compile_to_module(&context, &root_module.bir)
                    .map_err(|e| Report::new(e.into_diagnostic(filename, source)))?;

            let ee = module
                .create_jit_execution_engine(inkwell::OptimizationLevel::None)
                .map_err(|e| miette::miette!("JIT error: {e}"))?;
            let main_fn = unsafe {
                ee.get_function::<unsafe extern "C" fn() -> i32>("main")
                    .map_err(|e| miette::miette!("failed to find main: {e}"))?
            };
            let result = unsafe { main_fn.call() };
            println!("{result}");
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e:?}");
        process::exit(1);
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | head -20`
Expected: compiles

- [ ] **Step 3: Smoke test the CLI**

Run: `echo 'func main() -> Int32 { return 42; }' > /tmp/test_bengal.bengal && cargo run -- compile /tmp/test_bengal.bengal 2>&1`
Expected: "Wrote /tmp/test_bengal"

Run: `cargo run -- eval 'func main() -> Int32 { return 42; }' 2>&1`
Expected: "42"

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "Unify main.rs compile path through pipeline, update eval to use compile_source_to_bir"
```

---

### Task 10: Update test helpers

**Files:**
- Modify: `tests/common/mod.rs`

- [ ] **Step 1: Update test helpers**

In `tests/common/mod.rs`:

Replace `bengal::compile_source(source)` with `bengal::compile_source_to_objects(source)` at line 45 and line 98.

Replace `bengal::compile_package_to_executable(&entry_path, &exe_path)` with `bengal::compile_to_executable(&entry_path, &exe_path)` at line 121 and line 146.

The `compile_and_run` function (JIT path) and `compile_should_fail` function remain unchanged — they use internal APIs directly.

- [ ] **Step 2: Run all tests**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests PASS

- [ ] **Step 3: Run clippy**

Run: `cargo clippy 2>&1 | tail -20`
Expected: no warnings

- [ ] **Step 4: Run fmt**

Run: `cargo fmt`

- [ ] **Step 5: Commit**

```bash
git add tests/common/mod.rs
git commit -m "Update test helpers to use new pipeline API"
```

---

### Task 11: Remove dead code

**Files:**
- Modify: `src/lib.rs` (remove `collect_call_arg_types`, `BUILD_COUNTER`)

- [ ] **Step 1: Remove dead code from lib.rs**

The old `compile_source`, `compile_to_bir`, `compile_package_to_executable`, `collect_call_arg_types`, and `BUILD_COUNTER` should already be removed in Task 8. Verify no dead code remains:

Run: `cargo build 2>&1 | grep "unused\|dead_code"`
Expected: no dead code warnings (or only in pipeline.rs for yet-unused items)

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests PASS

- [ ] **Step 3: Commit if any cleanup was needed**

```bash
git add -A
git commit -m "Remove dead code after pipeline migration"
```

---

### Task 12: Update TODO.md with deferred items

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Update TODO.md**

Mark 1.6.1 as done. Add new deferred items under appropriate sections:

Under "Generics Future Enhancements" or a new section, add the mangling improvements.

Add a new section for pipeline future work:

```markdown
#### 1.6.1. ~~パイプライン再構成~~ **実装済み**

`pipeline.rs` に7段階のステージ関数（parse → analyze → lower → optimize → monomorphize → codegen → link）を実装。単一ファイル/パッケージのパイプライン統一、PipelineError によるエラーコンテキスト改善を含む。

##### 1.6.1a. パイプライン追加改善（将来）

- マングリングスキームの改善: エンティティ種別マーカー（関数/メソッド/イニシャライザの区別）、衝突防止、ジェネリック型エンコード統一（`mangle.rs` と `Instance::mangled_name()` の統合）
- `LoweringError`/`CodegenError` への `Span` 追加
- エラー修正提案（"did you mean ...?"）
- 複数エラーの一括報告
- `analyze_package` の事前解析結果再利用による冗長計算の排除
- ファイル変更検出に基づくインクリメンタル解析キャッシュ
- ディスク永続化による再ビルド高速化
- モジュール単位の並列解析
- `eval` サブコマンドのフルパイプライン統合
- `lower_program_with_inferred` の削除（パイプライン統一の確認後）
```

- [ ] **Step 2: Commit**

```bash
git add TODO.md
git commit -m "Mark pipeline restructuring as done, add deferred items to TODO"
```

---

### Task 13: Final verification

- [ ] **Step 1: Run cargo fmt**

Run: `cargo fmt`

- [ ] **Step 2: Run cargo clippy**

Run: `cargo clippy 2>&1`
Expected: no warnings

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1`
Expected: all tests PASS

- [ ] **Step 4: Run CLI smoke tests**

Run: `echo 'func main() -> Int32 { return 42; }' > /tmp/bengal_final.bengal && cargo run -- compile /tmp/bengal_final.bengal && /tmp/bengal_final; echo $?`
Expected: exit code 42

Run: `cargo run -- eval 'func main() -> Int32 { return 7; }'`
Expected: "7"

Run: `cargo run -- compile /tmp/bengal_final.bengal --emit-bir 2>&1 | head -5`
Expected: BIR text output

- [ ] **Step 5: Final commit if needed**

```bash
cargo fmt && cargo clippy
git add -A
git commit -m "Final cleanup after pipeline restructuring"
```
