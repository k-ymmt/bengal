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
        token_nodes("func main() -> Int32 { return 42; }"),
        vec![
            Token::Func,
            Token::Ident("main".to_string()),
            Token::LParen,
            Token::RParen,
            Token::Arrow,
            Token::Ident("Int32".to_string()),
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
        token_nodes("let x: Int32 = 10;"),
        vec![
            Token::Let,
            Token::Ident("x".to_string()),
            Token::Colon,
            Token::Ident("Int32".to_string()),
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
fn bool_literals() {
    assert_eq!(token_nodes("true"), vec![Token::True, Token::Eof]);
    assert_eq!(token_nodes("false"), vec![Token::False, Token::Eof]);
}

#[test]
fn if_else_tokens() {
    assert_eq!(
        token_nodes("if x == 0 { yield 1; } else { yield 2; }"),
        vec![
            Token::If,
            Token::Ident("x".to_string()),
            Token::EqEq,
            Token::Number(0),
            Token::LBrace,
            Token::Yield,
            Token::Number(1),
            Token::Semicolon,
            Token::RBrace,
            Token::Else,
            Token::LBrace,
            Token::Yield,
            Token::Number(2),
            Token::Semicolon,
            Token::RBrace,
            Token::Eof,
        ]
    );
}

#[test]
fn while_tokens() {
    assert_eq!(
        token_nodes("while i < n { }"),
        vec![
            Token::While,
            Token::Ident("i".to_string()),
            Token::Lt,
            Token::Ident("n".to_string()),
            Token::LBrace,
            Token::RBrace,
            Token::Eof,
        ]
    );
}

#[test]
fn logical_operator_tokens() {
    assert_eq!(
        token_nodes("a && b || !c"),
        vec![
            Token::Ident("a".to_string()),
            Token::AmpAmp,
            Token::Ident("b".to_string()),
            Token::PipePipe,
            Token::Bang,
            Token::Ident("c".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
fn comparison_operator_tokens() {
    assert_eq!(
        token_nodes("a <= b"),
        vec![
            Token::Ident("a".to_string()),
            Token::LtEq,
            Token::Ident("b".to_string()),
            Token::Eof,
        ]
    );
    assert_eq!(
        token_nodes("a >= b"),
        vec![
            Token::Ident("a".to_string()),
            Token::GtEq,
            Token::Ident("b".to_string()),
            Token::Eof,
        ]
    );
    assert_eq!(
        token_nodes("a != b"),
        vec![
            Token::Ident("a".to_string()),
            Token::NotEq,
            Token::Ident("b".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
#[allow(clippy::approx_constant)]
fn float_literal() {
    assert_eq!(token_nodes("3.14"), vec![Token::Float(3.14), Token::Eof]);
    assert_eq!(token_nodes("42.0"), vec![Token::Float(42.0), Token::Eof]);
}

#[test]
fn break_continue_keywords() {
    assert_eq!(token_nodes("break"), vec![Token::Break, Token::Eof]);
    assert_eq!(token_nodes("continue"), vec![Token::Continue, Token::Eof]);
}

#[test]
fn as_keyword() {
    assert_eq!(
        token_nodes("42 as Int64"),
        vec![
            Token::Number(42),
            Token::As,
            Token::Ident("Int64".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
fn nobreak_keyword() {
    assert_eq!(token_nodes("nobreak"), vec![Token::Nobreak, Token::Eof]);
}

#[test]
fn while_break_tokens() {
    assert_eq!(
        token_nodes("while true { break; }"),
        vec![
            Token::While,
            Token::True,
            Token::LBrace,
            Token::Break,
            Token::Semicolon,
            Token::RBrace,
            Token::Eof,
        ]
    );
}

#[test]
fn struct_keywords() {
    assert_eq!(token_nodes("struct"), vec![Token::Struct, Token::Eof]);
    assert_eq!(token_nodes("init"), vec![Token::Init, Token::Eof]);
    assert_eq!(token_nodes("self"), vec![Token::SelfKw, Token::Eof]);
}

#[test]
fn get_set_are_identifiers() {
    // get / set はコンテキストキーワード。レキサーでは Ident として扱う
    assert_eq!(
        token_nodes("get"),
        vec![Token::Ident("get".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("set"),
        vec![Token::Ident("set".to_string()), Token::Eof]
    );
}

#[test]
fn dot_token() {
    assert_eq!(
        token_nodes("f.x"),
        vec![
            Token::Ident("f".to_string()),
            Token::Dot,
            Token::Ident("x".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
#[allow(clippy::approx_constant)]
fn dot_does_not_conflict_with_float() {
    // 3.14 は Float、f.x は Ident.Ident
    assert_eq!(token_nodes("3.14"), vec![Token::Float(3.14), Token::Eof]);
    assert_eq!(
        token_nodes("a.b"),
        vec![
            Token::Ident("a".to_string()),
            Token::Dot,
            Token::Ident("b".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
fn struct_keyword_prefix() {
    // "structure" は Ident であり Struct + "ure" にならない
    assert_eq!(
        token_nodes("structure"),
        vec![Token::Ident("structure".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("initial"),
        vec![Token::Ident("initial".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("selfie"),
        vec![Token::Ident("selfie".to_string()), Token::Eof]
    );
}

#[test]
fn struct_definition_tokens() {
    assert_eq!(
        token_nodes("struct Foo { var x: Int32; }"),
        vec![
            Token::Struct,
            Token::Ident("Foo".to_string()),
            Token::LBrace,
            Token::Var,
            Token::Ident("x".to_string()),
            Token::Colon,
            Token::Ident("Int32".to_string()),
            Token::Semicolon,
            Token::RBrace,
            Token::Eof,
        ]
    );
}

#[test]
fn self_dot_field_tokens() {
    assert_eq!(
        token_nodes("self.foo = newValue;"),
        vec![
            Token::SelfKw,
            Token::Dot,
            Token::Ident("foo".to_string()),
            Token::Eq,
            Token::Ident("newValue".to_string()),
            Token::Semicolon,
            Token::Eof,
        ]
    );
}

#[test]
fn init_definition_tokens() {
    assert_eq!(
        token_nodes("init(x: Int32) { self.x = x; }"),
        vec![
            Token::Init,
            Token::LParen,
            Token::Ident("x".to_string()),
            Token::Colon,
            Token::Ident("Int32".to_string()),
            Token::RParen,
            Token::LBrace,
            Token::SelfKw,
            Token::Dot,
            Token::Ident("x".to_string()),
            Token::Eq,
            Token::Ident("x".to_string()),
            Token::Semicolon,
            Token::RBrace,
            Token::Eof,
        ]
    );
}

#[test]
fn computed_property_tokens() {
    assert_eq!(
        token_nodes("var bar: Int32 { get { return 0; } set { self.foo = newValue; } };"),
        vec![
            Token::Var,
            Token::Ident("bar".to_string()),
            Token::Colon,
            Token::Ident("Int32".to_string()),
            Token::LBrace,
            Token::Ident("get".to_string()),
            Token::LBrace,
            Token::Return,
            Token::Number(0),
            Token::Semicolon,
            Token::RBrace,
            Token::Ident("set".to_string()),
            Token::LBrace,
            Token::SelfKw,
            Token::Dot,
            Token::Ident("foo".to_string()),
            Token::Eq,
            Token::Ident("newValue".to_string()),
            Token::Semicolon,
            Token::RBrace,
            Token::RBrace,
            Token::Semicolon,
            Token::Eof,
        ]
    );
}

#[test]
fn module_keyword() {
    assert_eq!(token_nodes("module"), vec![Token::Module, Token::Eof]);
}

#[test]
fn import_keyword() {
    assert_eq!(token_nodes("import"), vec![Token::Import, Token::Eof]);
}

#[test]
fn visibility_keywords() {
    assert_eq!(token_nodes("public"), vec![Token::Public, Token::Eof]);
    assert_eq!(token_nodes("package"), vec![Token::Package, Token::Eof]);
    assert_eq!(token_nodes("internal"), vec![Token::Internal, Token::Eof]);
    assert_eq!(
        token_nodes("fileprivate"),
        vec![Token::Fileprivate, Token::Eof]
    );
    assert_eq!(token_nodes("private"), vec![Token::Private, Token::Eof]);
}

#[test]
fn super_keyword() {
    assert_eq!(token_nodes("super"), vec![Token::Super, Token::Eof]);
}

#[test]
fn colon_colon_token() {
    assert_eq!(
        token_nodes("foo::bar"),
        vec![
            Token::Ident("foo".to_string()),
            Token::ColonColon,
            Token::Ident("bar".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
fn colon_colon_does_not_break_existing_colon() {
    assert_eq!(
        token_nodes("x: Int32"),
        vec![
            Token::Ident("x".to_string()),
            Token::Colon,
            Token::Ident("Int32".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
fn keyword_prefix_not_captured() {
    assert_eq!(
        token_nodes("modules"),
        vec![Token::Ident("modules".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("imported"),
        vec![Token::Ident("imported".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("publicly"),
        vec![Token::Ident("publicly".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("superb"),
        vec![Token::Ident("superb".to_string()), Token::Eof]
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
