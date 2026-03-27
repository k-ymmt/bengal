use super::instruction::*;

fn format_type(ty: &BirType) -> String {
    match ty {
        BirType::Unit => "()".to_string(),
        BirType::I32 => "Int32".to_string(),
        BirType::I64 => "Int64".to_string(),
        BirType::F32 => "Float32".to_string(),
        BirType::F64 => "Float64".to_string(),
        BirType::Bool => "Bool".to_string(),
        BirType::Struct { name, type_args } => {
            if type_args.is_empty() {
                name.clone()
            } else {
                let args: Vec<String> = type_args.iter().map(format_type).collect();
                format!("{}<{}>", name, args.join(", "))
            }
        }
        BirType::Array { element, size } => {
            format!("[{}; {}]", format_type(element), size)
        }
        BirType::TypeParam(name) => name.clone(),
        BirType::Error => "<error>".to_string(),
    }
}

fn format_binop(op: &BirBinOp) -> &str {
    match op {
        BirBinOp::Add => "add",
        BirBinOp::Sub => "sub",
        BirBinOp::Mul => "mul",
        BirBinOp::Div => "div",
    }
}

fn format_compare_op(op: &BirCompareOp) -> &str {
    match op {
        BirCompareOp::Eq => "eq",
        BirCompareOp::Ne => "ne",
        BirCompareOp::Lt => "lt",
        BirCompareOp::Gt => "gt",
        BirCompareOp::Le => "le",
        BirCompareOp::Ge => "ge",
    }
}

fn format_value(v: &Value) -> String {
    format!("%{}", v.0)
}

pub fn print_module(module: &BirModule) -> String {
    let mut out = String::new();
    for func in &module.functions {
        print_function(func, &mut out);
    }
    out
}

fn print_function(func: &BirFunction, out: &mut String) {
    out.push_str("bir @");
    out.push_str(&func.name);
    if !func.type_params.is_empty() {
        out.push('<');
        out.push_str(&func.type_params.join(", "));
        out.push('>');
    }
    out.push('(');
    for (i, (val, ty)) in func.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{}: {}", format_value(val), format_type(ty)));
    }
    out.push_str(") -> ");
    out.push_str(&format_type(&func.return_type));
    out.push_str(" {\n");

    for block in &func.blocks {
        print_block(block, out);
    }

    out.push_str("}\n");
}

fn print_block(block: &BasicBlock, out: &mut String) {
    out.push_str(&format!("bb{}", block.label));
    if !block.params.is_empty() {
        out.push('(');
        for (i, (val, ty)) in block.params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{}: {}", format_value(val), format_type(ty)));
        }
        out.push(')');
    }
    out.push_str(":\n");

    for inst in &block.instructions {
        out.push_str("    ");
        print_instruction(inst, out);
        out.push('\n');
    }

    out.push_str("    ");
    print_terminator(&block.terminator, out);
    out.push('\n');
}

fn print_instruction(inst: &Instruction, out: &mut String) {
    match inst {
        Instruction::Literal { result, value, ty } => {
            out.push_str(&format!(
                "{} = literal {} : {}",
                format_value(result),
                value,
                format_type(ty)
            ));
        }
        Instruction::BinaryOp {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => {
            out.push_str(&format!(
                "{} = binary_op {} {}, {} : {}",
                format_value(result),
                format_binop(op),
                format_value(lhs),
                format_value(rhs),
                format_type(ty)
            ));
        }
        Instruction::Call {
            result,
            func_name,
            args,
            type_args,
            ty,
        } => {
            let args_str: Vec<String> = args.iter().map(format_value).collect();
            if type_args.is_empty() {
                out.push_str(&format!(
                    "{} = call @{}({}) : {}",
                    format_value(result),
                    func_name,
                    args_str.join(", "),
                    format_type(ty)
                ));
            } else {
                let type_args_str: Vec<String> = type_args.iter().map(format_type).collect();
                out.push_str(&format!(
                    "{} = call @{}({}) type_args=[{}] : {}",
                    format_value(result),
                    func_name,
                    args_str.join(", "),
                    type_args_str.join(", "),
                    format_type(ty)
                ));
            }
        }
        Instruction::Compare {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => {
            out.push_str(&format!(
                "{} = compare {} {}, {} : {}",
                format_value(result),
                format_compare_op(op),
                format_value(lhs),
                format_value(rhs),
                format_type(ty)
            ));
        }
        Instruction::Not { result, operand } => {
            out.push_str(&format!(
                "{} = not {} : Bool",
                format_value(result),
                format_value(operand)
            ));
        }
        Instruction::Cast {
            result,
            operand,
            from_ty,
            to_ty,
        } => {
            out.push_str(&format!(
                "{} = cast {} : {} -> {}",
                format_value(result),
                format_value(operand),
                format_type(from_ty),
                format_type(to_ty)
            ));
        }
        Instruction::StructInit {
            result,
            struct_name,
            fields,
            type_args,
            ty,
        } => {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|(name, val)| format!("{}: {}", name, format_value(val)))
                .collect();
            if type_args.is_empty() {
                out.push_str(&format!(
                    "{} = struct_init @{} {{ {} }} : {}",
                    format_value(result),
                    struct_name,
                    fields_str.join(", "),
                    format_type(ty),
                ));
            } else {
                let type_args_str: Vec<String> = type_args.iter().map(format_type).collect();
                out.push_str(&format!(
                    "{} = struct_init @{} {{ {} }} type_args=[{}] : {}",
                    format_value(result),
                    struct_name,
                    fields_str.join(", "),
                    type_args_str.join(", "),
                    format_type(ty),
                ));
            }
        }
        Instruction::FieldGet {
            result,
            object,
            field,
            ty,
            ..
        } => {
            out.push_str(&format!(
                "{} = field_get {}, \"{}\" : {}",
                format_value(result),
                format_value(object),
                field,
                format_type(ty),
            ));
        }
        Instruction::FieldSet {
            result,
            object,
            field,
            value,
            ty,
        } => {
            out.push_str(&format!(
                "{} = field_set {}, \"{}\", {} : {}",
                format_value(result),
                format_value(object),
                field,
                format_value(value),
                format_type(ty),
            ));
        }
        Instruction::ArrayInit {
            result,
            ty,
            elements,
        } => {
            let elems_str: Vec<String> = elements.iter().map(format_value).collect();
            out.push_str(&format!(
                "{} = array_init [{}] : {}",
                format_value(result),
                elems_str.join(", "),
                format_type(ty),
            ));
        }
        Instruction::ArrayGet {
            result,
            ty,
            array,
            index,
            array_size,
        } => {
            out.push_str(&format!(
                "{} = array_get {}, {} : {} (size {})",
                format_value(result),
                format_value(array),
                format_value(index),
                format_type(ty),
                array_size,
            ));
        }
        Instruction::ArraySet {
            result,
            ty,
            array,
            index,
            value,
            array_size,
        } => {
            out.push_str(&format!(
                "{} = array_set {}, {}, {} : {} (size {})",
                format_value(result),
                format_value(array),
                format_value(index),
                format_value(value),
                format_type(ty),
                array_size,
            ));
        }
    }
}

fn print_terminator(term: &Terminator, out: &mut String) {
    match term {
        Terminator::Return(val) => {
            out.push_str(&format!("return {}", format_value(val)));
        }
        Terminator::ReturnVoid => {
            out.push_str("return_void");
        }
        Terminator::Br { target, args } => {
            if args.is_empty() {
                out.push_str(&format!("br bb{}", target));
            } else {
                let args_str: Vec<String> = args
                    .iter()
                    .map(|(val, ty)| format!("{}: {}", format_value(val), format_type(ty)))
                    .collect();
                out.push_str(&format!("br bb{}({})", target, args_str.join(", ")));
            }
        }
        Terminator::CondBr {
            cond,
            then_bb,
            else_bb,
        } => {
            out.push_str(&format!(
                "cond_br {}, bb{}, bb{}",
                format_value(cond),
                then_bb,
                else_bb
            ));
        }
        Terminator::BrBreak {
            header_bb,
            exit_bb,
            args,
            value,
        } => {
            let mut parts = Vec::new();
            for (val, ty) in args {
                parts.push(format!("{}: {}", format_value(val), format_type(ty)));
            }
            if let Some((val, ty)) = value {
                parts.push(format!("value {}: {}", format_value(val), format_type(ty)));
            }
            if parts.is_empty() {
                out.push_str(&format!("br_break bb{} -> bb{}", header_bb, exit_bb));
            } else {
                out.push_str(&format!(
                    "br_break bb{} -> bb{}({})",
                    header_bb,
                    exit_bb,
                    parts.join(", ")
                ));
            }
        }
        Terminator::BrContinue { header_bb, args } => {
            if args.is_empty() {
                out.push_str(&format!("br_continue bb{}", header_bb));
            } else {
                let args_str: Vec<String> = args
                    .iter()
                    .map(|(val, ty)| format!("{}: {}", format_value(val), format_type(ty)))
                    .collect();
                out.push_str(&format!(
                    "br_continue bb{}({})",
                    header_bb,
                    args_str.join(", ")
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "printer_tests.rs"]
mod tests;
