use std::collections::HashMap;

use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use inkwell::values::{BasicValueEnum, FunctionValue};

use crate::bir::instruction::*;
use crate::error::Result;

use super::emit_structural::{emit_bounds_check, load_value};
use super::llvm::{EmitCtx, codegen_err};
use super::types::bir_type_to_llvm_type;

/// Emit an arithmetic/scalar instruction (Literal, BinOp, Compare, Cast, Not).
pub(super) fn emit_arithmetic_instruction<'ctx>(
    ctx: &EmitCtx<'_, 'ctx>,
    inst: &Instruction,
    _func_map: &HashMap<String, FunctionValue<'ctx>>,
    _bir_module: &BirModule,
) -> Result<()> {
    match inst {
        Instruction::Literal { result, value, ty } => {
            if *ty == BirType::Unit {
                return Ok(());
            }
            let llvm_val: BasicValueEnum = match ty {
                BirType::I32 => ctx
                    .context
                    .i32_type()
                    .const_int(*value as u32 as u64, false)
                    .into(),
                BirType::I64 => ctx.context.i64_type().const_int(*value as u64, true).into(),
                BirType::F32 => {
                    let f = f32::from_bits(*value as u32);
                    ctx.context.f32_type().const_float(f as f64).into()
                }
                BirType::F64 => {
                    let f = f64::from_bits(*value as u64);
                    ctx.context.f64_type().const_float(f).into()
                }
                BirType::Bool => ctx
                    .context
                    .bool_type()
                    .const_int(*value as u64, false)
                    .into(),
                BirType::Unit => return Ok(()),
                BirType::Struct { .. } => return Err(codegen_err("cannot create struct literal")),
                BirType::Array { .. } => return Err(codegen_err("cannot create array literal")),
                BirType::TypeParam(name) => {
                    panic!("unresolved TypeParam '{name}' in codegen")
                }
                BirType::Error => panic!("BirType::Error reached codegen — this is a compiler bug"),
            };
            ctx.builder
                .build_store(ctx.alloca_map[result], llvm_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::BinaryOp {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => {
            let lhs_val = load_value(ctx, lhs).ok_or_else(|| codegen_err("BinaryOp on Unit"))?;
            let rhs_val = load_value(ctx, rhs).ok_or_else(|| codegen_err("BinaryOp on Unit"))?;

            let result_val: BasicValueEnum = match ty {
                BirType::I32 | BirType::I64 => {
                    let l = lhs_val.into_int_value();
                    let r = rhs_val.into_int_value();
                    match op {
                        BirBinOp::Add => ctx.builder.build_int_add(l, r, "add"),
                        BirBinOp::Sub => ctx.builder.build_int_sub(l, r, "sub"),
                        BirBinOp::Mul => ctx.builder.build_int_mul(l, r, "mul"),
                        BirBinOp::Div => ctx.builder.build_int_signed_div(l, r, "div"),
                    }
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into()
                }
                BirType::F32 | BirType::F64 => {
                    let l = lhs_val.into_float_value();
                    let r = rhs_val.into_float_value();
                    match op {
                        BirBinOp::Add => ctx.builder.build_float_add(l, r, "fadd"),
                        BirBinOp::Sub => ctx.builder.build_float_sub(l, r, "fsub"),
                        BirBinOp::Mul => ctx.builder.build_float_mul(l, r, "fmul"),
                        BirBinOp::Div => ctx.builder.build_float_div(l, r, "fdiv"),
                    }
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into()
                }
                _ => return Err(codegen_err(format!("unsupported BinaryOp type: {:?}", ty))),
            };
            ctx.builder
                .build_store(ctx.alloca_map[result], result_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::Compare {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => {
            let lhs_val = load_value(ctx, lhs).ok_or_else(|| codegen_err("Compare on Unit"))?;
            let rhs_val = load_value(ctx, rhs).ok_or_else(|| codegen_err("Compare on Unit"))?;

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
                    ctx.builder
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
                    ctx.builder
                        .build_float_compare(pred, l, r, "fcmp")
                        .map_err(|e| codegen_err(e.to_string()))?
                        .into()
                }
                _ => return Err(codegen_err(format!("unsupported Compare type: {:?}", ty))),
            };
            ctx.builder
                .build_store(ctx.alloca_map[result], cmp_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::Not { result, operand } => {
            let val = load_value(ctx, operand).ok_or_else(|| codegen_err("Not on Unit"))?;
            let zero = ctx.context.bool_type().const_zero();
            let not_val = ctx
                .builder
                .build_int_compare(IntPredicate::EQ, val.into_int_value(), zero, "not")
                .map_err(|e| codegen_err(e.to_string()))?;
            ctx.builder
                .build_store(ctx.alloca_map[result], not_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::Cast {
            result,
            operand,
            from_ty,
            to_ty,
        } => {
            if from_ty == to_ty {
                let val = load_value(ctx, operand).ok_or_else(|| codegen_err("Cast on Unit"))?;
                ctx.builder
                    .build_store(ctx.alloca_map[result], val)
                    .map_err(|e| codegen_err(e.to_string()))?;
                return Ok(());
            }
            let val = load_value(ctx, operand).ok_or_else(|| codegen_err("Cast on Unit"))?;
            let dest_ty = bir_type_to_llvm_type(ctx.context, to_ty, ctx.struct_types)
                .ok_or_else(|| codegen_err("Cast to Unit"))?;

            let cast_val: BasicValueEnum = match (from_ty, to_ty) {
                (BirType::I32, BirType::I64) => ctx
                    .builder
                    .build_int_s_extend(val.into_int_value(), ctx.context.i64_type(), "sext")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::I64, BirType::I32) => ctx
                    .builder
                    .build_int_truncate(val.into_int_value(), ctx.context.i32_type(), "trunc")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::I32 | BirType::I64, BirType::F32 | BirType::F64) => ctx
                    .builder
                    .build_signed_int_to_float(
                        val.into_int_value(),
                        dest_ty.into_float_type(),
                        "sitofp",
                    )
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::F32 | BirType::F64, BirType::I32 | BirType::I64) => ctx
                    .builder
                    .build_float_to_signed_int(
                        val.into_float_value(),
                        dest_ty.into_int_type(),
                        "fptosi",
                    )
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::F32, BirType::F64) => ctx
                    .builder
                    .build_float_ext(val.into_float_value(), ctx.context.f64_type(), "fpext")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::F64, BirType::F32) => ctx
                    .builder
                    .build_float_trunc(val.into_float_value(), ctx.context.f32_type(), "fptrunc")
                    .map_err(|e| codegen_err(e.to_string()))?
                    .into(),
                (BirType::Bool, BirType::I32 | BirType::I64) => ctx
                    .builder
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
            ctx.builder
                .build_store(ctx.alloca_map[result], cast_val)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::ArrayInit {
            result,
            ty,
            elements,
        } => {
            let llvm_ty = bir_type_to_llvm_type(ctx.context, ty, ctx.struct_types)
                .ok_or_else(|| codegen_err("ArrayInit: unsupported type"))?;
            let arr_ty = llvm_ty.into_array_type();
            let mut agg: inkwell::values::AggregateValueEnum = arr_ty.get_undef().into();
            for (i, elem_val) in elements.iter().enumerate() {
                let val = load_value(ctx, elem_val)
                    .ok_or_else(|| codegen_err("ArrayInit: failed to load element"))?;
                agg = ctx
                    .builder
                    .build_insert_value(agg, val, i as u32, "arr_insert")
                    .map_err(|e| codegen_err(e.to_string()))?;
            }
            ctx.builder
                .build_store(ctx.alloca_map[result], agg.into_array_value())
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::ArrayGet {
            result,
            ty,
            array,
            index,
            array_size,
        } => {
            let arr_val = load_value(ctx, array)
                .ok_or_else(|| codegen_err("ArrayGet: failed to load array"))?;
            let idx_val = load_value(ctx, index)
                .ok_or_else(|| codegen_err("ArrayGet: failed to load index"))?;

            // Determine the LLVM array type for GEP
            let arr_bir_ty = ctx
                .value_types
                .get(array)
                .ok_or_else(|| codegen_err("ArrayGet: missing array type"))?;
            let arr_llvm_ty = bir_type_to_llvm_type(ctx.context, arr_bir_ty, ctx.struct_types)
                .ok_or_else(|| codegen_err("ArrayGet: unsupported array type"))?;

            // Store the array to a temporary alloca for GEP
            let tmp_alloca = ctx
                .builder
                .build_alloca(arr_llvm_ty, "arr_tmp")
                .map_err(|e| codegen_err(e.to_string()))?;
            ctx.builder
                .build_store(tmp_alloca, arr_val)
                .map_err(|e| codegen_err(e.to_string()))?;

            // Runtime bounds check for variable indices
            let idx_int = idx_val.into_int_value();
            emit_bounds_check(ctx, idx_int, *array_size)?;

            // GEP to element: [0, index]
            let zero = ctx.context.i32_type().const_zero();
            let elem_ptr = unsafe {
                ctx.builder
                    .build_in_bounds_gep(arr_llvm_ty, tmp_alloca, &[zero, idx_int], "arr_elem_ptr")
                    .map_err(|e| codegen_err(e.to_string()))?
            };

            let elem_llvm_ty = bir_type_to_llvm_type(ctx.context, ty, ctx.struct_types)
                .ok_or_else(|| codegen_err("ArrayGet: unsupported element type"))?;
            let elem = ctx
                .builder
                .build_load(elem_llvm_ty, elem_ptr, "arr_elem")
                .map_err(|e| codegen_err(e.to_string()))?;
            ctx.builder
                .build_store(ctx.alloca_map[result], elem)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        Instruction::ArraySet {
            result,
            ty,
            array,
            index,
            value,
            array_size,
        } => {
            let arr_val = load_value(ctx, array)
                .ok_or_else(|| codegen_err("ArraySet: failed to load array"))?;
            let idx_val = load_value(ctx, index)
                .ok_or_else(|| codegen_err("ArraySet: failed to load index"))?;
            let new_val = load_value(ctx, value)
                .ok_or_else(|| codegen_err("ArraySet: failed to load value"))?;

            let arr_llvm_ty = bir_type_to_llvm_type(ctx.context, ty, ctx.struct_types)
                .ok_or_else(|| codegen_err("ArraySet: unsupported array type"))?;

            // Store the array to a temporary alloca for GEP
            let tmp_alloca = ctx
                .builder
                .build_alloca(arr_llvm_ty, "arr_tmp")
                .map_err(|e| codegen_err(e.to_string()))?;
            ctx.builder
                .build_store(tmp_alloca, arr_val)
                .map_err(|e| codegen_err(e.to_string()))?;

            // Runtime bounds check for variable indices
            let idx_int = idx_val.into_int_value();
            emit_bounds_check(ctx, idx_int, *array_size)?;

            // GEP to element and store
            let zero = ctx.context.i32_type().const_zero();
            let elem_ptr = unsafe {
                ctx.builder
                    .build_in_bounds_gep(arr_llvm_ty, tmp_alloca, &[zero, idx_int], "arr_elem_ptr")
                    .map_err(|e| codegen_err(e.to_string()))?
            };
            ctx.builder
                .build_store(elem_ptr, new_val)
                .map_err(|e| codegen_err(e.to_string()))?;

            // Load updated array and store to result
            let updated_arr = ctx
                .builder
                .build_load(arr_llvm_ty, tmp_alloca, "arr_updated")
                .map_err(|e| codegen_err(e.to_string()))?;
            ctx.builder
                .build_store(ctx.alloca_map[result], updated_arr)
                .map_err(|e| codegen_err(e.to_string()))?;
        }

        _ => unreachable!("emit_arithmetic_instruction called with non-arithmetic instruction"),
    }
    Ok(())
}
