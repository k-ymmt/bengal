use std::collections::HashMap;

use inkwell::IntPredicate;
use inkwell::basic_block::BasicBlock as LlvmBasicBlock;
use inkwell::values::{BasicValueEnum, FunctionValue};

use crate::bir::instruction::*;
use crate::bir::mono::Instance;
use crate::error::Result;

use super::emit_arithmetic;
use super::llvm::{EmitCtx, codegen_err};
use super::types::bir_type_to_llvm_type;

/// Find a BasicBlock by label.
pub(super) fn find_block(blocks: &[BasicBlock], label: u32) -> &BasicBlock {
    blocks.iter().find(|b| b.label == label).unwrap()
}

/// Load a BIR Value from its alloca. Returns None if the value is Unit type.
pub(super) fn load_value<'ctx>(
    ctx: &EmitCtx<'_, 'ctx>,
    val: &Value,
) -> Option<BasicValueEnum<'ctx>> {
    let ty = ctx.value_types.get(val)?;
    if *ty == BirType::Unit {
        return None;
    }
    let llvm_ty = bir_type_to_llvm_type(ctx.context, ty, ctx.struct_types)?;
    let ptr = ctx.alloca_map.get(val)?;
    Some(
        ctx.builder
            .build_load(llvm_ty, *ptr, &format!("v{}", val.0))
            .unwrap(),
    )
}

/// Emit a runtime bounds check: if index >= size, call llvm.trap.
pub(super) fn emit_bounds_check<'ctx>(
    ctx: &EmitCtx<'_, 'ctx>,
    index_val: inkwell::values::IntValue<'ctx>,
    size: u64,
) -> Result<()> {
    let size_const = index_val.get_type().const_int(size, false);
    let in_bounds = ctx
        .builder
        .build_int_compare(IntPredicate::ULT, index_val, size_const, "bounds_check")
        .map_err(|e| codegen_err(e.to_string()))?;

    let ok_bb = ctx.context.append_basic_block(ctx.current_fn, "bounds_ok");
    let trap_bb = ctx
        .context
        .append_basic_block(ctx.current_fn, "bounds_trap");

    ctx.builder
        .build_conditional_branch(in_bounds, ok_bb, trap_bb)
        .map_err(|e| codegen_err(e.to_string()))?;

    // Trap block: call llvm.trap then unreachable
    ctx.builder.position_at_end(trap_bb);
    let trap_fn = ctx.module.get_function("llvm.trap").unwrap_or_else(|| {
        let fn_type = ctx.context.void_type().fn_type(&[], false);
        ctx.module.add_function("llvm.trap", fn_type, None)
    });
    ctx.builder
        .build_call(trap_fn, &[], "")
        .map_err(|e| codegen_err(e.to_string()))?;
    ctx.builder
        .build_unreachable()
        .map_err(|e| codegen_err(e.to_string()))?;

    // Continue in ok block
    ctx.builder.position_at_end(ok_bb);
    Ok(())
}

/// Emit a single BIR instruction, dispatching to arithmetic or structural helpers.
pub(super) fn emit_instruction<'ctx>(
    ctx: &EmitCtx<'_, 'ctx>,
    inst: &Instruction,
    func_map: &HashMap<String, FunctionValue<'ctx>>,
    bir_module: &BirModule,
) -> Result<()> {
    match inst {
        // Arithmetic / scalar / array ops
        Instruction::Literal { .. }
        | Instruction::BinaryOp { .. }
        | Instruction::Compare { .. }
        | Instruction::Not { .. }
        | Instruction::Cast { .. }
        | Instruction::ArrayInit { .. }
        | Instruction::ArrayGet { .. }
        | Instruction::ArraySet { .. } => {
            emit_arithmetic::emit_arithmetic_instruction(ctx, inst, func_map, bir_module)
        }

        // Structural ops (Call, Struct*)
        Instruction::Call { .. }
        | Instruction::StructInit { .. }
        | Instruction::FieldGet { .. }
        | Instruction::FieldSet { .. } => {
            emit_structural_instruction(ctx, inst, func_map, bir_module)
        }
    }
}

/// Emit a structural instruction (Call, StructInit, FieldGet, FieldSet).
fn emit_structural_instruction<'ctx>(
    ctx: &EmitCtx<'_, 'ctx>,
    inst: &Instruction,
    func_map: &HashMap<String, FunctionValue<'ctx>>,
    bir_module: &BirModule,
) -> Result<()> {
    match inst {
        Instruction::Call {
            result,
            func_name,
            args,
            type_args,
            ty,
        } => {
            // If this is a generic call (has type_args), mangle the function name
            // and resolve the return type.
            let (resolved_name, resolved_ty) = if !type_args.is_empty() {
                let inst = Instance {
                    func_name: func_name.clone(),
                    type_args: type_args.clone(),
                };
                // Build a substitution map from the generic function's type params
                // to the provided type args.
                let generic_func = bir_module.functions.iter().find(|f| f.name == *func_name);
                let subst: HashMap<String, BirType> = if let Some(gf) = generic_func {
                    gf.type_params
                        .iter()
                        .zip(type_args.iter())
                        .map(|(tp, ta)| (tp.clone(), ta.clone()))
                        .collect()
                } else {
                    HashMap::new()
                };
                let resolved = crate::bir::mono::resolve_bir_type(ty, &subst);
                (inst.mangled_name(), resolved)
            } else {
                (func_name.clone(), ty.clone())
            };
            let callee = func_map
                .get(resolved_name.as_str())
                .ok_or_else(|| codegen_err(format!("unknown function: {}", resolved_name)))?;
            let mut call_args: Vec<BasicValueEnum> = Vec::new();
            for arg in args {
                let arg_ty = ctx.value_types.get(arg);
                if arg_ty == Some(&BirType::Unit) || arg_ty.is_none() {
                    continue;
                }
                let v = load_value(ctx, arg)
                    .ok_or_else(|| codegen_err("Call: failed to load non-Unit argument"))?;
                call_args.push(v);
            }
            let args_meta: Vec<inkwell::values::BasicMetadataValueEnum> =
                call_args.iter().map(|v| (*v).into()).collect();
            let call_site = ctx
                .builder
                .build_call(*callee, &args_meta, "call")
                .map_err(|e| codegen_err(e.to_string()))?;
            if resolved_ty != BirType::Unit {
                match call_site.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(ret_val) => {
                        ctx.builder
                            .build_store(ctx.alloca_map[result], ret_val)
                            .map_err(|e| codegen_err(e.to_string()))?;
                    }
                    _ => {
                        return Err(codegen_err(
                            "Call: expected basic return value for non-Unit return type",
                        ));
                    }
                }
            }
        }

        Instruction::StructInit {
            result,
            struct_name,
            fields,
            type_args,
            ..
        } => {
            // For generic struct instances, look up LLVM type by mangled name.
            let llvm_lookup_name = if type_args.is_empty() {
                struct_name.clone()
            } else {
                let inst = Instance {
                    func_name: struct_name.clone(),
                    type_args: type_args.clone(),
                };
                inst.mangled_name()
            };
            let llvm_struct_ty = ctx
                .struct_types
                .get(llvm_lookup_name.as_str())
                .ok_or_else(|| codegen_err(format!("unknown struct: {}", llvm_lookup_name)))?;
            // Layout is always keyed by base struct name.
            let layout = bir_module
                .struct_layouts
                .get(struct_name.as_str())
                .ok_or_else(|| codegen_err(format!("no layout for struct: {}", struct_name)))?;
            let mut agg: inkwell::values::AggregateValueEnum = llvm_struct_ty.get_undef().into();
            for (field_name, field_val) in fields {
                let field_idx = layout
                    .iter()
                    .position(|(n, _)| n == field_name)
                    .ok_or_else(|| {
                        codegen_err(format!(
                            "unknown field {} in struct {}",
                            field_name, struct_name
                        ))
                    })?;
                let val = load_value(ctx, field_val)
                    .ok_or_else(|| codegen_err("StructInit: failed to load field value"))?;
                agg = ctx
                    .builder
                    .build_insert_value(agg, val, field_idx as u32, "insert")
                    .map_err(|e| codegen_err(e.to_string()))?;
            }
            ctx.builder
                .build_store(ctx.alloca_map[result], agg.into_struct_value())
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::FieldGet {
            result,
            object,
            field,
            object_ty,
            ..
        } => {
            let struct_name = match object_ty {
                BirType::Struct { name, .. } => name,
                _ => return Err(codegen_err("FieldGet on non-struct type")),
            };
            let layout = bir_module
                .struct_layouts
                .get(struct_name.as_str())
                .ok_or_else(|| codegen_err(format!("no layout for struct: {}", struct_name)))?;
            let field_idx = layout.iter().position(|(n, _)| n == field).ok_or_else(|| {
                codegen_err(format!("unknown field {} in struct {}", field, struct_name))
            })?;
            let struct_val = load_value(ctx, object)
                .ok_or_else(|| codegen_err("FieldGet: failed to load struct value"))?;
            let field_val = ctx
                .builder
                .build_extract_value(struct_val.into_struct_value(), field_idx as u32, "field")
                .map_err(|e| codegen_err(e.to_string()))?;
            ctx.builder
                .build_store(ctx.alloca_map[result], field_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::FieldSet {
            result,
            object,
            field,
            value,
            ty,
        } => {
            let struct_name = match ty {
                BirType::Struct { name, .. } => name,
                _ => return Err(codegen_err("FieldSet on non-struct type")),
            };
            let layout = bir_module
                .struct_layouts
                .get(struct_name.as_str())
                .ok_or_else(|| codegen_err(format!("no layout for struct: {}", struct_name)))?;
            let field_idx = layout.iter().position(|(n, _)| n == field).ok_or_else(|| {
                codegen_err(format!("unknown field {} in struct {}", field, struct_name))
            })?;
            let struct_val = load_value(ctx, object)
                .ok_or_else(|| codegen_err("FieldSet: failed to load struct value"))?;
            let new_field_val = load_value(ctx, value)
                .ok_or_else(|| codegen_err("FieldSet: failed to load new field value"))?;
            let updated = ctx
                .builder
                .build_insert_value(
                    struct_val.into_struct_value(),
                    new_field_val,
                    field_idx as u32,
                    "update",
                )
                .map_err(|e| codegen_err(e.to_string()))?;
            ctx.builder
                .build_store(ctx.alloca_map[result], updated.into_struct_value())
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        _ => unreachable!("emit_structural_instruction called with non-structural instruction"),
    }
    Ok(())
}

/// Store branch args to target block params' allocas.
pub(super) fn store_br_args<'ctx>(
    ctx: &EmitCtx<'_, 'ctx>,
    args: &[(Value, BirType)],
    target_params: &[(Value, BirType)],
) -> Result<()> {
    for (i, (val, ty)) in args.iter().enumerate() {
        if *ty == BirType::Unit {
            continue;
        }
        let loaded = load_value(ctx, val)
            .ok_or_else(|| codegen_err("store_br_args: failed to load value"))?;
        let target_val = &target_params[i].0;
        ctx.builder
            .build_store(ctx.alloca_map[target_val], loaded)
            .map_err(|e| codegen_err(e.to_string()))?;
    }
    Ok(())
}

/// Emit a BIR terminator.
pub(super) fn emit_terminator<'ctx>(
    ctx: &EmitCtx<'_, 'ctx>,
    terminator: &Terminator,
    bb_map: &HashMap<u32, LlvmBasicBlock<'ctx>>,
    bir_blocks: &[BasicBlock],
) -> Result<()> {
    match terminator {
        Terminator::Return(val) => {
            let ty = ctx.value_types.get(val);
            if ty == Some(&BirType::Unit) || ty.is_none() {
                ctx.builder
                    .build_return(None)
                    .map_err(|e| codegen_err(e.to_string()))?;
            } else {
                let loaded = load_value(ctx, val)
                    .ok_or_else(|| codegen_err("Return: failed to load value"))?;
                ctx.builder
                    .build_return(Some(&loaded))
                    .map_err(|e| codegen_err(e.to_string()))?;
            }
        }

        Terminator::ReturnVoid => {
            ctx.builder
                .build_return(None)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Terminator::Br { target, args } => {
            let target_block = find_block(bir_blocks, *target);
            store_br_args(ctx, args, &target_block.params)?;
            ctx.builder
                .build_unconditional_branch(bb_map[target])
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Terminator::CondBr {
            cond,
            then_bb,
            else_bb,
        } => {
            let cond_val = load_value(ctx, cond)
                .ok_or_else(|| codegen_err("CondBr: failed to load condition"))?;
            ctx.builder
                .build_conditional_branch(
                    cond_val.into_int_value(),
                    bb_map[then_bb],
                    bb_map[else_bb],
                )
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Terminator::BrBreak {
            header_bb,
            exit_bb,
            args,
            value,
        } => {
            let header_block = find_block(bir_blocks, *header_bb);
            store_br_args(ctx, args, &header_block.params)?;
            if let Some((val, ty)) = value
                && *ty != BirType::Unit
            {
                let exit_block = find_block(bir_blocks, *exit_bb);
                if !exit_block.params.is_empty() {
                    let loaded = load_value(ctx, val)
                        .ok_or_else(|| codegen_err("BrBreak: failed to load break value"))?;
                    let exit_param = &exit_block.params[0].0;
                    ctx.builder
                        .build_store(ctx.alloca_map[exit_param], loaded)
                        .map_err(|e| codegen_err(e.to_string()))?;
                }
            }
            ctx.builder
                .build_unconditional_branch(bb_map[exit_bb])
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Terminator::BrContinue { header_bb, args } => {
            let header_block = find_block(bir_blocks, *header_bb);
            store_br_args(ctx, args, &header_block.params)?;
            ctx.builder
                .build_unconditional_branch(bb_map[header_bb])
                .map_err(|e| codegen_err(e.to_string()))?;
        }
    }
    Ok(())
}
