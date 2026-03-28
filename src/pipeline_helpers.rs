use std::collections::HashMap;

use crate::bir::instruction::{BirModule, BirType};
use crate::bir::mono::MonoCollectResult;
use crate::package::{ModuleInfo, ModulePath};
use crate::semantic::{PackageSemanticInfo, SemanticInfo};

/// Build name_map: local function/method name -> mangled name.
pub(crate) fn build_name_map(
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
                crate::mangle::mangle_function(package_name, &module_segments, &func.name, &[]);
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
                let (pkg_name, src_segs) = resolve_mangle_context(
                    source_module,
                    package_name,
                    &pkg_sem_info.external_dep_names,
                );
                let source_segments: Vec<&str> = if src_segs.is_empty() {
                    vec![""]
                } else {
                    src_segs.iter().map(|s| s.as_str()).collect()
                };
                for method in &struct_info.methods {
                    let local_mangled = format!("{}_{}", struct_name, method.name);
                    let mangled = crate::mangle::mangle_method(
                        pkg_name,
                        &source_segments,
                        struct_name,
                        &method.name,
                        &[],
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
                    &[],
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
        let (pkg_name, source_segments) = resolve_mangle_context(
            source_module,
            package_name,
            &pkg_sem_info.external_dep_names,
        );
        let source_segs: Vec<&str> = if source_segments.is_empty() {
            vec![""]
        } else {
            source_segments.iter().map(|s| s.as_str()).collect()
        };
        let mangled = crate::mangle::mangle_function(pkg_name, &source_segs, imp_name, &[]);
        name_map.insert(imp_name.clone(), mangled);
    }

    name_map
}

/// Determine the package name and module segments for name mangling.
/// For external deps, uses the dep's package_name and strips the dep_name prefix.
/// For local imports, uses the consumer's package_name.
fn resolve_mangle_context<'a>(
    source_module: &'a ModulePath,
    package_name: &'a str,
    external_dep_names: &'a HashMap<ModulePath, String>,
) -> (&'a str, &'a [String]) {
    if let Some(dep_pkg_name) = external_dep_names.get(source_module) {
        // Strip the dep_name prefix (first segment) added by dep_module_path().
        // Invariant: dep_module_path always prepends exactly one segment.
        (dep_pkg_name.as_str(), &source_module.0[1..])
    } else {
        (package_name, source_module.0.as_slice())
    }
}

/// Collect functions called but not defined in a BIR module.
pub(crate) fn collect_external_functions(
    bir: &BirModule,
    mono_result: &MonoCollectResult,
) -> Vec<(String, Vec<BirType>, BirType)> {
    use crate::bir::instruction::{Instruction, Value};
    use std::collections::HashSet;

    let defined_funcs: HashSet<String> = bir.functions.iter().map(|f| f.name.clone()).collect();
    let resolved_instance_names: HashSet<String> = mono_result
        .func_instances
        .iter()
        .filter(|inst| defined_funcs.contains(&inst.func_name))
        .map(|inst| crate::mangle::mangle_generic_suffix(&inst.func_name, &inst.type_args))
        .collect();

    let mut external_functions = Vec::new();
    let mut seen_externals = HashSet::new();

    for func in &bir.functions {
        // Skip generic function templates — their bodies contain unresolved TypeParams
        // that would produce incorrect external function signatures.
        if !func.type_params.is_empty() {
            continue;
        }

        // Build value -> type map for this function
        let mut value_types: HashMap<Value, BirType> = HashMap::new();
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
                    type_args,
                    ty,
                    ..
                } = inst
                    && !defined_funcs.contains(func_name)
                    && !resolved_instance_names.contains(func_name)
                {
                    // For generic calls to external functions, use the mangled name
                    // so codegen can find the declaration after name resolution.
                    let effective_name = if !type_args.is_empty() {
                        crate::mangle::mangle_generic_suffix(func_name, type_args)
                    } else {
                        func_name.clone()
                    };
                    if seen_externals.contains(&effective_name) {
                        continue;
                    }
                    let arg_types: Vec<BirType> = args
                        .iter()
                        .map(|arg| value_types.get(arg).cloned().unwrap_or(BirType::I32))
                        .collect();
                    external_functions.push((effective_name.clone(), arg_types, ty.clone()));
                    seen_externals.insert(effective_name);
                }
            }
        }
    }

    external_functions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::ModulePath;

    #[test]
    fn resolve_mangle_context_local() {
        let external_dep_names = HashMap::new();
        let source = ModulePath(vec!["utils".to_string()]);
        let (pkg, segs) = resolve_mangle_context(&source, "myapp", &external_dep_names);
        assert_eq!(pkg, "myapp");
        assert_eq!(segs, &["utils".to_string()]);
    }

    #[test]
    fn resolve_mangle_context_external() {
        let mut external_dep_names = HashMap::new();
        external_dep_names.insert(ModulePath(vec!["math".to_string()]), "mathlib".to_string());
        let source = ModulePath(vec!["math".to_string()]);
        let (pkg, segs) = resolve_mangle_context(&source, "myapp", &external_dep_names);
        assert_eq!(pkg, "mathlib");
        assert!(segs.is_empty());
    }

    #[test]
    fn resolve_mangle_context_external_submodule() {
        let mut external_dep_names = HashMap::new();
        external_dep_names.insert(
            ModulePath(vec!["math".to_string(), "advanced".to_string()]),
            "mathlib".to_string(),
        );
        let source = ModulePath(vec!["math".to_string(), "advanced".to_string()]);
        let (pkg, segs) = resolve_mangle_context(&source, "myapp", &external_dep_names);
        assert_eq!(pkg, "mathlib");
        assert_eq!(segs, &["advanced".to_string()]);
    }
}
