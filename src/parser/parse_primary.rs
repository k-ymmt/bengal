use crate::error::{BengalError, Result};
use crate::lexer::token::Token;
use crate::parser::ast::*;

impl super::Parser {
    pub(super) fn parse_factor(&mut self) -> Result<Expr> {
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match &self.peek().node {
                Token::Dot => {
                    let start = expr.span.start;
                    self.advance();
                    let field = self.expect_ident()?;
                    // Check if this is a method call: field followed by `(`
                    if self.peek().node == Token::LParen {
                        self.advance(); // consume `(`
                        let mut args = Vec::new();
                        if self.peek().node != Token::RParen {
                            args.push(self.parse_expr()?);
                            while self.peek().node == Token::Comma {
                                self.advance();
                                args.push(self.parse_expr()?);
                            }
                        }
                        self.expect(Token::RParen)?;
                        let span = self.span_from(start);
                        expr = self.expr(
                            ExprKind::MethodCall {
                                object: Box::new(expr),
                                method: field,
                                args,
                            },
                            span,
                        );
                    } else {
                        let span = self.span_from(start);
                        expr = self.expr(
                            ExprKind::FieldAccess {
                                object: Box::new(expr),
                                field,
                            },
                            span,
                        );
                    }
                }
                Token::Lt if self.no_space_before_current() => {
                    let start = expr.span.start;
                    let name = match &expr.kind {
                        ExprKind::Ident(name) => name.clone(),
                        _ => break,
                    };
                    let type_args = self.parse_type_arg_list()?;
                    if self.peek().node != Token::LParen {
                        return Err(BengalError::ParseError {
                            message: "expected `(` after type arguments".to_string(),
                            span: self.peek().span,
                        });
                    }
                    expr = self.parse_postfix_call_with_type_args(name, type_args, start)?;
                }
                Token::LBracket => {
                    let start = expr.span.start;
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(Token::RBracket)?;
                    let span = self.span_from(start);
                    expr = self.expr(
                        ExprKind::IndexAccess {
                            object: Box::new(expr),
                            index: Box::new(index),
                        },
                        span,
                    );
                }
                Token::LParen => {
                    expr = self.parse_postfix_call(expr)?;
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let tok = self.peek();
        match &tok.node {
            Token::Number(n) => {
                let n = *n;
                let span = tok.span;
                self.advance();
                Ok(self.expr(ExprKind::Number(n), span))
            }
            Token::Float(f) => {
                let f = *f;
                let span = tok.span;
                self.advance();
                Ok(self.expr(ExprKind::Float(f), span))
            }
            Token::True => {
                let span = tok.span;
                self.advance();
                Ok(self.expr(ExprKind::Bool(true), span))
            }
            Token::False => {
                let span = tok.span;
                self.advance();
                Ok(self.expr(ExprKind::Bool(false), span))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            Token::Ident(_) => {
                let span = tok.span;
                let name = self.expect_ident()?;
                Ok(self.expr(ExprKind::Ident(name), span))
            }
            Token::LBracket => {
                let start = tok.span.start;
                self.advance();
                let mut elements = Vec::new();
                if self.peek().node != Token::RBracket {
                    elements.push(self.parse_expr()?);
                    while self.peek().node == Token::Comma {
                        self.advance();
                        elements.push(self.parse_expr()?);
                    }
                }
                self.expect(Token::RBracket)?;
                let span = self.span_from(start);
                Ok(self.expr(ExprKind::ArrayLiteral { elements }, span))
            }
            Token::LBrace => {
                let start = tok.span.start;
                let block = self.parse_block()?;
                let span = self.span_from(start);
                Ok(self.expr(ExprKind::Block(block), span))
            }
            Token::If => self.parse_if_expr(),
            Token::While => self.parse_while_expr(),
            Token::SelfKw => {
                let span = tok.span;
                self.advance();
                Ok(self.expr(ExprKind::SelfRef, span))
            }
            _ => Err(BengalError::ParseError {
                message: format!("unexpected token `{}`", tok.node),
                span: tok.span,
            }),
        }
    }

    fn parse_postfix_call(&mut self, callee: Expr) -> Result<Expr> {
        let start = callee.span.start;
        self.expect(Token::LParen)?;

        let name = match &callee.kind {
            ExprKind::Ident(name) => name.clone(),
            _ => {
                return Err(BengalError::ParseError {
                    message: "expected function or struct name".to_string(),
                    span: self.tokens[self.pos - 1].span,
                });
            }
        };

        // Empty args -> Call (semantic layer resolves if it's a struct init)
        if self.peek().node == Token::RParen {
            self.advance();
            let span = self.span_from(start);
            return Ok(self.expr(
                ExprKind::Call {
                    name,
                    type_args: vec![],
                    args: vec![],
                },
                span,
            ));
        }

        // Lookahead: IDENT followed by `:` -> named args -> StructInit
        let is_named = matches!(self.peek().node, Token::Ident(_))
            && self.tokens.get(self.pos + 1).map(|t| &t.node) == Some(&Token::Colon);

        if is_named {
            let mut args = Vec::new();
            let label = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let value = self.parse_expr()?;
            args.push((label, value));
            while self.peek().node == Token::Comma {
                self.advance();
                let label = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let value = self.parse_expr()?;
                args.push((label, value));
            }
            self.expect(Token::RParen)?;
            let span = self.span_from(start);
            Ok(self.expr(
                ExprKind::StructInit {
                    name,
                    type_args: vec![],
                    args,
                },
                span,
            ))
        } else {
            let mut args = Vec::new();
            args.push(self.parse_expr()?);
            while self.peek().node == Token::Comma {
                self.advance();
                args.push(self.parse_expr()?);
            }
            self.expect(Token::RParen)?;
            let span = self.span_from(start);
            Ok(self.expr(
                ExprKind::Call {
                    name,
                    type_args: vec![],
                    args,
                },
                span,
            ))
        }
    }

    fn parse_type_arg_list(&mut self) -> Result<Vec<TypeAnnotation>> {
        self.expect(Token::Lt)?;
        let mut args = vec![self.parse_type()?];
        while self.peek().node == Token::Comma {
            self.advance();
            args.push(self.parse_type()?);
        }
        self.expect(Token::Gt)?;
        Ok(args)
    }

    fn parse_postfix_call_with_type_args(
        &mut self,
        name: String,
        type_args: Vec<TypeAnnotation>,
        start: usize,
    ) -> Result<Expr> {
        self.expect(Token::LParen)?;

        // Empty args -> Call
        if self.peek().node == Token::RParen {
            self.advance();
            let span = self.span_from(start);
            return Ok(self.expr(
                ExprKind::Call {
                    name,
                    type_args,
                    args: vec![],
                },
                span,
            ));
        }

        // Lookahead: IDENT followed by `:` -> named args -> StructInit
        let is_named = matches!(self.peek().node, Token::Ident(_))
            && self.tokens.get(self.pos + 1).map(|t| &t.node) == Some(&Token::Colon);

        if is_named {
            let mut args = Vec::new();
            let label = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let value = self.parse_expr()?;
            args.push((label, value));
            while self.peek().node == Token::Comma {
                self.advance();
                let label = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let value = self.parse_expr()?;
                args.push((label, value));
            }
            self.expect(Token::RParen)?;
            let span = self.span_from(start);
            Ok(self.expr(
                ExprKind::StructInit {
                    name,
                    type_args,
                    args,
                },
                span,
            ))
        } else {
            let mut args = Vec::new();
            args.push(self.parse_expr()?);
            while self.peek().node == Token::Comma {
                self.advance();
                args.push(self.parse_expr()?);
            }
            self.expect(Token::RParen)?;
            let span = self.span_from(start);
            Ok(self.expr(
                ExprKind::Call {
                    name,
                    type_args,
                    args,
                },
                span,
            ))
        }
    }

    fn parse_if_expr(&mut self) -> Result<Expr> {
        let start = self.current_span_start();
        self.expect(Token::If)?;
        let condition = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let else_block = if self.peek().node == Token::Else {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };
        let span = self.span_from(start);
        Ok(self.expr(
            ExprKind::If {
                condition: Box::new(condition),
                then_block,
                else_block,
            },
            span,
        ))
    }

    fn parse_while_expr(&mut self) -> Result<Expr> {
        let start = self.current_span_start();
        self.expect(Token::While)?;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        let nobreak = if self.peek().node == Token::Nobreak {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };
        let span = self.span_from(start);
        Ok(self.expr(
            ExprKind::While {
                condition: Box::new(condition),
                body,
                nobreak,
            },
            span,
        ))
    }
}
