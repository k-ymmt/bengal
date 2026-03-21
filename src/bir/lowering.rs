use std::collections::HashMap;

use crate::error::Result;
use crate::parser::ast::*;

use super::instruction::*;

enum StmtResult {
    None,
    Return(Value),
    Yield(Value),
}

struct Lowering {
    next_value: u32,
    scopes: Vec<HashMap<String, Value>>,
}

impl Lowering {
    fn new() -> Self {
        Self {
            next_value: 0,
            scopes: Vec::new(),
        }
    }

    fn fresh_value(&mut self) -> Value {
        let v = Value(self.next_value);
        self.next_value += 1;
        v
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
        unreachable!("undefined variable `{}` (should be caught by semantic analysis)", name)
    }

    fn assign_var(&mut self, name: &str, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return;
            }
        }
        unreachable!("undefined variable `{}` (should be caught by semantic analysis)", name)
    }

    fn lower_function(&mut self, func: &Function) -> BirFunction {
        self.next_value = 0;
        self.scopes.clear();
        self.push_scope();

        // Register parameters
        let mut params = Vec::new();
        for param in &func.params {
            let val = self.fresh_value();
            let bir_ty = convert_type(&param.ty);
            params.push((val, bir_ty));
            self.define_var(param.name.clone(), val);
        }

        let mut instructions = Vec::new();
        let result = self.lower_block(&func.body, &mut instructions);

        self.pop_scope();

        // The function must end with a return (guaranteed by semantic analysis)
        let return_value = match result {
            Some(StmtResult::Return(v)) => v,
            _ => unreachable!("function must end with return (semantic analysis guarantees this)"),
        };

        let bb0 = BasicBlock {
            label: 0,
            params: vec![],
            instructions,
            terminator: Terminator::Return(return_value),
        };

        BirFunction {
            name: func.name.clone(),
            params,
            return_type: convert_type(&func.return_type),
            blocks: vec![bb0],
        }
    }

    fn lower_block(
        &mut self,
        block: &Block,
        instructions: &mut Vec<Instruction>,
    ) -> Option<StmtResult> {
        self.push_scope();
        let mut result = None;
        for stmt in &block.stmts {
            match self.lower_stmt(stmt, instructions) {
                StmtResult::Return(v) => {
                    result = Some(StmtResult::Return(v));
                    break;
                }
                StmtResult::Yield(v) => {
                    result = Some(StmtResult::Yield(v));
                    break;
                }
                StmtResult::None => {}
            }
        }
        self.pop_scope();
        result
    }

    fn lower_stmt(&mut self, stmt: &Stmt, instructions: &mut Vec<Instruction>) -> StmtResult {
        match stmt {
            Stmt::Let { name, value, .. } | Stmt::Var { name, value, .. } => {
                let val = self.lower_expr(value, instructions);
                self.define_var(name.clone(), val);
                StmtResult::None
            }
            Stmt::Assign { name, value } => {
                let val = self.lower_expr(value, instructions);
                self.assign_var(name, val);
                StmtResult::None
            }
            Stmt::Return(expr) => {
                let val = self.lower_expr(expr, instructions);
                StmtResult::Return(val)
            }
            Stmt::Yield(expr) => {
                let val = self.lower_expr(expr, instructions);
                StmtResult::Yield(val)
            }
            Stmt::Expr(expr) => {
                let _val = self.lower_expr(expr, instructions);
                StmtResult::None
            }
        }
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
            Expr::Ident(name) => self.lookup_var(name),
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
            Expr::Call { name, args } => {
                let arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.lower_expr(a, instructions))
                    .collect();
                let result = self.fresh_value();
                instructions.push(Instruction::Call {
                    result,
                    func_name: name.clone(),
                    args: arg_vals,
                    ty: BirType::I32,
                });
                result
            }
            Expr::Block(block) => {
                match self.lower_block(block, instructions) {
                    Some(StmtResult::Yield(v)) => v,
                    _ => unreachable!("block expression must yield (semantic analysis guarantees this)"),
                }
            }
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

fn convert_type(ty: &TypeAnnotation) -> BirType {
    match ty {
        TypeAnnotation::I32 => BirType::I32,
    }
}

pub fn lower_program(program: &Program) -> Result<BirModule> {
    let mut lowering = Lowering::new();
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

    fn lower_str(input: &str) -> String {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        let module = lower_program(&program).unwrap();
        print_module(&module)
    }

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
        // x_outer = %0 (literal 1)
        // x_inner = %1 (literal 10)
        // yield: %1 + 1 = %2 (literal 1), %3 (add %1, %2) → y = %3
        // return: %0 + %3 = %4
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
}
