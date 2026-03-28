mod common;

use std::path::Path;

/// Compile a library source into a .bengalmod file, return path.
fn compile_lib(name: &str, source: &str, dir: &Path) -> std::path::PathBuf {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(name, source).unwrap();
    let analyzed = bengal::pipeline::analyze(parsed, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    bengal::pipeline::emit_package_bengalmod(&lowered, dir);
    dir.join(format!("{}.bengalmod", name))
}

/// Compile app with external deps, link, run, return exit code.
fn compile_and_run_with_deps(source: &str, deps: &[(&str, &Path)]) -> i32 {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source("app", source).unwrap();

    let external_deps: Vec<bengal::pipeline::ExternalDep> = deps
        .iter()
        .map(|(name, path)| bengal::pipeline::load_external_dep(name, path).unwrap())
        .collect();

    let analyzed = bengal::pipeline::analyze_with_deps(parsed, &external_deps, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    let mut lowered = lowered;
    bengal::pipeline::merge_external_deps(&mut lowered, &external_deps);
    let optimized = bengal::pipeline::optimize(lowered);
    let mono = bengal::pipeline::monomorphize(optimized, &mut diag).unwrap();
    let compiled = bengal::pipeline::codegen(mono, &mut diag).unwrap();

    let link_dir = tempfile::TempDir::new().unwrap();
    let exe_path = link_dir.path().join("test_exe");
    bengal::pipeline::link(compiled, &exe_path).unwrap();

    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run compiled binary");
    output.status.code().unwrap_or(-1)
}

#[test]
fn separate_compilation_basic_function_call() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "mathlib",
        r#"
        public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import math::add;
        func main() -> Int32 {
            return add(1, 2);
        }
        "#,
        &[("math", &lib_path)],
    );
    assert_eq!(result, 3);
}
