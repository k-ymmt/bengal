mod common;

use std::path::Path;

/// Compile a library source into a .bengalmod file, return path.
fn compile_lib(name: &str, source: &str, dir: &Path) -> std::path::PathBuf {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(name, source).unwrap();
    let analyzed = bengal::pipeline::analyze(parsed, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    let optimized = bengal::pipeline::optimize(lowered);
    let emit_data = bengal::pipeline::EmitData::from_lowered(&optimized);
    let mono = bengal::pipeline::monomorphize(optimized, &mut diag).unwrap();
    let compiled = bengal::pipeline::codegen(mono, &mut diag).unwrap();
    bengal::pipeline::emit_package_bengalmod(&emit_data, &compiled, dir);
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

#[test]
fn separate_compilation_struct_usage() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "geomlib",
        r#"
        public struct Point {
            var x: Int32;
            var y: Int32;
            func sum() -> Int32 {
                return self.x + self.y;
            }
        }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import geom::Point;
        func main() -> Int32 {
            let p = Point(x: 10, y: 20);
            return p.sum();
        }
        "#,
        &[("geom", &lib_path)],
    );
    assert_eq!(result, 30);
}

#[test]
fn separate_compilation_generic_function() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "utillib",
        r#"
        public func identity<T>(x: T) -> T { return x; }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import util::identity;
        func main() -> Int32 {
            return identity<Int32>(42);
        }
        "#,
        &[("util", &lib_path)],
    );
    assert_eq!(result, 42);
}

#[test]
fn separate_compilation_protocol() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "protolib",
        r#"
        public protocol Summable {
            func sum() -> Int32;
        }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import proto::Summable;
        public struct Pair: Summable {
            var a: Int32;
            var b: Int32;
            func sum() -> Int32 {
                return self.a + self.b;
            }
        }
        func main() -> Int32 {
            let p = Pair(a: 7, b: 8);
            return p.sum();
        }
        "#,
        &[("proto", &lib_path)],
    );
    assert_eq!(result, 15);
}

#[test]
fn separate_compilation_multiple_deps() {
    let dir = tempfile::TempDir::new().unwrap();

    let math_path = compile_lib(
        "mathlib",
        r#"
        public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );
    let str_path = compile_lib(
        "strlib",
        r#"
        public func double(x: Int32) -> Int32 { return x + x; }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_deps(
        r#"
        import math::add;
        import str::double;
        func main() -> Int32 {
            return add(double(5), 3);
        }
        "#,
        &[("math", &math_path), ("str", &str_path)],
    );
    assert_eq!(result, 13);
}

#[test]
fn separate_compilation_visibility_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let lib_path = compile_lib(
        "privlib",
        r#"
        public func pub_fn() -> Int32 { return 1; }
        func internal_fn() -> Int32 { return 2; }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(
        "app",
        r#"
        import priv::internal_fn;
        func main() -> Int32 { return internal_fn(); }
        "#,
    )
    .unwrap();

    let dep = bengal::pipeline::load_external_dep("priv", &lib_path).unwrap();
    let result = bengal::pipeline::analyze_with_deps(parsed, &[dep], &mut diag);
    assert!(
        result.is_err(),
        "should fail: non-public function not accessible"
    );
}

#[test]
fn separate_compilation_missing_file_error() {
    let result = bengal::pipeline::load_external_dep(
        "nonexistent",
        std::path::Path::new("/tmp/nonexistent.bengalmod"),
    );
    assert!(result.is_err(), "should fail: file not found");
}
