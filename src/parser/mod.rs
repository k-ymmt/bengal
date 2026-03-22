pub mod ast;

use crate::error::{BengalError, Result};
use crate::lexer::token::{SpannedToken, Token};
use ast::*;

struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    next_id: u32,
}

impl Parser {
    fn new(tokens: Vec<SpannedToken>) -> Self {
        Self {
            tokens,
            pos: 0,
            next_id: 0,
        }
    }

    fn expr(&mut self, kind: ExprKind) -> Expr {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        Expr { id, kind }
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
        Ok(Program {
            structs: vec![],
            functions,
        })
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
            left = self.expr(ExprKind::BinaryOp {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            });
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
            left = self.expr(ExprKind::BinaryOp {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            });
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
            left = self.expr(ExprKind::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            });
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
            left = self.expr(ExprKind::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            });
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
            left = self.expr(ExprKind::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            });
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
            left = self.expr(ExprKind::BinaryOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            });
        }
        Ok(left)
    }

    // Level 7: as (postfix)
    fn parse_cast(&mut self) -> Result<Expr> {
        let mut expr = self.parse_unary()?;
        while self.peek().node == Token::As {
            self.advance();
            let target_type = self.parse_type()?;
            expr = self.expr(ExprKind::Cast {
                expr: Box::new(expr),
                target_type,
            });
        }
        Ok(expr)
    }

    // Level 8 (highest): ! (prefix)
    fn parse_unary(&mut self) -> Result<Expr> {
        if self.peek().node == Token::Bang {
            self.advance();
            let operand = self.parse_unary()?;
            let e = self.expr(ExprKind::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            });
            return Ok(e);
        }
        self.parse_factor()
    }

    fn parse_factor(&mut self) -> Result<Expr> {
        let tok = self.peek();
        match &tok.node {
            Token::Number(n) => {
                let n = *n;
                self.advance();
                Ok(self.expr(ExprKind::Number(n)))
            }
            Token::Float(f) => {
                let f = *f;
                self.advance();
                Ok(self.expr(ExprKind::Float(f)))
            }
            Token::True => {
                self.advance();
                Ok(self.expr(ExprKind::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(self.expr(ExprKind::Bool(false)))
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
                    Ok(self.expr(ExprKind::Ident(name)))
                }
            }
            Token::LBrace => {
                let block = self.parse_block()?;
                Ok(self.expr(ExprKind::Block(block)))
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
        Ok(self.expr(ExprKind::If {
            condition: Box::new(condition),
            then_block,
            else_block,
        }))
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
        Ok(self.expr(ExprKind::While {
            condition: Box::new(condition),
            body,
            nobreak,
        }))
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
        Ok(self.expr(ExprKind::Call { name, args }))
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
            structs: vec![],
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

    fn e(kind: ExprKind) -> Expr {
        Expr {
            id: NodeId(0),
            kind,
        }
    }

    fn normalize_expr(expr: &Expr) -> Expr {
        let kind = match &expr.kind {
            ExprKind::Number(n) => ExprKind::Number(*n),
            ExprKind::Float(f) => ExprKind::Float(*f),
            ExprKind::Bool(b) => ExprKind::Bool(*b),
            ExprKind::Ident(s) => ExprKind::Ident(s.clone()),
            ExprKind::BinaryOp { op, left, right } => ExprKind::BinaryOp {
                op: *op,
                left: Box::new(normalize_expr(left)),
                right: Box::new(normalize_expr(right)),
            },
            ExprKind::UnaryOp { op, operand } => ExprKind::UnaryOp {
                op: *op,
                operand: Box::new(normalize_expr(operand)),
            },
            ExprKind::Call { name, args } => ExprKind::Call {
                name: name.clone(),
                args: args.iter().map(normalize_expr).collect(),
            },
            ExprKind::Block(block) => ExprKind::Block(normalize_block(block)),
            ExprKind::If {
                condition,
                then_block,
                else_block,
            } => ExprKind::If {
                condition: Box::new(normalize_expr(condition)),
                then_block: normalize_block(then_block),
                else_block: else_block.as_ref().map(|b| normalize_block(b)),
            },
            ExprKind::While {
                condition,
                body,
                nobreak,
            } => ExprKind::While {
                condition: Box::new(normalize_expr(condition)),
                body: normalize_block(body),
                nobreak: nobreak.as_ref().map(|b| normalize_block(b)),
            },
            ExprKind::Cast { expr, target_type } => ExprKind::Cast {
                expr: Box::new(normalize_expr(expr)),
                target_type: target_type.clone(),
            },
            ExprKind::StructInit { name, args } => ExprKind::StructInit {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|(l, e)| (l.clone(), normalize_expr(e)))
                    .collect(),
            },
            ExprKind::FieldAccess { object, field } => ExprKind::FieldAccess {
                object: Box::new(normalize_expr(object)),
                field: field.clone(),
            },
            ExprKind::SelfRef => ExprKind::SelfRef,
        };
        Expr {
            id: NodeId(0),
            kind,
        }
    }

    fn normalize_stmt(stmt: &Stmt) -> Stmt {
        match stmt {
            Stmt::Let { name, ty, value } => Stmt::Let {
                name: name.clone(),
                ty: ty.clone(),
                value: normalize_expr(value),
            },
            Stmt::Var { name, ty, value } => Stmt::Var {
                name: name.clone(),
                ty: ty.clone(),
                value: normalize_expr(value),
            },
            Stmt::Assign { name, value } => Stmt::Assign {
                name: name.clone(),
                value: normalize_expr(value),
            },
            Stmt::Return(opt) => Stmt::Return(opt.as_ref().map(normalize_expr)),
            Stmt::Yield(expr) => Stmt::Yield(normalize_expr(expr)),
            Stmt::Break(opt) => Stmt::Break(opt.as_ref().map(normalize_expr)),
            Stmt::Continue => Stmt::Continue,
            Stmt::Expr(expr) => Stmt::Expr(normalize_expr(expr)),
            Stmt::FieldAssign {
                object,
                field,
                value,
            } => Stmt::FieldAssign {
                object: Box::new(normalize_expr(object)),
                field: field.clone(),
                value: normalize_expr(value),
            },
        }
    }

    fn normalize_block(block: &Block) -> Block {
        Block {
            stmts: block.stmts.iter().map(normalize_stmt).collect(),
        }
    }

    fn parse_expr_str(input: &str) -> Expr {
        let program = parse_str(input).unwrap();
        let expr = match program.functions[0].body.stmts.last().unwrap() {
            Stmt::Return(Some(expr)) => expr.clone(),
            _ => panic!("expected Return statement"),
        };
        normalize_expr(&expr)
    }

    fn collect_expr_ids(expr: &Expr, ids: &mut Vec<NodeId>) {
        ids.push(expr.id);
        match &expr.kind {
            ExprKind::BinaryOp { left, right, .. } => {
                collect_expr_ids(left, ids);
                collect_expr_ids(right, ids);
            }
            ExprKind::UnaryOp { operand, .. } => {
                collect_expr_ids(operand, ids);
            }
            ExprKind::Call { args, .. } => {
                for arg in args {
                    collect_expr_ids(arg, ids);
                }
            }
            ExprKind::Cast { expr, .. } => {
                collect_expr_ids(expr, ids);
            }
            ExprKind::Block(block) => collect_block_expr_ids(block, ids),
            ExprKind::If {
                condition,
                then_block,
                else_block,
            } => {
                collect_expr_ids(condition, ids);
                collect_block_expr_ids(then_block, ids);
                if let Some(b) = else_block {
                    collect_block_expr_ids(b, ids);
                }
            }
            ExprKind::While {
                condition,
                body,
                nobreak,
            } => {
                collect_expr_ids(condition, ids);
                collect_block_expr_ids(body, ids);
                if let Some(b) = nobreak {
                    collect_block_expr_ids(b, ids);
                }
            }
            ExprKind::StructInit { args, .. } => {
                for (_, arg) in args {
                    collect_expr_ids(arg, ids);
                }
            }
            ExprKind::FieldAccess { object, .. } => {
                collect_expr_ids(object, ids);
            }
            _ => {}
        }
    }

    fn collect_block_expr_ids(block: &Block, ids: &mut Vec<NodeId>) {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let { value, .. } | Stmt::Var { value, .. } | Stmt::Assign { value, .. } => {
                    collect_expr_ids(value, ids);
                }
                Stmt::Return(Some(e)) | Stmt::Yield(e) | Stmt::Break(Some(e)) | Stmt::Expr(e) => {
                    collect_expr_ids(e, ids);
                }
                Stmt::FieldAssign { object, value, .. } => {
                    collect_expr_ids(object, ids);
                    collect_expr_ids(value, ids);
                }
                _ => {}
            }
        }
    }

    // --- Phase 1 compatibility tests ---

    #[test]
    fn precedence_mul_over_add() {
        let expr = parse_expr_str("2 + 3 * 4");
        assert_eq!(
            expr,
            e(ExprKind::BinaryOp {
                op: BinOp::Add,
                left: Box::new(e(ExprKind::Number(2))),
                right: Box::new(e(ExprKind::BinaryOp {
                    op: BinOp::Mul,
                    left: Box::new(e(ExprKind::Number(3))),
                    right: Box::new(e(ExprKind::Number(4))),
                })),
            })
        );
    }

    #[test]
    fn parentheses_override_precedence() {
        let expr = parse_expr_str("(2 + 3) * 4");
        assert_eq!(
            expr,
            e(ExprKind::BinaryOp {
                op: BinOp::Mul,
                left: Box::new(e(ExprKind::BinaryOp {
                    op: BinOp::Add,
                    left: Box::new(e(ExprKind::Number(2))),
                    right: Box::new(e(ExprKind::Number(3))),
                })),
                right: Box::new(e(ExprKind::Number(4))),
            })
        );
    }

    #[test]
    fn single_number() {
        assert_eq!(parse_expr_str("10"), e(ExprKind::Number(10)));
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
        assert_eq!(
            normalize_stmt(&f.body.stmts[0]),
            Stmt::Return(Some(e(ExprKind::Number(42))))
        );
    }

    #[test]
    fn parse_let_return() {
        let program = parse_str("func main() -> Int32 { let x: Int32 = 10; return x; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert_eq!(
            normalize_stmt(&stmts[0]),
            Stmt::Let {
                name: "x".to_string(),
                ty: Some(TypeAnnotation::I32),
                value: e(ExprKind::Number(10)),
            }
        );
        assert_eq!(
            normalize_stmt(&stmts[1]),
            Stmt::Return(Some(e(ExprKind::Ident("x".to_string()))))
        );
    }

    #[test]
    fn parse_func_with_params() {
        let program = parse_str("func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap();
        let f = &program.functions[0];
        assert_eq!(f.name, "add");
        assert_eq!(
            f.params,
            vec![
                Param {
                    name: "a".to_string(),
                    ty: TypeAnnotation::I32
                },
                Param {
                    name: "b".to_string(),
                    ty: TypeAnnotation::I32
                },
            ]
        );
        assert_eq!(
            normalize_stmt(&f.body.stmts[0]),
            Stmt::Return(Some(e(ExprKind::BinaryOp {
                op: BinOp::Add,
                left: Box::new(e(ExprKind::Ident("a".to_string()))),
                right: Box::new(e(ExprKind::Ident("b".to_string()))),
            })))
        );
    }

    #[test]
    fn parse_block_expr_yield() {
        let program =
            parse_str("func main() -> Int32 { let x: Int32 = { yield 10; }; return x; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert_eq!(
            normalize_stmt(&stmts[0]),
            Stmt::Let {
                name: "x".to_string(),
                ty: Some(TypeAnnotation::I32),
                value: e(ExprKind::Block(Block {
                    stmts: vec![Stmt::Yield(e(ExprKind::Number(10)))],
                })),
            }
        );
        assert_eq!(
            normalize_stmt(&stmts[1]),
            Stmt::Return(Some(e(ExprKind::Ident("x".to_string()))))
        );
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
        assert!(matches!(
            &f.body.stmts[0],
            Stmt::Return(Some(Expr {
                kind: ExprKind::BinaryOp { .. },
                ..
            }))
        ));
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
        let program =
            parse_str("func main() -> Int32 { if true { yield 1; } else { yield 2; }; return 0; }")
                .unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert!(matches!(
            &stmts[0],
            Stmt::Expr(Expr {
                kind: ExprKind::If {
                    else_block: Some(_),
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn parse_while() {
        let program = parse_str("func main() -> Int32 { while false { }; return 0; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(stmts.len(), 2);
        assert!(matches!(
            &stmts[0],
            Stmt::Expr(Expr {
                kind: ExprKind::While { .. },
                ..
            })
        ));
    }

    #[test]
    fn parse_comparison() {
        let expr = parse_expr_str("1 < 2");
        assert_eq!(
            expr,
            e(ExprKind::BinaryOp {
                op: BinOp::Lt,
                left: Box::new(e(ExprKind::Number(1))),
                right: Box::new(e(ExprKind::Number(2))),
            })
        );
    }

    #[test]
    fn parse_unit_return_function() {
        let program = parse_str("func foo() { return; }").unwrap();
        let f = &program.functions[0];
        assert_eq!(f.name, "foo");
        assert_eq!(f.return_type, TypeAnnotation::Unit);
        assert_eq!(normalize_stmt(&f.body.stmts[0]), Stmt::Return(None));
    }

    #[test]
    fn parse_logical_precedence() {
        let expr = parse_expr_str("true && false || !true");
        assert_eq!(
            expr,
            e(ExprKind::BinaryOp {
                op: BinOp::Or,
                left: Box::new(e(ExprKind::BinaryOp {
                    op: BinOp::And,
                    left: Box::new(e(ExprKind::Bool(true))),
                    right: Box::new(e(ExprKind::Bool(false))),
                })),
                right: Box::new(e(ExprKind::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(e(ExprKind::Bool(true))),
                })),
            })
        );
    }

    // --- Phase 4 tests ---

    #[test]
    fn parse_let_type_inference() {
        let program = parse_str("func main() -> Int32 { let x = 42; return x; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(
            normalize_stmt(&stmts[0]),
            Stmt::Let {
                name: "x".to_string(),
                ty: None,
                value: e(ExprKind::Number(42)),
            }
        );
    }

    #[test]
    fn parse_let_with_i64() {
        let program = parse_str("func main() -> Int32 { let x: Int64 = 42; return 0; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        assert_eq!(
            normalize_stmt(&stmts[0]),
            Stmt::Let {
                name: "x".to_string(),
                ty: Some(TypeAnnotation::I64),
                value: e(ExprKind::Number(42)),
            }
        );
    }

    #[test]
    fn parse_cast_expr() {
        let expr = parse_expr_str("42 as Int64");
        assert_eq!(
            expr,
            e(ExprKind::Cast {
                expr: Box::new(e(ExprKind::Number(42))),
                target_type: TypeAnnotation::I64,
            })
        );
    }

    #[test]
    fn parse_break_no_value() {
        let program =
            parse_str("func main() -> Int32 { while true { break; }; return 0; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        if let Stmt::Expr(Expr {
            kind: ExprKind::While { body, .. },
            ..
        }) = &stmts[0]
        {
            assert_eq!(body.stmts[0], Stmt::Break(None));
        } else {
            panic!("expected while");
        }
    }

    #[test]
    fn parse_break_with_value() {
        let program =
            parse_str("func main() -> Int32 { while true { break 10; }; return 0; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        if let Stmt::Expr(Expr {
            kind: ExprKind::While { body, .. },
            ..
        }) = &stmts[0]
        {
            assert_eq!(
                normalize_stmt(&body.stmts[0]),
                Stmt::Break(Some(e(ExprKind::Number(10))))
            );
        } else {
            panic!("expected while");
        }
    }

    #[test]
    fn parse_continue() {
        let program =
            parse_str("func main() -> Int32 { while true { continue; }; return 0; }").unwrap();
        let stmts = &program.functions[0].body.stmts;
        if let Stmt::Expr(Expr {
            kind: ExprKind::While { body, .. },
            ..
        }) = &stmts[0]
        {
            assert_eq!(body.stmts[0], Stmt::Continue);
        } else {
            panic!("expected while");
        }
    }

    #[test]
    fn parse_float_literal() {
        let expr = parse_expr_str("3.14");
        assert_eq!(expr, e(ExprKind::Float(3.14)));
    }

    #[test]
    fn parse_cast_precedence() {
        let expr = parse_expr_str("1 + 2 as Int64");
        assert_eq!(
            expr,
            e(ExprKind::BinaryOp {
                op: BinOp::Add,
                left: Box::new(e(ExprKind::Number(1))),
                right: Box::new(e(ExprKind::Cast {
                    expr: Box::new(e(ExprKind::Number(2))),
                    target_type: TypeAnnotation::I64,
                })),
            })
        );
    }

    #[test]
    fn parse_while_nobreak() {
        let program = parse_str(
            "func main() -> Int32 { while true { break 1; } nobreak { yield 2; }; return 0; }",
        )
        .unwrap();
        let stmts = &program.functions[0].body.stmts;
        if let Stmt::Expr(Expr {
            kind: ExprKind::While { nobreak, .. },
            ..
        }) = &stmts[0]
        {
            assert!(nobreak.is_some());
            let nb = nobreak.as_ref().unwrap();
            assert_eq!(
                normalize_stmt(&nb.stmts[0]),
                Stmt::Yield(e(ExprKind::Number(2)))
            );
        } else {
            panic!("expected while");
        }
    }

    // --- NodeId allocation tests ---

    #[test]
    fn node_ids_are_unique() {
        let program = parse_str("func main() -> Int32 { return 1 + 2 * 3; }").unwrap();
        if let Stmt::Return(Some(expr)) = &program.functions[0].body.stmts[0] {
            let mut ids = Vec::new();
            collect_expr_ids(expr, &mut ids);
            assert_eq!(ids.len(), 5);
            let unique: std::collections::HashSet<_> = ids.iter().collect();
            assert_eq!(unique.len(), ids.len(), "all NodeIds must be unique");
        } else {
            panic!("expected return");
        }
    }

    #[test]
    fn node_ids_are_sequential() {
        let program = parse_str("func main() -> Int32 { return a + b; }").unwrap();
        if let Stmt::Return(Some(expr)) = &program.functions[0].body.stmts[0] {
            let mut ids = Vec::new();
            collect_expr_ids(expr, &mut ids);
            let mut sorted: Vec<u32> = ids.iter().map(|id| id.0).collect();
            sorted.sort();
            assert_eq!(sorted, vec![0, 1, 2]);
        } else {
            panic!("expected return");
        }
    }
}
