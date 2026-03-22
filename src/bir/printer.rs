use super::instruction::*;

fn format_type(ty: &BirType) -> &str {
    match ty {
        BirType::Unit => "()",
        BirType::I32 => "Int32",
        BirType::I64 => "Int64",
        BirType::F32 => "Float32",
        BirType::F64 => "Float64",
        BirType::Bool => "Bool",
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
    out.push('(');
    for (i, (val, ty)) in func.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{}: {}", format_value(val), format_type(ty)));
    }
    out.push_str(") -> ");
    out.push_str(format_type(&func.return_type));
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
            ty,
        } => {
            let args_str: Vec<String> = args.iter().map(format_value).collect();
            out.push_str(&format!(
                "{} = call @{}({}) : {}",
                format_value(result),
                func_name,
                args_str.join(", "),
                format_type(ty)
            ));
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
            exit_bb,
            args,
            value,
        } => {
            let mut parts = Vec::new();
            for (val, ty) in args {
                parts.push(format!("{}: {}", format_value(val), format_type(ty)));
            }
            if let Some((val, ty)) = value {
                parts.push(format!("{}: {}", format_value(val), format_type(ty)));
            }
            if parts.is_empty() {
                out.push_str(&format!("br_break bb{}", exit_bb));
            } else {
                out.push_str(&format!("br_break bb{}({})", exit_bb, parts.join(", ")));
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
mod tests {
    use super::super::lowering::lower_program;
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn print_str(input: &str) -> String {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = crate::semantic::analyze(&program).unwrap();
        let module = lower_program(&program, &sem_info).unwrap();
        print_module(&module)
    }

    #[test]
    fn print_literal() {
        let output = print_str("func main() -> Int32 { return 42; }");
        let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 42 : Int32
    return %0
}
";
        assert_eq!(output, expected);
    }

    #[test]
    fn print_binary_expr() {
        let output = print_str("2 + 3 * 4");
        let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 2 : Int32
    %1 = literal 3 : Int32
    %2 = literal 4 : Int32
    %3 = binary_op mul %1, %2 : Int32
    %4 = binary_op add %0, %3 : Int32
    return %4
}
";
        assert_eq!(output, expected);
    }

    #[test]
    fn print_call() {
        let output = print_str(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(1, 2); }",
        );
        assert!(output.contains("call @add(%0, %1) : Int32"));
    }
}
