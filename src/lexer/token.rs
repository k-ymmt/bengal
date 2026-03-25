use std::fmt;

use logos::Logos;

use crate::error::Span;

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(skip r"[ \t\n\r]+")]
pub enum Token {
    #[regex("[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Number(i64),

    #[regex("[0-9]+\\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,

    // Phase 2: keywords
    #[token("func")]
    Func,
    #[token("let")]
    Let,
    #[token("var")]
    Var,
    #[token("return")]
    Return,
    #[token("yield")]
    Yield,

    // Phase 3: keywords
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,

    // Phase 4: keywords
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("as")]
    As,
    #[token("nobreak")]
    Nobreak,

    // Struct: keywords
    #[token("struct")]
    Struct,
    #[token("init")]
    Init,
    #[token("self")]
    SelfKw,

    // Protocol: keywords
    #[token("protocol")]
    Protocol,

    // Phase 2: symbols
    #[token("->")]
    Arrow,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token(",")]
    Comma,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("=")]
    Eq,

    // Phase 3: comparison operators
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,

    // Struct: symbols
    #[token(".")]
    Dot,

    // Phase 3: logical operators
    #[token("&&")]
    AmpAmp,
    #[token("||")]
    PipePipe,
    #[token("!")]
    Bang,

    // Phase 2: identifiers
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    Eof,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Number(n) => write!(f, "{}", n),
            Token::Float(n) => write!(f, "{}", n),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Func => write!(f, "func"),
            Token::Let => write!(f, "let"),
            Token::Var => write!(f, "var"),
            Token::Return => write!(f, "return"),
            Token::Yield => write!(f, "yield"),
            Token::Arrow => write!(f, "->"),
            Token::Colon => write!(f, ":"),
            Token::Semicolon => write!(f, ";"),
            Token::Comma => write!(f, ","),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::Eq => write!(f, "="),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::If => write!(f, "if"),
            Token::Else => write!(f, "else"),
            Token::While => write!(f, "while"),
            Token::Break => write!(f, "break"),
            Token::Continue => write!(f, "continue"),
            Token::As => write!(f, "as"),
            Token::Nobreak => write!(f, "nobreak"),
            Token::Struct => write!(f, "struct"),
            Token::Init => write!(f, "init"),
            Token::SelfKw => write!(f, "self"),
            Token::Protocol => write!(f, "protocol"),
            Token::Dot => write!(f, "."),
            Token::EqEq => write!(f, "=="),
            Token::NotEq => write!(f, "!="),
            Token::Lt => write!(f, "<"),
            Token::Gt => write!(f, ">"),
            Token::LtEq => write!(f, "<="),
            Token::GtEq => write!(f, ">="),
            Token::AmpAmp => write!(f, "&&"),
            Token::PipePipe => write!(f, "||"),
            Token::Bang => write!(f, "!"),
            Token::Ident(s) => write!(f, "{}", s),
            Token::Eof => write!(f, "EOF"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

pub type SpannedToken = Spanned<Token>;
