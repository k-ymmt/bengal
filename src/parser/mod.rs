pub mod ast;

use crate::error::{BengalError, Result};
use crate::lexer::token::{SpannedToken, Token};
use ast::{BinOp, Expr};

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
        match tok.node {
            Token::Number(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            _ => Err(BengalError::ParseError {
                message: format!("unexpected token `{}`", tok.node),
                span: tok.span,
            }),
        }
    }
}

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Expr> {
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr()?;
    let next = parser.peek();
    if next.node != Token::Eof {
        return Err(BengalError::ParseError {
            message: format!("unexpected token `{}`", next.node),
            span: next.span,
        });
    }
    Ok(expr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse_str(input: &str) -> Result<Expr> {
        let tokens = tokenize(input).unwrap();
        parse(tokens)
    }

    #[test]
    fn precedence_mul_over_add() {
        // 2 + 3 * 4 → Add(2, Mul(3, 4))
        let expr = parse_str("2 + 3 * 4").unwrap();
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
        // (2 + 3) * 4 → Mul(Add(2, 3), 4)
        let expr = parse_str("(2 + 3) * 4").unwrap();
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
        assert_eq!(parse_str("10").unwrap(), Expr::Number(10));
    }

    #[test]
    fn error_incomplete_expr() {
        // "1 + " → ParseError (unexpected EOF)
        assert!(matches!(
            parse_str("1 + "),
            Err(BengalError::ParseError { .. })
        ));
    }

    #[test]
    fn error_unconsumed_token() {
        // "1 2" → ParseError (unconsumed token)
        assert!(matches!(
            parse_str("1 2"),
            Err(BengalError::ParseError { .. })
        ));
    }

    #[test]
    fn error_unconsumed_rparen() {
        // "2 + 3)" → ParseError (unconsumed closing paren)
        assert!(matches!(
            parse_str("2 + 3)"),
            Err(BengalError::ParseError { .. })
        ));
    }
}
