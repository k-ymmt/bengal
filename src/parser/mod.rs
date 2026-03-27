pub mod ast;
mod parse_definition;
mod parse_expr;
mod parse_primary;
mod parse_stmt;

#[cfg(test)]
mod tests;

use crate::error::{BengalError, Result, Span};
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

    fn expr(&mut self, kind: ExprKind, span: Span) -> Expr {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        Expr { id, kind, span }
    }

    fn current_span_start(&self) -> usize {
        self.tokens[self.pos].span.start
    }

    fn prev_span_end(&self) -> usize {
        if self.pos == 0 {
            0
        } else {
            self.tokens[self.pos - 1].span.end
        }
    }

    fn span_from(&self, start: usize) -> Span {
        Span {
            start,
            end: self.prev_span_end(),
        }
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

    fn no_space_before_current(&self) -> bool {
        if self.pos == 0 {
            return false;
        }
        let prev = &self.tokens[self.pos - 1];
        let curr = &self.tokens[self.pos];
        prev.span.end == curr.span.start
    }

    // --- Core type parsing ---

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
        if self.peek().node == Token::LBracket {
            self.advance(); // consume [
            let element = self.parse_type()?;
            self.expect(Token::Semicolon)?;
            let size_tok = self.expect(Token::Number(0))?;
            let size = match &size_tok.node {
                Token::Number(n) => *n as u64,
                _ => unreachable!(),
            };
            self.expect(Token::RBracket)?;
            return Ok(TypeAnnotation::Array {
                element: Box::new(element),
                size,
            });
        }
        if self.peek().node == Token::LParen {
            self.advance();
            self.expect(Token::RParen)?;
            return Ok(TypeAnnotation::Unit);
        }
        let tok = self.expect(Token::Ident(String::new()))?;
        let base = match &tok.node {
            Token::Ident(s) if s == "Int32" => return Ok(TypeAnnotation::I32),
            Token::Ident(s) if s == "Int64" => return Ok(TypeAnnotation::I64),
            Token::Ident(s) if s == "Float32" => return Ok(TypeAnnotation::F32),
            Token::Ident(s) if s == "Float64" => return Ok(TypeAnnotation::F64),
            Token::Ident(s) if s == "Bool" => return Ok(TypeAnnotation::Bool),
            Token::Ident(s) if s == "Void" => return Ok(TypeAnnotation::Unit),
            Token::Ident(s) => s.clone(),
            _ => unreachable!(),
        };
        if self.peek().node == Token::Lt && self.no_space_before_current() {
            self.advance(); // consume `<`
            let mut args = vec![self.parse_type()?];
            while self.peek().node == Token::Comma {
                self.advance();
                args.push(self.parse_type()?);
            }
            self.expect(Token::Gt)?;
            Ok(TypeAnnotation::Generic { name: base, args })
        } else {
            Ok(TypeAnnotation::Named(base))
        }
    }
}

pub fn parse(tokens: Vec<SpannedToken>) -> Result<Program> {
    let mut parser = Parser::new(tokens);

    // Phase 1 compatibility: if the first token is not a declaration keyword, treat as a bare expression
    if matches!(
        parser.peek().node,
        Token::Func
            | Token::Struct
            | Token::Protocol
            | Token::Module
            | Token::Import
            | Token::Public
            | Token::Package
            | Token::Internal
            | Token::Fileprivate
            | Token::Private
    ) {
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
        let expr_span = expr.span;
        Ok(Program {
            module_decls: vec![],
            import_decls: vec![],
            structs: vec![],
            protocols: vec![],
            functions: vec![Function {
                visibility: Visibility::Internal,
                name: "main".to_string(),
                type_params: vec![],
                params: vec![],
                return_type: TypeAnnotation::I32,
                body: Some(Block {
                    stmts: vec![Stmt::Return(Some(expr))],
                }),
                span: expr_span,
            }],
        })
    }
}
