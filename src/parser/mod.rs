pub mod ast;

use crate::error::{BengalError, Result};
use crate::lexer::token::{SpannedToken, Token};
use ast::*;

struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<SpannedToken>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &SpannedToken {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> &SpannedToken {
        let tok = &self.tokens[self.pos];
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: Token) -> Result<&SpannedToken> {
        let tok = &self.tokens[self.pos];
        if std::mem::discriminant(&tok.node) == std::mem::discriminant(&expected) {
            self.pos += 1;
            Ok(&self.tokens[self.pos - 1])
        } else {
            Err(BengalError::ParseError {
                message: format!("expected `{}`, found `{}`", expected, tok.node),
                span: tok.span,
            })
        }
    }

    // --- Program / Function ---

    fn parse_program(&mut self) -> Result<Program> {
        let mut functions = Vec::new();
        while self.peek().node != Token::Eof {
            functions.push(self.parse_function()?);
        }
        Ok(Program { functions })
    }

    fn parse_function(&mut self) -> Result<Function> {
        self.expect(Token::Func)?;
        let name_tok = self.expect(Token::Ident(String::new()))?;
        let name = match &name_tok.node {
            Token::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        let params = self.parse_param_list()?;
        let return_type = if self.peek().node == Token::Arrow {
            self.advance();
            self.parse_type()?
        } else {
            TypeAnnotation::Unit
        };
        let body = self.parse_block()?;
        Ok(Function {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_param_list(&mut self) -> Result<Vec<Param>> {
        self.expect(Token::LParen)?;
        let mut params = Vec::new();
        if self.peek().node != Token::RParen {
            params.push(self.parse_param()?);
            while self.peek().node == Token::Comma {
                self.advance();
                params.push(self.parse_param()?);
            }
        }
        self.expect(Token::RParen)?;
        Ok(params)
    }

    fn parse_param(&mut self) -> Result<Param> {
        let name_tok = self.expect(Token::Ident(String::new()))?;
        let name = match &name_tok.node {
            Token::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        self.expect(Token::Colon)?;
        let ty = self.parse_type()?;
        Ok(Param { name, ty })
    }

    fn parse_type(&mut self) -> Result<TypeAnnotation> {
        if self.peek().node == Token::LParen {
            self.advance();
            self.expect(Token::RParen)?;
            return Ok(TypeAnnotation::Unit);
        }
        let tok = self.expect(Token::Ident(String::new()))?;
        match &tok.node {
            Token::Ident(s) if s == "Int32" => Ok(TypeAnnotation::I32),
            Token::Ident(s) if s == "Int64" => Ok(TypeAnnotation::I64),
            Token::Ident(s) if s == "Float32" => Ok(TypeAnnotation::F32),
            Token::Ident(s) if s == "Float64" => Ok(TypeAnnotation::F64),
            Token::Ident(s) if s == "Bool" => Ok(TypeAnnotation::Bool),
            Token::Ident(s) if s == "Void" => Ok(TypeAnnotation::Unit),
            _ => Err(BengalError::ParseError {
                message: format!("unknown type `{}`", tok.node),
                span: tok.span,
            }),
        }
    }

    // --- Block / Stmt ---

    fn parse_block(&mut self) -> Result<Block> {
        self.expect(Token::LBrace)?;
        let mut stmts = Vec::new();
        while self.peek().node != Token::RBrace {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        Ok(Block { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        let stmt = match &self.peek().node {
            Token::Let => {
                self.advance();
                let name = self.expect_ident()?;
                let ty = if self.peek().node == Token::Colon {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(Token::Eq)?;
                let value = self.parse_expr()?;
                Stmt::Let { name, ty, value }
            }
            Token::Var => {
                self.advance();
                let name = self.expect_ident()?;
                let ty = if self.peek().node == Token::Colon {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(Token::Eq)?;
                let value = self.parse_expr()?;
                Stmt::Var { name, ty, value }
            }
            Token::Return => {
                self.advance();
                if self.peek().node == Token::Semicolon {
                    Stmt::Return(None)
                } else {
                    let expr = self.parse_expr()?;
                    Stmt::Return(Some(expr))
                }
            }
            Token::Break => {
                self.advance();
                if self.peek().node == Token::Semicolon {
                    Stmt::Break(None)
                } else {
                    let expr = self.parse_expr()?;
                    Stmt::Break(Some(expr))
                }
            }
            Token::Continue => {
                self.advance();
                Stmt::Continue
            }
            Token::Yield => {
                self.advance();
                let expr = self.parse_expr()?;
                Stmt::Yield(expr)
            }
            Token::Ident(_) => {
                // Lookahead: Ident followed by `=` → Assign, otherwise → Expr stmt
                if self.tokens.get(self.pos + 1).map(|t| &t.node) == Some(&Token::Eq) {
                    let name = self.expect_ident()?;
                    self.advance(); // consume `=`
                    let value = self.parse_expr()?;
                    Stmt::Assign { name, value }
                } else {
                    let expr = self.parse_expr()?;
                    Stmt::Expr(expr)
                }
            }
            _ => {
                let expr = self.parse_expr()?;
                Stmt::Expr(expr)
            }
        };
        self.expect(Token::Semicolon)?;
        Ok(stmt)
    }

    fn expect_ident(&mut self) -> Result<String> {
        let tok = self.expect(Token::Ident(String::new()))?;
        match &tok.node {
            Token::Ident(s) => Ok(s.clone()),
            _ => unreachable!(),
        }
    }

    // --- Expr (7-level precedence chain) ---

    // Level 1 (lowest): ||
    fn parse_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        loop {
            if self.peek().node != Token::PipePipe {
                break;
            }
            self.advance();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // Level 2: &&
    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_equality()?;
        loop {
            if self.peek().node != Token::AmpAmp {
                break;
            }
            self.advance();
            let right = self.parse_equality()?;
            left = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // Level 3: == !=
    fn parse_equality(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;
        loop {
            let op = match self.peek().node {
                Token::EqEq => BinOp::Eq,
                Token::NotEq => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // Level 4: < > <= >=
    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek().node {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEq => BinOp::Le,
                Token::GtEq => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // Level 5: + -
    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_term()?;
        loop {
            let op = match self.peek().node {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_term()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // Level 6: * /
    fn parse_term(&mut self) -> Result<Expr> {
        let mut left = self.parse_cast()?;
        loop {
            let op = match self.peek().node {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_cast()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    // Level 7: as (postfix)
    fn parse_cast(&mut self) -> Result<Expr> {
        let mut expr = self.parse_unary()?;
        while self.peek().node == Token::As {
            self.advance();
            let target_type = self.parse_type()?;
            expr = Expr::Cast {
                expr: Box::new(expr),
                target_type,
            };
        }
        Ok(expr)
    }

    // Level 8 (highest): ! (prefix)
    fn parse_unary(&mut self) -> Result<Expr> {
        if self.peek().node == Token::Bang {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            });
        }
        self.parse_factor()
    }

    fn parse_factor(&mut self) -> Result<Expr> {
        let tok = self.peek();
        match &tok.node {
            Token::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::Float(f) => {
                let f = *f;
                self.advance();
                Ok(Expr::Float(f))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            Token::Ident(_) => {
                let name = self.expect_ident()?;
                if self.peek().node == Token::LParen {
                    self.parse_call(name)
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            Token::LBrace => {
                let block = self.parse_block()?;
                Ok(Expr::Block(block))
            }
            Token::If => self.parse_if_expr(),
            Token::While => self.parse_while_expr(),
            _ => Err(BengalError::ParseError {
                message: format!("unexpected token `{}`", tok.node),
                span: tok.span,
            }),
        }
    }

    fn parse_if_expr(&mut self) -> Result<Expr> {
        self.expect(Token::If)?;
        let condition = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let else_block = if self.peek().node == Token::Else {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };
        Ok(Expr::If {
            condition: Box::new(condition),
            then_block,
            else_block,
        })
    }

    fn parse_while_expr(&mut self) -> Result<Expr> {
        self.expect(Token::While)?;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        let nobreak = if self.peek().node == Token::Nobreak {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };
        Ok(Expr::While {
            condition: Box::new(condition),
            body,
            nobreak,
        })
    }

    fn parse_call(&mut self, name: String) -> Result<Expr> {
        self.expect(Token::LParen)?;
        let mut args = Vec::new();
        if self.peek().node != Token::RParen {
            args.push(self.parse_expr()?);
            while self.peek().node == Token::Comma {
                self.advance();
                args.push(self.parse_expr()?);
            }
        }
        self.expect(Token::RParen)?;
        Ok(Expr::Call { name, args })
    }
}

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Program> {
    let mut parser = Parser::new(tokens);

    // Phase 1 compatibility: if the first token is not `func`, treat as a bare expression
    if parser.peek().node == Token::Func {
        let program = parser.parse_program()?;
        let next = parser.peek();
        if next.node != Token::Eof {
            return Err(BengalError::ParseError {
                message: format!("unexpected token `{}`", next.node),
                span: next.span,
            });
        }
        Ok(program)
    } else {
        let expr = parser.parse_expr()?;
        let next = parser.peek();
        if next.node != Token::Eof {
            return Err(BengalError::ParseError {
                message: format!("unexpected token `{}`", next.node),
                span: next.span,
            });
        }
        Ok(Program {
            functions: vec![Function {
                name: "main".to_string(),
                params: vec![],
                return_type: TypeAnnotation::I32,
                body: Block {
                    stmts: vec![Stmt::Return(Some(expr))],
                },
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse_str(input: &str) -> Result<Program> {
        let tokens = tokenize(input).unwrap();
        parse(tokens)
    }

    /// Helper: extract the return expr from the implicit main (Phase 1 compat)
    fn parse_expr_str(input: &str) -> Expr {
        let program = parse_str(input).unwrap();
        match program.functions[0].body.stmts.last().unwrap() {
            Stmt::Return(Some(expr)) => expr.clone(),
            _ => panic!("expected Return statement"),
        }
    }

    // --- Phase 1 compatibility tests ---

    #[test]
    fn precedence_mul_over_add() {
        let expr = parse_expr_str("2 + 3 * 4");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(Expr::Number(2)),
                right: Box::new(Expr::BinaryOp {
                    op: BinOp::Mul,
                    left: Box::new(Expr::Number(3)),
                    right: Box::new(Expr::Number(4)),
                }),
            }
        );
    }

    #[test]
    fn parentheses_override_precedence() {
        let expr = parse_expr_str("(2 + 3) * 4");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Mul,
                left: Box::new(Expr::BinaryOp {
                    op: BinOp::Add,
                    left: Box::new(Expr::Number(2)),
                    right: Box::new(Expr::Number(3)),
                }),
                right: Box::new(Expr::Number(4)),
            }
        );
    }

    #[test]
    fn single_number() {
        assert_eq!(parse_expr_str("10"), Expr::Number(10));
    }

    #[test]
    fn error_incomplete_expr() {
        assert!(matches!(
            parse_str("1 + "),
            Err(BengalError::ParseError { .. })
        ));
    }

    #[test]
    fn error_unconsumed_token() {
        assert!(matches!(
            parse_str("1 2"),
            Err(BengalError::ParseError { .. })
        ));
    }

    #[test]
    fn error_unconsumed_rparen() {
        assert!(matches!(
            parse_str("2 + 3)"),
            Err(BengalError::ParseError { .. })
        ));
    }

    // --- Phase 2 tests ---

    #[test]
    fn parse_func_return() {
        let program = parse_str("func main() -> Int32 { return 42; }").unwrap();
        assert_eq!(program.functions.len(), 1);
        let f = &program.functions[0];
        assert_eq!(f.name, "main");
        assert_eq!(f.params, vec![]);
        assert_eq!(f.return_type, TypeAnnotation::I32);
        assert_eq!(f.body.stmts, vec![Stmt::Return(Some(Expr::Number(42)))]);
    }

    #[test]
    fn parse_let_return() {
        let program =
            parse_str("func main() -> Int32 { let x: Int32 = 10; return x; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert_eq!(
            stmts[0],
            Stmt::Let {
                name: "x".to_string(),
                ty: Some(TypeAnnotation::I32),
                value: Expr::Number(10),
            }
        );
        assert_eq!(stmts[1], Stmt::Return(Some(Expr::Ident("x".to_string()))));
    }

    #[test]
    fn parse_func_with_params() {
        let program =
            parse_str("func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap();
        let f = &program.functions[0];
        assert_eq!(f.name, "add");
        assert_eq!(
            f.params,
            vec![
                Param { name: "a".to_string(), ty: TypeAnnotation::I32 },
                Param { name: "b".to_string(), ty: TypeAnnotation::I32 },
            ]
        );
        assert_eq!(
            f.body.stmts[0],
            Stmt::Return(Some(Expr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(Expr::Ident("a".to_string())),
                right: Box::new(Expr::Ident("b".to_string())),
            }))
        );
    }

    #[test]
    fn parse_block_expr_yield() {
        let program = parse_str(
            "func main() -> Int32 { let x: Int32 = { yield 10; }; return x; }",
        )
        .unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert_eq!(
            stmts[0],
            Stmt::Let {
                name: "x".to_string(),
                ty: Some(TypeAnnotation::I32),
                value: Expr::Block(Block {
                    stmts: vec![Stmt::Yield(Expr::Number(10))],
                }),
            }
        );
        assert_eq!(stmts[1], Stmt::Return(Some(Expr::Ident("x".to_string()))));
    }

    #[test]
    fn parse_phase1_compat() {
        let program = parse_str("2 + 3 * 4").unwrap();
        assert_eq!(program.functions.len(), 1);
        let f = &program.functions[0];
        assert_eq!(f.name, "main");
        assert_eq!(f.params, vec![]);
        assert_eq!(f.return_type, TypeAnnotation::I32);
        assert_eq!(f.body.stmts.len(), 1);
        assert!(matches!(&f.body.stmts[0], Stmt::Return(Some(Expr::BinaryOp { .. }))));
    }

    #[test]
    fn error_missing_type_annotation() {
        assert!(matches!(
            parse_str("func main() -> Int32 { let x: = 10; }"),
            Err(BengalError::ParseError { .. })
        ));
    }

    // --- Phase 3 tests ---

    #[test]
    fn parse_if_else() {
        let program = parse_str(
            "func main() -> Int32 { if true { yield 1; } else { yield 2; }; return 0; }",
        )
        .unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], Stmt::Expr(Expr::If { else_block: Some(_), .. })));
    }

    #[test]
    fn parse_while() {
        let program =
            parse_str("func main() -> Int32 { while false { }; return 0; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], Stmt::Expr(Expr::While { .. })));
    }

    #[test]
    fn parse_comparison() {
        let expr = parse_expr_str("1 < 2");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Lt,
                left: Box::new(Expr::Number(1)),
                right: Box::new(Expr::Number(2)),
            }
        );
    }

    #[test]
    fn parse_unit_return_function() {
        let program = parse_str("func foo() { return; }").unwrap();
        let f = &program.functions[0];
        assert_eq!(f.name, "foo");
        assert_eq!(f.return_type, TypeAnnotation::Unit);
        assert_eq!(f.body.stmts, vec![Stmt::Return(None)]);
    }

    #[test]
    fn parse_logical_precedence() {
        // true && false || !true → Or(And(true, false), Not(true))
        let expr = parse_expr_str("true && false || !true");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Or,
                left: Box::new(Expr::BinaryOp {
                    op: BinOp::And,
                    left: Box::new(Expr::Bool(true)),
                    right: Box::new(Expr::Bool(false)),
                }),
                right: Box::new(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(Expr::Bool(true)),
                }),
            }
        );
    }

    // --- Phase 4 tests ---

    #[test]
    fn parse_let_type_inference() {
        let program =
            parse_str("func main() -> Int32 { let x = 42; return x; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(
            stmts[0],
            Stmt::Let {
                name: "x".to_string(),
                ty: None,
                value: Expr::Number(42),
            }
        );
    }

    #[test]
    fn parse_let_with_i64() {
        let program =
            parse_str("func main() -> Int32 { let x: Int64 = 42; return 0; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(
            stmts[0],
            Stmt::Let {
                name: "x".to_string(),
                ty: Some(TypeAnnotation::I64),
                value: Expr::Number(42),
            }
        );
    }

    #[test]
    fn parse_cast_expr() {
        let expr = parse_expr_str("42 as Int64");
        assert_eq!(
            expr,
            Expr::Cast {
                expr: Box::new(Expr::Number(42)),
                target_type: TypeAnnotation::I64,
            }
        );
    }

    #[test]
    fn parse_break_no_value() {
        let program =
            parse_str("func main() -> Int32 { while true { break; }; return 0; }")
                .unwrap();
        let stmts = &program.functions[0].body.stmts;
        if let Stmt::Expr(Expr::While { body, .. }) = &stmts[0] {
            assert_eq!(body.stmts[0], Stmt::Break(None));
        } else {
            panic!("expected while");
        }
    }

    #[test]
    fn parse_break_with_value() {
        let program =
            parse_str("func main() -> Int32 { while true { break 10; }; return 0; }")
                .unwrap();
        let stmts = &program.functions[0].body.stmts;
        if let Stmt::Expr(Expr::While { body, .. }) = &stmts[0] {
            assert_eq!(body.stmts[0], Stmt::Break(Some(Expr::Number(10))));
        } else {
            panic!("expected while");
        }
    }

    #[test]
    fn parse_continue() {
        let program =
            parse_str("func main() -> Int32 { while true { continue; }; return 0; }")
                .unwrap();
        let stmts = &program.functions[0].body.stmts;
        if let Stmt::Expr(Expr::While { body, .. }) = &stmts[0] {
            assert_eq!(body.stmts[0], Stmt::Continue);
        } else {
            panic!("expected while");
        }
    }

    #[test]
    fn parse_float_literal() {
        let expr = parse_expr_str("3.14");
        assert_eq!(expr, Expr::Float(3.14));
    }

    #[test]
    fn parse_cast_precedence() {
        // 1 + 2 as Int64 → Add(1, Cast(2, I64))
        let expr = parse_expr_str("1 + 2 as Int64");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(Expr::Number(1)),
                right: Box::new(Expr::Cast {
                    expr: Box::new(Expr::Number(2)),
                    target_type: TypeAnnotation::I64,
                }),
            }
        );
    }

    #[test]
    fn parse_while_nobreak() {
        let program = parse_str(
            "func main() -> Int32 { while true { break 1; } nobreak { yield 2; }; return 0; }",
        )
        .unwrap();
        let stmts = &program.functions[0].body.stmts;
        if let Stmt::Expr(Expr::While { nobreak, .. }) = &stmts[0] {
            assert!(nobreak.is_some());
            let nb = nobreak.as_ref().unwrap();
            assert_eq!(nb.stmts[0], Stmt::Yield(Expr::Number(2)));
        } else {
            panic!("expected while");
        }
    }
}
