use crate::parser::ast::TypeAnnotation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Unit,
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
    }
}
