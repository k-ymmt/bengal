use std::fmt;

use crate::parser::ast::TypeAnnotation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Unit,
    Struct(String),
    TypeParam { name: String, bound: Option<String> },
    Generic { name: String, args: Vec<Type> },
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::I32 => write!(f, "Int32"),
            Type::I64 => write!(f, "Int64"),
            Type::F32 => write!(f, "Float32"),
            Type::F64 => write!(f, "Float64"),
            Type::Bool => write!(f, "Bool"),
            Type::Unit => write!(f, "()"),
            Type::Struct(name) => write!(f, "{}", name),
            Type::TypeParam { name, .. } => write!(f, "{}", name),
            Type::Generic { name, args } => {
                write!(f, "{}<", name)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ">")
            }
        }
    }
}

impl Type {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::I32 | Type::I64 | Type::F32 | Type::F64)
    }
    pub fn is_integer(&self) -> bool {
        matches!(self, Type::I32 | Type::I64)
    }
    pub fn is_float(&self) -> bool {
        matches!(self, Type::F32 | Type::F64)
    }
}

pub fn resolve_type(annotation: &TypeAnnotation) -> Type {
    match annotation {
        TypeAnnotation::I32 => Type::I32,
        TypeAnnotation::I64 => Type::I64,
        TypeAnnotation::F32 => Type::F32,
        TypeAnnotation::F64 => Type::F64,
        TypeAnnotation::Bool => Type::Bool,
        TypeAnnotation::Unit => Type::Unit,
        TypeAnnotation::Named(name) => Type::Struct(name.clone()),
        TypeAnnotation::Generic { name, args } => Type::Generic {
            name: name.clone(),
            args: args.iter().map(resolve_type).collect(),
        },
    }
}
