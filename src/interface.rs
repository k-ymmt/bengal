use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bir::instruction::BirModule;
use crate::error::{BengalError, Result};
use crate::package::ModulePath;
use crate::parser::ast::{TypeParam, Visibility};
use crate::pipeline::LoweredPackage;
use crate::semantic::SemanticInfo;
use crate::semantic::types::Type;

pub const MAGIC: &[u8; 4] = b"BGMD";
pub const FORMAT_VERSION: u32 = 2;
pub const TEXT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InterfaceType {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Unit,
    Struct(String),
    TypeParam {
        name: String,
        bound: Option<String>,
    },
    Generic {
        name: String,
        args: Vec<InterfaceType>,
    },
    Array {
        element: Box<InterfaceType>,
        size: u64,
    },
}

impl InterfaceType {
    pub fn from_type(ty: &Type) -> Self {
        match ty {
            Type::I32 => InterfaceType::I32,
            Type::I64 => InterfaceType::I64,
            Type::F32 => InterfaceType::F32,
            Type::F64 => InterfaceType::F64,
            Type::Bool => InterfaceType::Bool,
            Type::Unit => InterfaceType::Unit,
            Type::Struct(name) => InterfaceType::Struct(name.clone()),
            Type::TypeParam { name, bound } => InterfaceType::TypeParam {
                name: name.clone(),
                bound: bound.clone(),
            },
            Type::Generic { name, args } => InterfaceType::Generic {
                name: name.clone(),
                args: args.iter().map(InterfaceType::from_type).collect(),
            },
            Type::Array { element, size } => InterfaceType::Array {
                element: Box::new(InterfaceType::from_type(element)),
                size: *size,
            },
            Type::InferVar(_) | Type::IntegerLiteral(_) | Type::FloatLiteral(_) | Type::Error => {
                unreachable!("interface types must be fully resolved")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceTypeParam {
    pub name: String,
    pub bound: Option<String>,
}

impl InterfaceTypeParam {
    pub fn from_type_param(tp: &TypeParam) -> Self {
        InterfaceTypeParam {
            name: tp.name.clone(),
            bound: tp.bound.clone(),
        }
    }
}

fn is_exported(vis: Visibility) -> bool {
    matches!(vis, Visibility::Public | Visibility::Package)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleInterface {
    pub functions: Vec<InterfaceFuncEntry>,
    pub structs: Vec<InterfaceStructEntry>,
    pub protocols: Vec<InterfaceProtocolEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceFuncEntry {
    pub visibility: Visibility,
    pub name: String,
    pub sig: InterfaceFuncSig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceFuncSig {
    pub type_params: Vec<InterfaceTypeParam>,
    pub params: Vec<(String, InterfaceType)>,
    pub return_type: InterfaceType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceStructEntry {
    pub visibility: Visibility,
    pub name: String,
    pub type_params: Vec<InterfaceTypeParam>,
    pub conformances: Vec<String>,
    pub fields: Vec<(String, InterfaceType)>,
    pub methods: Vec<InterfaceMethodSig>,
    pub computed: Vec<InterfaceComputedProp>,
    pub init_params: Vec<(String, InterfaceType)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceMethodSig {
    pub name: String,
    pub params: Vec<(String, InterfaceType)>,
    pub return_type: InterfaceType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceComputedProp {
    pub name: String,
    pub ty: InterfaceType,
    pub has_setter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceProtocolEntry {
    pub visibility: Visibility,
    pub name: String,
    pub methods: Vec<InterfaceMethodSig>,
    pub properties: Vec<InterfacePropertyReq>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfacePropertyReq {
    pub name: String,
    pub ty: InterfaceType,
    pub has_setter: bool,
}

impl ModuleInterface {
    pub fn from_semantic_info(sem: &SemanticInfo) -> Self {
        let mut functions: Vec<InterfaceFuncEntry> = sem
            .functions
            .iter()
            .filter(|(name, _)| {
                sem.visibilities
                    .get(*name)
                    .copied()
                    .is_some_and(is_exported)
            })
            .map(|(name, sig)| InterfaceFuncEntry {
                visibility: sem.visibilities.get(name).copied().unwrap_or_default(),
                name: name.clone(),
                sig: InterfaceFuncSig {
                    type_params: sig
                        .type_params
                        .iter()
                        .map(InterfaceTypeParam::from_type_param)
                        .collect(),
                    params: sig
                        .params
                        .iter()
                        .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                        .collect(),
                    return_type: InterfaceType::from_type(&sig.return_type),
                },
            })
            .collect();

        let mut structs: Vec<InterfaceStructEntry> = sem
            .struct_defs
            .iter()
            .filter(|(name, _)| {
                sem.visibilities
                    .get(*name)
                    .copied()
                    .is_some_and(is_exported)
            })
            .map(|(name, info)| InterfaceStructEntry {
                visibility: sem.visibilities.get(name).copied().unwrap_or_default(),
                name: name.clone(),
                type_params: info
                    .type_params
                    .iter()
                    .map(InterfaceTypeParam::from_type_param)
                    .collect(),
                conformances: info.conformances.clone(),
                fields: info
                    .fields
                    .iter()
                    .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                    .collect(),
                methods: info
                    .methods
                    .iter()
                    .map(|m| InterfaceMethodSig {
                        name: m.name.clone(),
                        params: m
                            .params
                            .iter()
                            .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                            .collect(),
                        return_type: InterfaceType::from_type(&m.return_type),
                    })
                    .collect(),
                computed: info
                    .computed
                    .iter()
                    .map(|c| InterfaceComputedProp {
                        name: c.name.clone(),
                        ty: InterfaceType::from_type(&c.ty),
                        has_setter: c.has_setter,
                    })
                    .collect(),
                init_params: info
                    .init
                    .params
                    .iter()
                    .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                    .collect(),
            })
            .collect();

        let mut protocols: Vec<InterfaceProtocolEntry> = sem
            .protocols
            .iter()
            .filter(|(name, _)| {
                sem.visibilities
                    .get(*name)
                    .copied()
                    .is_some_and(is_exported)
            })
            .map(|(name, info)| InterfaceProtocolEntry {
                visibility: sem.visibilities.get(name).copied().unwrap_or_default(),
                name: name.clone(),
                methods: info
                    .methods
                    .iter()
                    .map(|m| InterfaceMethodSig {
                        name: m.name.clone(),
                        params: m
                            .params
                            .iter()
                            .map(|(n, t)| (n.clone(), InterfaceType::from_type(t)))
                            .collect(),
                        return_type: InterfaceType::from_type(&m.return_type),
                    })
                    .collect(),
                properties: info
                    .properties
                    .iter()
                    .map(|p| InterfacePropertyReq {
                        name: p.name.clone(),
                        ty: InterfaceType::from_type(&p.ty),
                        has_setter: p.has_setter,
                    })
                    .collect(),
            })
            .collect();

        // Sort for deterministic output (HashMap iteration order is random)
        functions.sort_by(|a, b| a.name.cmp(&b.name));
        structs.sort_by(|a, b| a.name.cmp(&b.name));
        protocols.sort_by(|a, b| a.name.cmp(&b.name));

        ModuleInterface {
            functions,
            structs,
            protocols,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
    pub interfaces: HashMap<ModulePath, ModuleInterface>,
}

/// Write a LoweredPackage to a .bengalmod interface file.
pub fn write_interface(package: &LoweredPackage, path: &Path) -> Result<()> {
    let mut modules: HashMap<ModulePath, BirModule> = HashMap::new();
    let mut interfaces: HashMap<ModulePath, ModuleInterface> = HashMap::new();

    for (mod_path, lowered_mod) in &package.modules {
        modules.insert(mod_path.clone(), lowered_mod.bir.clone());

        if let Some(sem_info) = package.pkg_sem_info.module_infos.get(mod_path) {
            interfaces.insert(
                mod_path.clone(),
                ModuleInterface::from_semantic_info(sem_info),
            );
        }
    }

    let file = BengalModFile {
        package_name: package.package_name.clone(),
        modules,
        interfaces,
    };

    let payload = rmp_serde::to_vec(&file).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to serialize interface: {}", e),
    })?;

    let mut out = std::fs::File::create(path).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to create file '{}': {}", path.display(), e),
    })?;

    out.write_all(MAGIC)
        .and_then(|()| out.write_all(&FORMAT_VERSION.to_le_bytes()))
        .and_then(|()| out.write_all(&payload))
        .map_err(|e| BengalError::InterfaceError {
            message: format!("failed to write interface file: {}", e),
        })?;

    Ok(())
}

/// Read a .bengalmod interface file.
pub fn read_interface(path: &Path) -> Result<BengalModFile> {
    let data = std::fs::read(path).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to read '{}': {}", path.display(), e),
    })?;

    if data.len() < 8 {
        return Err(BengalError::InterfaceError {
            message: "file too short to be a valid .bengalmod file".to_string(),
        });
    }

    if &data[..4] != MAGIC {
        return Err(BengalError::InterfaceError {
            message: "invalid magic bytes: not a .bengalmod file".to_string(),
        });
    }

    let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
    if version != FORMAT_VERSION {
        return Err(BengalError::InterfaceError {
            message: format!(
                "incompatible format version {} (expected {}), please rebuild",
                version, FORMAT_VERSION
            ),
        });
    }

    rmp_serde::from_slice(&data[8..]).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to deserialize interface: {}", e),
    })
}

// --- Text emitter helpers ---

fn emit_type(ty: &InterfaceType) -> String {
    match ty {
        InterfaceType::I32 => "Int32".to_string(),
        InterfaceType::I64 => "Int64".to_string(),
        InterfaceType::F32 => "Float32".to_string(),
        InterfaceType::F64 => "Float64".to_string(),
        InterfaceType::Bool => "Bool".to_string(),
        InterfaceType::Unit => "Void".to_string(),
        InterfaceType::Struct(name) => name.clone(),
        InterfaceType::TypeParam { name, .. } => name.clone(),
        InterfaceType::Generic { name, args } => {
            let args_str: Vec<String> = args.iter().map(emit_type).collect();
            format!("{}<{}>", name, args_str.join(", "))
        }
        InterfaceType::Array { element, size } => {
            format!("[{}; {}]", emit_type(element), size)
        }
    }
}

fn emit_visibility(vis: Visibility) -> &'static str {
    match vis {
        Visibility::Public => "public",
        Visibility::Package => "package",
        _ => "internal",
    }
}

fn emit_type_params(tps: &[InterfaceTypeParam]) -> String {
    if tps.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = tps
        .iter()
        .map(|tp| match &tp.bound {
            Some(bound) => format!("{}: {}", tp.name, bound),
            None => tp.name.clone(),
        })
        .collect();
    format!("<{}>", parts.join(", "))
}

fn emit_params(params: &[(String, InterfaceType)]) -> String {
    let parts: Vec<String> = params
        .iter()
        .map(|(name, ty)| format!("{}: {}", name, emit_type(ty)))
        .collect();
    parts.join(", ")
}

fn emit_return_type(ty: &InterfaceType) -> String {
    match ty {
        InterfaceType::Unit => String::new(),
        _ => format!(" -> {}", emit_type(ty)),
    }
}

/// Convert a `ModuleInterface` into `.bengalinterface` text format.
pub fn emit_text_interface(iface: &ModuleInterface) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// bengal-interface-format-version: {}\n",
        TEXT_FORMAT_VERSION
    ));

    let mut has_previous_section = false;

    // Functions section
    if !iface.functions.is_empty() {
        has_previous_section = true;
        for func in &iface.functions {
            out.push_str(&format!(
                "{} func {}{}({}){};",
                emit_visibility(func.visibility),
                func.name,
                emit_type_params(&func.sig.type_params),
                emit_params(&func.sig.params),
                emit_return_type(&func.sig.return_type),
            ));
            out.push('\n');
        }
    }

    // Structs section
    if !iface.structs.is_empty() {
        if has_previous_section {
            out.push('\n');
        }
        has_previous_section = true;
        for s in &iface.structs {
            // Header: visibility struct Name<T, U>: Proto1, Proto2 {
            let conformances = if s.conformances.is_empty() {
                String::new()
            } else {
                format!(": {}", s.conformances.join(", "))
            };
            out.push_str(&format!(
                "{} struct {}{}{} {{\n",
                emit_visibility(s.visibility),
                s.name,
                emit_type_params(&s.type_params),
                conformances,
            ));

            // Stored properties
            for (name, ty) in &s.fields {
                out.push_str(&format!("  var {}: {};\n", name, emit_type(ty)));
            }

            // Computed properties
            for comp in &s.computed {
                let accessor = if comp.has_setter {
                    "{ get set }"
                } else {
                    "{ get }"
                };
                out.push_str(&format!(
                    "  var {}: {} {};\n",
                    comp.name,
                    emit_type(&comp.ty),
                    accessor
                ));
            }

            // Init
            if !s.init_params.is_empty() {
                out.push_str(&format!("  init({});\n", emit_params(&s.init_params)));
            }

            // Methods
            for m in &s.methods {
                out.push_str(&format!(
                    "  func {}({}){};",
                    m.name,
                    emit_params(&m.params),
                    emit_return_type(&m.return_type),
                ));
                out.push('\n');
            }

            out.push_str("}\n");
        }
    }

    // Protocols section
    if !iface.protocols.is_empty() {
        if has_previous_section {
            out.push('\n');
        }
        for p in &iface.protocols {
            out.push_str(&format!(
                "{} protocol {} {{\n",
                emit_visibility(p.visibility),
                p.name,
            ));

            // Methods
            for m in &p.methods {
                out.push_str(&format!(
                    "  func {}({}){};",
                    m.name,
                    emit_params(&m.params),
                    emit_return_type(&m.return_type),
                ));
                out.push('\n');
            }

            // Properties
            for prop in &p.properties {
                let accessor = if prop.has_setter {
                    "{ get set }"
                } else {
                    "{ get }"
                };
                out.push_str(&format!(
                    "  var {}: {} {};\n",
                    prop.name,
                    emit_type(&prop.ty),
                    accessor
                ));
            }

            out.push_str("}\n");
        }
    }

    out
}

/// Write a `ModuleInterface` as text to a `.bengalinterface` file.
pub fn write_text_interface(iface: &ModuleInterface, path: &Path) -> Result<()> {
    let text = emit_text_interface(iface);
    std::fs::write(path, text).map_err(|e| BengalError::InterfaceError {
        message: format!(
            "failed to write text interface file '{}': {}",
            path.display(),
            e
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::types::Type;

    #[test]
    fn interface_type_from_primitives() {
        assert_eq!(InterfaceType::from_type(&Type::I32), InterfaceType::I32);
        assert_eq!(InterfaceType::from_type(&Type::I64), InterfaceType::I64);
        assert_eq!(InterfaceType::from_type(&Type::F32), InterfaceType::F32);
        assert_eq!(InterfaceType::from_type(&Type::F64), InterfaceType::F64);
        assert_eq!(InterfaceType::from_type(&Type::Bool), InterfaceType::Bool);
        assert_eq!(InterfaceType::from_type(&Type::Unit), InterfaceType::Unit);
    }

    #[test]
    fn interface_type_from_struct() {
        assert_eq!(
            InterfaceType::from_type(&Type::Struct("Point".to_string())),
            InterfaceType::Struct("Point".to_string()),
        );
    }

    #[test]
    fn interface_type_from_type_param() {
        assert_eq!(
            InterfaceType::from_type(&Type::TypeParam {
                name: "T".to_string(),
                bound: Some("Summable".to_string()),
            }),
            InterfaceType::TypeParam {
                name: "T".to_string(),
                bound: Some("Summable".to_string()),
            },
        );
    }

    #[test]
    fn interface_type_from_generic_recursive() {
        let ty = Type::Generic {
            name: "Pair".to_string(),
            args: vec![Type::I32, Type::Struct("Point".to_string())],
        };
        assert_eq!(
            InterfaceType::from_type(&ty),
            InterfaceType::Generic {
                name: "Pair".to_string(),
                args: vec![
                    InterfaceType::I32,
                    InterfaceType::Struct("Point".to_string())
                ],
            },
        );
    }

    #[test]
    fn interface_type_from_array_recursive() {
        let ty = Type::Array {
            element: Box::new(Type::Generic {
                name: "Box".to_string(),
                args: vec![Type::I64],
            }),
            size: 5,
        };
        assert_eq!(
            InterfaceType::from_type(&ty),
            InterfaceType::Array {
                element: Box::new(InterfaceType::Generic {
                    name: "Box".to_string(),
                    args: vec![InterfaceType::I64],
                }),
                size: 5,
            },
        );
    }

    #[test]
    fn module_interface_round_trip() {
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
            structs: vec![InterfaceStructEntry {
                visibility: Visibility::Public,
                name: "Point".to_string(),
                type_params: vec![],
                conformances: vec!["Summable".to_string()],
                fields: vec![
                    ("x".to_string(), InterfaceType::I32),
                    ("y".to_string(), InterfaceType::I32),
                ],
                methods: vec![InterfaceMethodSig {
                    name: "sum".to_string(),
                    params: vec![],
                    return_type: InterfaceType::I32,
                }],
                computed: vec![InterfaceComputedProp {
                    name: "magnitude".to_string(),
                    ty: InterfaceType::I32,
                    has_setter: false,
                }],
                init_params: vec![
                    ("x".to_string(), InterfaceType::I32),
                    ("y".to_string(), InterfaceType::I32),
                ],
            }],
            protocols: vec![InterfaceProtocolEntry {
                visibility: Visibility::Public,
                name: "Summable".to_string(),
                methods: vec![InterfaceMethodSig {
                    name: "sum".to_string(),
                    params: vec![],
                    return_type: InterfaceType::I32,
                }],
                properties: vec![InterfacePropertyReq {
                    name: "count".to_string(),
                    ty: InterfaceType::I32,
                    has_setter: false,
                }],
            }],
        };
        let bytes = rmp_serde::to_vec(&iface).unwrap();
        let loaded: ModuleInterface = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(iface, loaded);
    }

    use crate::parser::ast::{TypeParam, Visibility};
    use crate::semantic::SemanticInfo;
    use crate::semantic::resolver::{FuncSig, InitializerInfo, ProtocolInfo, StructInfo};
    use std::collections::{HashMap, HashSet};

    fn make_test_semantic_info() -> SemanticInfo {
        let mut functions = HashMap::new();
        functions.insert(
            "public_add".to_string(),
            FuncSig {
                type_params: vec![],
                params: vec![("a".to_string(), Type::I32), ("b".to_string(), Type::I32)],
                return_type: Type::I32,
            },
        );
        functions.insert(
            "internal_helper".to_string(),
            FuncSig {
                type_params: vec![],
                params: vec![],
                return_type: Type::Unit,
            },
        );
        functions.insert(
            "fileprivate_fn".to_string(),
            FuncSig {
                type_params: vec![],
                params: vec![],
                return_type: Type::Unit,
            },
        );
        functions.insert(
            "generic_pub".to_string(),
            FuncSig {
                type_params: vec![TypeParam {
                    name: "T".to_string(),
                    bound: Some("Summable".to_string()),
                }],
                params: vec![(
                    "x".to_string(),
                    Type::TypeParam {
                        name: "T".to_string(),
                        bound: Some("Summable".to_string()),
                    },
                )],
                return_type: Type::I32,
            },
        );

        let mut visibilities = HashMap::new();
        visibilities.insert("public_add".to_string(), Visibility::Public);
        visibilities.insert("internal_helper".to_string(), Visibility::Internal);
        visibilities.insert("fileprivate_fn".to_string(), Visibility::Fileprivate);
        visibilities.insert("generic_pub".to_string(), Visibility::Public);
        visibilities.insert("MyStruct".to_string(), Visibility::Package);
        visibilities.insert("MyProto".to_string(), Visibility::Private);

        let mut struct_defs = HashMap::new();
        struct_defs.insert(
            "MyStruct".to_string(),
            StructInfo {
                type_params: vec![],
                conformances: vec!["Proto".to_string()],
                fields: vec![("x".to_string(), Type::I32)],
                field_index: [("x".to_string(), 0)].into_iter().collect(),
                computed: vec![],
                computed_index: HashMap::new(),
                init: InitializerInfo {
                    params: vec![("x".to_string(), Type::I32)],
                    body: None,
                },
                methods: vec![],
                method_index: HashMap::new(),
            },
        );

        let mut protocols = HashMap::new();
        protocols.insert(
            "MyProto".to_string(),
            ProtocolInfo {
                name: "MyProto".to_string(),
                methods: vec![],
                properties: vec![],
            },
        );

        SemanticInfo {
            struct_defs,
            struct_init_calls: HashSet::new(),
            protocols,
            functions,
            visibilities,
        }
    }

    #[test]
    fn from_semantic_info_filters_visibility() {
        let sem = make_test_semantic_info();
        let iface = ModuleInterface::from_semantic_info(&sem);

        // Public + Package included; Internal, Fileprivate, Private excluded
        assert_eq!(iface.functions.len(), 2);
        let func_names: Vec<&str> = iface.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(func_names.contains(&"public_add"));
        assert!(func_names.contains(&"generic_pub"));
        assert!(!func_names.contains(&"internal_helper"));
        assert!(!func_names.contains(&"fileprivate_fn"));

        // Verify generic function has type_params with bound
        let generic = iface
            .functions
            .iter()
            .find(|f| f.name == "generic_pub")
            .unwrap();
        assert_eq!(generic.sig.type_params.len(), 1);
        assert_eq!(generic.sig.type_params[0].name, "T");
        assert_eq!(
            generic.sig.type_params[0].bound,
            Some("Summable".to_string())
        );

        // Package struct included
        assert_eq!(iface.structs.len(), 1);
        assert_eq!(iface.structs[0].name, "MyStruct");
        assert_eq!(iface.structs[0].conformances, vec!["Proto".to_string()]);

        // Private protocol excluded
        assert_eq!(iface.protocols.len(), 0);
    }

    #[test]
    fn from_semantic_info_empty() {
        let sem = SemanticInfo {
            struct_defs: HashMap::new(),
            struct_init_calls: HashSet::new(),
            protocols: HashMap::new(),
            functions: HashMap::new(),
            visibilities: HashMap::new(),
        };
        let iface = ModuleInterface::from_semantic_info(&sem);
        assert!(iface.functions.is_empty());
        assert!(iface.structs.is_empty());
        assert!(iface.protocols.is_empty());
    }
}
