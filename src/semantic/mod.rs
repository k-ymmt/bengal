pub mod infer;
pub mod resolver;
pub mod types;
mod unify;

mod expr_analysis;
mod expr_call_analysis;
mod expr_method_analysis;
mod function_analysis;
mod generic_validation;
mod package_analysis;
mod post_mono;
mod pre_mono;
mod single_module_analysis;
mod stmt_analysis;
mod struct_analysis;

pub use generic_validation::{validate_generics, validate_main};
pub use package_analysis::{
    GlobalSymbol, GlobalSymbolTable, SymbolKind, analyze_package, dep_module_path,
    interface_to_global_symbols,
};
pub use post_mono::analyze_post_mono;
pub use pre_mono::{analyze_pre_mono, analyze_pre_mono_lenient};
use single_module_analysis::analyze_single_module;

// Re-export extracted functions so existing sibling modules can
// continue to import them via `super::`.
use function_analysis::analyze_function;
use struct_analysis::analyze_struct_members;

use std::collections::HashMap;

use crate::error::{BengalError, DiagCtxt, Result, Span};
use crate::package::ModulePath;
use crate::parser::ast::*;
use crate::suggest::find_suggestion;
use resolver::Resolver;
use types::{Type, resolve_type};

#[derive(Debug, Clone)]
pub struct SemanticInfo {
    pub struct_defs: HashMap<String, resolver::StructInfo>,
    pub struct_init_calls: std::collections::HashSet<NodeId>,
    pub protocols: HashMap<String, resolver::ProtocolInfo>,
    pub functions: HashMap<String, resolver::FuncSig>,
    pub visibilities: HashMap<String, Visibility>,
}

#[derive(Debug, Clone)]
pub struct PackageSemanticInfo {
    pub module_infos: HashMap<ModulePath, SemanticInfo>,
    /// For each module, maps imported symbol names to their source module path.
    /// Key: (importing module, local symbol name) -> source module path.
    pub import_sources: HashMap<(ModulePath, String), ModulePath>,
    /// Maps external dep module paths to their original package names (for name mangling).
    pub external_dep_names: HashMap<ModulePath, String>,
}

fn sem_err(message: impl Into<String>) -> BengalError {
    BengalError::SemanticError {
        message: message.into(),
        span: Span { start: 0, end: 0 },
        help: None,
    }
}

fn sem_err_with_help(message: impl Into<String>, span: Span, help: Option<String>) -> BengalError {
    BengalError::SemanticError {
        message: message.into(),
        span,
        help,
    }
}

fn pkg_err(message: impl Into<String>) -> BengalError {
    BengalError::PackageError {
        message: message.into(),
    }
}

/// Return true if `name` is a built-in type name.
fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "Int32" | "Int64" | "Float32" | "Float64" | "Bool" | "Unit"
    )
}

/// Human-readable display name for a `TypeAnnotation`.
fn type_annotation_display_name(ta: &TypeAnnotation) -> String {
    match ta {
        TypeAnnotation::I32 => "Int32".to_string(),
        TypeAnnotation::I64 => "Int64".to_string(),
        TypeAnnotation::F32 => "Float32".to_string(),
        TypeAnnotation::F64 => "Float64".to_string(),
        TypeAnnotation::Bool => "Bool".to_string(),
        TypeAnnotation::Unit => "Unit".to_string(),
        TypeAnnotation::Named(name) => name.clone(),
        TypeAnnotation::Generic { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(type_annotation_display_name).collect();
            format!("{}<{}>", name, arg_strs.join(", "))
        }
        TypeAnnotation::Array { element, size } => {
            format!("[{}; {}]", type_annotation_display_name(element), size)
        }
    }
}

fn resolve_type_checked(ty: &TypeAnnotation, resolver: &Resolver) -> Result<Type> {
    match ty {
        TypeAnnotation::Named(name) => {
            // Check if the name refers to a type parameter currently in scope
            if let Some(tp) = resolver.lookup_type_param(name) {
                return Ok(Type::TypeParam {
                    name: tp.name.clone(),
                    bound: tp.bound.clone(),
                });
            }
            if resolver.lookup_struct(name).is_none() {
                let help = find_suggestion(name, resolver.all_struct_names())
                    .map(|s| format!("did you mean '{s}'?"));
                return Err(sem_err_with_help(
                    format!("undefined type `{}`", name),
                    Span { start: 0, end: 0 },
                    help,
                ));
            }
            Ok(Type::Struct(name.clone()))
        }
        TypeAnnotation::Generic { name, args } => {
            // Resolve all type arguments
            let resolved_args = args
                .iter()
                .map(|a| resolve_type_checked(a, resolver))
                .collect::<Result<Vec<_>>>()?;
            Ok(Type::Generic {
                name: name.clone(),
                args: resolved_args,
            })
        }
        TypeAnnotation::Array { element, size } => {
            let resolved_elem = resolve_type_checked(element, resolver)?;
            Ok(Type::Array {
                element: Box::new(resolved_elem),
                size: *size,
            })
        }
        other => Ok(resolve_type(other)),
    }
}

/// Check that `declared` and `actual` types match, with a specific error for array size mismatch.
fn check_type_match(declared: &Type, actual: &Type) -> Result<()> {
    if actual == declared {
        return Ok(());
    }
    // Provide a specific error for array size mismatch
    if let (
        Type::Array {
            element: d_elem,
            size: d_size,
        },
        Type::Array {
            element: a_elem,
            size: a_size,
        },
    ) = (declared, actual)
        && d_elem == a_elem
        && d_size != a_size
    {
        return Err(sem_err(format!(
            "expected array of size {}, found array of size {}",
            d_size, a_size
        )));
    }
    Err(sem_err(format!(
        "type mismatch: expected `{}`, found `{}`",
        declared, actual
    )))
}

/// Check whether two types are compatible for type checking purposes.
/// A TypeParam is compatible with any type (checked after BIR-level monomorphization).
fn types_compatible(a: &Type, b: &Type) -> bool {
    if a == b {
        return true;
    }
    matches!(a, Type::TypeParam { .. }) || matches!(b, Type::TypeParam { .. })
}

/// Substitute type parameters in a type using the given mapping.
/// Type params not in the map are left as-is.
fn substitute_type(ty: &Type, subst: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TypeParam { name, .. } => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Generic { name, args } => Type::Generic {
            name: name.clone(),
            args: args.iter().map(|a| substitute_type(a, subst)).collect(),
        },
        Type::Array { element, size } => Type::Array {
            element: Box::new(substitute_type(element, subst)),
            size: *size,
        },
        other => other.clone(),
    }
}

fn resolve_struct_members(struct_def: &StructDef, resolver: &mut Resolver) -> Result<()> {
    let name = &struct_def.name;

    // Push type params into scope for struct member type resolution
    resolver.push_type_params(&struct_def.type_params);

    let mut fields: Vec<(String, Type)> = Vec::new();
    let mut field_index: HashMap<String, usize> = HashMap::new();
    let mut computed: Vec<resolver::ComputedPropInfo> = Vec::new();
    let mut computed_index: HashMap<String, usize> = HashMap::new();
    let mut methods: Vec<resolver::MethodInfo> = Vec::new();
    let mut method_index: HashMap<String, usize> = HashMap::new();
    let mut explicit_init: Option<&StructMember> = None;

    for member in &struct_def.members {
        match member {
            StructMember::StoredProperty {
                visibility: _,
                name: fname,
                ty,
            } => {
                if field_index.contains_key(fname) || computed_index.contains_key(fname) {
                    return Err(sem_err(format!(
                        "duplicate field `{}` in struct `{}`",
                        fname, name
                    )));
                }
                let resolved_ty = resolve_type_checked(ty, resolver)?;
                let idx = fields.len();
                fields.push((fname.clone(), resolved_ty));
                field_index.insert(fname.clone(), idx);
            }
            StructMember::ComputedProperty {
                visibility: _,
                name: pname,
                ty,
                getter,
                setter,
            } => {
                if field_index.contains_key(pname) || computed_index.contains_key(pname) {
                    return Err(sem_err(format!(
                        "duplicate field `{}` in struct `{}`",
                        pname, name
                    )));
                }
                let resolved_ty = resolve_type_checked(ty, resolver)?;
                let has_setter = setter.is_some();
                let idx = computed.len();
                computed.push(resolver::ComputedPropInfo {
                    name: pname.clone(),
                    ty: resolved_ty,
                    has_setter,
                    getter: getter.clone().unwrap(),
                    setter: setter.clone(),
                });
                computed_index.insert(pname.clone(), idx);
            }
            StructMember::Initializer { .. } => {
                if explicit_init.is_some() {
                    return Err(sem_err(format!(
                        "multiple initializers defined for struct `{}`",
                        name
                    )));
                }
                explicit_init = Some(member);
            }
            StructMember::Method {
                name: mname,
                params,
                return_type,
                ..
            } => {
                if field_index.contains_key(mname)
                    || computed_index.contains_key(mname)
                    || method_index.contains_key(mname)
                {
                    return Err(sem_err(format!(
                        "duplicate member `{}` in struct `{}`",
                        mname, name
                    )));
                }
                let resolved_params: Vec<(String, Type)> = params
                    .iter()
                    .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, resolver)?)))
                    .collect::<Result<Vec<_>>>()?;
                let resolved_return = resolve_type_checked(return_type, resolver)?;
                let idx = methods.len();
                methods.push(resolver::MethodInfo {
                    name: mname.clone(),
                    params: resolved_params,
                    return_type: resolved_return,
                });
                method_index.insert(mname.clone(), idx);
            }
        }
    }

    let init = match explicit_init {
        Some(StructMember::Initializer {
            visibility: _,
            params,
            body,
        }) => {
            let resolved_params: Vec<(String, Type)> = params
                .iter()
                .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, resolver)?)))
                .collect::<Result<Vec<_>>>()?;
            resolver::InitializerInfo {
                params: resolved_params,
                body: body.clone(),
            }
        }
        _ => {
            let params = fields.clone();
            resolver::InitializerInfo { params, body: None }
        }
    };

    resolver.pop_type_params(struct_def.type_params.len());

    resolver.define_struct(
        name.clone(),
        resolver::StructInfo {
            type_params: struct_def.type_params.clone(),
            conformances: struct_def.conformances.clone(),
            fields,
            field_index,
            computed,
            computed_index,
            init,
            methods,
            method_index,
        },
    );

    Ok(())
}

/// Check whether a statement guarantees all control-flow paths end with `return`.
fn stmt_always_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) => true,
        Stmt::Expr(expr) => match &expr.kind {
            ExprKind::If {
                then_block,
                else_block: Some(else_blk),
                ..
            } => block_always_returns(then_block) && block_always_returns(else_blk),
            _ => false,
        },
        _ => false,
    }
}

/// Check whether a block guarantees all control-flow paths end with `return`.
fn block_always_returns(block: &Block) -> bool {
    match block.stmts.last() {
        Some(stmt) => stmt_always_returns(stmt),
        None => false,
    }
}

pub(super) fn collect_visibilities(program: &Program) -> HashMap<String, Visibility> {
    let mut vis = HashMap::new();
    for f in &program.functions {
        vis.insert(f.name.clone(), f.visibility);
    }
    for s in &program.structs {
        vis.insert(s.name.clone(), s.visibility);
    }
    for p in &program.protocols {
        vis.insert(p.name.clone(), p.visibility);
    }
    vis
}

#[cfg(test)]
mod tests;
