pub mod token;

use logos::Logos;

use crate::error::{BengalError, Result, Span};
use token::{Spanned, SpannedToken, Token};

pub fn tokenize(source: &str) -> Result<Vec<SpannedToken>> {
    let mut tokens = Vec::new();
    let lexer = Token::lexer(source);

    for (result, range) in lexer.spanned() {
        let span = Span {
            start: range.start,
            end: range.end,
        };
        match result {
            Ok(token) => tokens.push(Spanned { node: token, span }),
            Err(()) => {
                return Err(BengalError::LexError {
                    message: format!("unexpected character: `{}`", &source[range]),
                    span,
                });
            }
        }
    }

    tokens.push(Spanned {
        node: Token::Eof,
        span: Span {
            start: source.len(),
            end: source.len(),
        },
    });

    Ok(tokens)
}

#[cfg(test)]
mod tests;
