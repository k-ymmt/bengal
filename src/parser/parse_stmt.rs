use crate::error::{BengalError, Result};
use crate::lexer::token::Token;
use crate::parser::ast::*;

impl super::Parser {
    // --- Block / Stmt ---

    pub(super) fn parse_block(&mut self) -> Result<Block> {
        self.expect(Token::LBrace)?;
        let mut stmts = Vec::new();
        while self.peek().node != Token::RBrace {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        Ok(Block { stmts })
    }

    pub(super) fn parse_stmt(&mut self) -> Result<Stmt> {
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
            _ => {
                let lhs = self.parse_expr()?;
                if self.peek().node == Token::Eq {
                    self.advance();
                    let value = self.parse_expr()?;
                    match &lhs.kind {
                        ExprKind::Ident(name) => Stmt::Assign {
                            name: name.clone(),
                            value,
                        },
                        ExprKind::FieldAccess { object, field } => Stmt::FieldAssign {
                            object: Box::new((**object).clone()),
                            field: field.clone(),
                            value,
                        },
                        ExprKind::IndexAccess { object, index } => Stmt::IndexAssign {
                            object: Box::new((**object).clone()),
                            index: Box::new((**index).clone()),
                            value,
                        },
                        _ => {
                            return Err(BengalError::ParseError {
                                message: "invalid assignment target".to_string(),
                                span: self.tokens[self.pos - 1].span,
                            });
                        }
                    }
                } else {
                    Stmt::Expr(lhs)
                }
            }
        };
        self.expect(Token::Semicolon)?;
        Ok(stmt)
    }

    pub(super) fn expect_ident(&mut self) -> Result<String> {
        let tok = self.expect(Token::Ident(String::new()))?;
        match &tok.node {
            Token::Ident(s) => Ok(s.clone()),
            _ => unreachable!(),
        }
    }
}
