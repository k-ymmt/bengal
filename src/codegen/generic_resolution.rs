use std::collections::HashMap;

use crate::bir::instruction::*;
use crate::bir::mono::{Instance, resolve_bir_type};

/// Resolve all BirTypes in an instruction through the substitution map.
/// Also resolves protocol method calls via the conformance map.
pub(super) fn resolve_instruction(
    inst: &Instruction,
    subst: &HashMap<String, BirType>,
    conformance_map: &HashMap<(String, BirType), String>,
) -> Instruction {
    match inst {
        Instruction::Literal { result, value, ty } => Instruction::Literal {
            result: *result,
            value: *value,
            ty: resolve_bir_type(ty, subst),
        },
        Instruction::BinaryOp {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => Instruction::BinaryOp {
            result: *result,
            op: *op,
            lhs: *lhs,
            rhs: *rhs,
            ty: resolve_bir_type(ty, subst),
        },
        Instruction::Call {
            result,
            func_name,
            args,
            type_args,
            ty,
        } => {
            let resolved_type_args: Vec<BirType> = type_args
                .iter()
                .map(|t| resolve_bir_type(t, subst))
                .collect();

            // Check if this is a protocol method call that can be resolved
            // via the conformance map.
            if let Some(first_type_arg) = resolved_type_args.first() {
                let key = (func_name.clone(), first_type_arg.clone());
                if let Some(concrete_name) = conformance_map.get(&key) {
                    // Resolved to a concrete implementation — direct call.
                    return Instruction::Call {
                        result: *result,
                        func_name: concrete_name.clone(),
                        args: args.clone(),
                        type_args: vec![],
                        ty: resolve_bir_type(ty, subst),
                    };
                }
            }

            // If the call has type_args, mangle the function name.
            let resolved_func_name = if resolved_type_args.is_empty() {
                func_name.clone()
            } else {
                let inst = Instance {
                    func_name: func_name.clone(),
                    type_args: resolved_type_args.clone(),
                };
                inst.mangled_name()
            };
            Instruction::Call {
                result: *result,
                func_name: resolved_func_name,
                args: args.clone(),
                type_args: resolved_type_args,
                ty: resolve_bir_type(ty, subst),
            }
        }
        Instruction::Compare {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => Instruction::Compare {
            result: *result,
            op: *op,
            lhs: *lhs,
            rhs: *rhs,
            ty: resolve_bir_type(ty, subst),
        },
        Instruction::Not { result, operand } => Instruction::Not {
            result: *result,
            operand: *operand,
        },
        Instruction::Cast {
            result,
            operand,
            from_ty,
            to_ty,
        } => Instruction::Cast {
            result: *result,
            operand: *operand,
            from_ty: resolve_bir_type(from_ty, subst),
            to_ty: resolve_bir_type(to_ty, subst),
        },
        Instruction::StructInit {
            result,
            struct_name,
            fields,
            type_args,
            ty,
        } => {
            let resolved_type_args: Vec<BirType> = type_args
                .iter()
                .map(|t| resolve_bir_type(t, subst))
                .collect();
            Instruction::StructInit {
                result: *result,
                struct_name: struct_name.clone(),
                fields: fields.clone(),
                type_args: resolved_type_args,
                ty: resolve_bir_type(ty, subst),
            }
        }
        Instruction::FieldGet {
            result,
            object,
            field,
            object_ty,
            ty,
        } => Instruction::FieldGet {
            result: *result,
            object: *object,
            field: field.clone(),
            object_ty: resolve_bir_type(object_ty, subst),
            ty: resolve_bir_type(ty, subst),
        },
        Instruction::FieldSet {
            result,
            object,
            field,
            value,
            ty,
        } => Instruction::FieldSet {
            result: *result,
            object: *object,
            field: field.clone(),
            value: *value,
            ty: resolve_bir_type(ty, subst),
        },
        Instruction::ArrayInit {
            result,
            ty,
            elements,
        } => Instruction::ArrayInit {
            result: *result,
            ty: resolve_bir_type(ty, subst),
            elements: elements.clone(),
        },
        Instruction::ArrayGet {
            result,
            ty,
            array,
            index,
            array_size,
        } => Instruction::ArrayGet {
            result: *result,
            ty: resolve_bir_type(ty, subst),
            array: *array,
            index: *index,
            array_size: *array_size,
        },
        Instruction::ArraySet {
            result,
            ty,
            array,
            index,
            value,
            array_size,
        } => Instruction::ArraySet {
            result: *result,
            ty: resolve_bir_type(ty, subst),
            array: *array,
            index: *index,
            value: *value,
            array_size: *array_size,
        },
    }
}

/// Resolve all BirTypes in a terminator through the substitution map.
pub(super) fn resolve_terminator(
    term: &Terminator,
    subst: &HashMap<String, BirType>,
) -> Terminator {
    match term {
        Terminator::Return(val) => Terminator::Return(*val),
        Terminator::ReturnVoid => Terminator::ReturnVoid,
        Terminator::Br { target, args } => Terminator::Br {
            target: *target,
            args: args
                .iter()
                .map(|(v, ty)| (*v, resolve_bir_type(ty, subst)))
                .collect(),
        },
        Terminator::CondBr {
            cond,
            then_bb,
            else_bb,
        } => Terminator::CondBr {
            cond: *cond,
            then_bb: *then_bb,
            else_bb: *else_bb,
        },
        Terminator::BrBreak {
            header_bb,
            exit_bb,
            args,
            value,
        } => Terminator::BrBreak {
            header_bb: *header_bb,
            exit_bb: *exit_bb,
            args: args
                .iter()
                .map(|(v, ty)| (*v, resolve_bir_type(ty, subst)))
                .collect(),
            value: value
                .as_ref()
                .map(|(v, ty)| (*v, resolve_bir_type(ty, subst))),
        },
        Terminator::BrContinue { header_bb, args } => Terminator::BrContinue {
            header_bb: *header_bb,
            args: args
                .iter()
                .map(|(v, ty)| (*v, resolve_bir_type(ty, subst)))
                .collect(),
        },
    }
}

/// Resolve all BirTypes in a basic block through the substitution map.
/// Also resolves protocol method calls via the conformance map.
pub(super) fn resolve_basic_block(
    block: &BasicBlock,
    subst: &HashMap<String, BirType>,
    conformance_map: &HashMap<(String, BirType), String>,
) -> BasicBlock {
    BasicBlock {
        label: block.label,
        params: block
            .params
            .iter()
            .map(|(v, ty)| (*v, resolve_bir_type(ty, subst)))
            .collect(),
        instructions: block
            .instructions
            .iter()
            .map(|inst| resolve_instruction(inst, subst, conformance_map))
            .collect(),
        terminator: resolve_terminator(&block.terminator, subst),
    }
}

/// Create a fully resolved (monomorphized) BirFunction from a generic function
/// and a concrete Instance, resolving protocol method calls via the conformance map.
pub(super) fn resolve_function(
    generic_func: &BirFunction,
    instance: &Instance,
    conformance_map: &HashMap<(String, BirType), String>,
) -> BirFunction {
    let subst = instance.substitution_map(&generic_func.type_params);
    BirFunction {
        name: instance.mangled_name(),
        type_params: vec![],
        params: generic_func
            .params
            .iter()
            .map(|(v, ty)| (*v, resolve_bir_type(ty, &subst)))
            .collect(),
        return_type: resolve_bir_type(&generic_func.return_type, &subst),
        blocks: generic_func
            .blocks
            .iter()
            .map(|b| resolve_basic_block(b, &subst, conformance_map))
            .collect(),
        body: vec![],
    }
}
