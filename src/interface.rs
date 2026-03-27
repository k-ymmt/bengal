use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bir::instruction::BirModule;
use crate::error::{BengalError, Result};
use crate::package::ModulePath;
use crate::pipeline::LoweredPackage;
use crate::semantic::types::Type;

pub const MAGIC: &[u8; 4] = b"BGMD";
pub const FORMAT_VERSION: u32 = 1;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleInterface {
    pub functions: Vec<InterfaceFuncEntry>,
    pub structs: Vec<InterfaceStructEntry>,
    pub protocols: Vec<InterfaceProtocolEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterfaceFuncEntry {
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

#[derive(Debug, Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
}

/// Write a LoweredPackage to a .bengalmod interface file.
pub fn write_interface(package: &LoweredPackage, path: &Path) -> Result<()> {
    let modules: HashMap<ModulePath, BirModule> = package
        .modules
        .iter()
        .map(|(k, v)| (k.clone(), v.bir.clone()))
        .collect();

    let file = BengalModFile {
        package_name: package.package_name.clone(),
        modules,
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
}
