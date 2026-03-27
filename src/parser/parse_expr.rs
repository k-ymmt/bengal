use crate::error::{Result, Span};
use crate::lexer::token::Token;
use crate::parser::ast::*;

impl super::Parser {
    // --- Expr (7-level precedence chain) ---

    // Level 1 (lowest): ||
    pub(super) fn parse_expr(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        loop {
            if self.peek().node != Token::PipePipe {
                break;
            }
            self.advance();
            let right = self.parse_and()?;
            let span = Span {
                start: left.span.start,
                end: right.span.end,
            };
            left = self.expr(
                ExprKind::BinaryOp {
                    op: BinOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
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
            let span = Span {
                start: left.span.start,
                end: right.span.end,
            };
            left = self.expr(
                ExprKind::BinaryOp {
                    op: BinOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
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
            let span = Span {
                start: left.span.start,
                end: right.span.end,
            };
            left = self.expr(
                ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
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
            let span = Span {
                start: left.span.start,
                end: right.span.end,
            };
            left = self.expr(
                ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
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
            let span = Span {
                start: left.span.start,
                end: right.span.end,
            };
            left = self.expr(
                ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
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
            let span = Span {
                start: left.span.start,
                end: right.span.end,
            };
            left = self.expr(
                ExprKind::BinaryOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        Ok(left)
    }

    // Level 7: as (postfix)
    fn parse_cast(&mut self) -> Result<Expr> {
        let mut expr = self.parse_unary()?;
        while self.peek().node == Token::As {
            let start = expr.span.start;
            self.advance();
            let target_type = self.parse_type()?;
            let span = self.span_from(start);
            expr = self.expr(
                ExprKind::Cast {
                    expr: Box::new(expr),
                    target_type,
                },
                span,
            );
        }
        Ok(expr)
    }

    // Level 8 (highest): ! (prefix)
    pub(super) fn parse_unary(&mut self) -> Result<Expr> {
        if self.peek().node == Token::Bang {
            let start = self.current_span_start();
            self.advance();
            let operand = self.parse_unary()?;
            let span = self.span_from(start);
            let e = self.expr(
                ExprKind::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                },
                span,
            );
            return Ok(e);
        }
        self.parse_factor()
    }
}
