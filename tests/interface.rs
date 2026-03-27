mod common;

use std::io::Write;

use bengal::error::DiagCtxt;
use bengal::interface::{FORMAT_VERSION, MAGIC, read_interface, write_interface};
use bengal::package::ModulePath;
use bengal::pipeline::{self, LoweredPackage};
use tempfile::{NamedTempFile, TempDir};

/// Helper: compile source to LoweredPackage (through optimize stage).
fn source_to_lowered(source: &str) -> LoweredPackage {
    let parsed = pipeline::parse_source("test", source).unwrap();
    let analyzed = pipeline::analyze(parsed, &mut DiagCtxt::new()).unwrap();
    let lowered = pipeline::lower(analyzed, &mut DiagCtxt::new()).unwrap();
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

#[test]
fn round_trip_generic_function() {
    let lowered = source_to_lowered(
        "func identity<T>(x: T) -> T { return x; }
         func main() -> Int32 { return identity<Int32>(42); }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}

#[test]
fn round_trip_struct_with_methods() {
    let lowered = source_to_lowered(
        "struct Point {
            var x: Int32;
            var y: Int32;
            func sum() -> Int32 { return self.x + self.y; }
         }
         func main() -> Int32 {
            let p = Point(x: 3, y: 4);
            return p.sum();
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}

#[test]
fn round_trip_generic_struct() {
    let lowered = source_to_lowered(
        "struct Box<T> {
            var value: T;
         }
         func main() -> Int32 {
            let b = Box<Int32>(value: 42);
            return b.value;
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}

#[test]
fn round_trip_protocol_conformance() {
    let lowered = source_to_lowered(
        "protocol Summable {
            func sum() -> Int32;
         }
         struct Pair: Summable {
            var a: Int32;
            var b: Int32;
            func sum() -> Int32 { return self.a + self.b; }
         }
         func total<T: Summable>(item: T) -> Int32 { return item.sum(); }
         func main() -> Int32 {
            return total<Pair>(Pair(a: 10, b: 20));
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}

#[test]
fn round_trip_array() {
    let lowered = source_to_lowered(
        "func main() -> Int32 {
            let arr: [Int32; 3] = [10, 20, 30];
            return arr[1];
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}

#[test]
fn read_invalid_magic() {
    let file = NamedTempFile::new().unwrap();
    let mut f = std::fs::File::create(file.path()).unwrap();
    f.write_all(b"XXXX").unwrap();
    f.write_all(&FORMAT_VERSION.to_le_bytes()).unwrap();
    f.write_all(b"dummy").unwrap();
    drop(f);

    let err = read_interface(file.path()).unwrap_err();
    assert!(err.to_string().contains("not a .bengalmod file"), "{}", err);
}

#[test]
fn read_wrong_version() {
    let file = NamedTempFile::new().unwrap();
    let mut f = std::fs::File::create(file.path()).unwrap();
    f.write_all(MAGIC).unwrap();
    f.write_all(&(FORMAT_VERSION + 1).to_le_bytes()).unwrap();
    f.write_all(b"dummy").unwrap();
    drop(f);

    let err = read_interface(file.path()).unwrap_err();
    assert!(
        err.to_string().contains("incompatible format version"),
        "{}",
        err
    );
}

#[test]
fn read_empty_file() {
    let file = NamedTempFile::new().unwrap();
    // file is empty (0 bytes)

    let err = read_interface(file.path()).unwrap_err();
    assert!(err.to_string().contains("too short"), "{}", err);
}

#[test]
fn read_truncated_payload() {
    let file = NamedTempFile::new().unwrap();
    let mut f = std::fs::File::create(file.path()).unwrap();
    f.write_all(MAGIC).unwrap();
    f.write_all(&FORMAT_VERSION.to_le_bytes()).unwrap();
    f.write_all(&[0xff, 0xff]).unwrap(); // invalid msgpack
    drop(f);

    let err = read_interface(file.path()).unwrap_err();
    assert!(err.to_string().contains("failed to deserialize"), "{}", err);
}

#[test]
fn round_trip_semantic_info_functions() {
    let lowered = source_to_lowered(
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
         func internal_helper() -> Int32 { return 0; }
         func main() -> Int32 { return add(1, 2); }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let iface = loaded.interfaces.get(&ModulePath::root()).unwrap();
    // Only public function in interface (main and internal_helper are Internal)
    assert_eq!(iface.functions.len(), 1);
    assert_eq!(iface.functions[0].name, "add");
    assert_eq!(iface.functions[0].sig.params.len(), 2);
}

#[test]
fn round_trip_semantic_info_generic_function() {
    let lowered = source_to_lowered(
        "public func identity<T>(x: T) -> T { return x; }
         func main() -> Int32 { return identity<Int32>(42); }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let iface = loaded.interfaces.get(&ModulePath::root()).unwrap();
    assert_eq!(iface.functions.len(), 1);
    assert_eq!(iface.functions[0].name, "identity");
    assert!(!iface.functions[0].sig.type_params.is_empty());
    assert_eq!(iface.functions[0].sig.type_params[0].name, "T");
}

#[test]
fn round_trip_semantic_info_struct() {
    let lowered = source_to_lowered(
        "protocol Summable { func sum() -> Int32; }
         public struct Point: Summable {
            var x: Int32;
            var y: Int32;
            func sum() -> Int32 { return self.x + self.y; }
         }
         func main() -> Int32 {
            let p = Point(x: 1, y: 2);
            return p.sum();
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let iface = loaded.interfaces.get(&ModulePath::root()).unwrap();
    assert_eq!(iface.structs.len(), 1);
    assert_eq!(iface.structs[0].name, "Point");
    assert_eq!(iface.structs[0].conformances, vec!["Summable".to_string()]);
    assert_eq!(iface.structs[0].fields.len(), 2);
    assert_eq!(iface.structs[0].methods.len(), 1);
    assert_eq!(iface.structs[0].init_params.len(), 2);
}

#[test]
fn round_trip_semantic_info_protocol() {
    let lowered = source_to_lowered(
        "public protocol Drawable {
            func draw() -> Int32;
            var visible: Bool { get };
         }
         struct Canvas: Drawable {
            var visible: Bool { get { return true; } };
            func draw() -> Int32 { return 1; }
         }
         func main() -> Int32 { return 0; }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let iface = loaded.interfaces.get(&ModulePath::root()).unwrap();
    assert_eq!(iface.protocols.len(), 1);
    assert_eq!(iface.protocols[0].name, "Drawable");
    assert_eq!(iface.protocols[0].methods.len(), 1);
    assert_eq!(iface.protocols[0].properties.len(), 1);
    assert!(iface.protocols[0].properties[0].has_setter == false);
}

#[test]
fn round_trip_multi_module_semantic_info() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("Bengal.toml"),
        "[package]\nname = \"mypkg\"\nentry = \"main.bengal\"",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.bengal"),
        "module math;\nimport math::add;\nfunc main() -> Int32 { return add(1, 2); }",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("math.bengal"),
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
    )
    .unwrap();

    let parsed = pipeline::parse(&dir.path().join("main.bengal")).unwrap();
    let analyzed = pipeline::analyze(parsed, &mut bengal::error::DiagCtxt::new()).unwrap();
    let lowered = pipeline::lower(analyzed, &mut bengal::error::DiagCtxt::new()).unwrap();
    let optimized = pipeline::optimize(lowered);

    let interface_file = dir.path().join("mypkg.bengalmod");
    write_interface(&optimized, &interface_file).unwrap();
    let loaded = read_interface(&interface_file).unwrap();

    // Math module should have `add` in its interface
    let math_path = ModulePath::root().child("math");
    let math_iface = loaded.interfaces.get(&math_path).unwrap();
    assert!(
        math_iface.functions.iter().any(|f| f.name == "add"),
        "math module interface should contain public func add"
    );
}

#[test]
fn round_trip_multi_module_package() {
    // Create package on disk
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("Bengal.toml"),
        "[package]\nname = \"mypkg\"\nentry = \"main.bengal\"",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.bengal"),
        "module math;\nimport math::add;\nfunc main() -> Int32 { return add(1, 2); }",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("math.bengal"),
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
    )
    .unwrap();

    // Run pipeline through optimize
    let parsed = pipeline::parse(&dir.path().join("main.bengal")).unwrap();
    let analyzed = pipeline::analyze(parsed, &mut DiagCtxt::new()).unwrap();
    let lowered = pipeline::lower(analyzed, &mut DiagCtxt::new()).unwrap();
    let optimized = pipeline::optimize(lowered);

    // Round-trip
    let interface_file = dir.path().join("mypkg.bengalmod");
    write_interface(&optimized, &interface_file).unwrap();
    let loaded = read_interface(&interface_file).unwrap();

    assert_eq!(loaded.package_name, "mypkg");
    assert_eq!(loaded.modules.len(), optimized.modules.len());
    for (path, module) in &optimized.modules {
        let loaded_bir = loaded
            .modules
            .get(path)
            .unwrap_or_else(|| panic!("missing module {}", path));
        assert_eq!(&module.bir, loaded_bir);
    }
}
