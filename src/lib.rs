pub mod bir;
pub mod codegen;
pub mod error;
pub mod lexer;
pub mod parser;

use error::{BengalError, Result};
use parser::ast::{Program, Stmt};

pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let expr = extract_main_return_expr(&program)?;
    let bir = bir::lower(expr)?;
    let wasm = codegen::compile(&bir)?;
    Ok(wasm)
}

pub fn compile_to_bir(source: &str) -> Result<(bir::instruction::BirModule, String)> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let expr = extract_main_return_expr(&program)?;
    let bir_module = bir::lower(expr)?;
    let bir_text = bir::print_module(&bir_module);
    Ok((bir_module, bir_text))
}

/// Temporary: extract the return expression from main function.
/// This will be removed when BIR lowering is updated to handle Program directly (Step 7).
fn extract_main_return_expr(program: &Program) -> Result<&parser::ast::Expr> {
    let main_fn = program
        .functions
        .iter()
        .find(|f| f.name == "main")
        .ok_or_else(|| BengalError::LoweringError {
            message: "no `main` function found".to_string(),
        })?;
    match main_fn.body.stmts.last() {
        Some(Stmt::Return(expr)) => Ok(expr),
        _ => Err(BengalError::LoweringError {
            message: "main function must end with a return statement".to_string(),
        }),
    }
}
