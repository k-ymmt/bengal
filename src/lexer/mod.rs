pub mod token;

use logos::Logos;

use crate::error::{BengalError, Result, Span};
use token::{SpannedToken, Spanned, Token};

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
mod tests {
    use super::*;

    fn token_nodes(source: &str) -> Vec<Token> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|st| st.node)
            .collect()
    }

    #[test]
    fn single_number() {
        assert_eq!(token_nodes("42"), vec![Token::Number(42), Token::Eof]);
    }

    #[test]
    fn arithmetic_expression() {
        assert_eq!(
            token_nodes("2 + 3 * 4"),
            vec![
                Token::Number(2),
                Token::Plus,
                Token::Number(3),
                Token::Star,
                Token::Number(4),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn parenthesized_expression() {
        assert_eq!(
            token_nodes("(1 + 2) * 3"),
            vec![
                Token::LParen,
                Token::Number(1),
                Token::Plus,
                Token::Number(2),
                Token::RParen,
                Token::Star,
                Token::Number(3),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn func_declaration_tokens() {
        assert_eq!(
            token_nodes("func main() -> i32 { return 42; }"),
            vec![
                Token::Func,
                Token::Ident("main".to_string()),
                Token::LParen,
                Token::RParen,
                Token::Arrow,
                Token::Ident("i32".to_string()),
                Token::LBrace,
                Token::Return,
                Token::Number(42),
                Token::Semicolon,
                Token::RBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn let_binding_tokens() {
        assert_eq!(
            token_nodes("let x: i32 = 10;"),
            vec![
                Token::Let,
                Token::Ident("x".to_string()),
                Token::Colon,
                Token::Ident("i32".to_string()),
                Token::Eq,
                Token::Number(10),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn yield_expression_tokens() {
        assert_eq!(
            token_nodes("yield a + 1;"),
            vec![
                Token::Yield,
                Token::Ident("a".to_string()),
                Token::Plus,
                Token::Number(1),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lex_error_on_invalid_character() {
        let err = tokenize("2 @ 3").unwrap_err();
        match err {
            BengalError::LexError { span, .. } => {
                assert_eq!(span.start, 2);
                assert_eq!(span.end, 3);
            }
            _ => panic!("expected LexError"),
        }
    }
}
