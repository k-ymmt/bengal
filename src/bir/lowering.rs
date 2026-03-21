use std::collections::HashMap;

use crate::error::Result;
use crate::parser::ast::*;

use super::instruction::*;

enum StmtResult {
    None,
    Return(Value),
    ReturnVoid,
    Yield(Value),
}

struct Lowering {
    next_value: u32,
    next_block: u32,
    scopes: Vec<HashMap<String, Value>>,
    // Block management
    blocks: Vec<BasicBlock>,
    current_instructions: Vec<Instruction>,
    current_block_label: u32,
    current_block_params: Vec<(Value, BirType)>,
    // CfgRegion tracking — regions generated during expression lowering
    pending_regions: Vec<CfgRegion>,
    // Function signatures for Call return type lookup
    func_sigs: HashMap<String, BirType>,
    // Track which variables are mutable (for while loop block args)
    mutable_vars: Vec<String>,
}

impl Lowering {
    fn new(func_sigs: HashMap<String, BirType>) -> Self {
        Self {
            next_value: 0,
            next_block: 0,
            scopes: Vec::new(),
            blocks: Vec::new(),
            current_instructions: Vec::new(),
            current_block_label: 0,
            current_block_params: Vec::new(),
            pending_regions: Vec::new(),
            func_sigs,
            mutable_vars: Vec::new(),
        }
    }

    fn fresh_value(&mut self) -> Value {
        let v = Value(self.next_value);
        self.next_value += 1;
        v
    }

    fn fresh_block(&mut self) -> u32 {
        let label = self.next_block;
        self.next_block += 1;
        label
    }

    fn seal_block(&mut self, terminator: Terminator) {
        let block = BasicBlock {
            label: self.current_block_label,
            params: std::mem::take(&mut self.current_block_params),
            instructions: std::mem::take(&mut self.current_instructions),
            terminator,
        };
        self.blocks.push(block);
    }

    fn start_block(&mut self, label: u32, params: Vec<(Value, BirType)>) {
        self.current_block_label = label;
        self.current_block_params = params;
        self.current_instructions = Vec::new();
    }

    fn emit(&mut self, inst: Instruction) {
        self.current_instructions.push(inst);
    }

    fn take_pending_regions(&mut self) -> Vec<CfgRegion> {
        std::mem::take(&mut self.pending_regions)
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_var(&mut self, name: String, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    fn lookup_var(&self, name: &str) -> Value {
        for scope in self.scopes.iter().rev() {
            if let Some(&value) = scope.get(name) {
                return value;
            }
        }
        unreachable!(
            "undefined variable `{}` (should be caught by semantic analysis)",
            name
        )
    }

    fn assign_var(&mut self, name: &str, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return;
            }
        }
        unreachable!(
            "undefined variable `{}` (should be caught by semantic analysis)",
            name
        )
    }

    /// Collect current values of all mutable variables for while loop block args
    fn collect_mutable_var_values(&self) -> Vec<(String, Value, BirType)> {
        let mut result = Vec::new();
        for name in &self.mutable_vars {
            let val = self.lookup_var(name);
            result.push((name.clone(), val, BirType::I32));
        }
        result
    }

    // ========== Function ==========

    fn lower_function(&mut self, func: &Function) -> BirFunction {
        self.next_value = 0;
        self.next_block = 0;
        self.scopes.clear();
        self.blocks.clear();
        self.pending_regions.clear();
        self.mutable_vars.clear();

        let bb0 = self.fresh_block();
        self.start_block(bb0, vec![]);
        self.push_scope();

        // Register parameters
        let mut params = Vec::new();
        for param in &func.params {
            let val = self.fresh_value();
            let bir_ty = convert_type(&param.ty);
            params.push((val, bir_ty));
            self.define_var(param.name.clone(), val);
        }

        let (result, body_regions) = self.lower_block_stmts(&func.body);

        self.pop_scope();

        // Seal the final block with terminator
        match result {
            Some(StmtResult::Return(v)) => {
                self.seal_block(Terminator::Return(v));
            }
            Some(StmtResult::ReturnVoid) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            _ => unreachable!(
                "function must end with return (semantic analysis guarantees this)"
            ),
        }

        // Build CfgRegion body: body_regions + final block (which was just sealed)
        let final_label = self.blocks.last().unwrap().label;
        let mut body = body_regions;
        body.push(CfgRegion::Block(final_label));

        let blocks = std::mem::take(&mut self.blocks);

        BirFunction {
            name: func.name.clone(),
            params,
            return_type: convert_type(&func.return_type),
            blocks,
            body,
        }
    }

    // ========== Block ==========

    /// Lower a block's statements. Returns (StmtResult, collected CfgRegions).
    /// Does NOT push/pop scope — caller handles that.
    fn lower_block_stmts(&mut self, block: &Block) -> (Option<StmtResult>, Vec<CfgRegion>) {
        let mut regions = Vec::new();
        let mut result = None;

        for stmt in &block.stmts {
            let sr = self.lower_stmt(stmt);

            // Collect any CfgRegions generated during this statement
            regions.append(&mut self.take_pending_regions());

            match sr {
                StmtResult::Return(v) => {
                    result = Some(StmtResult::Return(v));
                    break;
                }
                StmtResult::ReturnVoid => {
                    result = Some(StmtResult::ReturnVoid);
                    break;
                }
                StmtResult::Yield(v) => {
                    result = Some(StmtResult::Yield(v));
                    break;
                }
                StmtResult::None => {}
            }
        }

        (result, regions)
    }

    // ========== Stmt ==========

    fn lower_stmt(&mut self, stmt: &Stmt) -> StmtResult {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let val = self.lower_expr(value);
                self.define_var(name.clone(), val);
                StmtResult::None
            }
            Stmt::Var { name, value, .. } => {
                let val = self.lower_expr(value);
                self.define_var(name.clone(), val);
                if !self.mutable_vars.contains(name) {
                    self.mutable_vars.push(name.clone());
                }
                StmtResult::None
            }
            Stmt::Assign { name, value } => {
                let val = self.lower_expr(value);
                self.assign_var(name, val);
                StmtResult::None
            }
            Stmt::Return(Some(expr)) => {
                let val = self.lower_expr(expr);
                StmtResult::Return(val)
            }
            Stmt::Return(None) => StmtResult::ReturnVoid,
            Stmt::Yield(expr) => {
                let val = self.lower_expr(expr);
                StmtResult::Yield(val)
            }
            Stmt::Expr(expr) => {
                let _val = self.lower_expr(expr);
                StmtResult::None
            }
        }
    }

    // ========== Expr ==========

    fn lower_expr(&mut self, expr: &Expr) -> Value {
        match expr {
            Expr::Number(n) => {
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: *n as i64,
                    ty: BirType::I32,
                });
                result
            }
            Expr::Bool(b) => {
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: if *b { 1 } else { 0 },
                    ty: BirType::Bool,
                });
                result
            }
            Expr::Ident(name) => self.lookup_var(name),
            Expr::UnaryOp { op, operand } => {
                let operand_val = self.lower_expr(operand);
                match op {
                    UnaryOp::Not => {
                        let result = self.fresh_value();
                        self.emit(Instruction::Not {
                            result,
                            operand: operand_val,
                        });
                        result
                    }
                }
            }
            Expr::BinaryOp { op, left, right } => match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    let lhs = self.lower_expr(left);
                    let rhs = self.lower_expr(right);
                    let result = self.fresh_value();
                    self.emit(Instruction::BinaryOp {
                        result,
                        op: convert_binop(*op),
                        lhs,
                        rhs,
                        ty: BirType::I32,
                    });
                    result
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    let lhs = self.lower_expr(left);
                    let rhs = self.lower_expr(right);
                    let result = self.fresh_value();
                    self.emit(Instruction::Compare {
                        result,
                        op: convert_compare_op(*op),
                        lhs,
                        rhs,
                    });
                    result
                }
                BinOp::And => self.lower_short_circuit_and(left, right),
                BinOp::Or => self.lower_short_circuit_or(left, right),
            },
            Expr::Call { name, args } => {
                let arg_vals: Vec<Value> =
                    args.iter().map(|a| self.lower_expr(a)).collect();
                let ty = self
                    .func_sigs
                    .get(name)
                    .copied()
                    .unwrap_or(BirType::I32);
                let result = self.fresh_value();
                self.emit(Instruction::Call {
                    result,
                    func_name: name.clone(),
                    args: arg_vals,
                    ty,
                });
                result
            }
            Expr::Block(block) => {
                self.push_scope();
                let (result, mut inner_regions) = self.lower_block_stmts(block);
                self.pop_scope();
                // Collect regions from inner block
                self.pending_regions.append(&mut inner_regions);
                match result {
                    Some(StmtResult::Yield(v)) => v,
                    _ => unreachable!(
                        "block expression must yield (semantic analysis guarantees this)"
                    ),
                }
            }
            Expr::If {
                condition,
                then_block,
                else_block,
            } => self.lower_if(condition, then_block, else_block.as_ref()),
            Expr::While { condition, body } => self.lower_while(condition, body),
        }
    }

    // ========== Short-circuit &&  ==========

    fn lower_short_circuit_and(&mut self, left: &Expr, right: &Expr) -> Value {
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

    fn lower_short_circuit_or(&mut self, left: &Expr, right: &Expr) -> Value {
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

    // ========== If/Else ==========

    fn lower_if(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Value {
        let cond_val = self.lower_expr(condition);
        let cond_regions = self.take_pending_regions();
        let cond_bb = self.current_block_label;

        let bb_then = self.fresh_block();
        let bb_merge = self.fresh_block();

        match else_block {
            Some(else_blk) => {
                let bb_else = self.fresh_block();

                self.seal_block(Terminator::CondBr {
                    cond: cond_val,
                    then_bb: bb_then,
                    else_bb: bb_else,
                });

                // then block
                self.start_block(bb_then, vec![]);
                self.push_scope();
                let (then_result, then_inner_regions) = self.lower_block_stmts(then_block);
                self.pop_scope();
                let mut then_region = then_inner_regions;
                // Don't seal yet — need to know merge type first

                // else block
                let then_block_label = self.current_block_label;
                let then_instructions = std::mem::take(&mut self.current_instructions);
                let then_params = std::mem::take(&mut self.current_block_params);

                self.start_block(bb_else, vec![]);
                self.push_scope();
                let (else_result, else_inner_regions) = self.lower_block_stmts(else_blk);
                self.pop_scope();
                let mut else_region = else_inner_regions;

                let else_block_label = self.current_block_label;
                let else_instructions = std::mem::take(&mut self.current_instructions);
                let else_params = std::mem::take(&mut self.current_block_params);

                // Determine if merge block needs a value parameter
                let then_yields = matches!(&then_result, Some(StmtResult::Yield(_)));
                let else_yields = matches!(&else_result, Some(StmtResult::Yield(_)));
                let has_merge_value = then_yields || else_yields;
                let merge_type = BirType::I32; // Phase 3: yield values are always i32

                // Seal then block
                self.current_block_label = then_block_label;
                self.current_instructions = then_instructions;
                self.current_block_params = then_params;
                match &then_result {
                    Some(StmtResult::Yield(v)) => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![(*v, merge_type)],
                        });
                    }
                    Some(StmtResult::Return(v)) => {
                        self.seal_block(Terminator::Return(*v));
                    }
                    Some(StmtResult::ReturnVoid) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    _ => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![],
                        });
                    }
                }
                then_region.push(CfgRegion::Block(bb_then));

                // Seal else block
                self.current_block_label = else_block_label;
                self.current_instructions = else_instructions;
                self.current_block_params = else_params;
                match &else_result {
                    Some(StmtResult::Yield(v)) => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![(*v, merge_type)],
                        });
                    }
                    Some(StmtResult::Return(v)) => {
                        self.seal_block(Terminator::Return(*v));
                    }
                    Some(StmtResult::ReturnVoid) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    _ => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![],
                        });
                    }
                }
                else_region.push(CfgRegion::Block(bb_else));

                // merge block
                let merge_params = if has_merge_value {
                    let result = self.fresh_value();
                    self.start_block(bb_merge, vec![(result, merge_type)]);
                    result
                } else {
                    let result = self.fresh_value(); // dummy for Unit
                    self.start_block(bb_merge, vec![]);
                    result
                };

                self.pending_regions.push(CfgRegion::IfElse {
                    cond_region: cond_regions,
                    cond_bb,
                    cond_value: cond_val,
                    then_region,
                    else_region,
                    merge_bb: bb_merge,
                });

                merge_params
            }
            None => {
                // if without else
                self.seal_block(Terminator::CondBr {
                    cond: cond_val,
                    then_bb: bb_then,
                    else_bb: bb_merge,
                });

                self.start_block(bb_then, vec![]);
                self.push_scope();
                let (then_result, then_inner_regions) = self.lower_block_stmts(then_block);
                self.pop_scope();

                let mut then_region = then_inner_regions;

                match then_result {
                    Some(StmtResult::Return(v)) => {
                        self.seal_block(Terminator::Return(v));
                    }
                    Some(StmtResult::ReturnVoid) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    _ => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![],
                        });
                    }
                }
                then_region.push(CfgRegion::Block(bb_then));

                self.start_block(bb_merge, vec![]);

                self.pending_regions.push(CfgRegion::IfOnly {
                    cond_region: cond_regions,
                    cond_bb,
                    cond_value: cond_val,
                    then_region,
                    merge_bb: bb_merge,
                });

                // If without else returns Unit — return dummy value
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: 0,
                    ty: BirType::Unit,
                });
                result
            }
        }
    }

    // ========== While ==========

    fn lower_while(&mut self, condition: &Expr, body: &Block) -> Value {
        let entry_bb = self.current_block_label;

        // Collect mutable var values for block args
        let mutable_vars = self.collect_mutable_var_values();

        let bb_header = self.fresh_block();
        let bb_body = self.fresh_block();
        let bb_exit = self.fresh_block();

        // Seal entry block with Br to header
        let entry_args: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(_, val, ty)| (*val, *ty))
            .collect();
        self.seal_block(Terminator::Br {
            target: bb_header,
            args: entry_args,
        });

        // Header block: receives mutable vars as params
        let header_params: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(_, _, ty)| {
                let v = self.fresh_value();
                (v, *ty)
            })
            .collect();

        self.start_block(bb_header, header_params.clone());

        // Remap mutable vars to header params
        for (i, (name, _, _)) in mutable_vars.iter().enumerate() {
            let (param_val, _) = header_params[i];
            self.assign_var(name, param_val);
        }

        // Evaluate condition
        let cond_val = self.lower_expr(condition);
        let header_regions = self.take_pending_regions();

        self.seal_block(Terminator::CondBr {
            cond: cond_val,
            then_bb: bb_body,
            else_bb: bb_exit,
        });

        // Body block
        self.start_block(bb_body, vec![]);
        self.push_scope();
        let (_body_result, body_inner_regions) = self.lower_block_stmts(body);
        self.pop_scope();

        let mut body_region = body_inner_regions;

        // Collect updated mutable var values for back-edge
        let updated_args: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(name, _, ty)| (self.lookup_var(name), *ty))
            .collect();

        self.seal_block(Terminator::Br {
            target: bb_header,
            args: updated_args,
        });

        body_region.push(CfgRegion::Block(bb_body));

        // Exit block
        self.start_block(bb_exit, vec![]);

        self.pending_regions.push(CfgRegion::While {
            entry_bb,
            header_region: header_regions,
            header_bb: bb_header,
            cond_value: cond_val,
            body_region,
            exit_bb: bb_exit,
        });

        // While returns Unit — dummy value
        let result = self.fresh_value();
        self.emit(Instruction::Literal {
            result,
            value: 0,
            ty: BirType::Unit,
        });
        result
    }
}

fn convert_binop(op: BinOp) -> BirBinOp {
    match op {
        BinOp::Add => BirBinOp::Add,
        BinOp::Sub => BirBinOp::Sub,
        BinOp::Mul => BirBinOp::Mul,
        BinOp::Div => BirBinOp::Div,
        _ => unreachable!("non-arithmetic BinOp should be handled separately"),
    }
}

fn convert_compare_op(op: BinOp) -> BirCompareOp {
    match op {
        BinOp::Eq => BirCompareOp::Eq,
        BinOp::Ne => BirCompareOp::Ne,
        BinOp::Lt => BirCompareOp::Lt,
        BinOp::Gt => BirCompareOp::Gt,
        BinOp::Le => BirCompareOp::Le,
        BinOp::Ge => BirCompareOp::Ge,
        _ => unreachable!("non-comparison BinOp"),
    }
}

fn convert_type(ty: &TypeAnnotation) -> BirType {
    match ty {
        TypeAnnotation::I32 => BirType::I32,
        TypeAnnotation::Bool => BirType::Bool,
        TypeAnnotation::Unit => BirType::Unit,
    }
}

pub fn lower_program(program: &Program) -> Result<BirModule> {
    // Pre-collect function signatures
    let mut func_sigs = HashMap::new();
    for func in &program.functions {
        func_sigs.insert(func.name.clone(), convert_type(&func.return_type));
    }

    let mut lowering = Lowering::new(func_sigs);
    let functions = program
        .functions
        .iter()
        .map(|f| lowering.lower_function(f))
        .collect();
    Ok(BirModule { functions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bir::printer::print_module;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::semantic;

    fn lower_str(input: &str) -> String {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        semantic::analyze(&program).unwrap();
        let module = lower_program(&program).unwrap();
        print_module(&module)
    }

    // --- Phase 2 tests (maintained) ---

    #[test]
    fn lower_simple_return() {
        let output = lower_str("func main() -> i32 { return 42; }");
        let expected = "\
bir @main() -> i32 {
bb0:
    %0 = literal 42 : i32
    return %0
}
";
        assert_eq!(output, expected);
    }

    #[test]
    fn lower_let_return() {
        let output = lower_str("func main() -> i32 { let x: i32 = 10; return x; }");
        let expected = "\
bir @main() -> i32 {
bb0:
    %0 = literal 10 : i32
    return %0
}
";
        assert_eq!(output, expected);
    }

    #[test]
    fn lower_call() {
        let output = lower_str(
            "func add(a: i32, b: i32) -> i32 { return a + b; } func main() -> i32 { return add(3, 4); }",
        );
        let expected = "\
bir @add(%0: i32, %1: i32) -> i32 {
bb0:
    %2 = binary_op add %0, %1 : i32
    return %2
}
bir @main() -> i32 {
bb0:
    %0 = literal 3 : i32
    %1 = literal 4 : i32
    %2 = call @add(%0, %1) : i32
    return %2
}
";
        assert_eq!(output, expected);
    }

    #[test]
    fn lower_block_scope() {
        let output = lower_str(
            "func main() -> i32 { let x: i32 = 1; let y: i32 = { let x: i32 = 10; yield x + 1; }; return x + y; }",
        );
        let expected = "\
bir @main() -> i32 {
bb0:
    %0 = literal 1 : i32
    %1 = literal 10 : i32
    %2 = literal 1 : i32
    %3 = binary_op add %1, %2 : i32
    %4 = binary_op add %0, %3 : i32
    return %4
}
";
        assert_eq!(output, expected);
    }

    // --- Phase 3 tests ---

    #[test]
    fn lower_if_else() {
        let output = lower_str(
            "func main() -> i32 { let x: i32 = if true { yield 1; } else { yield 2; }; return x; }",
        );
        // Should have 4 blocks with cond_br
        assert!(output.contains("cond_br"));
        assert!(output.contains("bb1"));
        assert!(output.contains("bb2"));
        assert!(output.contains("bb3"));
    }

    #[test]
    fn lower_while() {
        let output = lower_str(
            "func main() -> i32 { var i: i32 = 0; while i < 3 { i = i + 1; }; return i; }",
        );
        // Should have blocks for entry, header, body, exit
        assert!(output.contains("cond_br"));
        assert!(output.contains("compare lt"));
    }

    #[test]
    fn lower_short_circuit_and() {
        let output = lower_str(
            "func main() -> i32 { let b: bool = true && false; if b { yield 1; } else { yield 0; }; return 0; }",
        );
        // Should have cond_br for && short-circuit
        assert!(output.contains("cond_br"));
        assert!(output.contains("literal 0 : bool"));
    }
}
