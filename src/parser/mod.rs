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
        self.expect(Token::Arrow)?;
        let return_type = self.parse_type()?;
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
        let tok = self.expect(Token::Ident(String::new()))?;
        match &tok.node {
            Token::Ident(s) if s == "i32" => Ok(TypeAnnotation::I32),
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
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                self.expect(Token::Eq)?;
                let value = self.parse_expr()?;
                Stmt::Let { name, ty, value }
            }
            Token::Var => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                self.expect(Token::Eq)?;
                let value = self.parse_expr()?;
                Stmt::Var { name, ty, value }
            }
            Token::Return => {
                self.advance();
                let expr = self.parse_expr()?;
                Stmt::Return(Some(expr))
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

    // --- Expr (arithmetic + ident + call + block) ---

    fn parse_expr(&mut self) -> Result<Expr> {
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

    fn parse_term(&mut self) -> Result<Expr> {
        let mut left = self.parse_factor()?;
        loop {
            let op = match self.peek().node {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_factor()?;
            left = Expr::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr> {
        let tok = self.peek();
        match &tok.node {
            Token::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Number(n))
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
            _ => Err(BengalError::ParseError {
                message: format!("unexpected token `{}`", tok.node),
                span: tok.span,
            }),
        }
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
        let program = parse_str("func main() -> i32 { return 42; }").unwrap();
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
            parse_str("func main() -> i32 { let x: i32 = 10; return x; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert_eq!(
            stmts[0],
            Stmt::Let {
                name: "x".to_string(),
                ty: TypeAnnotation::I32,
                value: Expr::Number(10),
            }
        );
        assert_eq!(stmts[1], Stmt::Return(Some(Expr::Ident("x".to_string()))));
    }

    #[test]
    fn parse_func_with_params() {
        let program =
            parse_str("func add(a: i32, b: i32) -> i32 { return a + b; }").unwrap();
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
            "func main() -> i32 { let x: i32 = { yield 10; }; return x; }",
        )
        .unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert_eq!(
            stmts[0],
            Stmt::Let {
                name: "x".to_string(),
                ty: TypeAnnotation::I32,
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
            parse_str("func main() -> i32 { let x: = 10; }"),
            Err(BengalError::ParseError { .. })
        ));
    }
}
