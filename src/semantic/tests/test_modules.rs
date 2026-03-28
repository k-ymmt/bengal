use super::super::*;
use crate::package::build_module_graph;
use std::fs;
use tempfile::TempDir;

fn analyze_test_package(files: &[(&str, &str)]) -> Result<PackageSemanticInfo> {
    let dir = TempDir::new().unwrap();
    for (path, source) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full_path, source).unwrap();
    }
    let entry = dir.path().join(files[0].0);
    let graph = build_module_graph(&entry)?;
    let mut diag = DiagCtxt::new();
    let result = analyze_package(&graph, "test_pkg", &[], &mut diag);
    if result.is_err() {
        // Return the first real error from diag instead of the sentinel
        let errors = diag.take_errors();
        if let Some(first) = errors.into_iter().next() {
            return Err(first);
        }
    }
    result
}

#[test]
fn cross_module_function_import() {
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module math; import math::add; func main() -> Int32 { return add(1, 2); }",
        ),
        (
            "math.bengal",
            "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
        ),
    ]);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn visibility_violation_internal() {
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module math; import math::helper; func main() -> Int32 { return helper(); }",
        ),
        ("math.bengal", "func helper() -> Int32 { return 1; }"),
    ]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("cannot"),
        "expected 'cannot' in error: {}",
        msg
    );
}

#[test]
fn glob_import() {
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module math; import math::*; func main() -> Int32 { return add(1, 2); }",
        ),
        (
            "math.bengal",
            "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
        ),
    ]);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn cross_module_struct_import() {
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module shapes; import shapes::Point; func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x; }",
        ),
        (
            "shapes.bengal",
            "public struct Point { public var x: Int32; public var y: Int32; }",
        ),
    ]);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn glob_import_skips_internal() {
    // Internal symbols should NOT be imported by glob
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module math; import math::*; func main() -> Int32 { return secret(); }",
        ),
        (
            "math.bengal",
            "func secret() -> Int32 { return 42; } public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
        ),
    ]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("undefined function") || msg.contains("secret"),
        "expected undefined function error, got: {}",
        msg
    );
}

#[test]
fn group_import() {
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module math; import math::{add, sub}; func main() -> Int32 { return add(1, sub(3, 1)); }",
        ),
        (
            "math.bengal",
            "public func add(a: Int32, b: Int32) -> Int32 { return a + b; } public func sub(a: Int32, b: Int32) -> Int32 { return a - b; }",
        ),
    ]);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn unresolved_import_symbol() {
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module math; import math::nonexistent; func main() -> Int32 { return 0; }",
        ),
        (
            "math.bengal",
            "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
        ),
    ]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("nonexistent"),
        "expected error about 'nonexistent', got: {}",
        msg
    );
}

#[test]
fn package_visibility_accessible() {
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module math; import math::helper; func main() -> Int32 { return helper(); }",
        ),
        (
            "math.bengal",
            "package func helper() -> Int32 { return 42; }",
        ),
    ]);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn non_root_module_no_main_required() {
    // Child modules should not require a main function
    let result = analyze_test_package(&[
        (
            "main.bengal",
            "module math; import math::add; func main() -> Int32 { return add(1, 2); }",
        ),
        (
            "math.bengal",
            "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
        ),
    ]);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    // Verify the graph has 2 modules
    let info = result.unwrap();
    assert_eq!(info.module_infos.len(), 2);
}

#[test]
fn super_at_root_is_error() {
    let result = analyze_test_package(&[(
        "main.bengal",
        "import super::foo; func main() -> Int32 { return 0; }",
    )]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("super"),
        "expected error about 'super', got: {}",
        msg
    );
}

#[test]
fn unresolved_module_in_import() {
    let result = analyze_test_package(&[(
        "main.bengal",
        "import nonexistent::foo; func main() -> Int32 { return 0; }",
    )]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not found") || msg.contains("nonexistent"),
        "expected error about unresolved module, got: {}",
        msg
    );
}
