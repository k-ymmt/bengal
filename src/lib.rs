pub mod bir;
pub mod codegen;
pub mod error;
pub mod lexer;
pub mod mangle;
pub mod package;
pub mod parser;
pub mod semantic;

use error::Result;

pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let sem_info = semantic::analyze(&program)?;
    let mut bir = bir::lower_program(&program, &sem_info)?;
    bir::optimize_module(&mut bir);
    let obj_bytes = codegen::compile(&bir)?;
    Ok(obj_bytes)
}

pub fn compile_to_bir(source: &str) -> Result<(bir::instruction::BirModule, String)> {
    let tokens = lexer::tokenize(source)?;
    let program = parser::parse(tokens)?;
    let sem_info = semantic::analyze(&program)?;
    let bir_module = bir::lower_program(&program, &sem_info)?;
    let bir_text = bir::print_module(&bir_module);
    Ok((bir_module, bir_text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_source_returns_object() {
        let obj = compile_source("func main() -> Int32 { return 1; }").unwrap();
        assert!(
            !obj.is_empty(),
            "compile_source must return non-empty object bytes"
        );
    }

    #[test]
    fn test_compile_to_module_reexport() {
        let source = "func main() -> Int32 { return 1; }";
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
        bir::optimize_module(&mut bir_module);

        let context = inkwell::context::Context::create();
        let _module = codegen::compile_to_module(&context, &bir_module).unwrap();
    }
}
