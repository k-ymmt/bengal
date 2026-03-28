use bengal::lexer::tokenize;
use bengal::parser::parse;
use bengal::semantic;
use std::sync::atomic::{AtomicU64, Ordering};

pub static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Compile and run a single-file Bengal program, returning the exit code.
pub fn compile_and_run(source: &str) -> i32 {
    compile_to_native_and_run(source)
}

/// Compile to native object, link, run, and return the exit code.
pub fn compile_to_native_and_run(source: &str) -> i32 {
    let obj_bytes = bengal::compile_source_to_objects(source).unwrap();

    let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bengal_test_{}_{}", std::process::id(), id));
    std::fs::create_dir_all(&dir).unwrap();
    let obj_path = dir.join("test.o");
    let exe_path = dir.join("test");
    std::fs::write(&obj_path, &obj_bytes).unwrap();

    let link = std::process::Command::new("cc")
        .arg(&obj_path)
        .arg("-o")
        .arg(&exe_path)
        .output()
        .expect("cc not found - C compiler/linker required for native tests");
    assert!(
        link.status.success(),
        "link failed: {}",
        String::from_utf8_lossy(&link.stderr)
    );

    let run = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to execute compiled binary");

    let _ = std::fs::remove_dir_all(&dir);

    match run.status.code() {
        Some(code) => code,
        None => panic!(
            "process terminated by signal, stderr: {}",
            String::from_utf8_lossy(&run.stderr)
        ),
    }
}

/// Run semantic analysis and return the error string.
/// Use for tests that specifically target semantic errors.
pub fn compile_should_fail(source: &str) -> String {
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    if let Err(e) = semantic::validate_generics(&program) {
        return e.to_string();
    }
    match semantic::analyze_pre_mono(&program) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected semantic error but analysis succeeded"),
    }
}

/// Run the full compilation pipeline and return the error string.
/// Use when the error phase is unimportant or for non-semantic errors.
pub fn compile_source_should_fail(source: &str) -> String {
    match bengal::compile_source_to_objects(source) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected compilation error but compilation succeeded"),
    }
}

/// Compile a multi-file package, link, run, and return the exit code.
pub fn compile_and_run_package(files: &[(&str, &str)]) -> i32 {
    let dir = tempfile::TempDir::new().unwrap();

    let toml_content = format!("[package]\nname = \"test_pkg\"\nentry = \"{}\"", files[0].0);
    std::fs::write(dir.path().join("Bengal.toml"), toml_content).unwrap();

    for (path, source) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full_path, source).unwrap();
    }

    let entry_path = dir.path().join(files[0].0);
    let exe_path = dir.path().join("test_exe");
    bengal::compile_to_executable(&entry_path, &exe_path, &[])
        .map_err(|e| e.source_error)
        .unwrap();

    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run compiled executable");
    output.status.code().unwrap_or(-1)
}

/// Compile a multi-file package and return the error string.
pub fn compile_package_should_fail(files: &[(&str, &str)]) -> String {
    let dir = tempfile::TempDir::new().unwrap();

    let toml_content = format!("[package]\nname = \"test_pkg\"\nentry = \"{}\"", files[0].0);
    std::fs::write(dir.path().join("Bengal.toml"), toml_content).unwrap();

    for (path, source) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full_path, source).unwrap();
    }

    let entry_path = dir.path().join(files[0].0);
    let exe_path = dir.path().join("test_exe");
    let err = bengal::compile_to_executable(&entry_path, &exe_path, &[])
        .map_err(|e| e.source_error)
        .unwrap_err();
    err.to_string()
}
