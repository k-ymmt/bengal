pub mod bir;
pub mod codegen;
pub mod error;
pub mod lexer;
pub mod parser;

use error::Result;

pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(tokens)?;
    let bir = bir::lower(&ast)?;
    let wasm = codegen::compile(&bir)?;
    Ok(wasm)
}

pub fn compile_to_bir(source: &str) -> Result<(bir::instruction::BirModule, String)> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(tokens)?;
    let bir_module = bir::lower(&ast)?;
    let bir_text = bir::print_module(&bir_module);
    Ok((bir_module, bir_text))
}
