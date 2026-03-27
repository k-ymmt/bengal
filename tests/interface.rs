mod common;

use std::io::Write;

use bengal::error::DiagCtxt;
use bengal::interface::{
    FORMAT_VERSION, InterfaceComputedProp, InterfaceFuncEntry, InterfaceFuncSig,
    InterfaceMethodSig, InterfacePropertyReq, InterfaceProtocolEntry, InterfaceStructEntry,
    InterfaceType, InterfaceTypeParam, MAGIC, ModuleInterface, emit_text_interface, read_interface,
    read_text_interface, read_text_interface_file, write_interface, write_text_interface,
};
use bengal::package::ModulePath;
use bengal::parser::ast::Visibility;
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

// ---------- Text emitter tests ----------

#[test]
fn emit_empty_interface() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("// bengal-interface-format-version: 1"));
    // Only the header line, no extra sections
    assert_eq!(text.lines().count(), 1);
}

#[test]
fn emit_simple_function() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![
                    ("a".to_string(), InterfaceType::I32),
                    ("b".to_string(), InterfaceType::I32),
                ],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public func add(a: Int32, b: Int32) -> Int32;"));
}

#[test]
fn emit_generic_function() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "identity".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![InterfaceTypeParam {
                    name: "T".to_string(),
                    bound: Some("Summable".to_string()),
                }],
                params: vec![(
                    "x".to_string(),
                    InterfaceType::TypeParam {
                        name: "T".to_string(),
                        bound: Some("Summable".to_string()),
                    },
                )],
                return_type: InterfaceType::TypeParam {
                    name: "T".to_string(),
                    bound: Some("Summable".to_string()),
                },
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public func identity<T: Summable>(x: T) -> T;"));
}

#[test]
fn emit_unit_return_omitted() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "doSomething".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![],
                return_type: InterfaceType::Unit,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public func doSomething();"));
    assert!(!text.contains("-> Void"));
}

#[test]
fn emit_struct_with_members() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Pair".to_string(),
            type_params: vec![
                InterfaceTypeParam {
                    name: "T".to_string(),
                    bound: None,
                },
                InterfaceTypeParam {
                    name: "U".to_string(),
                    bound: None,
                },
            ],
            conformances: vec!["Printable".to_string()],
            fields: vec![
                (
                    "first".to_string(),
                    InterfaceType::TypeParam {
                        name: "T".to_string(),
                        bound: None,
                    },
                ),
                (
                    "second".to_string(),
                    InterfaceType::TypeParam {
                        name: "U".to_string(),
                        bound: None,
                    },
                ),
            ],
            computed: vec![InterfaceComputedProp {
                name: "total".to_string(),
                ty: InterfaceType::I32,
                has_setter: false,
            }],
            init_params: vec![
                (
                    "first".to_string(),
                    InterfaceType::TypeParam {
                        name: "T".to_string(),
                        bound: None,
                    },
                ),
                (
                    "second".to_string(),
                    InterfaceType::TypeParam {
                        name: "U".to_string(),
                        bound: None,
                    },
                ),
            ],
            methods: vec![InterfaceMethodSig {
                name: "swap".to_string(),
                params: vec![],
                return_type: InterfaceType::Generic {
                    name: "Pair".to_string(),
                    args: vec![
                        InterfaceType::TypeParam {
                            name: "U".to_string(),
                            bound: None,
                        },
                        InterfaceType::TypeParam {
                            name: "T".to_string(),
                            bound: None,
                        },
                    ],
                },
            }],
        }],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public struct Pair<T, U>: Printable {"));
    assert!(text.contains("  var first: T;"));
    assert!(text.contains("  var second: U;"));
    assert!(text.contains("  var total: Int32 { get };"));
    assert!(text.contains("  init(first: T, second: U);"));
    assert!(text.contains("  func swap() -> Pair<U, T>;"));
}

#[test]
fn emit_protocol() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Summable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "sum".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
            properties: vec![InterfacePropertyReq {
                name: "value".to_string(),
                ty: InterfaceType::I32,
                has_setter: true,
            }],
        }],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public protocol Summable {"));
    assert!(text.contains("  func sum() -> Int32;"));
    assert!(text.contains("  var value: Int32 { get set };"));
}

#[test]
fn emit_array_types() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "getArray".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![],
                return_type: InterfaceType::Array {
                    element: Box::new(InterfaceType::I32),
                    size: 4,
                },
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("[Int32; 4]"));
}

#[test]
fn emit_package_visibility() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Package,
            name: "helperFunc".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("x".to_string(), InterfaceType::I32)],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("package func helperFunc(x: Int32) -> Int32;"));
}

#[test]
fn write_text_interface_creates_file() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "foo".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.bengalinterface");
    write_text_interface(&iface, &path).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("// bengal-interface-format-version: 1"));
    assert!(content.contains("public func foo() -> Int32;"));
}

// ---------- Text reader round-trip tests ----------

#[test]
fn text_round_trip_simple_function() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![
                    ("a".into(), InterfaceType::I32),
                    ("b".into(), InterfaceType::I32),
                ],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_generic_function() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "identity".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![InterfaceTypeParam {
                    name: "T".to_string(),
                    bound: None,
                }],
                params: vec![(
                    "x".to_string(),
                    InterfaceType::TypeParam {
                        name: "T".to_string(),
                        bound: None,
                    },
                )],
                return_type: InterfaceType::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                },
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_struct_full() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec!["Summable".to_string()],
            fields: vec![
                ("x".to_string(), InterfaceType::I32),
                ("y".to_string(), InterfaceType::I32),
            ],
            computed: vec![InterfaceComputedProp {
                name: "magnitude".to_string(),
                ty: InterfaceType::I32,
                has_setter: false,
            }],
            init_params: vec![
                ("x".to_string(), InterfaceType::I32),
                ("y".to_string(), InterfaceType::I32),
            ],
            methods: vec![InterfaceMethodSig {
                name: "sum".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
        }],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_protocol() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Drawable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "draw".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
            properties: vec![InterfacePropertyReq {
                name: "visible".to_string(),
                ty: InterfaceType::Bool,
                has_setter: true,
            }],
        }],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_array_types() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "getArray".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![(
                    "arr".to_string(),
                    InterfaceType::Array {
                        element: Box::new(InterfaceType::I32),
                        size: 4,
                    },
                )],
                return_type: InterfaceType::Array {
                    element: Box::new(InterfaceType::I64),
                    size: 8,
                },
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_mixed() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "compute".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("x".into(), InterfaceType::I32)],
                return_type: InterfaceType::I64,
            },
        }],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Data".to_string(),
            type_params: vec![],
            conformances: vec![],
            fields: vec![("value".to_string(), InterfaceType::F64)],
            computed: vec![],
            init_params: vec![("value".to_string(), InterfaceType::F64)],
            methods: vec![],
        }],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Hashable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "hash".to_string(),
                params: vec![],
                return_type: InterfaceType::I64,
            }],
            properties: vec![],
        }],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_generic_struct_with_conformance() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Container".to_string(),
            type_params: vec![InterfaceTypeParam {
                name: "T".to_string(),
                bound: None,
            }],
            conformances: vec!["Printable".to_string()],
            fields: vec![(
                "item".to_string(),
                InterfaceType::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                },
            )],
            computed: vec![],
            init_params: vec![(
                "item".to_string(),
                InterfaceType::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                },
            )],
            methods: vec![InterfaceMethodSig {
                name: "get".to_string(),
                params: vec![],
                return_type: InterfaceType::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                },
            }],
        }],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_package_visibility() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Package,
            name: "helper".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("x".into(), InterfaceType::I32)],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_generic_with_bound() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "total".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![InterfaceTypeParam {
                    name: "T".to_string(),
                    bound: Some("Summable".to_string()),
                }],
                params: vec![(
                    "item".to_string(),
                    InterfaceType::TypeParam {
                        name: "T".to_string(),
                        bound: Some("Summable".to_string()),
                    },
                )],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

// ---------- Text reader error tests ----------

#[test]
fn read_text_missing_header() {
    let text = "public func foo() -> Int32;";
    let err = read_text_interface(text).unwrap_err();
    assert!(
        err.to_string().contains("missing interface format header"),
        "{}",
        err
    );
}

#[test]
fn read_text_wrong_version() {
    let text = "// bengal-interface-format-version: 999\npublic func foo() -> Int32;";
    let err = read_text_interface(text).unwrap_err();
    assert!(
        err.to_string()
            .contains("unsupported interface format version"),
        "{}",
        err
    );
}

#[test]
fn read_text_invalid_syntax() {
    let text = "// bengal-interface-format-version: 1\npublic func ??? broken;";
    let err = read_text_interface(text);
    assert!(err.is_err(), "invalid syntax should produce an error");
}

// ---------- Text reader file I/O test ----------

#[test]
fn read_text_interface_file_round_trip() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "greet".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.bengalinterface");
    write_text_interface(&iface, &path).unwrap();
    let restored = read_text_interface_file(&path).unwrap();
    assert_eq!(iface, restored);
}

// ---------- emit_interfaces pipeline tests ----------

#[test]
fn emit_interfaces_creates_cache_files() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("cache");

    let lowered = source_to_lowered(
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
         func main() -> Int32 { return add(1, 2); }",
    );
    bengal::pipeline::emit_interfaces(&lowered, &cache_dir);

    // Verify file exists for root module
    let root_path = cache_dir.join("root.bengalmod");
    assert!(root_path.exists(), "root.bengalmod should be created");

    // Verify file is readable and contains expected data
    let restored = read_interface(&root_path).unwrap();
    assert_eq!(restored.package_name, lowered.package_name);
    let root_mod = ModulePath::root();
    assert!(restored.interfaces.contains_key(&root_mod));
    let iface = &restored.interfaces[&root_mod];
    assert!(!iface.functions.is_empty());
}

// ---------- Resolver::register_interface tests ----------

#[test]
fn register_interface_functions() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![
                    ("a".to_string(), InterfaceType::I32),
                    ("b".to_string(), InterfaceType::I32),
                ],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let mut resolver = bengal::semantic::resolver::Resolver::default();
    resolver.register_interface(&iface);
    let sig = resolver
        .lookup_func("add")
        .expect("function should be registered");
    assert_eq!(sig.params.len(), 2);
    assert_eq!(sig.return_type, bengal::semantic::types::Type::I32);
}

#[test]
fn register_interface_structs() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec![],
            fields: vec![("x".to_string(), InterfaceType::I32)],
            methods: vec![],
            computed: vec![],
            init_params: vec![("x".to_string(), InterfaceType::I32)],
        }],
        protocols: vec![],
    };
    let mut resolver = bengal::semantic::resolver::Resolver::default();
    resolver.register_interface(&iface);
    let info = resolver
        .lookup_struct("Point")
        .expect("struct should be registered");
    assert_eq!(info.fields.len(), 1);
}

#[test]
fn register_interface_protocols() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Runnable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "run".to_string(),
                params: vec![],
                return_type: InterfaceType::Unit,
            }],
            properties: vec![],
        }],
    };
    let mut resolver = bengal::semantic::resolver::Resolver::default();
    resolver.register_interface(&iface);
    let info = resolver
        .lookup_protocol("Runnable")
        .expect("protocol should be registered");
    assert_eq!(info.methods.len(), 1);
}

#[test]
fn interface_to_global_symbols_all_types() {
    use bengal::package::ModulePath;
    use bengal::semantic::interface_to_global_symbols;

    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("a".to_string(), InterfaceType::I32)],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Package,
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec![],
            fields: vec![("x".to_string(), InterfaceType::I32)],
            methods: vec![],
            computed: vec![],
            init_params: vec![("x".to_string(), InterfaceType::I32)],
        }],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Runnable".to_string(),
            methods: vec![],
            properties: vec![],
        }],
    };

    let mod_path = ModulePath(vec!["math".to_string()]);
    let symbols = interface_to_global_symbols(&iface, &mod_path);

    assert_eq!(symbols.len(), 3);
    assert!(symbols.contains_key("add"));
    assert!(symbols.contains_key("Point"));
    assert!(symbols.contains_key("Runnable"));

    // Verify visibility
    assert_eq!(symbols["add"].visibility, Visibility::Public);
    assert_eq!(symbols["Point"].visibility, Visibility::Package);

    // Verify module path
    assert_eq!(symbols["add"].module, mod_path);
}
