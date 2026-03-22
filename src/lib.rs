pub mod bir;
pub mod codegen;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod semantic;

use error::Result;

pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let sem_info = semantic::analyze(&program)?;
    let mut bir = bir::lower_program(&program, &sem_info)?;
    bir::optimize_module(&mut bir);
    let wasm = codegen::compile(&bir)?;
    Ok(wasm)
}

pub fn compile_to_bir(source: &str) -> Result<(bir::instruction::BirModule, String)> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let sem_info = semantic::analyze(&program)?;
    let bir_module = bir::lower_program(&program, &sem_info)?;
    let bir_text = bir::print_module(&bir_module);
    Ok((bir_module, bir_text))
}
