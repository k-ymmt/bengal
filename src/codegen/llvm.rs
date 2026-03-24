use std::collections::HashMap;

use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock as LlvmBasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum};
use inkwell::values::{BasicValueEnum, FunctionValue, PointerValue};

use crate::bir::instruction::*;
use crate::error::{BengalError, Result};

fn codegen_err(msg: impl Into<String>) -> BengalError {
    BengalError::CodegenError {
        message: msg.into(),
    }
}

/// Convert BIR type to LLVM type. Returns None for Unit.
fn bir_type_to_llvm_type<'ctx>(
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
        BirType::Struct(name) => Some(struct_types.get(name)?.as_basic_type_enum()),
    }
}

/// Collect all Values in a BirFunction with their types.
fn collect_value_types(func: &BirFunction) -> HashMap<Value, BirType> {
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
                Instruction::Call { result, ty, .. } => (*result, ty.clone()),
                Instruction::StructInit { result, ty, .. } => (*result, ty.clone()),
                Instruction::FieldGet { result, ty, .. } => (*result, ty.clone()),
                Instruction::FieldSet { result, ty, .. } => (*result, ty.clone()),
            };
            value_types.insert(result, ty);
        }
    }

    value_types
}

/// Find a BasicBlock by label.
fn find_block(blocks: &[BasicBlock], label: u32) -> &BasicBlock {
    blocks.iter().find(|b| b.label == label).unwrap()
}

/// Load a BIR Value from its alloca. Returns None if the value is Unit type.
fn load_value<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    val: &Value,
    alloca_map: &HashMap<Value, PointerValue<'ctx>>,
    value_types: &HashMap<Value, BirType>,
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
) -> Option<BasicValueEnum<'ctx>> {
    let ty = value_types.get(val)?;
    if *ty == BirType::Unit {
        return None;
    }
    let llvm_ty = bir_type_to_llvm_type(context, ty, struct_types)?;
    let ptr = alloca_map.get(val)?;
    Some(
        builder
            .build_load(llvm_ty, *ptr, &format!("v{}", val.0))
            .unwrap(),
    )
}

/// Emit a single BIR instruction.
#[allow(clippy::too_many_arguments)]
fn emit_instruction<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    inst: &Instruction,
    alloca_map: &HashMap<Value, PointerValue<'ctx>>,
    value_types: &HashMap<Value, BirType>,
    func_map: &HashMap<String, FunctionValue<'ctx>>,
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
    bir_module: &BirModule,
) -> Result<()> {
    match inst {
        Instruction::Literal { result, value, ty } => {
            if *ty == BirType::Unit {
                return Ok(());
            }
            let llvm_val: BasicValueEnum = match ty {
                BirType::I32 => context
                    .i32_type()
                    .const_int(*value as u32 as u64, false)
                    .into(),
                BirType::I64 => context.i64_type().const_int(*value as u64, true).into(),
                BirType::F32 => {
                    let f = f32::from_bits(*value as u32);
                    context.f32_type().const_float(f as f64).into()
                }
                BirType::F64 => {
                    let f = f64::from_bits(*value as u64);
                    context.f64_type().const_float(f).into()
                }
                BirType::Bool => context.bool_type().const_int(*value as u64, false).into(),
                BirType::Unit => return Ok(()),
                BirType::Struct(_) => return Err(codegen_err("cannot create struct literal")),
            };
            builder
                .build_store(alloca_map[result], llvm_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::BinaryOp {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => {
            let lhs_val = load_value(context, builder, lhs, alloca_map, value_types, struct_types)
                .ok_or_else(|| codegen_err("BinaryOp on Unit"))?;
            let rhs_val = load_value(context, builder, rhs, alloca_map, value_types, struct_types)
                .ok_or_else(|| codegen_err("BinaryOp on Unit"))?;

            let result_val: BasicValueEnum = match ty {
                BirType::I32 | BirType::I64 => {
                    let l = lhs_val.into_int_value();
                    let r = rhs_val.into_int_value();
                    match op {
                        BirBinOp::Add => builder.build_int_add(l, r, "add"),
                        BirBinOp::Sub => builder.build_int_sub(l, r, "sub"),
                        BirBinOp::Mul => builder.build_int_mul(l, r, "mul"),
                        BirBinOp::Div => builder.build_int_signed_div(l, r, "div"),
                    }
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into()
                }
                BirType::F32 | BirType::F64 => {
                    let l = lhs_val.into_float_value();
                    let r = rhs_val.into_float_value();
                    match op {
                        BirBinOp::Add => builder.build_float_add(l, r, "fadd"),
                        BirBinOp::Sub => builder.build_float_sub(l, r, "fsub"),
                        BirBinOp::Mul => builder.build_float_mul(l, r, "fmul"),
                        BirBinOp::Div => builder.build_float_div(l, r, "fdiv"),
                    }
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into()
                }
                _ => return Err(codegen_err(format!("unsupported BinaryOp type: {:?}", ty))),
            };
            builder
                .build_store(alloca_map[result], result_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::Compare {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => {
            let lhs_val = load_value(context, builder, lhs, alloca_map, value_types, struct_types)
                .ok_or_else(|| codegen_err("Compare on Unit"))?;
            let rhs_val = load_value(context, builder, rhs, alloca_map, value_types, struct_types)
                .ok_or_else(|| codegen_err("Compare on Unit"))?;

            let cmp_val: BasicValueEnum = match ty {
                BirType::I32 | BirType::I64 | BirType::Bool => {
                    let l = lhs_val.into_int_value();
                    let r = rhs_val.into_int_value();
                    let pred = match op {
                        BirCompareOp::Eq => IntPredicate::EQ,
                        BirCompareOp::Ne => IntPredicate::NE,
                        BirCompareOp::Lt => IntPredicate::SLT,
                        BirCompareOp::Gt => IntPredicate::SGT,
                        BirCompareOp::Le => IntPredicate::SLE,
                        BirCompareOp::Ge => IntPredicate::SGE,
                    };
                    builder
                        .build_int_compare(pred, l, r, "cmp")
                        .map_err(|e| codegen_err(e.to_string()))?
                        .into()
                }
                BirType::F32 | BirType::F64 => {
                    let l = lhs_val.into_float_value();
                    let r = rhs_val.into_float_value();
                    let pred = match op {
                        BirCompareOp::Eq => FloatPredicate::OEQ,
                        BirCompareOp::Ne => FloatPredicate::ONE,
                        BirCompareOp::Lt => FloatPredicate::OLT,
                        BirCompareOp::Gt => FloatPredicate::OGT,
                        BirCompareOp::Le => FloatPredicate::OLE,
                        BirCompareOp::Ge => FloatPredicate::OGE,
                    };
                    builder
                        .build_float_compare(pred, l, r, "fcmp")
                        .map_err(|e| codegen_err(e.to_string()))?
                        .into()
                }
                _ => return Err(codegen_err(format!("unsupported Compare type: {:?}", ty))),
            };
            builder
                .build_store(alloca_map[result], cmp_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::Not { result, operand } => {
            let val = load_value(
                context,
                builder,
                operand,
                alloca_map,
                value_types,
                struct_types,
            )
            .ok_or_else(|| codegen_err("Not on Unit"))?;
            let zero = context.bool_type().const_zero();
            let not_val = builder
                .build_int_compare(IntPredicate::EQ, val.into_int_value(), zero, "not")
                .map_err(|e| codegen_err(e.to_string()))?;
            builder
                .build_store(alloca_map[result], not_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::Cast {
            result,
            operand,
            from_ty,
            to_ty,
        } => {
            if from_ty == to_ty {
                let val = load_value(
                    context,
                    builder,
                    operand,
                    alloca_map,
                    value_types,
                    struct_types,
                )
                .ok_or_else(|| codegen_err("Cast on Unit"))?;
                builder
                    .build_store(alloca_map[result], val)
                    .map_err(|e| codegen_err(e.to_string()))?;
                return Ok(());
            }
            let val = load_value(
                context,
                builder,
                operand,
                alloca_map,
                value_types,
                struct_types,
            )
            .ok_or_else(|| codegen_err("Cast on Unit"))?;
            let dest_ty = bir_type_to_llvm_type(context, to_ty, struct_types)
                .ok_or_else(|| codegen_err("Cast to Unit"))?;

            let cast_val: BasicValueEnum = match (from_ty, to_ty) {
                (BirType::I32, BirType::I64) => builder
                    .build_int_s_extend(val.into_int_value(), context.i64_type(), "sext")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::I64, BirType::I32) => builder
                    .build_int_truncate(val.into_int_value(), context.i32_type(), "trunc")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::I32 | BirType::I64, BirType::F32 | BirType::F64) => builder
                    .build_signed_int_to_float(
                        val.into_int_value(),
                        dest_ty.into_float_type(),
                        "sitofp",
                    )
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::F32 | BirType::F64, BirType::I32 | BirType::I64) => builder
                    .build_float_to_signed_int(
                        val.into_float_value(),
                        dest_ty.into_int_type(),
                        "fptosi",
                    )
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::F32, BirType::F64) => builder
                    .build_float_ext(val.into_float_value(), context.f64_type(), "fpext")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::F64, BirType::F32) => builder
                    .build_float_trunc(val.into_float_value(), context.f32_type(), "fptrunc")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::Bool, BirType::I32 | BirType::I64) => builder
                    .build_int_z_extend(val.into_int_value(), dest_ty.into_int_type(), "zext")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                _ => {
                    return Err(codegen_err(format!(
                        "unsupported cast: {:?} -> {:?}",
                        from_ty, to_ty
                    )));
                }
            };
            builder
                .build_store(alloca_map[result], cast_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::Call {
            result,
            func_name,
            args,
            ty,
        } => {
            let callee = func_map
                .get(func_name.as_str())
                .ok_or_else(|| codegen_err(format!("unknown function: {}", func_name)))?;
            let mut call_args: Vec<BasicValueEnum> = Vec::new();
            for arg in args {
                let arg_ty = value_types.get(arg);
                if arg_ty == Some(&BirType::Unit) || arg_ty.is_none() {
                    continue;
                }
                let v = load_value(context, builder, arg, alloca_map, value_types, struct_types)
                    .ok_or_else(|| codegen_err("Call: failed to load non-Unit argument"))?;
                call_args.push(v);
            }
            let args_meta: Vec<inkwell::values::BasicMetadataValueEnum> =
                call_args.iter().map(|v| (*v).into()).collect();
            let call_site = builder
                .build_call(*callee, &args_meta, "call")
                .map_err(|e| codegen_err(e.to_string()))?;
            if *ty != BirType::Unit {
                match call_site.try_as_basic_value() {
                    inkwell::values::ValueKind::Basic(ret_val) => {
                        builder
                            .build_store(alloca_map[result], ret_val)
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
            ..
        } => {
            let llvm_struct_ty = struct_types
                .get(struct_name.as_str())
                .ok_or_else(|| codegen_err(format!("unknown struct: {}", struct_name)))?;
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
                let val = load_value(
                    context,
                    builder,
                    field_val,
                    alloca_map,
                    value_types,
                    struct_types,
                )
                .ok_or_else(|| codegen_err("StructInit: failed to load field value"))?;
                agg = builder
                    .build_insert_value(agg, val, field_idx as u32, "insert")
                    .map_err(|e| codegen_err(e.to_string()))?;
            }
            builder
                .build_store(alloca_map[result], agg.into_struct_value())
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
                BirType::Struct(name) => name,
                _ => return Err(codegen_err("FieldGet on non-struct type")),
            };
            let layout = bir_module
                .struct_layouts
                .get(struct_name.as_str())
                .ok_or_else(|| codegen_err(format!("no layout for struct: {}", struct_name)))?;
            let field_idx = layout.iter().position(|(n, _)| n == field).ok_or_else(|| {
                codegen_err(format!("unknown field {} in struct {}", field, struct_name))
            })?;
            let struct_val = load_value(
                context,
                builder,
                object,
                alloca_map,
                value_types,
                struct_types,
            )
            .ok_or_else(|| codegen_err("FieldGet: failed to load struct value"))?;
            let field_val = builder
                .build_extract_value(struct_val.into_struct_value(), field_idx as u32, "field")
                .map_err(|e| codegen_err(e.to_string()))?;
            builder
                .build_store(alloca_map[result], field_val)
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
                BirType::Struct(name) => name,
                _ => return Err(codegen_err("FieldSet on non-struct type")),
            };
            let layout = bir_module
                .struct_layouts
                .get(struct_name.as_str())
                .ok_or_else(|| codegen_err(format!("no layout for struct: {}", struct_name)))?;
            let field_idx = layout.iter().position(|(n, _)| n == field).ok_or_else(|| {
                codegen_err(format!("unknown field {} in struct {}", field, struct_name))
            })?;
            let struct_val = load_value(
                context,
                builder,
                object,
                alloca_map,
                value_types,
                struct_types,
            )
            .ok_or_else(|| codegen_err("FieldSet: failed to load struct value"))?;
            let new_field_val = load_value(
                context,
                builder,
                value,
                alloca_map,
                value_types,
                struct_types,
            )
            .ok_or_else(|| codegen_err("FieldSet: failed to load new field value"))?;
            let updated = builder
                .build_insert_value(
                    struct_val.into_struct_value(),
                    new_field_val,
                    field_idx as u32,
                    "update",
                )
                .map_err(|e| codegen_err(e.to_string()))?;
            builder
                .build_store(alloca_map[result], updated.into_struct_value())
                .map_err(|e| codegen_err(e.to_string()))?;
        }
    }
    Ok(())
}

/// Store branch args to target block params' allocas.
fn store_br_args<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    args: &[(Value, BirType)],
    target_params: &[(Value, BirType)],
    alloca_map: &HashMap<Value, PointerValue<'ctx>>,
    value_types: &HashMap<Value, BirType>,
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
) -> Result<()> {
    for (i, (val, ty)) in args.iter().enumerate() {
        if *ty == BirType::Unit {
            continue;
        }
        let loaded = load_value(context, builder, val, alloca_map, value_types, struct_types)
            .ok_or_else(|| codegen_err("store_br_args: failed to load value"))?;
        let target_val = &target_params[i].0;
        builder
            .build_store(alloca_map[target_val], loaded)
            .map_err(|e| codegen_err(e.to_string()))?;
    }
    Ok(())
}

/// Emit a BIR terminator.
#[allow(clippy::too_many_arguments)]
fn emit_terminator<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    terminator: &Terminator,
    alloca_map: &HashMap<Value, PointerValue<'ctx>>,
    bb_map: &HashMap<u32, LlvmBasicBlock<'ctx>>,
    bir_blocks: &[BasicBlock],
    value_types: &HashMap<Value, BirType>,
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
) -> Result<()> {
    match terminator {
        Terminator::Return(val) => {
            let ty = value_types.get(val);
            if ty == Some(&BirType::Unit) || ty.is_none() {
                builder
                    .build_return(None)
                    .map_err(|e| codegen_err(e.to_string()))?;
            } else {
                let loaded =
                    load_value(context, builder, val, alloca_map, value_types, struct_types)
                        .ok_or_else(|| codegen_err("Return: failed to load value"))?;
                builder
                    .build_return(Some(&loaded))
                    .map_err(|e| codegen_err(e.to_string()))?;
            }
        }

        Terminator::ReturnVoid => {
            builder
                .build_return(None)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Terminator::Br { target, args } => {
            let target_block = find_block(bir_blocks, *target);
            store_br_args(
                context,
                builder,
                args,
                &target_block.params,
                alloca_map,
                value_types,
                struct_types,
            )?;
            builder
                .build_unconditional_branch(bb_map[target])
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Terminator::CondBr {
            cond,
            then_bb,
            else_bb,
        } => {
            let cond_val = load_value(
                context,
                builder,
                cond,
                alloca_map,
                value_types,
                struct_types,
            )
            .ok_or_else(|| codegen_err("CondBr: failed to load condition"))?;
            builder
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
            // Store mutable var args to header block params
            let header_block = find_block(bir_blocks, *header_bb);
            store_br_args(
                context,
                builder,
                args,
                &header_block.params,
                alloca_map,
                value_types,
                struct_types,
            )?;
            // Store break value to exit block params
            if let Some((val, ty)) = value
                && *ty != BirType::Unit
            {
                let exit_block = find_block(bir_blocks, *exit_bb);
                if !exit_block.params.is_empty() {
                    let loaded =
                        load_value(context, builder, val, alloca_map, value_types, struct_types)
                            .ok_or_else(|| codegen_err("BrBreak: failed to load break value"))?;
                    let exit_param = &exit_block.params[0].0;
                    builder
                        .build_store(alloca_map[exit_param], loaded)
                        .map_err(|e| codegen_err(e.to_string()))?;
                }
            }
            builder
                .build_unconditional_branch(bb_map[exit_bb])
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Terminator::BrContinue { header_bb, args } => {
            let header_block = find_block(bir_blocks, *header_bb);
            store_br_args(
                context,
                builder,
                args,
                &header_block.params,
                alloca_map,
                value_types,
                struct_types,
            )?;
            builder
                .build_unconditional_branch(bb_map[header_bb])
                .map_err(|e| codegen_err(e.to_string()))?;
        }
    }
    Ok(())
}

/// Compile a single BIR function into LLVM IR.
fn compile_function<'ctx>(
    context: &'ctx Context,
    builder: &Builder<'ctx>,
    bir_func: &BirFunction,
    llvm_func: FunctionValue<'ctx>,
    func_map: &HashMap<String, FunctionValue<'ctx>>,
    struct_types: &HashMap<String, inkwell::types::StructType<'ctx>>,
    bir_module: &BirModule,
) -> Result<()> {
    let value_types = collect_value_types(bir_func);

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
    for bir_block in &bir_func.blocks {
        let llvm_bb = bb_map[&bir_block.label];
        builder.position_at_end(llvm_bb);

        for inst in &bir_block.instructions {
            emit_instruction(
                context,
                builder,
                inst,
                &alloca_map,
                &value_types,
                func_map,
                struct_types,
                bir_module,
            )?;
        }

        emit_terminator(
            context,
            builder,
            &bir_block.terminator,
            &alloca_map,
            &bb_map,
            &bir_func.blocks,
            &value_types,
            struct_types,
        )?;
    }

    Ok(())
}

/// Build LLVM named struct types from BIR struct layouts (2-pass).
fn build_struct_types<'ctx>(
    context: &'ctx Context,
    bir_module: &BirModule,
) -> HashMap<String, inkwell::types::StructType<'ctx>> {
    let mut struct_types = HashMap::new();

    // Pass 1: Create opaque structs
    for name in bir_module.struct_layouts.keys() {
        let llvm_struct = context.opaque_struct_type(name);
        struct_types.insert(name.clone(), llvm_struct);
    }

    // Pass 2: Set struct bodies
    for (name, fields) in &bir_module.struct_layouts {
        let field_types: Vec<BasicTypeEnum<'ctx>> = fields
            .iter()
            .map(|(_, ty)| {
                bir_type_to_llvm_type(context, ty, &struct_types)
                    .expect("struct field must have a valid LLVM type")
            })
            .collect();
        struct_types[name].set_body(&field_types, false);
    }

    struct_types
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
mod tests {
    use super::*;
    use crate::bir;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::semantic;

    fn compile_and_run(source: &str) -> i32 {
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
        bir::optimize_module(&mut bir_module);

        let context = Context::create();
        let module = compile_to_module(&context, &bir_module).unwrap();

        let engine = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .unwrap();

        unsafe {
            let main_fn = engine
                .get_function::<unsafe extern "C" fn() -> i32>("main")
                .unwrap();
            main_fn.call()
        }
    }

    #[test]
    fn test_literal_return() {
        assert_eq!(compile_and_run("func main() -> Int32 { return 42; }"), 42);
    }

    #[test]
    fn test_arithmetic() {
        assert_eq!(compile_and_run("func main() -> Int32 { return 2 + 3; }"), 5);
    }

    #[test]
    fn test_call() {
        assert_eq!(
            compile_and_run(
                "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }"
            ),
            7
        );
    }

    #[test]
    fn test_let_variable() {
        assert_eq!(
            compile_and_run("func main() -> Int32 { let x: Int32 = 10; return x + 1; }"),
            11
        );
    }

    #[test]
    fn test_if_else() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }"
            ),
            1
        );
    }

    #[test]
    fn test_divergent_if_else() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int32 = if false { return 99; } else { yield 42; }; return x; }"
            ),
            42
        );
    }

    #[test]
    fn test_while() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var s: Int32 = 0; var i: Int32 = 0; while i < 3 { s = s + i; i = i + 1; }; return s; }"
            ),
            3
        );
    }

    #[test]
    fn test_break() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var i: Int32 = 0; while true { if i == 3 { break; }; i = i + 1; }; return i; }"
            ),
            3
        );
    }

    #[test]
    fn test_continue() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }"
            ),
            12
        );
    }

    #[test]
    fn test_break_value_mutable_var() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { i = i + 1; if i == 5 { break i * 10; }; } nobreak { yield 0; }; return x + i; }"
            ),
            55
        );
    }

    #[test]
    fn test_nobreak_condition_false() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }"
            ),
            3
        );
    }

    #[test]
    fn test_cast() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int64 = 100 as Int64; return x as Int32; }"
            ),
            100
        );
    }

    #[test]
    fn test_unit_call() {
        assert_eq!(
            compile_and_run("func noop() { return; } func main() -> Int32 { noop(); return 42; }"),
            42
        );
    }

    #[test]
    fn test_float() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Float64 = 3.5; let y: Float64 = 1.5; return (x + y) as Int32; }"
            ),
            5
        );
    }

    #[test]
    fn test_comparison() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int32 = if 3 > 2 { yield 1; } else { yield 0; }; return x; }"
            ),
            1
        );
    }

    #[test]
    fn test_i64_arithmetic() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int64 = 10 as Int64; let y: Int64 = 20 as Int64; return (x + y) as Int32; }"
            ),
            30
        );
    }

    #[test]
    fn test_object_emit() {
        let source = "func main() -> Int32 { return 42; }";
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
        bir::optimize_module(&mut bir_module);

        let obj_bytes = compile(&bir_module).unwrap();
        assert!(!obj_bytes.is_empty(), "object output must not be empty");
    }

    // --- Phase 3: Struct codegen tests ---

    #[test]
    fn test_struct_basic() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x + p.y; }"
            ),
            7
        );
    }

    #[test]
    fn test_struct_field_assign() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1, y: 2); p.x = 10; return p.x; }"
            ),
            10
        );
    }

    #[test]
    fn test_struct_as_function_arg() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func get_x(p: Point) -> Int32 { return p.x; } func main() -> Int32 { let p = Point(x: 42, y: 0); return get_x(p); }"
            ),
            42
        );
    }

    #[test]
    fn test_struct_as_return_value() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func make_point() -> Point { return Point(x: 5, y: 6); } func main() -> Int32 { let p = make_point(); return p.x + p.y; }"
            ),
            11
        );
    }

    #[test]
    fn test_struct_in_if() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = if true { yield Point(x: 1, y: 2); } else { yield Point(x: 3, y: 4); }; return p.x; }"
            ),
            1
        );
    }

    #[test]
    fn test_struct_computed_property() {
        assert_eq!(
            compile_and_run(
                "struct Rect { var w: Int32; var h: Int32; var area: Int32 { get { return self.w * self.h; } }; } func main() -> Int32 { let r = Rect(w: 3, h: 4); return r.area; }"
            ),
            12
        );
    }

    #[test]
    fn test_struct_explicit_init() {
        assert_eq!(
            compile_and_run(
                "struct Counter { var count: Int32; init(start: Int32) { self.count = start * 2; } } func main() -> Int32 { let c = Counter(start: 5); return c.count; }"
            ),
            10
        );
    }

    #[test]
    fn test_struct_nested_field_assign() {
        assert_eq!(
            compile_and_run(
                "struct Inner { var x: Int32; } struct Outer { var inner: Inner; } func main() -> Int32 { var o = Outer(inner: Inner(x: 1)); o.inner.x = 10; return o.inner.x; }"
            ),
            10
        );
    }

    #[test]
    fn test_struct_param_no_local_init() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func sum(p: Point) -> Int32 { return p.x + p.y; } func main() -> Int32 { return sum(Point(x: 10, y: 20)); }"
            ),
            30
        );
    }

    #[test]
    fn test_struct_mutable_in_loop() {
        assert_eq!(
            compile_and_run(
                "struct Acc { var val: Int32; } func main() -> Int32 { var a = Acc(val: 0); var i: Int32 = 0; while i < 5 { a.val = a.val + i; i = i + 1; }; return a.val; }"
            ),
            10
        );
    }

    #[test]
    fn test_struct_valued_while_break() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var i: Int32 = 0; let p = while i < 10 { i = i + 1; if i == 3 { break Point(x: i, y: i * 2); }; } nobreak { yield Point(x: 0, y: 0); }; return p.x + p.y; }"
            ),
            9
        );
    }

    #[test]
    fn test_struct_computed_setter() {
        assert_eq!(
            compile_and_run(
                "struct Foo { var x: Int32; var bar: Int32 { get { return self.x; } set { self.x = newValue * 2; } }; } func main() -> Int32 { var f = Foo(x: 1); f.bar = 5; return f.x; }"
            ),
            10
        );
    }

    #[test]
    fn test_struct_object_emit() {
        let source = "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x + p.y; }";
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
        bir::optimize_module(&mut bir_module);

        let obj_bytes = compile(&bir_module).unwrap();
        assert!(!obj_bytes.is_empty(), "object output must not be empty");
    }

    #[test]
    fn test_struct_init_field_access() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { return Point(x: 1, y: 2).x; }"
            ),
            1
        );
    }

    #[test]
    fn test_struct_empty() {
        assert_eq!(
            compile_and_run("struct Empty {} func main() -> Int32 { let e = Empty(); return 0; }"),
            0
        );
    }

    #[test]
    fn test_struct_continue_in_loop() {
        assert_eq!(
            compile_and_run(
                "struct Acc { var val: Int32; } func main() -> Int32 { var a = Acc(val: 0); var i: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; a.val = a.val + 1; }; return a.val; }"
            ),
            4
        );
    }

    #[test]
    fn test_struct_nobreak_yield() {
        assert_eq!(
            compile_and_run(
                "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = while false { break Point(x: 0, y: 0); } nobreak { yield Point(x: 7, y: 8); }; return p.x + p.y; }"
            ),
            15
        );
    }
}
