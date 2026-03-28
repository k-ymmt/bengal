use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::bir::instruction::{BirModule, BirType};
use crate::bir::mono::MonoCollectResult;
use crate::error::{BengalError, DiagCtxt};
use crate::interface::ModuleInterface;
use crate::package::{ModuleGraph, ModulePath};
use crate::parser::ast::{NodeId, TypeAnnotation};
use crate::semantic::PackageSemanticInfo;

static BUILD_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Output of the `parse` stage.
pub struct ParsedPackage {
    pub package_name: String,
    pub graph: ModuleGraph,
}

/// An external package dependency loaded from a `.bengalmod` file.
pub struct ExternalDep {
    /// Dependency name as specified in --dep (used in import path resolution)
    pub name: String,
    /// Package name from the .bengalmod file (used for symbol mangling)
    pub package_name: String,
    /// Per-module interface data
    pub interfaces: HashMap<ModulePath, ModuleInterface>,
    /// Per-module BIR (for codegen and monomorphization)
    pub bir_modules: HashMap<ModulePath, BirModule>,
    /// Pre-compiled object code per module (for linking without re-compilation)
    pub object_bytes: HashMap<ModulePath, Vec<u8>>,
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
    pub pkg_sem_info: PackageSemanticInfo,
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

/// Data saved from LoweredPackage for emit_package_bengalmod.
/// Extracted after optimize() but before monomorphize() consumes the package.
pub struct EmitData {
    pub package_name: String,
    pub pkg_sem_info: PackageSemanticInfo,
    pub modules_bir: HashMap<ModulePath, BirModule>,
}

impl EmitData {
    pub fn from_lowered(lowered: &LoweredPackage) -> Self {
        EmitData {
            package_name: lowered.package_name.clone(),
            pkg_sem_info: lowered.pkg_sem_info.clone(),
            modules_bir: lowered
                .modules
                .iter()
                .map(|(path, m)| (path.clone(), m.bir.clone()))
                .collect(),
        }
    }
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

/// Semantic analysis with external dependencies.
pub fn analyze_with_deps(
    parsed: ParsedPackage,
    external_deps: &[ExternalDep],
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
    let pkg_sem_info =
        crate::semantic::analyze_package(&parsed.graph, &parsed.package_name, external_deps, diag)
            .map_err(|e| crate::error::PipelineError::package("analyze", e))?;

    Ok(AnalyzedPackage {
        package_name: parsed.package_name,
        graph: parsed.graph,
        inferred_maps,
        pkg_sem_info,
    })
}

/// Semantic analysis (no external deps). Delegates to `analyze_with_deps`.
pub fn analyze(
    parsed: ParsedPackage,
    diag: &mut DiagCtxt,
) -> Result<AnalyzedPackage, crate::error::PipelineError> {
    analyze_with_deps(parsed, &[], diag)
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
        pkg_sem_info: analyzed.pkg_sem_info,
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
///
/// After per-module collection, propagates generic instances across module
/// boundaries so that external dep modules containing generic templates
/// also emit the required concrete specialisations.
pub fn monomorphize(
    lowered: LoweredPackage,
    _diag: &mut DiagCtxt,
) -> Result<MonomorphizedPackage, crate::error::PipelineError> {
    use crate::bir::mono::Instance;
    use std::collections::HashSet;

    // Phase 1: per-module mono_collect.
    let mut modules = HashMap::new();
    for (mod_path, module) in lowered.modules {
        let mono_result = crate::bir::mono::mono_collect(&module.bir, "main");
        modules.insert(
            mod_path,
            MonomorphizedModule {
                bir: module.bir,
                mono_result,
                external_functions: Vec::new(),
                is_entry: module.is_entry,
            },
        );
    }

    // Phase 2: propagate cross-module generic instances.
    // Build an index: function_name -> module_path for generic templates.
    let mut generic_owner: HashMap<String, ModulePath> = HashMap::new();
    for (mod_path, mono_mod) in &modules {
        for func in &mono_mod.bir.functions {
            if !func.type_params.is_empty() {
                generic_owner.insert(func.name.clone(), mod_path.clone());
            }
        }
    }

    // Collect instances that need to be forwarded to other modules.
    let mut forwarded: HashMap<ModulePath, Vec<Instance>> = HashMap::new();
    for (mod_path, mono_mod) in &modules {
        let defined: HashSet<&str> = mono_mod
            .bir
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        for inst in &mono_mod.mono_result.func_instances {
            if !defined.contains(inst.func_name.as_str())
                && let Some(owner_path) = generic_owner.get(&inst.func_name)
                && owner_path != mod_path
            {
                forwarded
                    .entry(owner_path.clone())
                    .or_default()
                    .push(inst.clone());
            }
        }
    }

    // Merge forwarded instances into target modules.
    for (target_path, instances) in forwarded {
        if let Some(target_mod) = modules.get_mut(&target_path) {
            let existing: HashSet<Instance> = target_mod
                .mono_result
                .func_instances
                .iter()
                .cloned()
                .collect();
            for inst in instances {
                if !existing.contains(&inst) {
                    target_mod.mono_result.func_instances.push(inst);
                }
            }
        }
    }

    // Phase 3: collect external functions per module.
    for mono_mod in modules.values_mut() {
        mono_mod.external_functions = crate::pipeline_helpers::collect_external_functions(
            &mono_mod.bir,
            &mono_mod.mono_result,
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

/// Collect pre-compiled object bytes from external dependencies for linking.
pub fn collect_external_objects(external_deps: &[ExternalDep]) -> HashMap<ModulePath, Vec<u8>> {
    let mut objects = HashMap::new();
    for dep in external_deps {
        for (mod_path, bytes) in &dep.object_bytes {
            let ext_path = crate::semantic::dep_module_path(&dep.name, mod_path);
            objects.insert(ext_path, bytes.clone());
        }
    }
    objects
}

/// Link object files into an executable.
pub fn link(
    compiled: CompiledPackage,
    external_objects: &HashMap<ModulePath, Vec<u8>>,
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

    // Write external dependency object files
    for (mod_path, bytes) in external_objects {
        let obj_name = if mod_path.0.is_empty() {
            "ext_root.o".to_string()
        } else {
            format!("ext_{}.o", mod_path.0.join("_"))
        };
        let obj_path = temp_dir.join(&obj_name);
        std::fs::write(&obj_path, bytes).map_err(|e| {
            crate::error::PipelineError::package(
                "link",
                BengalError::PackageError {
                    message: format!("failed to write external object file: {}", e),
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

/// Emit per-module `.bengalmod` interface files into the given cache directory.
/// Errors are non-fatal — they are reported as warnings on stderr.
pub fn emit_interfaces(lowered: &LoweredPackage, cache_dir: &std::path::Path) {
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("warning: failed to create cache directory: {}", e);
        return;
    }

    for (module_path, module) in &lowered.modules {
        let sem_info = match lowered.pkg_sem_info.module_infos.get(module_path) {
            Some(info) => info,
            None => continue,
        };
        let iface = crate::interface::ModuleInterface::from_semantic_info(sem_info);
        let mod_file = crate::interface::BengalModFile {
            package_name: lowered.package_name.clone(),
            modules: HashMap::from([(module_path.clone(), module.bir.clone())]),
            interfaces: HashMap::from([(module_path.clone(), iface)]),
            object_bytes: HashMap::new(),
        };

        let file_path = cache_dir.join(module_path.to_file_path("bengalmod"));
        if let Some(parent) = file_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("warning: failed to create cache subdirectory: {}", e);
            continue;
        }
        if let Err(e) = crate::interface::write_bengalmod_file(&mod_file, &file_path) {
            eprintln!("warning: failed to write interface cache: {}", e);
        }
    }
}

/// Load an external dependency from a `.bengalmod` file.
pub fn load_external_dep(
    name: &str,
    path: &std::path::Path,
) -> Result<ExternalDep, crate::error::PipelineError> {
    let mod_file = crate::interface::read_interface(path)
        .map_err(|e| crate::error::PipelineError::package("load_dep", e))?;
    Ok(ExternalDep {
        name: name.to_string(),
        package_name: mod_file.package_name,
        interfaces: mod_file.interfaces,
        bir_modules: mod_file.modules,
        object_bytes: mod_file.object_bytes,
    })
}

/// Merge external dependency BIR modules into the lowered package.
/// Must be called AFTER emit_interfaces and BEFORE optimize.
pub fn merge_external_deps(lowered: &mut LoweredPackage, external_deps: &[ExternalDep]) {
    for dep in external_deps {
        for (mod_path, bir_module) in &dep.bir_modules {
            let ext_path = crate::semantic::dep_module_path(&dep.name, mod_path);
            // Strip the `main` entry point from external deps to avoid
            // duplicate-symbol errors when linking with the consumer's `main`.
            let mut filtered = bir_module.clone();
            filtered.functions.retain(|f| f.name != "main");
            lowered.modules.insert(
                ext_path,
                LoweredModule {
                    bir: filtered,
                    is_entry: false,
                },
            );
        }
    }
}

/// Filter a BIR module to remove the `main` entry point.
/// Used when producing library object code for archives.
fn filter_main(bir: &BirModule) -> BirModule {
    BirModule {
        functions: bir
            .functions
            .iter()
            .filter(|f| f.name != "main")
            .cloned()
            .collect(),
        struct_layouts: bir.struct_layouts.clone(),
        struct_type_params: bir.struct_type_params.clone(),
        conformance_map: bir.conformance_map.clone(),
    }
}

/// Filter a BIR module to retain only generic functions.
/// Non-function data (struct_layouts, struct_type_params, conformance_map) is preserved.
fn filter_generic_functions(bir: &BirModule) -> BirModule {
    BirModule {
        functions: bir
            .functions
            .iter()
            .filter(|f| !f.type_params.is_empty())
            .cloned()
            .collect(),
        struct_layouts: bir.struct_layouts.clone(),
        struct_type_params: bir.struct_type_params.clone(),
        conformance_map: bir.conformance_map.clone(),
    }
}

/// Emit a single `.bengalmod` containing all modules of the package.
/// This is consumed by `--dep` in other packages.
///
/// The emitted file contains:
/// - Generic-only BIR (non-generic functions are stripped)
/// - Module interfaces (for type checking)
/// - Pre-compiled object code (for linking without re-compilation)
///
/// Object bytes are compiled from BIR with `main` stripped, so that
/// consumers do not get duplicate symbol errors at link time.
pub fn emit_package_bengalmod(emit_data: &EmitData, cache_dir: &std::path::Path) {
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        eprintln!("warning: failed to create cache directory: {}", e);
        return;
    }

    let mut all_modules = HashMap::new();
    let mut all_interfaces = HashMap::new();
    let mut all_object_bytes = HashMap::new();

    for (module_path, bir) in &emit_data.modules_bir {
        let sem_info = match emit_data.pkg_sem_info.module_infos.get(module_path) {
            Some(info) => info,
            None => continue,
        };
        let iface = crate::interface::ModuleInterface::from_semantic_info(sem_info);
        all_modules.insert(module_path.clone(), filter_generic_functions(bir));
        all_interfaces.insert(module_path.clone(), iface);

        // Compile library object bytes from BIR with `main` stripped so that
        // consumers do not get duplicate `main` symbols at link time.
        let lib_bir = filter_main(bir);
        if !lib_bir.functions.is_empty() || !lib_bir.struct_layouts.is_empty() {
            let mono_result = crate::bir::mono::mono_collect(&lib_bir, "main");
            match crate::codegen::compile_module_with_mono(&lib_bir, &mono_result, &[]) {
                Ok(obj) => {
                    all_object_bytes.insert(module_path.clone(), obj);
                }
                Err(e) => {
                    eprintln!(
                        "warning: failed to compile library objects for {}: {}",
                        module_path, e
                    );
                }
            }
        }
    }

    let mod_file = crate::interface::BengalModFile {
        package_name: emit_data.package_name.clone(),
        modules: all_modules,
        interfaces: all_interfaces,
        object_bytes: all_object_bytes,
    };

    let file_path = cache_dir.join(format!("{}.bengalmod", emit_data.package_name));
    if let Err(e) = crate::interface::write_bengalmod_file(&mod_file, &file_path) {
        eprintln!("warning: failed to write package interface: {}", e);
    }
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
    fn emit_package_bengalmod_creates_file() {
        let parsed = parse_source(
            "testlib",
            "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }\nfunc main() -> Int32 { return 0; }",
        )
        .unwrap();
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();
        let optimized = optimize(lowered);
        let emit_data = EmitData::from_lowered(&optimized);

        let dir = tempfile::TempDir::new().unwrap();
        emit_package_bengalmod(&emit_data, dir.path());

        let file_path = dir.path().join("testlib.bengalmod");
        assert!(file_path.exists());
        let loaded = crate::interface::read_interface(&file_path).unwrap();
        assert_eq!(loaded.package_name, "testlib");
        assert!(!loaded.interfaces.is_empty());
        assert!(!loaded.object_bytes.is_empty(), "should have object code");
    }

    #[test]
    fn load_external_dep_round_trip() {
        let parsed = parse_source(
            "mathlib",
            "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }\nfunc main() -> Int32 { return 0; }",
        )
        .unwrap();
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();
        let optimized = optimize(lowered);
        let emit_data = EmitData::from_lowered(&optimized);

        let dir = tempfile::TempDir::new().unwrap();
        emit_package_bengalmod(&emit_data, dir.path());

        let dep = load_external_dep("math", &dir.path().join("mathlib.bengalmod")).unwrap();
        assert_eq!(dep.name, "math");
        assert_eq!(dep.package_name, "mathlib");
        assert!(!dep.interfaces.is_empty());
        assert!(!dep.bir_modules.is_empty());
    }

    #[test]
    fn merge_external_deps_adds_modules() {
        let parsed = parse_source("app", "func main() -> Int32 { return 1; }").unwrap();
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        let mut lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();

        assert_eq!(lowered.modules.len(), 1); // just root

        let lib_parsed = parse_source("mathlib", "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }\nfunc main() -> Int32 { return 0; }").unwrap();
        let lib_analyzed = analyze(lib_parsed, &mut DiagCtxt::new()).unwrap();
        let lib_lowered = lower(lib_analyzed, &mut DiagCtxt::new()).unwrap();
        let lib_optimized = optimize(lib_lowered);
        let lib_emit_data = EmitData::from_lowered(&lib_optimized);

        let dir = tempfile::TempDir::new().unwrap();
        emit_package_bengalmod(&lib_emit_data, dir.path());
        let dep = load_external_dep("math", &dir.path().join("mathlib.bengalmod")).unwrap();

        merge_external_deps(&mut lowered, &[dep]);

        assert!(lowered.modules.len() > 1, "external BIR should be merged");
        let ext_path = crate::semantic::dep_module_path("math", &ModulePath::root());
        let ext_module = lowered.modules.get(&ext_path);
        assert!(
            ext_module.is_some(),
            "external module should exist at dep_module_path"
        );
        assert!(
            !ext_module.unwrap().is_entry,
            "external module should not be entry"
        );
    }

    #[test]
    fn filter_generic_functions_mixed() {
        use crate::bir::instruction::BirFunction;

        let generic_fn = BirFunction {
            name: "identity".to_string(),
            type_params: vec!["T".to_string()],
            params: vec![],
            return_type: BirType::TypeParam("T".to_string()),
            blocks: vec![],
            body: vec![],
        };
        let non_generic_fn = BirFunction {
            name: "add".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: BirType::I32,
            blocks: vec![],
            body: vec![],
        };
        let bir = BirModule {
            functions: vec![generic_fn, non_generic_fn],
            struct_layouts: HashMap::from([(
                "Point".to_string(),
                vec![("x".to_string(), BirType::I32)],
            )]),
            struct_type_params: HashMap::from([("Box".to_string(), vec!["T".to_string()])]),
            conformance_map: HashMap::new(),
        };

        let filtered = filter_generic_functions(&bir);
        assert_eq!(filtered.functions.len(), 1);
        assert_eq!(filtered.functions[0].name, "identity");
        assert_eq!(filtered.struct_layouts.len(), 1);
        assert_eq!(filtered.struct_type_params.len(), 1);
    }

    #[test]
    fn filter_generic_functions_no_generics() {
        use crate::bir::instruction::BirFunction;

        let bir = BirModule {
            functions: vec![BirFunction {
                name: "add".to_string(),
                type_params: vec![],
                params: vec![],
                return_type: BirType::I32,
                blocks: vec![],
                body: vec![],
            }],
            struct_layouts: HashMap::from([("Point".to_string(), vec![])]),
            struct_type_params: HashMap::new(),
            conformance_map: HashMap::new(),
        };

        let filtered = filter_generic_functions(&bir);
        assert!(filtered.functions.is_empty());
        assert_eq!(filtered.struct_layouts.len(), 1, "struct_layouts preserved");
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

    #[test]
    fn emit_package_bengalmod_with_object_code() {
        let source = r#"
            public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
            public func identity<T>(x: T) -> T { return x; }
            func main() -> Int32 { return add(1, 2); }
        "#;
        let parsed = parse_source("testlib", source).unwrap();
        let analyzed = analyze(parsed, &mut DiagCtxt::new()).unwrap();
        let lowered = lower(analyzed, &mut DiagCtxt::new()).unwrap();
        let optimized = optimize(lowered);
        let emit_data = EmitData::from_lowered(&optimized);

        let dir = tempfile::TempDir::new().unwrap();
        emit_package_bengalmod(&emit_data, dir.path());

        let loaded =
            crate::interface::read_interface(&dir.path().join("testlib.bengalmod")).unwrap();
        assert!(
            !loaded.object_bytes.is_empty(),
            "object_bytes should be non-empty"
        );

        let root_bir = loaded.modules.get(&ModulePath::root()).unwrap();
        for func in &root_bir.functions {
            assert!(
                !func.type_params.is_empty(),
                "BIR should only contain generic functions, found '{}'",
                func.name
            );
        }
    }

    #[test]
    fn collect_external_objects_maps_correctly() {
        let dep = ExternalDep {
            name: "math".to_string(),
            package_name: "mathlib".to_string(),
            interfaces: HashMap::new(),
            bir_modules: HashMap::new(),
            object_bytes: HashMap::from([(ModulePath::root(), vec![1, 2, 3])]),
        };

        let objects = collect_external_objects(&[dep]);
        let ext_path = crate::semantic::dep_module_path("math", &ModulePath::root());
        assert!(objects.contains_key(&ext_path));
        assert_eq!(objects[&ext_path], vec![1, 2, 3]);
    }
}
