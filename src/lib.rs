pub mod bir;
pub mod codegen;
pub mod error;
pub mod interface;
pub mod lexer;
pub mod mangle;
pub mod package;
pub mod parser;
pub mod pipeline;
pub mod pipeline_helpers;
pub mod semantic;
pub mod suggest;
pub mod sysroot;

use std::collections::HashMap;
use std::path::Path;

use error::{DiagCtxt, Result};
use pipeline::BirOutput;

/// Compile a Bengal source file (or package) to an executable.
pub fn compile_to_executable(
    entry_path: &Path,
    output_path: &Path,
    external_deps: &[pipeline::ExternalDep],
) -> std::result::Result<(), error::PipelineError> {
    let mut diag = DiagCtxt::new();
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze_with_deps(parsed, external_deps, &mut diag)?;
    let lowered = pipeline::lower(analyzed, &mut diag)?;
    pipeline::emit_interfaces(&lowered, std::path::Path::new(".build/cache"));
    let mut lowered = lowered;
    pipeline::merge_external_deps(&mut lowered, external_deps);
    let optimized = pipeline::optimize(lowered);
    let emit_data = pipeline::EmitData::from_lowered(&optimized);
    let mono = pipeline::monomorphize(optimized, &mut diag)?;
    let compiled = pipeline::codegen(mono, &mut diag)?;
    pipeline::emit_package_bengalmod(&emit_data, std::path::Path::new(".build/cache"));
    let ext_objects = pipeline::collect_external_objects(external_deps);
    pipeline::link(compiled, &ext_objects, output_path, &[])
}

/// Compile a Bengal source file (or package) to BIR output.
pub fn compile_to_bir(entry_path: &Path) -> std::result::Result<BirOutput, error::PipelineError> {
    let mut diag = DiagCtxt::new();
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze(parsed, &mut diag)?;
    let lowered = pipeline::lower(analyzed, &mut diag)?;
    let optimized = pipeline::optimize(lowered);
    let mut bir_texts = HashMap::new();
    let mut modules = HashMap::new();
    for (path, module) in optimized.modules {
        bir_texts.insert(path.clone(), bir::print_module(&module.bir));
        modules.insert(path, module);
    }
    Ok(BirOutput { modules, bir_texts })
}

/// Compile BIR from an in-memory source string (for eval subcommand).
pub fn compile_source_to_bir(source: &str) -> std::result::Result<BirOutput, error::PipelineError> {
    let mut diag = DiagCtxt::new();
    let parsed = pipeline::parse_source("<eval>", source)?;
    let analyzed = pipeline::analyze(parsed, &mut diag)?;
    let lowered = pipeline::lower(analyzed, &mut diag)?;
    let optimized = pipeline::optimize(lowered);
    let mut bir_texts = HashMap::new();
    let mut modules = HashMap::new();
    for (path, module) in optimized.modules {
        bir_texts.insert(path.clone(), bir::print_module(&module.bir));
        modules.insert(path, module);
    }
    Ok(BirOutput { modules, bir_texts })
}

/// Compile a file/package to object bytes (no linking).
pub fn compile_to_objects(
    entry_path: &Path,
) -> std::result::Result<pipeline::CompiledPackage, error::PipelineError> {
    let mut diag = DiagCtxt::new();
    let parsed = pipeline::parse(entry_path)?;
    let analyzed = pipeline::analyze(parsed, &mut diag)?;
    let lowered = pipeline::lower(analyzed, &mut diag)?;
    let optimized = pipeline::optimize(lowered);
    let mono = pipeline::monomorphize(optimized, &mut diag)?;
    pipeline::codegen(mono, &mut diag)
}

/// Compile from a source string to object bytes (for integration tests).
pub fn compile_source_to_objects(source: &str) -> Result<Vec<u8>> {
    let mut diag = DiagCtxt::new();
    let parsed = pipeline::parse_source("test", source).map_err(|e| e.source_error)?;
    let analyzed = pipeline::analyze(parsed, &mut diag).map_err(|e| e.source_error)?;
    let lowered = pipeline::lower(analyzed, &mut diag).map_err(|e| e.source_error)?;
    let optimized = pipeline::optimize(lowered);
    let mono = pipeline::monomorphize(optimized, &mut diag).map_err(|e| e.source_error)?;
    let compiled = pipeline::codegen(mono, &mut diag).map_err(|e| e.source_error)?;
    compiled
        .object_bytes
        .into_values()
        .next()
        .ok_or_else(|| error::BengalError::CodegenError {
            message: "no object code produced".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_source_returns_object() {
        let obj = compile_source_to_objects("func main() -> Int32 { return 1; }").unwrap();
        assert!(
            !obj.is_empty(),
            "compile_source_to_objects must return non-empty object bytes"
        );
    }

    #[test]
    fn test_bir_generic_function_has_type_params() {
        let source = r#"
            func identity<T>(x: T) -> T { return x; }
            func main() -> Int32 { return identity<Int32>(42); }
        "#;
        let output = compile_source_to_bir(source).unwrap();
        let root_text = output.bir_texts.get(&package::ModulePath::root()).unwrap();
        assert!(
            root_text.contains("identity"),
            "BIR must contain the generic function 'identity'"
        );
        assert!(
            root_text.contains("T"),
            "BIR must contain TypeParam 'T' for the generic function"
        );
    }

    #[test]
    fn test_compile_to_module_reexport() {
        let source = "func main() -> Int32 { return 1; }";
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(tokens).unwrap();
        let (_inferred, sem_info) = semantic::analyze_pre_mono(&program).unwrap();
        let mut bir_module = bir::lower_program(&program, &sem_info).unwrap();
        bir::optimize_module(&mut bir_module);

        let context = inkwell::context::Context::create();
        let _module = codegen::compile_to_module(&context, &bir_module).unwrap();
    }
}
