use super::*;
use crate::lexer::tokenize;
use crate::parser::parse;

mod test_expressions;
mod test_modules;
mod test_structs;

fn analyze_str(input: &str) -> Result<SemanticInfo> {
    let tokens = tokenize(input).unwrap();
    let program = parse(tokens).unwrap();
    analyze_post_mono(&program)
}
