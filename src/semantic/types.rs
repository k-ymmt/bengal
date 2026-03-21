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
