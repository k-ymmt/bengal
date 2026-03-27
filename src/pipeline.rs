use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::bir::instruction::{BirModule, BirType};
use crate::bir::mono::MonoCollectResult;
use crate::error::{BengalError, DiagCtxt};
use crate::package::{ModuleGraph, ModulePath};
use crate::parser::ast::{NodeId, TypeAnnotation};
use crate::semantic::PackageSemanticInfo;

static BUILD_COUNTER: AtomicU64 = AtomicU64::new(0);

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

/// Parse from a file path. Detects single-file vs package mode.
pub fn parse(entry_path: &Path) -> Result<ParsedPackage, crate::error::PipelineError> {
    let entry_path = entry_path.canonicalize().map_err(|e| {
        crate::error::PipelineError::package(
            "parse",
            BengalError::PackageError {
                message: format!("failed to resolve path '{}': {}", entry_path.display(), e),
            },
        )
    })?;
    let entry_dir = entry_path.parent().unwrap_or_else(|| Path::new("."));

    match crate::package::find_package_root(entry_dir)
        .map_err(|e| crate::error::PipelineError::package("parse", e))?
    {
        Some(root) => {
            let config = crate::package::load_package(&root)
                .map_err(|e| crate::error::PipelineError::package("parse", e))?;
            let graph = crate::package::build_module_graph(&root.join(&config.package.entry))
                .map_err(|e| crate::error::PipelineError::package("parse", e))?;
            Ok(ParsedPackage {
                package_name: config.package.name,
                graph,
            })
        }
        None => {
            let source = std::fs::read_to_string(&entry_path).map_err(|e| {
                crate::error::PipelineError::package(
                    "parse",
                    BengalError::PackageError {
                        message: format!("failed to read '{}': {}", entry_path.display(), e),
                    },
                )
            })?;
            let name = entry_path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("bengal");
            let graph = ModuleGraph::from_source(name, &source).map_err(|e| {
                crate::error::PipelineError::new(
                    "parse",
                    &entry_path.display().to_string(),
                    Some(&source),
                    e,
                )
            })?;
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
pub fn parse_source(
    name: &str,
    source: &str,
) -> Result<ParsedPackage, crate::error::PipelineError> {
    let graph = ModuleGraph::from_source(name, source)
        .map_err(|e| crate::error::PipelineError::new("parse", name, Some(source), e))?;
    Ok(ParsedPackage {
        package_name: name.to_string(),
        graph,
    })
}

/// Semantic analysis: validate generics, run pre-mono inference, cross-module resolution.
pub fn analyze(
    parsed: ParsedPackage,
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

    // Bail out if any per-module errors were collected above
    if diag.has_errors() {
        return Err(crate::error::PipelineError::package(
            "analyze",
            BengalError::PackageError {
                message: "analysis failed due to module errors".to_string(),
            },
        ));
    }

    // Cross-module semantic analysis
    let pkg_sem_info = crate::semantic::analyze_package(&parsed.graph, &parsed.package_name, diag)
        .map_err(|e| crate::error::PipelineError::package("analyze", e))?;

    Ok(AnalyzedPackage {
        package_name: parsed.package_name,
        graph: parsed.graph,
        inferred_maps,
        pkg_sem_info,
    })
}

/// BIR lowering: build name maps, lower each module's AST to BIR.
pub fn lower(
    analyzed: AnalyzedPackage,
    diag: &mut DiagCtxt,
) -> Result<LoweredPackage, crate::error::PipelineError> {
    let mut modules = HashMap::new();
    let mut sources = HashMap::new();

    for (mod_path, mod_info) in &analyzed.graph.modules {
        let sem_info = analyzed
            .pkg_sem_info
            .module_infos
            .get(mod_path)
            .ok_or_else(|| {
                crate::error::PipelineError::package(
                    "lower",
                    BengalError::PackageError {
                        message: format!("missing semantic info for module '{}'", mod_path),
                    },
                )
            })?;

        let name_map = crate::pipeline_helpers::build_name_map(
            &analyzed.package_name,
            mod_path,
            mod_info,
            sem_info,
            &analyzed.pkg_sem_info,
        );

        let empty_inferred = HashMap::new();
        let inferred_map = analyzed
            .inferred_maps
            .get(mod_path)
            .unwrap_or(&empty_inferred);

        let bir_module = crate::bir::lowering::lower_module_with_inferred(
            &mod_info.ast,
            sem_info,
            &name_map,
            inferred_map,
            diag,
        )
        .map_err(|e| {
            crate::error::PipelineError::new(
                "lower",
                &mod_path.to_string(),
                Some(&mod_info.source),
                e,
            )
        })?;

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

/// BIR optimization: run optimization passes on each module's BIR.
pub fn optimize(mut lowered: LoweredPackage) -> LoweredPackage {
    for module in lowered.modules.values_mut() {
        crate::bir::optimize_module(&mut module.bir);
    }
    lowered
}

/// Monomorphization collection: find all concrete instantiations needed.
pub fn monomorphize(
    lowered: LoweredPackage,
    _diag: &mut DiagCtxt,
) -> Result<MonomorphizedPackage, crate::error::PipelineError> {
    let mut modules = HashMap::new();

    for (mod_path, module) in lowered.modules {
        let mono_result = crate::bir::mono::mono_collect(&module.bir, "main");
        let external_functions =
            crate::pipeline_helpers::collect_external_functions(&module.bir, &mono_result);

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

/// Code generation: compile each module's BIR to native object code.
pub fn codegen(
    mono: MonomorphizedPackage,
    diag: &mut DiagCtxt,
) -> Result<CompiledPackage, crate::error::PipelineError> {
    let mut object_bytes = HashMap::new();

    for (mod_path, module) in &mono.modules {
        match crate::codegen::compile_module_with_mono(
            &module.bir,
            &module.mono_result,
            &module.external_functions,
        ) {
            Ok(obj) => {
                object_bytes.insert(mod_path.clone(), obj);
            }
            Err(e) => {
                diag.emit(e);
                continue;
            }
        }
    }

    if diag.has_errors() {
        return Err(crate::error::PipelineError::package(
            "codegen",
            BengalError::CodegenError {
                message: format!("{} error(s) during code generation", diag.error_count()),
            },
        ));
    }

    Ok(CompiledPackage { object_bytes })
}

/// Link object files into an executable.
pub fn link(
    compiled: CompiledPackage,
    output_path: &Path,
) -> Result<(), crate::error::PipelineError> {
    let build_id = BUILD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_dir =
        std::env::temp_dir().join(format!("bengal_build_{}_{}", std::process::id(), build_id));
    std::fs::create_dir_all(&temp_dir).map_err(|e| {
        crate::error::PipelineError::package(
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
            crate::error::PipelineError::package(
                "link",
                BengalError::PackageError {
                    message: format!("failed to write object file: {}", e),
                },
            )
        })?;
        obj_files.push(obj_path);
    }

    crate::codegen::link_objects(&obj_files, output_path)
        .map_err(|e| crate::error::PipelineError::package("link", e))?;

    let _ = std::fs::remove_dir_all(&temp_dir);
    Ok(())
}

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

    #[test]
    fn analyze_single_file() {
        let parsed = parse_source("test", "func main() -> Int32 { return 1; }").unwrap();
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        assert_eq!(analyzed.inferred_maps.len(), 1);
    }

    #[test]
    fn analyze_generic_function() {
        let source = r#"
            func identity<T>(x: T) -> T { return x; }
            func main() -> Int32 { return identity<Int32>(42); }
        "#;
        let parsed = parse_source("test", source).unwrap();
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        // The inferred_maps contains one entry per module.
        // With explicit type args (identity<Int32>), no inference is needed,
        // so the root module's map may be empty — but the map itself must exist.
        assert!(analyzed.inferred_maps.contains_key(&ModulePath::root()));
    }

    #[test]
    fn lower_single_file() {
        let parsed = parse_source("test", "func main() -> Int32 { return 1; }").unwrap();
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();
        assert_eq!(lowered.modules.len(), 1);
        let root = lowered.modules.get(&ModulePath::root()).unwrap();
        assert!(root.is_entry);
        assert!(!root.bir.functions.is_empty());
    }

    #[test]
    fn full_pipeline_single_file() {
        let parsed = parse_source("test", "func main() -> Int32 { return 42; }").unwrap();
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();
        let optimized = optimize(lowered);
        let mono = monomorphize(optimized, &mut DiagCtxt::new()).unwrap();
        let compiled = codegen(mono, &mut DiagCtxt::new()).unwrap();
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
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();
        let optimized = optimize(lowered);
        let mono = monomorphize(optimized, &mut DiagCtxt::new()).unwrap();
        let compiled = codegen(mono, &mut DiagCtxt::new()).unwrap();
        assert!(!compiled.object_bytes.is_empty());
    }
}
