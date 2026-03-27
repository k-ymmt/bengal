use std::collections::HashMap;

use super::super::instruction::*;
use super::{Lowering, SemInfoRef, check_acyclic_structs, semantic_type_to_bir};
use crate::error::{BengalError, DiagCtxt, Result};
use crate::parser::ast::*;

pub fn lower_program(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
) -> Result<BirModule> {
    let mut diag = DiagCtxt::new();
    match lower_program_with_inferred(program, sem_info, &HashMap::new(), &mut diag) {
        Ok(module) => Ok(module),
        Err(fatal) => {
            // If diag has errors, those are the accumulated lowering errors; return the first.
            // Otherwise, the error is a fatal pre-lowering error (e.g. recursive struct) -- return as-is.
            let mut errors = diag.take_errors();
            if errors.is_empty() {
                Err(fatal)
            } else {
                Err(errors.remove(0))
            }
        }
    }
}

/// Lower program with inferred type args for call sites with omitted type arguments.
pub(crate) fn lower_program_with_inferred(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
    inferred_type_args: &HashMap<NodeId, Vec<TypeAnnotation>>,
    diag: &mut DiagCtxt,
) -> Result<BirModule> {
    // Build struct_layouts from semantic StructInfo
    let mut struct_layouts: HashMap<String, Vec<(String, BirType)>> = HashMap::new();
    for (name, info) in &sem_info.struct_defs {
        let fields: Vec<(String, BirType)> = info
            .fields
            .iter()
            .map(|(n, t)| (n.clone(), semantic_type_to_bir(t)))
            .collect();
        struct_layouts.insert(name.clone(), fields);
    }

    // Build name-to-span lookup from AST StructDefs
    let struct_spans = build_struct_spans(program);

    // Reject Unit-typed stored fields
    reject_unit_fields(&struct_layouts, &struct_spans)?;

    // Reject recursive structs (infinitely sized)
    check_acyclic_structs(&struct_layouts, &struct_spans)?;

    let sem_info_ref = SemInfoRef {
        struct_defs: sem_info.struct_defs.clone(),
        struct_init_calls: sem_info.struct_init_calls.clone(),
        protocols: sem_info.protocols.clone(),
    };
    let mut lowering = Lowering::new(HashMap::new(), sem_info_ref);
    lowering.inferred_type_args = inferred_type_args.clone();

    // Build func_sigs using convert_type_with_structs (supports Named types)
    for func in &program.functions {
        // Push type params so generic return types resolve to TypeParam, not Struct
        lowering.current_type_params = func.type_params.clone();
        let bir_ty = lowering.convert_type_with_structs(&func.return_type);
        lowering.current_type_params.clear();
        lowering.func_sigs.insert(func.name.clone(), bir_ty);
        if !func.type_params.is_empty() {
            lowering.func_type_param_names.insert(
                func.name.clone(),
                func.type_params.iter().map(|tp| tp.name.clone()).collect(),
            );
        }
    }

    // Register mangled method signatures
    register_method_sigs(&mut lowering, sem_info);

    let mut functions: Vec<BirFunction> = program
        .functions
        .iter()
        .map(|f| lowering.lower_function(f))
        .collect();

    // Lower methods as flattened functions
    lower_methods(program, &mut lowering, &mut functions);

    collect_lowering_errors(&mut lowering, diag)?;

    // Build conformance_map
    let conformance_map = build_conformance_map_simple(program, sem_info);

    // Build struct_type_params from semantic StructInfo
    let struct_type_params = build_struct_type_params(sem_info);

    Ok(BirModule {
        struct_layouts,
        struct_type_params,
        functions,
        conformance_map,
    })
}

/// Lower a single module's AST to BIR with name mangling.
///
/// `name_map` maps local names (function names, `StructName_method` method names)
/// to their mangled equivalents. The caller is responsible for building this map
/// using `mangle::mangle_function()` and `mangle::mangle_method()`.
///
/// For the entry module's `main` function, the name_map should map "main" -> "main"
/// (i.e., not mangled) so that the linker can find the entry point.
pub fn lower_module(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
    name_map: &HashMap<String, String>,
) -> Result<BirModule> {
    let mut diag = DiagCtxt::new();
    match lower_module_with_inferred(program, sem_info, name_map, &HashMap::new(), &mut diag) {
        Ok(module) => Ok(module),
        Err(fatal) => {
            let mut errors = diag.take_errors();
            if errors.is_empty() {
                Err(fatal)
            } else {
                Err(errors.remove(0))
            }
        }
    }
}

/// Lower a single module's AST to BIR with name mangling and inferred type args.
///
/// Like `lower_module`, but also accepts `inferred_type_args` for call sites with
/// omitted type arguments (needed for BIR-level monomorphization).
pub(crate) fn lower_module_with_inferred(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
    name_map: &HashMap<String, String>,
    inferred_type_args: &HashMap<NodeId, Vec<TypeAnnotation>>,
    diag: &mut DiagCtxt,
) -> Result<BirModule> {
    // Build struct_layouts from semantic StructInfo
    let mut struct_layouts: HashMap<String, Vec<(String, BirType)>> = HashMap::new();
    for (name, info) in &sem_info.struct_defs {
        let fields: Vec<(String, BirType)> = info
            .fields
            .iter()
            .map(|(n, t)| (n.clone(), semantic_type_to_bir(t)))
            .collect();
        struct_layouts.insert(name.clone(), fields);
    }

    // Build name-to-span lookup from AST StructDefs
    let struct_spans = build_struct_spans(program);

    // Reject Unit-typed stored fields
    reject_unit_fields(&struct_layouts, &struct_spans)?;

    // Reject recursive structs (infinitely sized)
    check_acyclic_structs(&struct_layouts, &struct_spans)?;

    let sem_info_ref = SemInfoRef {
        struct_defs: sem_info.struct_defs.clone(),
        struct_init_calls: sem_info.struct_init_calls.clone(),
        protocols: sem_info.protocols.clone(),
    };
    let mut lowering = Lowering::new(HashMap::new(), sem_info_ref);
    lowering.name_map = Some(name_map.clone());
    lowering.inferred_type_args = inferred_type_args.clone();

    // Build func_sigs using mangled names
    for func in &program.functions {
        // Push type params so generic return types resolve to TypeParam, not Struct
        lowering.current_type_params = func.type_params.clone();
        let bir_ty = lowering.convert_type_with_structs(&func.return_type);
        lowering.current_type_params.clear();
        let resolved = lowering.resolve_name(&func.name);
        if !func.type_params.is_empty() {
            lowering.func_type_param_names.insert(
                resolved.clone(),
                func.type_params.iter().map(|tp| tp.name.clone()).collect(),
            );
        }
        lowering.func_sigs.insert(resolved, bir_ty);
    }

    // Register mangled method signatures (using resolve_name for mangling)
    for (struct_name, info) in &sem_info.struct_defs {
        for method in &info.methods {
            let local_mangled = format!("{}_{}", struct_name, method.name);
            let resolved = lowering.resolve_name(&local_mangled);
            let bir_ret = semantic_type_to_bir(&method.return_type);
            if !info.type_params.is_empty() {
                lowering.func_type_param_names.insert(
                    resolved.clone(),
                    info.type_params.iter().map(|tp| tp.name.clone()).collect(),
                );
            }
            lowering.func_sigs.insert(resolved, bir_ret);
        }
    }

    let mut functions: Vec<BirFunction> = program
        .functions
        .iter()
        .map(|f| lowering.lower_function(f))
        .collect();

    // Lower methods as flattened functions (using local_mangled_name so resolve_name can map it)
    for struct_def in &program.structs {
        for member in &struct_def.members {
            if let StructMember::Method {
                visibility: _,
                name: mname,
                params,
                return_type,
                body,
            } = member
            {
                let local_mangled_name = format!("{}_{}", struct_def.name, mname);
                let self_ty = build_self_type(struct_def);
                let mut all_params = vec![Param {
                    name: "self".to_string(),
                    ty: self_ty,
                }];
                all_params.extend(params.clone());
                // Use the local_mangled_name so resolve_name can map it
                let func = Function {
                    visibility: Visibility::Internal,
                    name: local_mangled_name,
                    type_params: struct_def.type_params.clone(),
                    params: all_params,
                    return_type: return_type.clone(),
                    body: body.clone(),
                    span: struct_def.span,
                };

                // Set up self context for lowering
                lowering.self_var_name = Some("self".to_string());
                let bir_func = lowering.lower_function(&func);
                lowering.self_var_name = None;
                functions.push(bir_func);
            }
        }
    }

    collect_lowering_errors(&mut lowering, diag)?;

    // Build conformance_map using resolve_name so the impl_name matches the mangled BIR function name.
    let mut conformance_map: HashMap<(String, BirType), String> = HashMap::new();
    for struct_def in &program.structs {
        for proto_name in &struct_def.conformances {
            if let Some(proto_info) = sem_info.protocols.get(proto_name) {
                for method in &proto_info.methods {
                    let key = (
                        format!("{}_{}", proto_name, method.name),
                        BirType::struct_simple(struct_def.name.clone()),
                    );
                    let local_impl_name = format!("{}_{}", struct_def.name, method.name);
                    let impl_name = lowering.resolve_name(&local_impl_name);
                    conformance_map.insert(key, impl_name);
                }
            }
        }
    }

    // Build struct_type_params from semantic StructInfo
    let struct_type_params = build_struct_type_params(sem_info);

    Ok(BirModule {
        struct_layouts,
        struct_type_params,
        functions,
        conformance_map,
    })
}

// ========== Shared helpers ==========

fn build_struct_spans(program: &Program) -> HashMap<&str, crate::error::Span> {
    program
        .structs
        .iter()
        .map(|s| (s.name.as_str(), s.span))
        .collect()
}

fn reject_unit_fields(
    struct_layouts: &HashMap<String, Vec<(String, BirType)>>,
    struct_spans: &HashMap<&str, crate::error::Span>,
) -> Result<()> {
    for (name, fields) in struct_layouts {
        for (fname, fty) in fields {
            if matches!(fty, BirType::Unit) {
                return Err(BengalError::LoweringError {
                    message: format!(
                        "struct `{}` has Unit-typed stored field `{}`; Unit fields are not supported",
                        name, fname
                    ),
                    span: struct_spans.get(name.as_str()).copied(),
                });
            }
        }
    }
    Ok(())
}

fn register_method_sigs(lowering: &mut Lowering, sem_info: &crate::semantic::SemanticInfo) {
    for (struct_name, info) in &sem_info.struct_defs {
        for method in &info.methods {
            let mangled = format!("{}_{}", struct_name, method.name);
            let bir_ret = semantic_type_to_bir(&method.return_type);
            lowering.func_sigs.insert(mangled.clone(), bir_ret);
            if !info.type_params.is_empty() {
                lowering.func_type_param_names.insert(
                    mangled,
                    info.type_params.iter().map(|tp| tp.name.clone()).collect(),
                );
            }
        }
    }
}

fn build_self_type(struct_def: &StructDef) -> TypeAnnotation {
    if struct_def.type_params.is_empty() {
        TypeAnnotation::Named(struct_def.name.clone())
    } else {
        TypeAnnotation::Generic {
            name: struct_def.name.clone(),
            args: struct_def
                .type_params
                .iter()
                .map(|tp| TypeAnnotation::Named(tp.name.clone()))
                .collect(),
        }
    }
}

fn lower_methods(program: &Program, lowering: &mut Lowering, functions: &mut Vec<BirFunction>) {
    for struct_def in &program.structs {
        for member in &struct_def.members {
            if let StructMember::Method {
                visibility: _,
                name: mname,
                params,
                return_type,
                body,
            } = member
            {
                let mangled_name = format!("{}_{}", struct_def.name, mname);
                let self_ty = build_self_type(struct_def);
                let mut all_params = vec![Param {
                    name: "self".to_string(),
                    ty: self_ty,
                }];
                all_params.extend(params.clone());
                let func = Function {
                    visibility: Visibility::Internal,
                    name: mangled_name,
                    type_params: struct_def.type_params.clone(),
                    params: all_params,
                    return_type: return_type.clone(),
                    body: body.clone(),
                    span: struct_def.span,
                };

                // Set up self context for lowering
                lowering.self_var_name = Some("self".to_string());
                let bir_func = lowering.lower_function(&func);
                lowering.self_var_name = None;
                functions.push(bir_func);
            }
        }
    }
}

fn collect_lowering_errors(lowering: &mut Lowering, diag: &mut DiagCtxt) -> Result<()> {
    for err in lowering.lowering_errors.drain(..) {
        diag.emit(err);
    }
    if diag.has_errors() {
        return Err(BengalError::LoweringError {
            message: "lowering failed".to_string(),
            span: None,
        });
    }
    Ok(())
}

fn build_conformance_map_simple(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
) -> HashMap<(String, BirType), String> {
    let mut conformance_map: HashMap<(String, BirType), String> = HashMap::new();
    for struct_def in &program.structs {
        for proto_name in &struct_def.conformances {
            if let Some(proto_info) = sem_info.protocols.get(proto_name) {
                for method in &proto_info.methods {
                    let key = (
                        format!("{}_{}", proto_name, method.name),
                        BirType::struct_simple(struct_def.name.clone()),
                    );
                    let impl_name = format!("{}_{}", struct_def.name, method.name);
                    conformance_map.insert(key, impl_name);
                }
            }
        }
    }
    conformance_map
}

fn build_struct_type_params(
    sem_info: &crate::semantic::SemanticInfo,
) -> HashMap<String, Vec<String>> {
    sem_info
        .struct_defs
        .iter()
        .filter(|(_, info)| !info.type_params.is_empty())
        .map(|(name, info)| {
            (
                name.clone(),
                info.type_params.iter().map(|tp| tp.name.clone()).collect(),
            )
        })
        .collect()
}
