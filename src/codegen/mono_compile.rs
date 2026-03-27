use std::collections::HashMap;

use inkwell::OptimizationLevel;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::FunctionValue;

use crate::bir::instruction::*;
use crate::bir::mono::MonoCollectResult;
use crate::error::Result;

use super::generic_resolution::resolve_function;
use super::llvm::{codegen_err, compile_function};
use super::types::{bir_type_to_llvm_type, build_generic_struct_types, build_struct_types};

/// Compile a single module's BIR to native object code with BIR-level monomorphization.
///
/// Combines `compile_module` (handles external function declarations for cross-module
/// calls) with `compile_with_mono` (handles BIR-level monomorphization of generics).
/// Generic instantiations are emitted with `LinkOnceODR` linkage so that duplicate
/// symbols from different modules are merged at link time.
pub fn compile_module_with_mono(
    bir_module: &BirModule,
    mono_result: &MonoCollectResult,
    external_functions: &[(String, Vec<BirType>, BirType)],
) -> Result<Vec<u8>> {
    use inkwell::module::Linkage;

    let context = Context::create();
    let module = context.create_module("bengal");
    let builder = context.create_builder();

    Target::initialize_native(&InitializationConfig {
        asm_printer: true,
        ..Default::default()
    })
    .map_err(|e| codegen_err(e.to_string()))?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| codegen_err(e.to_string()))?;
    let target_machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or_else(|| codegen_err("failed to create target machine"))?;

    module.set_data_layout(&target_machine.get_target_data().get_data_layout());
    module.set_triple(&triple);

    let mut struct_types = build_struct_types(&context, bir_module);
    build_generic_struct_types(&context, bir_module, mono_result, &mut struct_types);

    // Build function lookup map for resolving generic instances.
    let func_map_bir: HashMap<&str, &BirFunction> = bir_module
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();

    // Resolve all generic function instances into concrete BirFunctions.
    let resolved_instances: Vec<BirFunction> = mono_result
        .func_instances
        .iter()
        .filter_map(|instance| {
            let generic_func = func_map_bir.get(instance.func_name.as_str())?;
            Some(resolve_function(
                generic_func,
                instance,
                &bir_module.conformance_map,
            ))
        })
        .collect();

    // Collect non-generic functions (skip generic function templates).
    let non_generic_funcs: Vec<&BirFunction> = bir_module
        .functions
        .iter()
        .filter(|f| f.type_params.is_empty())
        .collect();

    // Pass 1: Declare external functions (from other modules).
    let mut func_map: HashMap<String, FunctionValue> = HashMap::new();
    declare_functions(
        &context,
        &module,
        external_functions,
        &struct_types,
        &mut func_map,
    )?;

    // Pass 2: Declare non-generic functions defined in this module.
    declare_bir_functions(
        &context,
        &module,
        &non_generic_funcs,
        &struct_types,
        &mut func_map,
    )?;

    // Pass 3: Declare resolved generic instances with LinkOnceODR linkage.
    for func in &resolved_instances {
        let param_types: Vec<BasicMetadataTypeEnum> = func
            .params
            .iter()
            .filter(|(_, ty)| !matches!(ty, BirType::Unit))
            .map(|(_, ty)| {
                bir_type_to_llvm_type(&context, ty, &struct_types).ok_or_else(|| {
                    codegen_err(format!("non-Unit param must have LLVM type: {:?}", ty))
                })
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|t| t.into())
            .collect();

        let fn_type = if func.return_type == BirType::Unit {
            context.void_type().fn_type(&param_types, false)
        } else {
            let ret_ty = bir_type_to_llvm_type(&context, &func.return_type, &struct_types)
                .ok_or_else(|| codegen_err("unsupported return type"))?;
            ret_ty.fn_type(&param_types, false)
        };

        let llvm_func = module.add_function(&func.name, fn_type, Some(Linkage::LinkOnceODR));
        func_map.insert(func.name.clone(), llvm_func);
    }

    // Pass 4: Compile non-generic functions.
    compile_bir_function_list(
        &context,
        &module,
        &builder,
        &non_generic_funcs,
        &func_map,
        &struct_types,
        bir_module,
    )?;

    // Pass 5: Compile resolved generic instances.
    let resolved_refs: Vec<&BirFunction> = resolved_instances.iter().collect();
    compile_bir_function_list(
        &context,
        &module,
        &builder,
        &resolved_refs,
        &func_map,
        &struct_types,
        bir_module,
    )?;

    let buf = target_machine
        .write_to_memory_buffer(&module, FileType::Object)
        .map_err(|e| codegen_err(e.to_string()))?;

    Ok(buf.as_slice().to_vec())
}

/// Compile BIR module with monomorphization support.
///
/// Takes a `BirModule` and `MonoCollectResult`, builds LLVM struct types for
/// generic struct instances, declares mangled generic function instances,
/// and compiles them with on-the-fly type substitution.
pub fn compile_with_mono(
    bir_module: &BirModule,
    mono_result: &MonoCollectResult,
) -> Result<Vec<u8>> {
    let context = Context::create();
    let module = context.create_module("bengal");
    let builder = context.create_builder();

    Target::initialize_native(&InitializationConfig {
        asm_printer: true,
        ..Default::default()
    })
    .map_err(|e| codegen_err(e.to_string()))?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| codegen_err(e.to_string()))?;
    let target_machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or_else(|| codegen_err("failed to create target machine"))?;

    module.set_data_layout(&target_machine.get_target_data().get_data_layout());
    module.set_triple(&triple);

    let mut struct_types = build_struct_types(&context, bir_module);
    build_generic_struct_types(&context, bir_module, mono_result, &mut struct_types);

    // Build function lookup map for resolving generic instances.
    let func_map_bir: HashMap<&str, &BirFunction> = bir_module
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();

    // Resolve all generic function instances into concrete BirFunctions.
    let resolved_instances: Vec<BirFunction> = mono_result
        .func_instances
        .iter()
        .filter_map(|instance| {
            let generic_func = func_map_bir.get(instance.func_name.as_str())?;
            Some(resolve_function(
                generic_func,
                instance,
                &bir_module.conformance_map,
            ))
        })
        .collect();

    // Collect non-generic functions (skip generic function templates).
    let non_generic_funcs: Vec<&BirFunction> = bir_module
        .functions
        .iter()
        .filter(|f| f.type_params.is_empty())
        .collect();

    // Pass 1: Declare all functions (non-generic + resolved instances).
    let mut func_map: HashMap<String, FunctionValue> = HashMap::new();

    let all_funcs_to_declare: Vec<&BirFunction> = non_generic_funcs
        .iter()
        .copied()
        .chain(resolved_instances.iter())
        .collect();

    declare_bir_functions(
        &context,
        &module,
        &all_funcs_to_declare,
        &struct_types,
        &mut func_map,
    )?;

    // Pass 2: Compile non-generic functions.
    compile_bir_function_list(
        &context,
        &module,
        &builder,
        &non_generic_funcs,
        &func_map,
        &struct_types,
        bir_module,
    )?;

    // Pass 3: Compile resolved generic instances.
    let resolved_refs: Vec<&BirFunction> = resolved_instances.iter().collect();
    compile_bir_function_list(
        &context,
        &module,
        &builder,
        &resolved_refs,
        &func_map,
        &struct_types,
        bir_module,
    )?;

    let buf = target_machine
        .write_to_memory_buffer(&module, FileType::Object)
        .map_err(|e| codegen_err(e.to_string()))?;

    Ok(buf.as_slice().to_vec())
}

/// Build an LLVM Module from a BIR module with monomorphization support.
///
/// Returns an LLVM `Module` suitable for JIT execution. This is the mono-aware
/// equivalent of `compile_to_module`.
pub fn compile_to_module_with_mono<'ctx>(
    context: &'ctx Context,
    bir_module: &BirModule,
    mono_result: &MonoCollectResult,
) -> Result<Module<'ctx>> {
    let module = context.create_module("bengal");
    let builder = context.create_builder();

    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| codegen_err(e.to_string()))?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| codegen_err(e.to_string()))?;
    let target_machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or_else(|| codegen_err("failed to create target machine"))?;

    module.set_data_layout(&target_machine.get_target_data().get_data_layout());
    module.set_triple(&triple);

    let mut struct_types = build_struct_types(context, bir_module);
    build_generic_struct_types(context, bir_module, mono_result, &mut struct_types);

    // Build function lookup map for resolving generic instances.
    let func_map_bir: HashMap<&str, &BirFunction> = bir_module
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f))
        .collect();

    // Resolve all generic function instances into concrete BirFunctions.
    let resolved_instances: Vec<BirFunction> = mono_result
        .func_instances
        .iter()
        .filter_map(|instance| {
            let generic_func = func_map_bir.get(instance.func_name.as_str())?;
            Some(resolve_function(
                generic_func,
                instance,
                &bir_module.conformance_map,
            ))
        })
        .collect();

    // Collect non-generic functions (skip generic function templates).
    let non_generic_funcs: Vec<&BirFunction> = bir_module
        .functions
        .iter()
        .filter(|f| f.type_params.is_empty())
        .collect();

    // Pass 1: Declare all functions (non-generic + resolved instances).
    let mut func_map: HashMap<String, FunctionValue> = HashMap::new();

    let all_funcs_to_declare: Vec<&BirFunction> = non_generic_funcs
        .iter()
        .copied()
        .chain(resolved_instances.iter())
        .collect();

    declare_bir_functions(
        context,
        &module,
        &all_funcs_to_declare,
        &struct_types,
        &mut func_map,
    )?;

    // Pass 2: Compile non-generic functions.
    compile_bir_function_list(
        context,
        &module,
        &builder,
        &non_generic_funcs,
        &func_map,
        &struct_types,
        bir_module,
    )?;

    // Pass 3: Compile resolved generic instances.
    let resolved_refs: Vec<&BirFunction> = resolved_instances.iter().collect();
    compile_bir_function_list(
        context,
        &module,
        &builder,
        &resolved_refs,
        &func_map,
        &struct_types,
        bir_module,
    )?;

    Ok(module)
}

/// Declare external functions (from other modules) in the LLVM module.
fn declare_functions<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    external_functions: &[(String, Vec<BirType>, BirType)],
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
    func_map: &mut HashMap<String, FunctionValue<'ctx>>,
) -> Result<()> {
    for (name, param_tys, ret_ty) in external_functions {
        let param_types: Vec<BasicMetadataTypeEnum> = param_tys
            .iter()
            .filter(|ty| !matches!(ty, BirType::Unit))
            .map(|ty| {
                bir_type_to_llvm_type(context, ty, struct_types).ok_or_else(|| {
                    codegen_err(format!("non-Unit param must have LLVM type: {:?}", ty))
                })
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|t| t.into())
            .collect();

        let fn_type = if *ret_ty == BirType::Unit {
            context.void_type().fn_type(&param_types, false)
        } else {
            let llvm_ret_ty = bir_type_to_llvm_type(context, ret_ty, struct_types)
                .ok_or_else(|| codegen_err("unsupported return type"))?;
            llvm_ret_ty.fn_type(&param_types, false)
        };

        let llvm_func = module.add_function(name, fn_type, None);
        func_map.insert(name.clone(), llvm_func);
    }
    Ok(())
}

/// Declare a list of BIR functions in the LLVM module (no linkage override).
fn declare_bir_functions<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    funcs: &[&BirFunction],
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
    func_map: &mut HashMap<String, FunctionValue<'ctx>>,
) -> Result<()> {
    for func in funcs {
        let param_types: Vec<BasicMetadataTypeEnum> = func
            .params
            .iter()
            .filter(|(_, ty)| !matches!(ty, BirType::Unit))
            .map(|(_, ty)| {
                bir_type_to_llvm_type(context, ty, struct_types).ok_or_else(|| {
                    codegen_err(format!("non-Unit param must have LLVM type: {:?}", ty))
                })
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|t| t.into())
            .collect();

        let fn_type = if func.return_type == BirType::Unit {
            context.void_type().fn_type(&param_types, false)
        } else {
            let ret_ty = bir_type_to_llvm_type(context, &func.return_type, struct_types)
                .ok_or_else(|| codegen_err("unsupported return type"))?;
            ret_ty.fn_type(&param_types, false)
        };

        let llvm_func = module.add_function(&func.name, fn_type, None);
        func_map.insert(func.name.clone(), llvm_func);
    }
    Ok(())
}

/// Compile a list of BIR functions.
fn compile_bir_function_list<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    funcs: &[&BirFunction],
    func_map: &HashMap<String, FunctionValue<'ctx>>,
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
    bir_module: &BirModule,
) -> Result<()> {
    for func in funcs {
        let llvm_func = func_map[&func.name];
        compile_function(
            context,
            module,
            builder,
            func,
            llvm_func,
            func_map,
            struct_types,
            bir_module,
        )?;
    }
    Ok(())
}
