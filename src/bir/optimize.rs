use std::collections::HashMap;

use super::instruction::*;

pub fn optimize_module(module: &mut BirModule) {
    for func in &mut module.functions {
        fold_constants(&mut func.blocks);
    }
}

fn fold_constants(blocks: &mut [BasicBlock]) {
    // Track constant values: Value → (raw i64 bits, type)
    let mut value_map: HashMap<Value, (i64, BirType)> = HashMap::new();

    for block in blocks.iter_mut() {
        for inst in &mut block.instructions {
            match inst {
                Instruction::Literal { result, value, ty } => {
                    value_map.insert(*result, (*value, ty.clone()));
                }
                Instruction::BinaryOp {
                    result,
                    op,
                    lhs,
                    rhs,
                    ty,
                } => {
                    if let (Some(&(lv, _)), Some(&(rv, _))) =
                        (value_map.get(lhs), value_map.get(rhs))
                        && let Some(folded) = fold_binop(*op, lv, rv, ty.clone())
                    {
                        value_map.insert(*result, (folded, ty.clone()));
                        *inst = Instruction::Literal {
                            result: *result,
                            value: folded,
                            ty: ty.clone(),
                        };
                    }
                }
                Instruction::Compare {
                    result,
                    op,
                    lhs,
                    rhs,
                    ty,
                } => {
                    if let (Some(&(lv, _)), Some(&(rv, _))) =
                        (value_map.get(lhs), value_map.get(rhs))
                        && let Some(folded) = fold_compare(*op, lv, rv, ty.clone())
                    {
                        let bool_val = if folded { 1i64 } else { 0i64 };
                        value_map.insert(*result, (bool_val, BirType::Bool));
                        *inst = Instruction::Literal {
                            result: *result,
                            value: bool_val,
                            ty: BirType::Bool,
                        };
                    }
                }
                _ => {}
            }
        }
    }
}

fn fold_binop(op: BirBinOp, lv: i64, rv: i64, ty: BirType) -> Option<i64> {
    match ty {
        BirType::I32 => {
            let l = lv as i32;
            let r = rv as i32;
            let result = match op {
                BirBinOp::Add => l.wrapping_add(r),
                BirBinOp::Sub => l.wrapping_sub(r),
                BirBinOp::Mul => l.wrapping_mul(r),
                BirBinOp::Div => {
                    if r == 0 {
                        return None;
                    }
                    l.wrapping_div(r)
                }
            };
            Some(result as i64)
        }
        BirType::I64 => {
            let result = match op {
                BirBinOp::Add => lv.wrapping_add(rv),
                BirBinOp::Sub => lv.wrapping_sub(rv),
                BirBinOp::Mul => lv.wrapping_mul(rv),
                BirBinOp::Div => {
                    if rv == 0 {
                        return None;
                    }
                    lv.wrapping_div(rv)
                }
            };
            Some(result)
        }
        BirType::F32 => {
            let l = f32::from_bits(lv as u32);
            let r = f32::from_bits(rv as u32);
            let result = match op {
                BirBinOp::Add => l + r,
                BirBinOp::Sub => l - r,
                BirBinOp::Mul => l * r,
                BirBinOp::Div => l / r,
            };
            Some(result.to_bits() as i64)
        }
        BirType::F64 => {
            let l = f64::from_bits(lv as u64);
            let r = f64::from_bits(rv as u64);
            let result = match op {
                BirBinOp::Add => l + r,
                BirBinOp::Sub => l - r,
                BirBinOp::Mul => l * r,
                BirBinOp::Div => l / r,
            };
            Some(result.to_bits() as i64)
        }
        _ => None,
    }
}

fn fold_compare(op: BirCompareOp, lv: i64, rv: i64, ty: BirType) -> Option<bool> {
    match ty {
        BirType::I32 => {
            let l = lv as i32;
            let r = rv as i32;
            Some(match op {
                BirCompareOp::Eq => l == r,
                BirCompareOp::Ne => l != r,
                BirCompareOp::Lt => l < r,
                BirCompareOp::Gt => l > r,
                BirCompareOp::Le => l <= r,
                BirCompareOp::Ge => l >= r,
            })
        }
        BirType::I64 => Some(match op {
            BirCompareOp::Eq => lv == rv,
            BirCompareOp::Ne => lv != rv,
            BirCompareOp::Lt => lv < rv,
            BirCompareOp::Gt => lv > rv,
            BirCompareOp::Le => lv <= rv,
            BirCompareOp::Ge => lv >= rv,
        }),
        BirType::F32 => {
            let l = f32::from_bits(lv as u32);
            let r = f32::from_bits(rv as u32);
            Some(match op {
                BirCompareOp::Eq => l == r,
                BirCompareOp::Ne => l != r,
                BirCompareOp::Lt => l < r,
                BirCompareOp::Gt => l > r,
                BirCompareOp::Le => l <= r,
                BirCompareOp::Ge => l >= r,
            })
        }
        BirType::F64 => {
            let l = f64::from_bits(lv as u64);
            let r = f64::from_bits(rv as u64);
            Some(match op {
                BirCompareOp::Eq => l == r,
                BirCompareOp::Ne => l != r,
                BirCompareOp::Lt => l < r,
                BirCompareOp::Gt => l > r,
                BirCompareOp::Le => l <= r,
                BirCompareOp::Ge => l >= r,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bir::lowering::lower_program;
    use crate::bir::printer::print_module;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::semantic;

    fn lower_and_optimize(source: &str) -> BirModule {
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let mut module = lower_program(&program, &sem_info).unwrap();
        optimize_module(&mut module);
        module
    }

    fn bir_text(source: &str) -> String {
        let module = lower_and_optimize(source);
        print_module(&module)
    }

    #[test]
    fn fold_add() {
        let text = bir_text("func main() -> Int32 { return 2 + 3; }");
        // 2 + 3 should be folded to literal 5
        assert!(text.contains("literal 5 : Int32"), "BIR:\n{}", text);
        assert!(!text.contains("binary_op add"), "BIR:\n{}", text);
    }

    #[test]
    fn fold_chain() {
        let text = bir_text("func main() -> Int32 { return 2 + 3 * 4; }");
        // 3 * 4 = 12, then 2 + 12 = 14
        assert!(text.contains("literal 14 : Int32"), "BIR:\n{}", text);
    }

    #[test]
    fn fold_compare() {
        let text = bir_text(
            "func main() -> Int32 { let x: Int32 = if 1 < 2 { yield 10; } else { yield 20; }; return x; }",
        );
        // 1 < 2 should be folded to literal 1 : Bool
        assert!(text.contains("literal 1 : Bool"), "BIR:\n{}", text);
        assert!(!text.contains("compare lt"), "BIR:\n{}", text);
    }

    #[test]
    fn no_fold_param() {
        let text = bir_text(
            "func add1(x: Int32) -> Int32 { return x + 1; } func main() -> Int32 { return add1(5); }",
        );
        // x + 1 should NOT be folded (x is a parameter, not a constant)
        assert!(text.contains("binary_op add"), "BIR:\n{}", text);
    }
}
