use std::collections::HashMap;

use inkwell::context::Context;
use inkwell::types::{BasicType, BasicTypeEnum};

use crate::bir::instruction::*;
use crate::bir::mono::{Instance, MonoCollectResult, resolve_bir_type};

/// Convert BIR type to LLVM type. Returns None for Unit.
pub(super) fn bir_type_to_llvm_type<'ctx>(
    context: &'ctx Context,
    ty: &BirType,
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
) -> Option<BasicTypeEnum<'ctx>> {
    match ty {
        BirType::I32 => Some(context.i32_type().into()),
        BirType::I64 => Some(context.i64_type().into()),
        BirType::F32 => Some(context.f32_type().into()),
        BirType::F64 => Some(context.f64_type().into()),
        BirType::Bool => Some(context.bool_type().into()),
        BirType::Unit => None,
        BirType::Struct { name, type_args } => {
            // For generic struct instances, look up by mangled name.
            let lookup_name = if type_args.is_empty() {
                name.clone()
            } else {
                let inst = Instance {
                    func_name: name.clone(),
                    type_args: type_args.clone(),
                };
                inst.mangled_name()
            };
            Some(struct_types.get(&lookup_name)?.as_basic_type_enum())
        }
        BirType::Array { element, size } => {
            let elem_ty = bir_type_to_llvm_type(context, element, struct_types)?;
            Some(elem_ty.array_type(*size as u32).into())
        }
        BirType::TypeParam(name) => panic!("unresolved TypeParam '{name}' in codegen"),
        BirType::Error => panic!("BirType::Error reached codegen — this is a compiler bug"),
    }
}

/// Collect all Values in a BirFunction with their types.
///
/// For generic call/struct-init instructions (non-empty type_args), this resolves
/// TypeParam return types to their concrete substitutions using the generic function's
/// type_params from the BirModule.
pub(super) fn collect_value_types(
    func: &BirFunction,
    bir_module: &BirModule,
) -> HashMap<Value, BirType> {
    use crate::bir::mono::resolve_bir_type_lenient;

    // Build lookup: func_name -> type_params for resolving generic call return types
    let func_type_params: HashMap<&str, &[String]> = bir_module
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f.type_params.as_slice()))
        .collect();

    let mut value_types = HashMap::new();

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
                Instruction::Call {
                    result,
                    func_name,
                    type_args,
                    ty,
                    ..
                } => {
                    if type_args.is_empty() {
                        (*result, ty.clone())
                    } else {
                        // Resolve the return type by substituting type_args
                        let subst: HashMap<String, BirType> = func_type_params
                            .get(func_name.as_str())
                            .map(|params| {
                                params
                                    .iter()
                                    .zip(type_args.iter())
                                    .map(|(p, a)| (p.clone(), a.clone()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        (*result, resolve_bir_type_lenient(ty, &subst))
                    }
                }
                Instruction::StructInit {
                    result,
                    type_args,
                    ty,
                    ..
                } => {
                    if type_args.is_empty() {
                        (*result, ty.clone())
                    } else {
                        // Generic struct init — resolve TypeParam in the type
                        (*result, ty.clone())
                    }
                }
                Instruction::FieldGet {
                    result, object, ty, ..
                } => {
                    // If the field type contains TypeParam, resolve it using
                    // the object's concrete type_args.
                    if contains_type_param(ty) {
                        if let Some(BirType::Struct {
                            name: sname,
                            type_args,
                        }) = value_types.get(object)
                        {
                            let subst: HashMap<String, BirType> = bir_module
                                .struct_type_params
                                .get(sname)
                                .map(|params| {
                                    params
                                        .iter()
                                        .zip(type_args.iter())
                                        .map(|(p, a)| (p.clone(), a.clone()))
                                        .collect()
                                })
                                .unwrap_or_default();
                            (*result, resolve_bir_type_lenient(ty, &subst))
                        } else {
                            (*result, ty.clone())
                        }
                    } else {
                        (*result, ty.clone())
                    }
                }
                Instruction::FieldSet { result, ty, .. } => (*result, ty.clone()),
                Instruction::ArrayInit { result, ty, .. } => (*result, ty.clone()),
                Instruction::ArrayGet { result, ty, .. } => (*result, ty.clone()),
                Instruction::ArraySet { result, ty, .. } => (*result, ty.clone()),
            };
            value_types.insert(result, ty);
        }
    }

    value_types
}

/// Check if a BirType contains any TypeParam (indicates a generic template).
pub(super) fn contains_type_param(ty: &BirType) -> bool {
    match ty {
        BirType::TypeParam(_) => true,
        BirType::Array { element, .. } => contains_type_param(element),
        BirType::Struct { type_args, .. } => type_args.iter().any(contains_type_param),
        _ => false,
    }
}

/// Build LLVM named struct types from BIR struct layouts (2-pass).
pub(super) fn build_struct_types<'ctx>(
    context: &'ctx Context,
    bir_module: &BirModule,
) -> HashMap<String, inkwell::types::StructType<'ctx>> {
    let mut struct_types = HashMap::new();

    // Skip generic struct templates (fields contain TypeParam).
    // They are handled by build_generic_struct_types after mono resolution.
    let concrete_layouts: Vec<(&String, &Vec<(String, BirType)>)> = bir_module
        .struct_layouts
        .iter()
        .filter(|(_, fields)| !fields.iter().any(|(_, ty)| contains_type_param(ty)))
        .collect();

    // Pass 1: Create opaque structs
    for (name, _) in &concrete_layouts {
        let llvm_struct = context.opaque_struct_type(name);
        struct_types.insert((*name).clone(), llvm_struct);
    }

    // Pass 2: Set struct bodies
    for (name, fields) in &concrete_layouts {
        let field_types: Vec<BasicTypeEnum<'ctx>> = fields
            .iter()
            .map(|(_, ty)| {
                bir_type_to_llvm_type(context, ty, &struct_types)
                    .expect("struct field must have a valid LLVM type")
            })
            .collect();
        struct_types[*name].set_body(&field_types, false);
    }

    struct_types
}

/// Build LLVM struct types for generic struct instances.
///
/// For each `(struct_name, concrete_type_args)` in the mono result, looks up the
/// generic layout, extracts TypeParam names in order of first appearance, builds
/// a substitution map, resolves field types, and creates an LLVM struct under the
/// mangled name (e.g., `Pair_Int32_Bool`).
pub(super) fn build_generic_struct_types<'ctx>(
    context: &'ctx Context,
    bir_module: &BirModule,
    mono_result: &MonoCollectResult,
    struct_types: &mut HashMap<String, inkwell::types::StructType<'ctx>>,
) {
    // Pass 1: Create opaque structs for all generic instances.
    let mut instance_infos: Vec<(String, Vec<(String, BirType)>)> = Vec::new();

    for (struct_name, concrete_type_args) in &mono_result.struct_instances {
        let layout = match bir_module.struct_layouts.get(struct_name) {
            Some(layout) => layout,
            None => continue,
        };

        // Extract unique TypeParam names from layout fields in order of first appearance.
        let mut type_param_names: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (_, ty) in layout {
            collect_type_params(ty, &mut type_param_names, &mut seen);
        }

        // Build substitution map.
        let subst: HashMap<String, BirType> = type_param_names
            .iter()
            .zip(concrete_type_args.iter())
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();

        // Resolve field types.
        let resolved_fields: Vec<(String, BirType)> = layout
            .iter()
            .map(|(name, ty)| (name.clone(), resolve_bir_type(ty, &subst)))
            .collect();

        // Compute mangled name.
        let mangled_name = {
            let inst = Instance {
                func_name: struct_name.clone(),
                type_args: concrete_type_args.clone(),
            };
            inst.mangled_name()
        };

        let llvm_struct = context.opaque_struct_type(&mangled_name);
        struct_types.insert(mangled_name.clone(), llvm_struct);
        instance_infos.push((mangled_name, resolved_fields));
    }

    // Pass 2: Set struct bodies (after all are created, to allow mutual references).
    for (mangled_name, resolved_fields) in &instance_infos {
        let field_types: Vec<BasicTypeEnum<'ctx>> = resolved_fields
            .iter()
            .map(|(_, ty)| {
                bir_type_to_llvm_type(context, ty, struct_types)
                    .expect("generic struct field must have a valid LLVM type")
            })
            .collect();
        struct_types[mangled_name].set_body(&field_types, false);
    }
}

/// Recursively collect TypeParam names from a BirType in order of first appearance.
pub(super) fn collect_type_params(
    ty: &BirType,
    names: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    match ty {
        BirType::TypeParam(name) => {
            if seen.insert(name.clone()) {
                names.push(name.clone());
            }
        }
        BirType::Array { element, .. } => {
            collect_type_params(element, names, seen);
        }
        BirType::Struct { type_args, .. } => {
            for arg in type_args {
                collect_type_params(arg, names, seen);
            }
        }
        _ => {}
    }
}
