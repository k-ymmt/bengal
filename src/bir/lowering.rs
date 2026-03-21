use crate::error::Result;
use crate::parser::ast::{BinOp, Expr};

use super::instruction::*;

struct Lowering {
    next_value: u32,
}

impl Lowering {
    fn new() -> Self {
        Self { next_value: 0 }
    }

    fn fresh_value(&mut self) -> Value {
        let v = Value(self.next_value);
        self.next_value += 1;
        v
    }

    fn lower_expr(&mut self, expr: &Expr, instructions: &mut Vec<Instruction>) -> Value {
        match expr {
            Expr::Number(n) => {
                let result = self.fresh_value();
                instructions.push(Instruction::Literal {
                    result,
                    value: *n as i64,
                    ty: BirType::I32,
                });
                result
            }
            Expr::BinaryOp { op, left, right } => {
                let lhs = self.lower_expr(left, instructions);
                let rhs = self.lower_expr(right, instructions);
                let result = self.fresh_value();
                instructions.push(Instruction::BinaryOp {
                    result,
                    op: convert_binop(*op),
                    lhs,
                    rhs,
                    ty: BirType::I32,
                });
                result
            }
            _ => todo!("Phase 2: Ident, Call, Block lowering"),
        }
    }
}

fn convert_binop(op: BinOp) -> BirBinOp {
    match op {
        BinOp::Add => BirBinOp::Add,
        BinOp::Sub => BirBinOp::Sub,
        BinOp::Mul => BirBinOp::Mul,
        BinOp::Div => BirBinOp::Div,
    }
}

pub fn lower(expr: &Expr) -> Result<BirModule> {
    let mut lowering = Lowering::new();
    let mut instructions = Vec::new();
    let result = lowering.lower_expr(expr, &mut instructions);

    let bb0 = BasicBlock {
        label: 0,
        params: vec![],
        instructions,
        terminator: Terminator::Return(result),
    };

    let func = BirFunction {
        name: "main".to_string(),
        params: vec![],
        return_type: BirType::I32,
        blocks: vec![bb0],
    };

    Ok(BirModule {
        functions: vec![func],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_number() {
        let expr = Expr::Number(42);
        let module = lower(&expr).unwrap();

        assert_eq!(module.functions.len(), 1);
        let func = &module.functions[0];
        assert_eq!(func.blocks.len(), 1);

        let bb = &func.blocks[0];
        assert_eq!(
            bb.instructions,
            vec![Instruction::Literal {
                result: Value(0),
                value: 42,
                ty: BirType::I32,
            }]
        );
        assert_eq!(bb.terminator, Terminator::Return(Value(0)));
    }

    #[test]
    fn lower_binary_expr() {
        // 2 + 3 * 4
        let expr = Expr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(Expr::Number(2)),
            right: Box::new(Expr::BinaryOp {
                op: BinOp::Mul,
                left: Box::new(Expr::Number(3)),
                right: Box::new(Expr::Number(4)),
            }),
        };
        let module = lower(&expr).unwrap();

        let bb = &module.functions[0].blocks[0];
        assert_eq!(bb.instructions.len(), 5);

        // literal 2 -> Value(0)
        // literal 3 -> Value(1)
        // literal 4 -> Value(2)
        // mul Value(1), Value(2) -> Value(3)
        // add Value(0), Value(3) -> Value(4)
        assert_eq!(
            bb.instructions[0],
            Instruction::Literal {
                result: Value(0),
                value: 2,
                ty: BirType::I32,
            }
        );
        assert_eq!(
            bb.instructions[3],
            Instruction::BinaryOp {
                result: Value(3),
                op: BirBinOp::Mul,
                lhs: Value(1),
                rhs: Value(2),
                ty: BirType::I32,
            }
        );
        assert_eq!(
            bb.instructions[4],
            Instruction::BinaryOp {
                result: Value(4),
                op: BirBinOp::Add,
                lhs: Value(0),
                rhs: Value(3),
                ty: BirType::I32,
            }
        );
        assert_eq!(bb.terminator, Terminator::Return(Value(4)));
    }
}
