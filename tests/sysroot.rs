mod common;

use std::path::{Path, PathBuf};

/// Compile a library source into a .bengalmod file in the given directory.
fn compile_lib(name: &str, source: &str, dir: &Path) -> PathBuf {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(name, source).unwrap();
    let analyzed = bengal::pipeline::analyze(parsed, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    let optimized = bengal::pipeline::optimize(lowered);
    let emit_data = bengal::pipeline::EmitData::from_lowered(&optimized);
    bengal::pipeline::emit_package_bengalmod(&emit_data, dir);
    dir.join(format!("{}.bengalmod", name))
}

/// Get the LLVM target triple for constructing sysroot paths.
fn target_triple() -> String {
    inkwell::targets::TargetMachine::get_default_triple()
        .as_str()
        .to_string_lossy()
        .into_owned()
}

/// Create a sysroot directory structure and place a .bengalmod file in it.
fn create_test_sysroot(lib_name: &str, lib_source: &str) -> (tempfile::TempDir, PathBuf) {
    let sysroot_dir = tempfile::TempDir::new().unwrap();
    let triple = target_triple();
    let lib_dir = sysroot_dir
        .path()
        .join("lib")
        .join("bengallib")
        .join(&triple)
        .join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();
    let bengalmod_path = compile_lib(lib_name, lib_source, &lib_dir);
    (sysroot_dir, bengalmod_path)
}

/// Compile app source with a sysroot and search paths, link, run, return exit code.
fn compile_and_run_with_searcher(
    source: &str,
    explicit_deps: &[(&str, &Path)],
    sysroot: Option<PathBuf>,
    search_paths: Vec<bengal::sysroot::SearchPath>,
) -> i32 {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source("app", source).unwrap();

    let mut external_deps: Vec<bengal::pipeline::ExternalDep> = explicit_deps
        .iter()
        .map(|(name, path)| bengal::pipeline::load_external_dep(name, path).unwrap())
        .collect();
    let explicit_names: std::collections::HashSet<String> =
        explicit_deps.iter().map(|(n, _)| n.to_string()).collect();

    let searcher = bengal::sysroot::LibrarySearcher::new(sysroot, search_paths);
    let discovered =
        bengal::pipeline::pre_scan_imports(&parsed.graph, &explicit_names, &searcher).unwrap();
    external_deps.extend(discovered);

    let analyzed = bengal::pipeline::analyze_with_deps(parsed, &external_deps, &mut diag).unwrap();
    let lowered = bengal::pipeline::lower(analyzed, &mut diag).unwrap();
    let mut lowered = lowered;
    bengal::pipeline::merge_external_deps(&mut lowered, &external_deps);
    let optimized = bengal::pipeline::optimize(lowered);
    let mono = bengal::pipeline::monomorphize(optimized, &mut diag).unwrap();
    let compiled = bengal::pipeline::codegen(mono, &mut diag).unwrap();

    let ext_objects = bengal::pipeline::collect_external_objects(&external_deps);
    let link_dir = tempfile::TempDir::new().unwrap();
    let exe_path = link_dir.path().join("test_exe");
    bengal::pipeline::link(
        compiled,
        &ext_objects,
        &exe_path,
        searcher.native_search_paths(),
    )
    .unwrap();

    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run compiled binary");
    output.status.code().unwrap_or(-1)
}

#[test]
fn sysroot_auto_discovery() {
    let (sysroot_dir, _) = create_test_sysroot(
        "core",
        r#"
        public func core_version() -> Int32 { return 1; }
        func main() -> Int32 { return 0; }
        "#,
    );

    let result = compile_and_run_with_searcher(
        r#"
        import core::core_version;
        func main() -> Int32 {
            return core_version();
        }
        "#,
        &[],
        Some(sysroot_dir.path().to_path_buf()),
        vec![],
    );
    assert_eq!(result, 1);
}

#[test]
fn bengal_search_path_discovery() {
    let dir = tempfile::TempDir::new().unwrap();
    compile_lib(
        "math",
        r#"
        public func triple(x: Int32) -> Int32 { return x + x + x; }
        func main() -> Int32 { return 0; }
        "#,
        dir.path(),
    );

    let result = compile_and_run_with_searcher(
        r#"
        import math::triple;
        func main() -> Int32 {
            return triple(3);
        }
        "#,
        &[],
        None,
        vec![bengal::sysroot::SearchPath {
            kind: bengal::sysroot::SearchPathKind::Bengal,
            path: dir.path().to_path_buf(),
        }],
    );
    assert_eq!(result, 9);
}

#[test]
fn bengal_search_path_priority_over_sysroot() {
    // sysroot has prio returning 1
    let (sysroot_dir, _) = create_test_sysroot(
        "prio",
        r#"
        public func get_prio() -> Int32 { return 1; }
        func main() -> Int32 { return 0; }
        "#,
    );

    // search path dir has prio returning 2
    let search_dir = tempfile::TempDir::new().unwrap();
    compile_lib(
        "prio",
        r#"
        public func get_prio() -> Int32 { return 2; }
        func main() -> Int32 { return 0; }
        "#,
        search_dir.path(),
    );

    let result = compile_and_run_with_searcher(
        r#"
        import prio::get_prio;
        func main() -> Int32 {
            return get_prio();
        }
        "#,
        &[],
        Some(sysroot_dir.path().to_path_buf()),
        vec![bengal::sysroot::SearchPath {
            kind: bengal::sysroot::SearchPathKind::Bengal,
            path: search_dir.path().to_path_buf(),
        }],
    );
    // -L bengal= takes priority over sysroot, so should return 2
    assert_eq!(result, 2);
}

#[test]
fn explicit_dep_coexists_with_sysroot() {
    // core in sysroot returns 10
    let (sysroot_dir, _) = create_test_sysroot(
        "core",
        r#"
        public func core_val() -> Int32 { return 10; }
        func main() -> Int32 { return 0; }
        "#,
    );

    // extra as explicit dep returns 20
    let extra_dir = tempfile::TempDir::new().unwrap();
    let extra_path = compile_lib(
        "extra",
        r#"
        public func extra_val() -> Int32 { return 20; }
        func main() -> Int32 { return 0; }
        "#,
        extra_dir.path(),
    );

    let result = compile_and_run_with_searcher(
        r#"
        import core::core_val;
        import extra::extra_val;
        func main() -> Int32 {
            return core_val() + extra_val();
        }
        "#,
        &[("extra", &extra_path)],
        Some(sysroot_dir.path().to_path_buf()),
        vec![],
    );
    assert_eq!(result, 30);
}

#[test]
fn malformed_sysroot_falls_back_silently() {
    // Empty TempDir as sysroot — no lib/bengallib/<triple>/lib directory
    let empty_dir = tempfile::TempDir::new().unwrap();
    let searcher =
        bengal::sysroot::LibrarySearcher::new(Some(empty_dir.path().to_path_buf()), vec![]);
    assert!(
        searcher.find_bengalmod("Core").is_none(),
        "should return None when sysroot lib dir does not exist"
    );
}

#[test]
fn import_nonexistent_library_fails() {
    let mut diag = bengal::error::DiagCtxt::new();
    let parsed = bengal::pipeline::parse_source(
        "app",
        r#"
        import nonexistent::foo;
        func main() -> Int32 { return foo(); }
        "#,
    )
    .unwrap();

    let explicit_names = std::collections::HashSet::new();
    let searcher = bengal::sysroot::LibrarySearcher::new(None, vec![]);
    let discovered =
        bengal::pipeline::pre_scan_imports(&parsed.graph, &explicit_names, &searcher).unwrap();
    // No deps discovered
    assert!(discovered.is_empty());

    // Analysis should fail because nonexistent is not available
    let result = bengal::pipeline::analyze_with_deps(parsed, &discovered, &mut diag);
    assert!(
        result.is_err(),
        "should fail: import of nonexistent library with no searcher or deps"
    );
}

#[test]
fn multiple_bengal_search_paths_first_wins() {
    let dir1 = tempfile::TempDir::new().unwrap();
    compile_lib(
        "dup",
        r#"
        public func get_val() -> Int32 { return 1; }
        func main() -> Int32 { return 0; }
        "#,
        dir1.path(),
    );

    let dir2 = tempfile::TempDir::new().unwrap();
    compile_lib(
        "dup",
        r#"
        public func get_val() -> Int32 { return 2; }
        func main() -> Int32 { return 0; }
        "#,
        dir2.path(),
    );

    let result = compile_and_run_with_searcher(
        r#"
        import dup::get_val;
        func main() -> Int32 {
            return get_val();
        }
        "#,
        &[],
        None,
        vec![
            bengal::sysroot::SearchPath {
                kind: bengal::sysroot::SearchPathKind::Bengal,
                path: dir1.path().to_path_buf(),
            },
            bengal::sysroot::SearchPath {
                kind: bengal::sysroot::SearchPathKind::Bengal,
                path: dir2.path().to_path_buf(),
            },
        ],
    );
    // First search path wins
    assert_eq!(result, 1);
}
