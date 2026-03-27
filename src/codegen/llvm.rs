use std::collections::HashMap;

use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock as LlvmBasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType};
use inkwell::values::{FunctionValue, PointerValue};

use crate::bir::instruction::*;
use crate::error::Result;

use super::emit_structural::{emit_instruction, emit_terminator};
use super::types::{bir_type_to_llvm_type, build_struct_types, collect_value_types};

// Re-export mono functions so that `codegen::llvm::{...}` paths continue to work.
pub use super::mono_compile::{
    compile_module_with_mono, compile_to_module_with_mono, compile_with_mono,
};

pub(super) fn codegen_err(msg: impl Into<String>) -> crate::error::BengalError {
    crate::error::BengalError::CodegenError {
        message: msg.into(),
    }
}

/// Shared context for instruction/terminator emission within a single function.
pub(super) struct EmitCtx<'a, 'ctx> {
    pub(super) context: &'ctx Context,
    pub(super) module: &'a Module<'ctx>,
    pub(super) builder: &'a Builder<'ctx>,
    pub(super) current_fn: FunctionValue<'ctx>,
    pub(super) alloca_map: &'a HashMap<Value, PointerValue<'ctx>>,
    pub(super) value_types: &'a HashMap<Value, BirType>,
    pub(super) struct_types: &'a HashMap<String, inkwell::types::StructType<'ctx>>,
}

/// Compile a single BIR function into LLVM IR.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_function<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    builder: &Builder<'ctx>,
    bir_func: &BirFunction,
    llvm_func: FunctionValue<'ctx>,
    func_map: &HashMap<String, FunctionValue<'ctx>>,
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
    bir_module: &BirModule,
) -> Result<()> {
    let value_types = collect_value_types(bir_func, bir_module);

    // Pass 1: Declare all LLVM basic blocks
    let mut bb_map: HashMap<u32, LlvmBasicBlock<'ctx>> = HashMap::new();
    for bir_block in &bir_func.blocks {
        let llvm_bb = context.append_basic_block(llvm_func, &format!("bb{}", bir_block.label));
        bb_map.insert(bir_block.label, llvm_bb);
    }

    // Pass 2: Emit allocas in entry block and store function params
    let entry_bb = bb_map[&bir_func.blocks[0].label];
    builder.position_at_end(entry_bb);

    let mut alloca_map: HashMap<Value, PointerValue<'ctx>> = HashMap::new();
    for (val, ty) in &value_types {
        if *ty == BirType::Unit {
            continue;
        }
        let llvm_ty = bir_type_to_llvm_type(context, ty, struct_types)
            .ok_or_else(|| codegen_err(format!("unsupported type for alloca: {:?}", ty)))?;
        let alloca = builder
            .build_alloca(llvm_ty, &format!("v{}", val.0))
            .map_err(|e| codegen_err(e.to_string()))?;
        alloca_map.insert(*val, alloca);
    }

    // Store function params (track LLVM param index separately for Unit skipping)
    let mut llvm_param_idx = 0u32;
    for (val, ty) in &bir_func.params {
        if *ty == BirType::Unit {
            continue;
        }
        let param_val = llvm_func.get_nth_param(llvm_param_idx).unwrap();
        builder
            .build_store(alloca_map[val], param_val)
            .map_err(|e| codegen_err(e.to_string()))?;
        llvm_param_idx += 1;
    }

    // Pass 3: Emit instructions and terminators for each block
    let ctx = EmitCtx {
        context,
        module,
        builder,
        current_fn: llvm_func,
        alloca_map: &alloca_map,
        value_types: &value_types,
        struct_types,
    };

    for bir_block in &bir_func.blocks {
        let llvm_bb = bb_map[&bir_block.label];
        ctx.builder.position_at_end(llvm_bb);

        for inst in &bir_block.instructions {
            emit_instruction(&ctx, inst, func_map, bir_module)?;
        }

        emit_terminator(&ctx, &bir_block.terminator, &bb_map, &bir_func.blocks)?;
    }

    Ok(())
}

/// Compile BIR module to LLVM Module.
pub fn compile_to_module<'ctx>(
    context: &'ctx Context,
    bir_module: &BirModule,
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

    let struct_types = build_struct_types(context, bir_module);

    // Pass 1: Declare all functions
    let mut func_map: HashMap<String, FunctionValue<'ctx>> = HashMap::new();
    for bir_func in &bir_module.functions {
        let param_types: Vec<BasicMetadataTypeEnum> = bir_func
            .params
            .iter()
            .filter(|(_, ty)| !matches!(ty, BirType::Unit))
            .map(|(_, ty)| {
                bir_type_to_llvm_type(context, ty, &struct_types).ok_or_else(|| {
                    codegen_err(format!("non-Unit param must have LLVM type: {:?}", ty))
                })
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|t| t.into())
            .collect();

        let fn_type = if bir_func.return_type == BirType::Unit {
            context.void_type().fn_type(&param_types, false)
        } else {
            let ret_ty = bir_type_to_llvm_type(context, &bir_func.return_type, &struct_types)
                .ok_or_else(|| codegen_err("unsupported return type"))?;
            ret_ty.fn_type(&param_types, false)
        };

        let llvm_func = module.add_function(&bir_func.name, fn_type, None);
        func_map.insert(bir_func.name.clone(), llvm_func);
    }

    // Pass 2: Compile each function
    for bir_func in &bir_module.functions {
        let llvm_func = func_map[&bir_func.name];
        compile_function(
            context,
            &module,
            &builder,
            bir_func,
            llvm_func,
            &func_map,
            &struct_types,
            bir_module,
        )?;
    }

    Ok(module)
}

/// Compile a BIR module to native object code bytes, with external function declarations.
///
/// `external_functions` lists functions that are called by this module but defined in
/// other modules. Each entry is (mangled_name, param_types, return_type).
/// These are declared (but not defined) in the LLVM module so that call instructions
/// can reference them; the linker resolves them at link time.
pub fn compile_module(
    bir_module: &BirModule,
    external_functions: &[(String, Vec<BirType>, BirType)],
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

    let struct_types = build_struct_types(&context, bir_module);

    // Declare external functions first
    let mut func_map: HashMap<String, FunctionValue> = HashMap::new();
    for (name, param_tys, ret_ty) in external_functions {
        let param_types: Vec<BasicMetadataTypeEnum> = param_tys
            .iter()
            .filter(|ty| !matches!(ty, BirType::Unit))
            .map(|ty| {
                bir_type_to_llvm_type(&context, ty, &struct_types).ok_or_else(|| {
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
            let llvm_ret_ty = bir_type_to_llvm_type(&context, ret_ty, &struct_types)
                .ok_or_else(|| codegen_err("unsupported return type"))?;
            llvm_ret_ty.fn_type(&param_types, false)
        };

        let llvm_func = module.add_function(name, fn_type, None);
        func_map.insert(name.clone(), llvm_func);
    }

    // Declare all functions defined in this module
    for bir_func in &bir_module.functions {
        let param_types: Vec<BasicMetadataTypeEnum> = bir_func
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

        let fn_type = if bir_func.return_type == BirType::Unit {
            context.void_type().fn_type(&param_types, false)
        } else {
            let ret_ty = bir_type_to_llvm_type(&context, &bir_func.return_type, &struct_types)
                .ok_or_else(|| codegen_err("unsupported return type"))?;
            ret_ty.fn_type(&param_types, false)
        };

        let llvm_func = module.add_function(&bir_func.name, fn_type, None);
        func_map.insert(bir_func.name.clone(), llvm_func);
    }

    // Compile each function
    for bir_func in &bir_module.functions {
        let llvm_func = func_map[&bir_func.name];
        compile_function(
            &context,
            &module,
            &builder,
            bir_func,
            llvm_func,
            &func_map,
            &struct_types,
            bir_module,
        )?;
    }

    let buf = target_machine
        .write_to_memory_buffer(&module, FileType::Object)
        .map_err(|e| codegen_err(e.to_string()))?;

    Ok(buf.as_slice().to_vec())
}

/// Link multiple object files into an executable using the system linker.
pub fn link_objects(obj_files: &[std::path::PathBuf], output: &std::path::Path) -> Result<()> {
    let status = std::process::Command::new("cc")
        .args(obj_files.iter().map(|p| p.as_os_str()))
        .arg("-o")
        .arg(output)
        .status()
        .map_err(|e| codegen_err(format!("linker failed: {}", e)))?;
    if !status.success() {
        return Err(codegen_err("linker failed"));
    }
    Ok(())
}

/// Compile BIR module to native object code bytes.
pub fn compile(bir_module: &BirModule) -> Result<Vec<u8>> {
    let context = Context::create();
    let module = compile_to_module(&context, bir_module)?;

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

    let buf = target_machine
        .write_to_memory_buffer(&module, FileType::Object)
        .map_err(|e| codegen_err(e.to_string()))?;

    Ok(buf.as_slice().to_vec())
}

#[cfg(test)]
#[path = "llvm_tests.rs"]
mod tests;
