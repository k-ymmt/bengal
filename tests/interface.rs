mod common;

use bengal::interface::{read_interface, write_interface};
use bengal::package::ModulePath;
use bengal::pipeline::{self, LoweredPackage};
use tempfile::NamedTempFile;

/// Helper: compile source to LoweredPackage (through optimize stage).
fn source_to_lowered(source: &str) -> LoweredPackage {
    let parsed = pipeline::parse_source("test", source).unwrap();
    let analyzed = pipeline::analyze(parsed).unwrap();
    let lowered = pipeline::lower(analyzed).unwrap();
    pipeline::optimize(lowered)
}

#[test]
fn write_interface_creates_file() {
    let lowered = source_to_lowered("func main() -> Int32 { return 42; }");
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let metadata = std::fs::metadata(file.path()).unwrap();
    assert!(metadata.len() > 8, "file must contain header + payload");
}

#[test]
fn round_trip_simple_function() {
    let lowered = source_to_lowered(
        "func add(a: Int32, b: Int32) -> Int32 { return a + b; }
         func main() -> Int32 { return add(1, 2); }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    assert_eq!(loaded.package_name, "test");
    let original_bir = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original_bir, loaded_bir);
}
