use crate::parser::ast::TypeAnnotation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I32,
}

pub fn resolve_type(annotation: &TypeAnnotation) -> Type {
    match annotation {
        TypeAnnotation::I32 => Type::I32,
    }
}
