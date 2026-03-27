use super::super::instruction::*;
use crate::parser::ast::*;

impl super::Lowering {
    // ========== Short-circuit &&  ==========

    pub(super) fn lower_short_circuit_and(&mut self, left: &Expr, right: &Expr) -> Value {
        let lhs = self.lower_expr(left);
        let cond_regions = self.take_pending_regions();

        let cond_bb = self.current_block_label;
        let bb_rhs = self.fresh_block();
        let bb_false = self.fresh_block();
        let bb_merge = self.fresh_block();

        self.seal_block(Terminator::CondBr {
            cond: lhs,
            then_bb: bb_rhs,
            else_bb: bb_false,
        });

        // bb_rhs: evaluate right operand
        self.start_block(bb_rhs, vec![]);
        let rhs = self.lower_expr(right);
        let rhs_regions = self.take_pending_regions();
        self.seal_block(Terminator::Br {
            target: bb_merge,
            args: vec![(rhs, BirType::Bool)],
        });

        // bb_false: literal false
        self.start_block(bb_false, vec![]);
        let false_val = self.fresh_value();
        self.emit(Instruction::Literal {
            result: false_val,
            value: 0,
            ty: BirType::Bool,
        });
        self.seal_block(Terminator::Br {
            target: bb_merge,
            args: vec![(false_val, BirType::Bool)],
        });

        // bb_merge: result
        let result = self.fresh_value();
        self.start_block(bb_merge, vec![(result, BirType::Bool)]);

        // Build then_region with any nested regions from rhs
        let mut then_region = rhs_regions;
        then_region.push(CfgRegion::Block(bb_rhs));

        self.pending_regions.push(CfgRegion::IfElse {
            cond_region: cond_regions,
            cond_bb,
            cond_value: lhs,
            then_region,
            else_region: vec![CfgRegion::Block(bb_false)],
            merge_bb: bb_merge,
        });

        result
    }

    // ========== Short-circuit ||  ==========

    pub(super) fn lower_short_circuit_or(&mut self, left: &Expr, right: &Expr) -> Value {
        let lhs = self.lower_expr(left);
        let cond_regions = self.take_pending_regions();

        let cond_bb = self.current_block_label;
        let bb_true = self.fresh_block();
        let bb_rhs = self.fresh_block();
        let bb_merge = self.fresh_block();

        self.seal_block(Terminator::CondBr {
            cond: lhs,
            then_bb: bb_true,
            else_bb: bb_rhs,
        });

        // bb_true: literal true
        self.start_block(bb_true, vec![]);
        let true_val = self.fresh_value();
        self.emit(Instruction::Literal {
            result: true_val,
            value: 1,
            ty: BirType::Bool,
        });
        self.seal_block(Terminator::Br {
            target: bb_merge,
            args: vec![(true_val, BirType::Bool)],
        });

        // bb_rhs: evaluate right operand
        self.start_block(bb_rhs, vec![]);
        let rhs = self.lower_expr(right);
        let rhs_regions = self.take_pending_regions();
        self.seal_block(Terminator::Br {
            target: bb_merge,
            args: vec![(rhs, BirType::Bool)],
        });

        // bb_merge: result
        let result = self.fresh_value();
        self.start_block(bb_merge, vec![(result, BirType::Bool)]);

        let mut else_region = rhs_regions;
        else_region.push(CfgRegion::Block(bb_rhs));

        self.pending_regions.push(CfgRegion::IfElse {
            cond_region: cond_regions,
            cond_bb,
            cond_value: lhs,
            then_region: vec![CfgRegion::Block(bb_true)],
            else_region,
            merge_bb: bb_merge,
        });

        result
    }
}

#[cfg(test)]
mod tests {
    use crate::bir::lowering::lower_program;
    use crate::bir::printer::print_module;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::semantic;

    fn lower_str(input: &str) -> String {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let module = lower_program(&program, &sem_info).unwrap();
        print_module(&module)
    }

    #[test]
    fn lower_short_circuit_and() {
        let output = lower_str(
            "func main() -> Int32 { let b: Bool = true && false; if b { yield 1; } else { yield 0; }; return 0; }",
        );
        // Should have cond_br for && short-circuit
        assert!(output.contains("cond_br"));
        assert!(output.contains("literal 0 : Bool"));
    }
}
