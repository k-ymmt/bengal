pub mod bir;
pub mod codegen;
pub mod error;
pub mod lexer;
pub mod mangle;
pub mod monomorphize;
pub mod package;
pub mod parser;
pub mod semantic;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use error::{BengalError, Result};

static BUILD_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    semantic::validate_generics(&program)?;
    semantic::validate_main(&program)?;
    let (inferred, sem_info) = semantic::analyze_pre_mono(&program)?;
    // Convert inferred type args to the lowering format
    let inferred_map: HashMap<parser::ast::NodeId, Vec<parser::ast::TypeAnnotation>> = inferred
        .map
        .into_iter()
        .map(|(id, site)| (id, site.type_args))
        .collect();
    // No AST monomorphize — lower generics directly to BIR
    let mut bir = bir::lowering::lower_program_with_inferred(&program, &sem_info, &inferred_map)?;
    bir::optimize_module(&mut bir);
    let mono_result = bir::mono::mono_collect(&bir, "main");
    let obj_bytes = codegen::compile_with_mono(&bir, &mono_result)?;
    Ok(obj_bytes)
}

pub fn compile_to_bir(source: &str) -> Result<(bir::instruction::BirModule, String)> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    semantic::validate_generics(&program)?;
    let (inferred, sem_info) = semantic::analyze_pre_mono(&program)?;
    let inferred_map: HashMap<parser::ast::NodeId, Vec<parser::ast::TypeAnnotation>> = inferred
        .map
        .into_iter()
        .map(|(id, site)| (id, site.type_args))
        .collect();
    let mut bir_module =
        bir::lowering::lower_program_with_inferred(&program, &sem_info, &inferred_map)?;
    bir::optimize_module(&mut bir_module);
    let bir_text = bir::print_module(&bir_module);
    Ok((bir_module, bir_text))
}

/// Compile a multi-file Bengal package to an executable.
///
/// 1. Find the package root (Bengal.toml) starting from `entry_path`'s parent.
/// 2. Build the module graph from the entry file.
/// 3. For each module: validate generics, run pre-mono type inference (no AST mono).
/// 4. Run `analyze_package()` for cross-module semantic analysis.
/// 5. For each module: lower the ORIGINAL AST (with generics) to BIR, optimize,
///    run BIR-level mono_collect, compile with compile_module_with_mono.
/// 6. Link all .o files into the final executable at `output_path`.
/// 7. Clean up temporary .o files.
pub fn compile_package_to_executable(entry_path: &Path, output_path: &Path) -> Result<()> {
    // 1. Find package root and load config
    let entry_dir = entry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let (package_name, _entry_file) = match package::find_package_root(&entry_dir)? {
        Some(root) => {
            let config = package::load_package(&root)?;
            (config.package.name, config.package.entry)
        }
        None => {
            // No Bengal.toml — use directory name as package name
            let name = entry_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("bengal")
                .to_string();
            let entry = entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("main.bengal")
                .to_string();
            (name, entry)
        }
    };

    // 2. Build module graph
    let graph = package::build_module_graph(entry_path)?;

    // 2.5. Validate generics and run pre-mono analysis per module.
    //       No AST monomorphization — generics are preserved for BIR-level mono.
    for mod_info in graph.modules.values() {
        semantic::validate_generics(&mod_info.ast)?;
    }
    let mut inferred_maps: HashMap<
        package::ModulePath,
        HashMap<parser::ast::NodeId, Vec<parser::ast::TypeAnnotation>>,
    > = HashMap::new();
    for (mod_path, mod_info) in &graph.modules {
        let (inferred, _pre_mono_sem_info) = semantic::analyze_pre_mono_lenient(&mod_info.ast)?;
        let inferred_map: HashMap<parser::ast::NodeId, Vec<parser::ast::TypeAnnotation>> = inferred
            .map
            .into_iter()
            .map(|(id, site)| (id, site.type_args))
            .collect();
        inferred_maps.insert(mod_path.clone(), inferred_map);
    }

    // 3. Run cross-module semantic analysis on the original (un-monomorphized) AST
    let pkg_sem_info = semantic::analyze_package(&graph, &package_name)?;

    // 4. For each module: build name map, lower with inferred type args, optimize,
    //    run BIR mono_collect, compile with compile_module_with_mono
    let mut obj_files = Vec::new();
    let build_id = BUILD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_dir =
        std::env::temp_dir().join(format!("bengal_build_{}_{}", std::process::id(), build_id));
    std::fs::create_dir_all(&temp_dir).map_err(|e| BengalError::PackageError {
        message: format!("failed to create temp dir: {}", e),
    })?;

    for (mod_path, mod_info) in &graph.modules {
        let sem_info =
            pkg_sem_info
                .module_infos
                .get(mod_path)
                .ok_or_else(|| BengalError::PackageError {
                    message: format!("missing semantic info for module '{}'", mod_path),
                })?;

        let is_entry = mod_path.is_root();
        let module_segments: Vec<&str> = if mod_path.0.is_empty() {
            vec![""]
        } else {
            mod_path.0.iter().map(|s| s.as_str()).collect()
        };

        // Build name_map: local name -> mangled name
        let mut name_map: HashMap<String, String> = HashMap::new();

        // Local functions
        for func in &mod_info.ast.functions {
            if is_entry && func.name == "main" {
                name_map.insert("main".to_string(), "main".to_string());
            } else {
                let mangled = mangle::mangle_function(&package_name, &module_segments, &func.name);
                name_map.insert(func.name.clone(), mangled);
            }
        }

        // Local methods
        for (struct_name, struct_info) in &sem_info.struct_defs {
            // Only process structs defined in this module (check if it's NOT imported)
            let is_imported = pkg_sem_info
                .import_sources
                .contains_key(&(mod_path.clone(), struct_name.clone()));
            if is_imported {
                // For imported structs, map their method names to the source module's mangling
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
                        let mangled = mangle::mangle_method(
                            &package_name,
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
                    let mangled = mangle::mangle_method(
                        &package_name,
                        &module_segments,
                        struct_name,
                        &method.name,
                    );
                    name_map.insert(local_mangled, mangled);
                }
            }
        }

        // Imported functions: map local name to mangled name using source module
        for ((imp_mod, imp_name), source_module) in &pkg_sem_info.import_sources {
            if imp_mod != mod_path {
                continue;
            }
            // Skip if it's a struct (already handled above)
            if sem_info.struct_defs.contains_key(imp_name) {
                continue;
            }
            let source_segments: Vec<&str> = if source_module.0.is_empty() {
                vec![""]
            } else {
                source_module.0.iter().map(|s| s.as_str()).collect()
            };
            let mangled = mangle::mangle_function(&package_name, &source_segments, imp_name);
            name_map.insert(imp_name.clone(), mangled);
        }

        // Get the inferred type args for this module
        let empty_inferred = HashMap::new();
        let inferred_map = inferred_maps.get(mod_path).unwrap_or(&empty_inferred);

        // Lower the original AST (with generics) to BIR
        let mut bir_module = bir::lowering::lower_module_with_inferred(
            &mod_info.ast,
            sem_info,
            &name_map,
            inferred_map,
        )?;
        bir::optimize_module(&mut bir_module);

        // Run BIR-level monomorphization collection
        let mono_result = bir::mono::mono_collect(&bir_module, "main");

        // Collect external functions (functions called but not defined in this module).
        // For BIR mono, we also need to account for calls from resolved generic instances.
        let defined_funcs: std::collections::HashSet<String> = bir_module
            .functions
            .iter()
            .map(|f| f.name.clone())
            .collect();
        // Also consider resolved generic instance names as "defined"
        let resolved_instance_names: std::collections::HashSet<String> = mono_result
            .func_instances
            .iter()
            .filter(|inst| defined_funcs.contains(&inst.func_name))
            .map(|inst| inst.mangled_name())
            .collect();
        let mut external_functions = Vec::new();
        let mut seen_externals = std::collections::HashSet::new();

        for func in &bir_module.functions {
            for block in &func.blocks {
                for inst in &block.instructions {
                    if let bir::instruction::Instruction::Call {
                        func_name,
                        args,
                        ty,
                        ..
                    } = inst
                        && !defined_funcs.contains(func_name)
                        && !resolved_instance_names.contains(func_name)
                        && !seen_externals.contains(func_name)
                    {
                        external_functions.push((
                            func_name.clone(),
                            collect_call_arg_types(func, args),
                            ty.clone(),
                        ));
                        seen_externals.insert(func_name.clone());
                    }
                }
            }
        }

        // Compile to object file with BIR mono support
        let obj_bytes =
            codegen::compile_module_with_mono(&bir_module, &mono_result, &external_functions)?;

        let obj_name = if mod_path.0.is_empty() {
            "root.o".to_string()
        } else {
            format!("{}.o", mod_path.0.join("_"))
        };
        let obj_path = temp_dir.join(&obj_name);
        std::fs::write(&obj_path, &obj_bytes).map_err(|e| BengalError::PackageError {
            message: format!("failed to write object file: {}", e),
        })?;
        obj_files.push(obj_path);
    }

    // 5. Link all object files
    codegen::link_objects(&obj_files, output_path)?;

    // 6. Clean up temp directory
    let _ = std::fs::remove_dir_all(&temp_dir);

    Ok(())
}

/// Collect the BIR types of call arguments by looking up value types in a BIR function.
fn collect_call_arg_types(
    func: &bir::instruction::BirFunction,
    args: &[bir::instruction::Value],
) -> Vec<bir::instruction::BirType> {
    // Build a local value -> type map for this function
    let mut value_types: HashMap<bir::instruction::Value, bir::instruction::BirType> =
        HashMap::new();

    for (val, ty) in &func.params {
        value_types.insert(*val, ty.clone());
    }
    for block in &func.blocks {
        for (val, ty) in &block.params {
            value_types.insert(*val, ty.clone());
        }
        for inst in &block.instructions {
            let (result, ty) = match inst {
                bir::instruction::Instruction::Literal { result, ty, .. } => (*result, ty.clone()),
                bir::instruction::Instruction::BinaryOp { result, ty, .. } => (*result, ty.clone()),
                bir::instruction::Instruction::Compare { result, .. } => {
                    (*result, bir::instruction::BirType::Bool)
                }
                bir::instruction::Instruction::Not { result, .. } => {
                    (*result, bir::instruction::BirType::Bool)
                }
                bir::instruction::Instruction::Cast { result, to_ty, .. } => {
                    (*result, to_ty.clone())
                }
                bir::instruction::Instruction::Call { result, ty, .. } => (*result, ty.clone()),
                bir::instruction::Instruction::StructInit { result, ty, .. } => {
                    (*result, ty.clone())
                }
                bir::instruction::Instruction::FieldGet { result, ty, .. } => (*result, ty.clone()),
                bir::instruction::Instruction::FieldSet { result, ty, .. } => (*result, ty.clone()),
                bir::instruction::Instruction::ArrayInit { result, ty, .. } => {
                    (*result, ty.clone())
                }
                bir::instruction::Instruction::ArrayGet { result, ty, .. } => (*result, ty.clone()),
                bir::instruction::Instruction::ArraySet { result, ty, .. } => (*result, ty.clone()),
            };
            value_types.insert(result, ty);
        }
    }

    args.iter()
        .map(|arg| {
            value_types
                .get(arg)
                .cloned()
                .unwrap_or(bir::instruction::BirType::I32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_source_returns_object() {
        let obj = compile_source("func main() -> Int32 { return 1; }").unwrap();
        assert!(
            !obj.is_empty(),
            "compile_source must return non-empty object bytes"
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
