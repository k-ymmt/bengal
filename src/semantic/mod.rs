pub mod infer;
pub mod resolver;
pub mod types;

mod generic_validation;
mod package_analysis;
mod post_mono;
mod pre_mono;
mod single_module_analysis;

pub use generic_validation::{validate_generics, validate_main};
pub use package_analysis::analyze_package;
pub use post_mono::analyze_post_mono;
pub use pre_mono::{analyze_pre_mono, analyze_pre_mono_lenient};
use single_module_analysis::analyze_single_module;

use std::collections::{HashMap, HashSet};

use crate::error::{BengalError, DiagCtxt, Result, Span};
use crate::package::ModulePath;
use crate::parser::ast::*;
use crate::suggest::find_suggestion;
use infer::{InferVarId, InferenceContext};
use resolver::{ProtocolInfo, Resolver, StructInfo, VarInfo};
use types::{Type, resolve_type};

#[derive(Debug)]
pub struct SemanticInfo {
    pub struct_defs: HashMap<String, StructInfo>,
    pub struct_init_calls: HashSet<NodeId>,
    pub protocols: HashMap<String, ProtocolInfo>,
}

#[derive(Debug)]
pub struct PackageSemanticInfo {
    pub module_infos: HashMap<ModulePath, SemanticInfo>,
    /// For each module, maps imported symbol names to their source module path.
    /// Key: (importing module, local symbol name) -> source module path.
    pub import_sources: HashMap<(ModulePath, String), ModulePath>,
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
                    getter: getter.clone(),
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
                body: Some(body.clone()),
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

fn analyze_function(
    func: &Function,
    resolver: &mut Resolver,
    ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) {
    // Push type params into scope for the duration of this function analysis
    resolver.push_type_params(&func.type_params);

    let return_type = match resolve_type_checked(&func.return_type, resolver) {
        Ok(t) => t,
        Err(e) => {
            diag.emit(e);
            resolver.pop_type_params(func.type_params.len());
            return;
        }
    };
    resolver.current_return_type = Some(return_type.clone());
    resolver.push_scope();

    // Register function parameters as immutable variables
    for param in &func.params {
        let param_ty = match resolve_type_checked(&param.ty, resolver) {
            Ok(t) => t,
            Err(e) => {
                diag.emit(e);
                Type::Error
            }
        };
        resolver.define_var(
            param.name.clone(),
            VarInfo {
                ty: param_ty,
                mutable: false,
            },
        );
    }

    let stmts = &func.body.stmts;

    // Check that all paths end with a return
    if !block_always_returns(&func.body) {
        diag.emit(sem_err(format!(
            "function `{}` must end with a `return` statement",
            func.name
        )));
        resolver.pop_scope();
        resolver.current_return_type = None;
        resolver.pop_type_params(func.type_params.len());
        return;
    }

    let mut ctx = ctx;
    for stmt in stmts.iter() {
        // Yield is not allowed in function bodies
        if matches!(stmt, Stmt::Yield(_)) {
            diag.emit(sem_err(
                "`yield` cannot be used in function body (use `return` instead)",
            ));
            resolver.pop_scope();
            resolver.current_return_type = None;
            resolver.pop_type_params(func.type_params.len());
            return;
        }

        analyze_stmt(stmt, resolver, ctx.as_deref_mut(), diag);
    }

    resolver.pop_scope();
    resolver.current_return_type = None;
    resolver.pop_type_params(func.type_params.len());
}

/// Analyze a block expression (Expr::Block) — yield required, return forbidden
fn analyze_block_expr(
    block: &Block,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Type {
    resolver.push_scope();

    let stmts = &block.stmts;

    if stmts.is_empty() {
        diag.emit(sem_err(
            "block expression must end with a `yield` statement",
        ));
        resolver.pop_scope();
        return Type::Error;
    }

    // Check that the last statement is Yield
    if !matches!(stmts.last(), Some(Stmt::Yield(_))) {
        diag.emit(sem_err(
            "block expression must end with a `yield` statement",
        ));
        resolver.pop_scope();
        return Type::Error;
    }

    let mut yield_type = Type::I32; // will be overwritten

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Return is not allowed in block expressions
        if matches!(stmt, Stmt::Return(_)) {
            diag.emit(sem_err("`return` cannot be used inside a block expression"));
            resolver.pop_scope();
            return Type::Error;
        }

        // Yield is only allowed as the last statement
        if matches!(stmt, Stmt::Yield(_)) && !is_last {
            diag.emit(sem_err(
                "`yield` must be the last statement in the block expression",
            ));
            resolver.pop_scope();
            return Type::Error;
        }

        analyze_stmt(stmt, resolver, ctx.as_deref_mut(), diag);

        // If this is the Yield statement, get the type
        if let Stmt::Yield(expr) = stmt {
            yield_type = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
        }
    }

    resolver.pop_scope();
    yield_type
}

/// Analyze a control block (if then/else) — yield and return both allowed.
/// Returns Some(type) if block yields a value, None if block diverges via return.
fn analyze_control_block(
    block: &Block,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Option<Type> {
    resolver.push_scope();

    let stmts = &block.stmts;

    if stmts.is_empty() {
        resolver.pop_scope();
        return Some(Type::Unit);
    }

    let mut result: Option<Type> = None;

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Yield is only allowed as the last statement
        if matches!(stmt, Stmt::Yield(_)) && !is_last {
            diag.emit(sem_err("`yield` must be the last statement in the block"));
            resolver.pop_scope();
            return Some(Type::Error);
        }

        analyze_stmt(stmt, resolver, ctx.as_deref_mut(), diag);

        if is_last {
            match stmt {
                Stmt::Yield(expr) => {
                    let ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
                    result = Some(ty);
                }
                Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue => {
                    // Block diverges (control flow exits)
                    result = None;
                }
                _ => {
                    result = Some(Type::Unit);
                }
            }
        }
    }

    resolver.pop_scope();
    result
}

/// Analyze a loop body block — return allowed, yield forbidden.
fn analyze_loop_block(
    block: &Block,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) {
    resolver.push_scope();

    for stmt in &block.stmts {
        if matches!(stmt, Stmt::Yield(_)) {
            diag.emit(sem_err("`yield` cannot be used in a while loop body"));
            resolver.pop_scope();
            return;
        }
        analyze_stmt(stmt, resolver, ctx.as_deref_mut(), diag);
    }

    resolver.pop_scope();
}

fn analyze_stmt(
    stmt: &Stmt,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) {
    match stmt {
        Stmt::Let { name, ty, value } => {
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            let var_ty = if val_ty == Type::Error {
                // Still register the variable with the declared type if available
                match ty {
                    Some(ann) => match resolve_type_checked(ann, resolver) {
                        Ok(declared) => declared,
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    },
                    None => Type::Error,
                }
            } else {
                match ty {
                    Some(ann) => match resolve_type_checked(ann, resolver) {
                        Ok(declared) => {
                            if let Some(ref mut c) = ctx {
                                if let Err(e) = c.unify(val_ty.clone(), declared.clone()) {
                                    diag.emit(e);
                                }
                            } else if let Err(e) = check_type_match(&declared, &val_ty) {
                                diag.emit(e);
                            }
                            declared
                        }
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    },
                    None => val_ty,
                }
            };
            resolver.define_var(
                name.clone(),
                VarInfo {
                    ty: var_ty,
                    mutable: false,
                },
            );
        }
        Stmt::Var { name, ty, value } => {
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            let var_ty = if val_ty == Type::Error {
                match ty {
                    Some(ann) => match resolve_type_checked(ann, resolver) {
                        Ok(declared) => declared,
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    },
                    None => Type::Error,
                }
            } else {
                match ty {
                    Some(ann) => match resolve_type_checked(ann, resolver) {
                        Ok(declared) => {
                            if let Some(ref mut c) = ctx {
                                if let Err(e) = c.unify(val_ty.clone(), declared.clone()) {
                                    diag.emit(e);
                                }
                            } else if let Err(e) = check_type_match(&declared, &val_ty) {
                                diag.emit(e);
                            }
                            declared
                        }
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    },
                    None => val_ty,
                }
            };
            resolver.define_var(
                name.clone(),
                VarInfo {
                    ty: var_ty,
                    mutable: true,
                },
            );
        }
        Stmt::Assign { name, value } => {
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            match resolver.lookup_var(name) {
                None => {
                    let help = find_suggestion(name, resolver.all_variable_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("undefined variable `{}`", name),
                        Span { start: 0, end: 0 },
                        help,
                    ));
                }
                Some(info) => {
                    if !info.mutable {
                        diag.emit(sem_err(format!(
                            "cannot assign to immutable variable `{}`",
                            name
                        )));
                        return;
                    }
                    if val_ty == Type::Error {
                        return;
                    }
                    let expected_ty = info.ty.clone();
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(val_ty.clone(), expected_ty) {
                            diag.emit(e);
                        }
                    } else if val_ty != expected_ty {
                        diag.emit(sem_err(format!(
                            "type mismatch in assignment: expected `{}`, found `{}`",
                            expected_ty, val_ty
                        )));
                    }
                }
            }
        }
        Stmt::Return(Some(expr)) => {
            let ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
            if ty == Type::Error {
                return;
            }
            if let Some(ref return_type) = resolver.current_return_type {
                if let Some(ref mut c) = ctx {
                    // In inference mode, unify return value with return type
                    // (but skip TypeParam since those are generic and will be checked later)
                    if !matches!(return_type, Type::TypeParam { .. })
                        && let Err(e) = c.unify(ty.clone(), return_type.clone())
                    {
                        diag.emit(e);
                    }
                } else if !types_compatible(&ty, return_type) {
                    diag.emit(sem_err(format!(
                        "return type mismatch: expected `{}`, found `{}`",
                        return_type, ty
                    )));
                }
            }
        }
        Stmt::Return(None) => {
            if let Some(ref return_type) = resolver.current_return_type
                && !types_compatible(&Type::Unit, return_type)
            {
                diag.emit(sem_err(format!(
                    "return type mismatch: expected `{}`, found `()`",
                    return_type
                )));
            }
        }
        Stmt::Yield(expr) => {
            let _ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
        }
        Stmt::Expr(expr) => {
            let _ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
        }
        Stmt::Break(opt_expr) => {
            if !resolver.in_loop() {
                diag.emit(sem_err("break outside of loop"));
                return;
            }
            let break_ty = match opt_expr {
                Some(expr) => {
                    let ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
                    if ty == Type::Error {
                        return;
                    }
                    ty
                }
                None => Type::Unit,
            };
            if let Some(ref mut c) = ctx {
                // In inference mode, unify with existing break type instead of equality check
                if let Err(e) = resolver.set_or_unify_break_type(break_ty, c) {
                    diag.emit(e);
                }
            } else if let Err(e) = resolver.set_break_type(break_ty) {
                diag.emit(e);
            }
        }
        Stmt::Continue => {
            if !resolver.in_loop() {
                diag.emit(sem_err("continue outside of loop"));
            }
        }
        Stmt::FieldAssign {
            object,
            field,
            value,
        } => {
            let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            if obj_ty == Type::Error || val_ty == Type::Error {
                return;
            }
            match &obj_ty {
                Type::Struct(struct_name) => {
                    let struct_info = match resolver.lookup_struct(struct_name) {
                        Some(s) => s.clone(),
                        None => {
                            let help = find_suggestion(struct_name, resolver.all_struct_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined struct `{}`", struct_name),
                                Span { start: 0, end: 0 },
                                help,
                            ));
                            return;
                        }
                    };
                    let field_ty = if let Some(&idx) = struct_info.field_index.get(field.as_str()) {
                        struct_info.fields[idx].1.clone()
                    } else if let Some(&idx) = struct_info.computed_index.get(field.as_str()) {
                        let prop = &struct_info.computed[idx];
                        if !prop.has_setter {
                            diag.emit(sem_err(format!(
                                "computed property `{}` is read-only (no setter)",
                                field
                            )));
                            return;
                        }
                        prop.ty.clone()
                    } else {
                        let help = find_suggestion(
                            field,
                            struct_info
                                .field_index
                                .keys()
                                .chain(struct_info.computed_index.keys())
                                .map(|s| s.as_str()),
                        )
                        .map(|s| format!("did you mean '{s}'?"));
                        diag.emit(sem_err_with_help(
                            format!("struct `{}` has no field `{}`", struct_name, field),
                            Span { start: 0, end: 0 },
                            help,
                        ));
                        return;
                    };
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(val_ty.clone(), field_ty) {
                            diag.emit(e);
                        }
                    } else if val_ty != field_ty {
                        diag.emit(sem_err(format!(
                            "type mismatch in field assignment: expected `{}`, found `{}`",
                            field_ty, val_ty
                        )));
                    }
                    if let Err(e) = check_assignment_target_mutable(object, resolver) {
                        diag.emit(e);
                    }
                }
                Type::Generic { name, args } => {
                    let struct_info = match resolver.lookup_struct(name) {
                        Some(s) => s.clone(),
                        None => {
                            let help = find_suggestion(name, resolver.all_struct_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined struct `{}`", name),
                                Span { start: 0, end: 0 },
                                help,
                            ));
                            return;
                        }
                    };
                    let subst: HashMap<String, Type> = struct_info
                        .type_params
                        .iter()
                        .zip(args.iter())
                        .map(|(tp, arg)| (tp.name.clone(), arg.clone()))
                        .collect();
                    let field_ty = if let Some(&idx) = struct_info.field_index.get(field.as_str()) {
                        substitute_type(&struct_info.fields[idx].1, &subst)
                    } else if let Some(&idx) = struct_info.computed_index.get(field.as_str()) {
                        let prop = &struct_info.computed[idx];
                        if !prop.has_setter {
                            diag.emit(sem_err(format!(
                                "computed property `{}` is read-only (no setter)",
                                field
                            )));
                            return;
                        }
                        substitute_type(&prop.ty, &subst)
                    } else {
                        let help = find_suggestion(
                            field,
                            struct_info
                                .field_index
                                .keys()
                                .chain(struct_info.computed_index.keys())
                                .map(|s| s.as_str()),
                        )
                        .map(|s| format!("did you mean '{s}'?"));
                        diag.emit(sem_err_with_help(
                            format!("struct `{}` has no field `{}`", name, field),
                            Span { start: 0, end: 0 },
                            help,
                        ));
                        return;
                    };
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(val_ty.clone(), field_ty) {
                            diag.emit(e);
                        }
                    } else if val_ty != field_ty {
                        diag.emit(sem_err(format!(
                            "type mismatch in field assignment: expected `{}`, found `{}`",
                            field_ty, val_ty
                        )));
                    }
                    if let Err(e) = check_assignment_target_mutable(object, resolver) {
                        diag.emit(e);
                    }
                }
                _ => {
                    diag.emit(sem_err(format!(
                        "field assignment on non-struct type `{}`",
                        obj_ty
                    )));
                }
            }
        }
        Stmt::IndexAssign {
            object,
            index,
            value,
        } => {
            let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
            let idx_ty = analyze_expr(index, resolver, ctx.as_deref_mut(), diag);
            let val_ty = analyze_expr(value, resolver, ctx.as_deref_mut(), diag);
            if obj_ty == Type::Error || idx_ty == Type::Error || val_ty == Type::Error {
                return;
            }
            match &obj_ty {
                Type::Array { element, size } => {
                    if !idx_ty.is_integer() {
                        diag.emit(sem_err(format!(
                            "array index must be an integer type, found '{}'",
                            idx_ty
                        )));
                        return;
                    }
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(val_ty.clone(), *element.clone()) {
                            diag.emit(e);
                        }
                    } else if val_ty != **element {
                        diag.emit(sem_err(format!(
                            "type mismatch in index assignment: expected '{}', found '{}'",
                            element, val_ty
                        )));
                    }
                    // Compile-time bounds check for constant indices
                    if let ExprKind::Number(n) = &index.kind {
                        let idx = *n;
                        if idx < 0 || idx as u64 >= *size {
                            diag.emit(sem_err(format!(
                                "array index {} is out of bounds for array of size {}",
                                idx, size
                            )));
                        }
                    }
                    // Check mutability: object must be a mutable variable
                    match &object.kind {
                        ExprKind::Ident(name) => match resolver.lookup_var(name) {
                            Some(info) if !info.mutable => {
                                diag.emit(sem_err(format!(
                                    "cannot assign to index of immutable variable '{}'",
                                    name
                                )));
                            }
                            Some(_) => {}
                            None => {
                                let help = find_suggestion(name, resolver.all_variable_names())
                                    .map(|s| format!("did you mean '{s}'?"));
                                diag.emit(sem_err_with_help(
                                    format!("undefined variable '{}'", name),
                                    Span { start: 0, end: 0 },
                                    help,
                                ));
                            }
                        },
                        _ => {
                            diag.emit(sem_err("cannot assign to index of non-variable expression"));
                        }
                    }
                }
                _ => {
                    diag.emit(sem_err(format!("cannot index into type '{}'", obj_ty)));
                }
            }
        }
    }
}

fn analyze_expr(
    expr: &Expr,
    resolver: &mut Resolver,
    mut ctx: Option<&mut InferenceContext>,
    diag: &mut DiagCtxt,
) -> Type {
    match &expr.kind {
        ExprKind::Number(n) => {
            if let Some(ref mut c) = ctx {
                // In inference mode, create an IntegerLiteral variable and defer
                // the range check until after the concrete type is resolved.
                let id = c.fresh_integer();
                c.register_int_range_check(id, *n);
                Type::IntegerLiteral(id)
            } else {
                if *n < i32::MIN as i64 || *n > i32::MAX as i64 {
                    diag.emit(sem_err(format!(
                        "integer literal `{}` is out of range for `Int32`",
                        n
                    )));
                    return Type::Error;
                }
                Type::I32
            }
        }
        ExprKind::Bool(_) => Type::Bool,
        ExprKind::Ident(name) => match resolver.lookup_var(name) {
            Some(info) => info.ty.clone(),
            None => {
                let help = find_suggestion(name, resolver.all_variable_names())
                    .map(|s| format!("did you mean '{s}'?"));
                diag.emit(sem_err_with_help(
                    format!("undefined variable `{}`", name),
                    expr.span,
                    help,
                ));
                Type::Error
            }
        },
        ExprKind::UnaryOp { op, operand } => {
            let operand_ty = analyze_expr(operand, resolver, ctx.as_deref_mut(), diag);
            if operand_ty == Type::Error {
                return Type::Error;
            }
            match op {
                UnaryOp::Not => {
                    if operand_ty != Type::Bool {
                        diag.emit(sem_err("operand of `!` must be `Bool`"));
                        return Type::Error;
                    }
                    Type::Bool
                }
            }
        }
        ExprKind::BinaryOp { op, left, right } => {
            let left_ty = analyze_expr(left, resolver, ctx.as_deref_mut(), diag);
            let right_ty = analyze_expr(right, resolver, ctx.as_deref_mut(), diag);
            if left_ty == Type::Error || right_ty == Type::Error {
                return Type::Error;
            }
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    if let Some(ref mut c) = ctx {
                        // In inference mode, unify left and right operands
                        if let Err(e) = c.unify(left_ty.clone(), right_ty.clone()) {
                            diag.emit(e);
                            return Type::Error;
                        }
                        left_ty
                    } else {
                        if !left_ty.is_numeric() || left_ty != right_ty {
                            diag.emit(sem_err(format!(
                                "arithmetic operation requires matching numeric operands, found `{}` and `{}`",
                                left_ty, right_ty
                            )));
                            return Type::Error;
                        }
                        left_ty
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    if let Some(ref mut c) = ctx {
                        if let Err(e) = c.unify(left_ty.clone(), right_ty.clone()) {
                            diag.emit(e);
                            return Type::Error;
                        }
                        Type::Bool
                    } else {
                        if !left_ty.is_numeric() || left_ty != right_ty {
                            diag.emit(sem_err(format!(
                                "comparison requires matching numeric operands, found `{}` and `{}`",
                                left_ty, right_ty
                            )));
                            return Type::Error;
                        }
                        Type::Bool
                    }
                }
                // Logical: bool x bool → bool
                BinOp::And | BinOp::Or => {
                    if left_ty != Type::Bool || right_ty != Type::Bool {
                        diag.emit(sem_err("logical operation requires `Bool` operands"));
                        return Type::Error;
                    }
                    Type::Bool
                }
            }
        }
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            // Empty-arg call may be a struct init
            if args.is_empty()
                && let Some(struct_info) = resolver.lookup_struct(name)
            {
                let struct_info = struct_info.clone();
                if struct_info.init.params.is_empty() {
                    resolver.record_struct_init_call(expr.id);
                    return Type::Struct(name.clone());
                } else {
                    diag.emit(sem_err(format!(
                        "struct `{}` initializer expects {} arguments, but 0 were given",
                        name,
                        struct_info.init.params.len()
                    )));
                    return Type::Error;
                }
            }
            let sig = match resolver.lookup_func(name) {
                Some(s) => s.clone(),
                None => {
                    let help = find_suggestion(name, resolver.all_function_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("undefined function `{}`", name),
                        expr.span,
                        help,
                    ));
                    return Type::Error;
                }
            };
            if args.len() != sig.params.len() {
                diag.emit(sem_err(format!(
                    "function `{}` expects {} arguments, but {} were given",
                    name,
                    sig.params.len(),
                    args.len()
                )));
                return Type::Error;
            }

            // Build type param substitution map
            let subst: HashMap<String, Type> = if !type_args.is_empty() {
                // Explicit type args provided
                let mut map = HashMap::new();
                let mut failed = false;
                for (tp, ta) in sig.type_params.iter().zip(type_args.iter()) {
                    match resolve_type_checked(ta, resolver) {
                        Ok(resolved) => {
                            map.insert(tp.name.clone(), resolved);
                        }
                        Err(e) => {
                            diag.emit(e);
                            failed = true;
                        }
                    }
                }
                if failed {
                    return Type::Error;
                }
                map
            } else if !sig.type_params.is_empty() {
                if let Some(ref mut c) = ctx {
                    // Inference mode: create InferVars for each type param
                    let var_ids: Vec<InferVarId> = sig
                        .type_params
                        .iter()
                        .map(|tp| {
                            c.fresh_var_with_provenance(infer::VarProvenance {
                                type_param_name: tp.name.clone(),
                                def_name: name.clone(),
                                arg_name: None,
                                span: expr.span,
                            })
                        })
                        .collect();
                    c.register_call_site(
                        expr.id,
                        var_ids.clone(),
                        sig.type_params.clone(),
                        name.clone(),
                    );
                    sig.type_params
                        .iter()
                        .zip(var_ids.iter())
                        .map(|(tp, &id)| (tp.name.clone(), Type::InferVar(id)))
                        .collect()
                } else {
                    HashMap::new()
                }
            } else {
                HashMap::new()
            };

            let mut any_arg_error = false;
            for (arg, (param_name, expected_ty)) in args.iter().zip(sig.params.iter()) {
                let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut(), diag);
                if arg_ty == Type::Error {
                    any_arg_error = true;
                    continue;
                }
                let effective_ty = substitute_type(expected_ty, &subst);
                if let Some(ref mut c) = ctx {
                    if let Type::InferVar(id) = &effective_ty {
                        c.update_arg_name(*id, param_name.clone());
                    }
                    if let Type::IntegerLiteral(id) | Type::FloatLiteral(id) = &arg_ty {
                        c.set_provenance(
                            *id,
                            infer::VarProvenance {
                                type_param_name: String::new(),
                                def_name: name.clone(),
                                arg_name: Some(param_name.clone()),
                                span: arg.span,
                            },
                        );
                    }
                    // In inference mode, unify arg type with expected parameter type
                    if let Err(e) = c.unify(arg_ty.clone(), effective_ty) {
                        diag.emit(e);
                        any_arg_error = true;
                    }
                } else if !types_compatible(&arg_ty, &effective_ty) {
                    diag.emit(sem_err(format!(
                        "argument type mismatch: expected `{}`, found `{}`",
                        effective_ty, arg_ty
                    )));
                    any_arg_error = true;
                }
            }
            if any_arg_error {
                return Type::Error;
            }
            substitute_type(&sig.return_type, &subst)
        }
        ExprKind::Block(block) => analyze_block_expr(block, resolver, ctx, diag),
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            let cond_ty = analyze_expr(condition, resolver, ctx.as_deref_mut(), diag);
            if cond_ty == Type::Error {
                return Type::Error;
            }
            if cond_ty != Type::Bool {
                diag.emit(sem_err("if condition must be `Bool`"));
                return Type::Error;
            }

            let then_ty = analyze_control_block(then_block, resolver, ctx.as_deref_mut(), diag);

            match else_block {
                Some(else_blk) => {
                    let else_ty =
                        analyze_control_block(else_blk, resolver, ctx.as_deref_mut(), diag);
                    // Type merging with divergence
                    match (then_ty, else_ty) {
                        (Some(t1), Some(t2)) => {
                            if t1 == Type::Error || t2 == Type::Error {
                                return Type::Error;
                            }
                            if let Some(ref mut c) = ctx {
                                if let Err(e) = c.unify(t1.clone(), t2.clone()) {
                                    diag.emit(e);
                                    return Type::Error;
                                }
                                t1
                            } else {
                                if t1 != t2 {
                                    diag.emit(sem_err(format!(
                                        "if/else branch type mismatch: `{}` vs `{}`",
                                        t1, t2
                                    )));
                                    return Type::Error;
                                }
                                t1
                            }
                        }
                        (None, Some(t)) => t, // then diverges, use else type
                        (Some(t), None) => t, // else diverges, use then type
                        (None, None) => Type::Unit, // both diverge
                    }
                }
                None => {
                    // if without else: type is Unit
                    if let Some(ref ty) = then_ty
                        && *ty != Type::Unit
                        && *ty != Type::Error
                    {
                        diag.emit(sem_err(
                            "if without else must have unit type (use `yield` in both branches for a value)",
                        ));
                        return Type::Error;
                    }
                    Type::Unit
                }
            }
        }
        ExprKind::While {
            condition,
            body,
            nobreak,
        } => {
            let cond_ty = analyze_expr(condition, resolver, ctx.as_deref_mut(), diag);
            if cond_ty == Type::Error {
                return Type::Error;
            }
            if cond_ty != Type::Bool {
                diag.emit(sem_err("while condition must be `Bool`"));
                return Type::Error;
            }
            let is_while_true = condition.kind == ExprKind::Bool(true);

            resolver.enter_loop();
            analyze_loop_block(body, resolver, ctx.as_deref_mut(), diag);
            let break_ty = resolver.exit_loop();

            let while_ty = break_ty.unwrap_or(Type::Unit);

            match (is_while_true, nobreak) {
                (true, Some(_)) => {
                    diag.emit(sem_err("`nobreak` is unreachable in `while true`"));
                    return Type::Error;
                }
                (false, None) if while_ty != Type::Unit => {
                    diag.emit(sem_err(
                        "`while` with non-unit break requires `nobreak` block",
                    ));
                    return Type::Error;
                }
                (false, Some(nobreak_block)) => {
                    let nobreak_ty =
                        analyze_control_block(nobreak_block, resolver, ctx.as_deref_mut(), diag);
                    if let Some(t) = nobreak_ty {
                        if t == Type::Error {
                            return Type::Error;
                        }
                        if let Some(ref mut c) = ctx {
                            if let Err(e) = c.unify(t.clone(), while_ty.clone()) {
                                diag.emit(e);
                                return Type::Error;
                            }
                        } else if t != while_ty {
                            diag.emit(sem_err(format!(
                                "nobreak type `{}` does not match while type `{}`",
                                t, while_ty
                            )));
                            return Type::Error;
                        }
                    }
                }
                _ => {}
            }

            while_ty
        }
        ExprKind::Float(_) => {
            if let Some(ref mut c) = ctx {
                Type::FloatLiteral(c.fresh_float())
            } else {
                Type::F64
            }
        }
        ExprKind::StructInit {
            name,
            type_args,
            args,
        } => {
            let struct_info = match resolver.lookup_struct(name) {
                Some(s) => s.clone(),
                None => {
                    let help = find_suggestion(name, resolver.all_struct_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("undefined struct `{}`", name),
                        Span { start: 0, end: 0 },
                        help,
                    ));
                    return Type::Error;
                }
            };
            let init = &struct_info.init;
            if args.len() != init.params.len() {
                diag.emit(sem_err(format!(
                    "struct `{}` initializer expects {} arguments, but {} were given",
                    name,
                    init.params.len(),
                    args.len()
                )));
                return Type::Error;
            }

            // Build type param substitution map
            let subst: HashMap<String, Type> = if !type_args.is_empty() {
                // Explicit type args provided
                let mut map = HashMap::new();
                let mut failed = false;
                for (tp, ta) in struct_info.type_params.iter().zip(type_args.iter()) {
                    match resolve_type_checked(ta, resolver) {
                        Ok(resolved) => {
                            map.insert(tp.name.clone(), resolved);
                        }
                        Err(e) => {
                            diag.emit(e);
                            failed = true;
                        }
                    }
                }
                if failed {
                    return Type::Error;
                }
                map
            } else if !struct_info.type_params.is_empty() {
                if let Some(ref mut c) = ctx {
                    // Inference mode: create InferVars for each type param
                    let var_ids: Vec<InferVarId> = struct_info
                        .type_params
                        .iter()
                        .map(|tp| {
                            c.fresh_var_with_provenance(infer::VarProvenance {
                                type_param_name: tp.name.clone(),
                                def_name: name.clone(),
                                arg_name: None,
                                span: expr.span,
                            })
                        })
                        .collect();
                    c.register_call_site(
                        expr.id,
                        var_ids.clone(),
                        struct_info.type_params.clone(),
                        name.clone(),
                    );
                    struct_info
                        .type_params
                        .iter()
                        .zip(var_ids.iter())
                        .map(|(tp, &id)| (tp.name.clone(), Type::InferVar(id)))
                        .collect()
                } else {
                    HashMap::new()
                }
            } else {
                HashMap::new()
            };

            let mut any_arg_error = false;
            for ((label, arg_expr), (param_name, param_ty)) in args.iter().zip(init.params.iter()) {
                if label != param_name {
                    diag.emit(sem_err(format!(
                        "expected argument label `{}`, found `{}`",
                        param_name, label
                    )));
                    any_arg_error = true;
                    continue;
                }
                let arg_ty = analyze_expr(arg_expr, resolver, ctx.as_deref_mut(), diag);
                if arg_ty == Type::Error {
                    any_arg_error = true;
                    continue;
                }
                let effective_ty = substitute_type(param_ty, &subst);
                if let Some(ref mut c) = ctx {
                    if let Type::InferVar(id) = &effective_ty {
                        c.update_arg_name(*id, param_name.clone());
                    }
                    if let Type::IntegerLiteral(id) | Type::FloatLiteral(id) = &arg_ty {
                        c.set_provenance(
                            *id,
                            infer::VarProvenance {
                                type_param_name: String::new(),
                                def_name: name.clone(),
                                arg_name: Some(param_name.clone()),
                                span: arg_expr.span,
                            },
                        );
                    }
                    if let Err(e) = c.unify(arg_ty.clone(), effective_ty) {
                        diag.emit(e);
                        any_arg_error = true;
                    }
                } else if !types_compatible(&arg_ty, &effective_ty) {
                    diag.emit(sem_err(format!(
                        "argument type mismatch: expected `{}`, found `{}`",
                        effective_ty, arg_ty
                    )));
                    any_arg_error = true;
                }
            }
            if any_arg_error {
                return Type::Error;
            }

            // Build the result type
            if subst.is_empty() && struct_info.type_params.is_empty() {
                Type::Struct(name.clone())
            } else if !subst.is_empty() {
                let args: Vec<Type> = struct_info
                    .type_params
                    .iter()
                    .map(|tp| {
                        subst
                            .get(&tp.name)
                            .cloned()
                            .unwrap_or_else(|| Type::TypeParam {
                                name: tp.name.clone(),
                                bound: tp.bound.clone(),
                            })
                    })
                    .collect();
                Type::Generic {
                    name: name.clone(),
                    args,
                }
            } else {
                Type::Struct(name.clone())
            }
        }
        ExprKind::FieldAccess { object, field } => {
            let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
            if obj_ty == Type::Error {
                return Type::Error;
            }
            match &obj_ty {
                Type::Struct(struct_name) => {
                    let struct_info = match resolver.lookup_struct(struct_name) {
                        Some(s) => s.clone(),
                        None => {
                            let help = find_suggestion(struct_name, resolver.all_struct_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined struct `{}`", struct_name),
                                Span { start: 0, end: 0 },
                                help,
                            ));
                            return Type::Error;
                        }
                    };
                    if let Some(&idx) = struct_info.field_index.get(field.as_str()) {
                        struct_info.fields[idx].1.clone()
                    } else if let Some(&idx) = struct_info.computed_index.get(field.as_str()) {
                        struct_info.computed[idx].ty.clone()
                    } else {
                        let help = find_suggestion(
                            field,
                            struct_info
                                .field_index
                                .keys()
                                .chain(struct_info.computed_index.keys())
                                .map(|s| s.as_str()),
                        )
                        .map(|s| format!("did you mean '{s}'?"));
                        diag.emit(sem_err_with_help(
                            format!("struct `{}` has no field `{}`", struct_name, field),
                            expr.span,
                            help,
                        ));
                        Type::Error
                    }
                }
                Type::Generic { name, args } => {
                    let struct_info = match resolver.lookup_struct(name) {
                        Some(s) => s.clone(),
                        None => {
                            let help = find_suggestion(name, resolver.all_struct_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined struct `{}`", name),
                                Span { start: 0, end: 0 },
                                help,
                            ));
                            return Type::Error;
                        }
                    };
                    let subst: HashMap<String, Type> = struct_info
                        .type_params
                        .iter()
                        .zip(args.iter())
                        .map(|(tp, arg)| (tp.name.clone(), arg.clone()))
                        .collect();
                    if let Some(&idx) = struct_info.field_index.get(field.as_str()) {
                        substitute_type(&struct_info.fields[idx].1, &subst)
                    } else if let Some(&idx) = struct_info.computed_index.get(field.as_str()) {
                        substitute_type(&struct_info.computed[idx].ty, &subst)
                    } else {
                        let help = find_suggestion(
                            field,
                            struct_info
                                .field_index
                                .keys()
                                .chain(struct_info.computed_index.keys())
                                .map(|s| s.as_str()),
                        )
                        .map(|s| format!("did you mean '{s}'?"));
                        diag.emit(sem_err_with_help(
                            format!("struct `{}` has no field `{}`", name, field),
                            expr.span,
                            help,
                        ));
                        Type::Error
                    }
                }
                _ => {
                    diag.emit(sem_err(format!(
                        "field access on non-struct type `{}`",
                        obj_ty
                    )));
                    Type::Error
                }
            }
        }
        ExprKind::SelfRef => match &resolver.self_context {
            Some(ctx) => Type::Struct(ctx.struct_name.clone()),
            None => {
                diag.emit(sem_err(
                    "`self` can only be used inside struct initializers, computed properties, or methods",
                ));
                Type::Error
            }
        },
        ExprKind::Cast { expr, target_type } => {
            let source_ty = analyze_expr(expr, resolver, ctx.as_deref_mut(), diag);
            if source_ty == Type::Error {
                return Type::Error;
            }
            let target_ty = match resolve_type_checked(target_type, resolver) {
                Ok(t) => t,
                Err(e) => {
                    diag.emit(e);
                    return Type::Error;
                }
            };
            if !source_ty.is_numeric() || !target_ty.is_numeric() {
                diag.emit(sem_err(format!(
                    "cannot cast `{}` to `{}`",
                    source_ty, target_ty
                )));
                return Type::Error;
            }
            target_ty
        }
        ExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
            if obj_ty == Type::Error {
                return Type::Error;
            }
            match &obj_ty {
                Type::Struct(struct_name) => {
                    let struct_info = match resolver.lookup_struct(struct_name) {
                        Some(s) => s.clone(),
                        None => {
                            let help = find_suggestion(struct_name, resolver.all_struct_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined struct `{}`", struct_name),
                                Span { start: 0, end: 0 },
                                help,
                            ));
                            return Type::Error;
                        }
                    };
                    let method_info = match struct_info.method_index.get(method.as_str()) {
                        Some(&idx) => struct_info.methods[idx].clone(),
                        None => {
                            let help = find_suggestion(
                                method,
                                struct_info.method_index.keys().map(|s| s.as_str()),
                            )
                            .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("type `{}` has no method `{}`", struct_name, method),
                                expr.span,
                                help,
                            ));
                            return Type::Error;
                        }
                    };
                    if args.len() != method_info.params.len() {
                        diag.emit(sem_err(format!(
                            "method `{}` expects {} argument(s) but {} were given",
                            method,
                            method_info.params.len(),
                            args.len()
                        )));
                        return Type::Error;
                    }
                    let mut any_arg_error = false;
                    for (arg, (param_name, param_ty)) in args.iter().zip(method_info.params.iter())
                    {
                        let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut(), diag);
                        if arg_ty == Type::Error {
                            any_arg_error = true;
                            continue;
                        }
                        if let Some(ref mut c) = ctx {
                            if !matches!(param_ty, Type::TypeParam { .. })
                                && let Err(e) = c.unify(arg_ty.clone(), param_ty.clone())
                            {
                                diag.emit(e);
                                any_arg_error = true;
                            }
                        } else if arg_ty != *param_ty {
                            diag.emit(sem_err(format!(
                                "expected `{}` but got `{}` in argument `{}` of method `{}`",
                                param_ty, arg_ty, param_name, method
                            )));
                            any_arg_error = true;
                        }
                    }
                    if any_arg_error {
                        return Type::Error;
                    }
                    method_info.return_type
                }
                Type::Generic {
                    name,
                    args: type_args,
                } => {
                    let struct_info = match resolver.lookup_struct(name) {
                        Some(s) => s.clone(),
                        None => {
                            let help = find_suggestion(name, resolver.all_struct_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined struct `{}`", name),
                                Span { start: 0, end: 0 },
                                help,
                            ));
                            return Type::Error;
                        }
                    };
                    // Build substitution map: type_param_name → actual type arg
                    let subst: HashMap<String, Type> = struct_info
                        .type_params
                        .iter()
                        .zip(type_args.iter())
                        .map(|(tp, arg)| (tp.name.clone(), arg.clone()))
                        .collect();
                    let method_info = match struct_info.method_index.get(method.as_str()) {
                        Some(&idx) => struct_info.methods[idx].clone(),
                        None => {
                            let help = find_suggestion(
                                method,
                                struct_info.method_index.keys().map(|s| s.as_str()),
                            )
                            .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("type `{}` has no method `{}`", name, method),
                                expr.span,
                                help,
                            ));
                            return Type::Error;
                        }
                    };
                    if args.len() != method_info.params.len() {
                        diag.emit(sem_err(format!(
                            "method `{}` expects {} argument(s) but {} were given",
                            method,
                            method_info.params.len(),
                            args.len()
                        )));
                        return Type::Error;
                    }
                    let mut any_arg_error = false;
                    for (arg, (param_name, param_ty)) in args.iter().zip(method_info.params.iter())
                    {
                        let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut(), diag);
                        if arg_ty == Type::Error {
                            any_arg_error = true;
                            continue;
                        }
                        let expected_ty = substitute_type(param_ty, &subst);
                        if let Some(ref mut c) = ctx {
                            if !matches!(expected_ty, Type::TypeParam { .. })
                                && let Err(e) = c.unify(arg_ty.clone(), expected_ty)
                            {
                                diag.emit(e);
                                any_arg_error = true;
                            }
                        } else if arg_ty != expected_ty {
                            diag.emit(sem_err(format!(
                                "expected `{}` but got `{}` in argument `{}` of method `{}`",
                                expected_ty, arg_ty, param_name, method
                            )));
                            any_arg_error = true;
                        }
                    }
                    if any_arg_error {
                        return Type::Error;
                    }
                    substitute_type(&method_info.return_type, &subst)
                }
                Type::TypeParam {
                    name: _,
                    bound: Some(proto),
                } => {
                    let proto_info = match resolver.lookup_protocol(proto) {
                        Some(p) => p.clone(),
                        None => {
                            let help = find_suggestion(proto, resolver.all_protocol_names())
                                .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("undefined protocol `{}`", proto),
                                expr.span,
                                help,
                            ));
                            return Type::Error;
                        }
                    };
                    let method_sig = match proto_info.methods.iter().find(|m| m.name == *method) {
                        Some(sig) => sig.clone(),
                        None => {
                            let help = find_suggestion(
                                method,
                                proto_info.methods.iter().map(|m| m.name.as_str()),
                            )
                            .map(|s| format!("did you mean '{s}'?"));
                            diag.emit(sem_err_with_help(
                                format!("protocol `{}` has no method `{}`", proto, method),
                                expr.span,
                                help,
                            ));
                            return Type::Error;
                        }
                    };
                    if args.len() != method_sig.params.len() {
                        diag.emit(sem_err(format!(
                            "method `{}` expects {} argument(s) but {} were given",
                            method,
                            method_sig.params.len(),
                            args.len()
                        )));
                        return Type::Error;
                    }
                    let mut any_arg_error = false;
                    for (arg, param) in args.iter().zip(method_sig.params.iter()) {
                        let arg_ty = analyze_expr(arg, resolver, ctx.as_deref_mut(), diag);
                        if arg_ty == Type::Error {
                            any_arg_error = true;
                            continue;
                        }
                        if let Some(ref mut c) = ctx {
                            if !matches!(param.1, Type::TypeParam { .. })
                                && let Err(e) = c.unify(arg_ty.clone(), param.1.clone())
                            {
                                diag.emit(e);
                                any_arg_error = true;
                            }
                        } else if arg_ty != param.1 {
                            diag.emit(sem_err(format!(
                                "expected `{}` but got `{}` in argument `{}` of method `{}`",
                                param.1, arg_ty, param.0, method
                            )));
                            any_arg_error = true;
                        }
                    }
                    if any_arg_error {
                        return Type::Error;
                    }
                    method_sig.return_type.clone()
                }
                Type::TypeParam { name, bound: None } => {
                    diag.emit(sem_err(format!(
                        "method call on unconstrained type parameter `{}`",
                        name
                    )));
                    Type::Error
                }
                _ => {
                    diag.emit(sem_err(format!(
                        "method call on non-struct type `{}`",
                        obj_ty
                    )));
                    Type::Error
                }
            }
        }
        ExprKind::ArrayLiteral { elements } => {
            if elements.is_empty() {
                diag.emit(sem_err("cannot infer type of empty array literal"));
                return Type::Error;
            }
            let first_ty = analyze_expr(&elements[0], resolver, ctx.as_deref_mut(), diag);
            if first_ty == Type::Error {
                return Type::Error;
            }
            let mut any_error = false;
            for elem in &elements[1..] {
                let elem_ty = analyze_expr(elem, resolver, ctx.as_deref_mut(), diag);
                if elem_ty == Type::Error {
                    any_error = true;
                    continue;
                }
                if let Some(ref mut c) = ctx {
                    if c.unify(elem_ty.clone(), first_ty.clone()).is_err() {
                        diag.emit(sem_err(format!(
                            "array elements must all have the same type: expected '{}', found '{}'",
                            first_ty, elem_ty
                        )));
                        any_error = true;
                    }
                } else if elem_ty != first_ty {
                    diag.emit(sem_err(format!(
                        "array elements must all have the same type: expected '{}', found '{}'",
                        first_ty, elem_ty
                    )));
                    any_error = true;
                }
            }
            if any_error {
                return Type::Error;
            }
            Type::Array {
                element: Box::new(first_ty),
                size: elements.len() as u64,
            }
        }
        ExprKind::IndexAccess { object, index } => {
            let obj_ty = analyze_expr(object, resolver, ctx.as_deref_mut(), diag);
            let idx_ty = analyze_expr(index, resolver, ctx, diag);
            if obj_ty == Type::Error || idx_ty == Type::Error {
                return Type::Error;
            }
            match &obj_ty {
                Type::Array { element, size } => {
                    if !idx_ty.is_integer() {
                        diag.emit(sem_err(format!(
                            "array index must be an integer type, found '{}'",
                            idx_ty
                        )));
                        return Type::Error;
                    }
                    // Compile-time bounds check for constant indices
                    if let ExprKind::Number(n) = &index.kind {
                        let idx = *n;
                        if idx < 0 || idx as u64 >= *size {
                            diag.emit(sem_err(format!(
                                "array index {} is out of bounds for array of size {}",
                                idx, size
                            )));
                            return Type::Error;
                        }
                    }
                    *element.clone()
                }
                _ => {
                    diag.emit(sem_err(format!("cannot index into type '{}'", obj_ty)));
                    Type::Error
                }
            }
        }
    }
}

fn analyze_struct_members(
    struct_def: &StructDef,
    resolver: &mut Resolver,
    ctx: &mut InferenceContext,
    diag: &mut DiagCtxt,
) {
    use resolver::SelfContext;

    for member in &struct_def.members {
        match member {
            StructMember::Initializer {
                visibility: _,
                params,
                body,
            } => {
                let prev_self = resolver.self_context.clone();
                resolver.self_context = Some(SelfContext {
                    struct_name: struct_def.name.clone(),
                    mutable: true,
                });
                let prev_return = resolver.current_return_type.clone();
                resolver.current_return_type = Some(Type::Unit);

                resolver.push_scope();
                for param in params {
                    let param_ty = match resolve_type_checked(&param.ty, resolver) {
                        Ok(t) => t,
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    };
                    resolver.define_var(
                        param.name.clone(),
                        VarInfo {
                            ty: param_ty,
                            mutable: false,
                        },
                    );
                }
                for stmt in &body.stmts {
                    analyze_stmt(stmt, resolver, Some(ctx), diag);
                }
                resolver.pop_scope();

                if let Err(e) = check_all_fields_initialized(&struct_def.name, body, resolver) {
                    diag.emit(e);
                }

                resolver.current_return_type = prev_return;
                resolver.self_context = prev_self;
            }
            StructMember::ComputedProperty {
                ty, getter, setter, ..
            } => {
                let resolved_ty = match resolve_type_checked(ty, resolver) {
                    Ok(t) => t,
                    Err(e) => {
                        diag.emit(e);
                        continue;
                    }
                };

                // Analyze getter
                {
                    let prev_self = resolver.self_context.clone();
                    resolver.self_context = Some(SelfContext {
                        struct_name: struct_def.name.clone(),
                        mutable: false,
                    });
                    let prev_return = resolver.current_return_type.clone();
                    resolver.current_return_type = Some(resolved_ty.clone());

                    resolver.push_scope();
                    analyze_getter_block(getter, resolver, ctx, diag);
                    resolver.pop_scope();

                    resolver.current_return_type = prev_return;
                    resolver.self_context = prev_self;
                }

                // Analyze setter
                if let Some(setter_block) = setter {
                    let prev_self = resolver.self_context.clone();
                    resolver.self_context = Some(SelfContext {
                        struct_name: struct_def.name.clone(),
                        mutable: true,
                    });
                    let prev_return = resolver.current_return_type.clone();
                    resolver.current_return_type = Some(Type::Unit);

                    resolver.push_scope();
                    resolver.define_var(
                        "newValue".to_string(),
                        VarInfo {
                            ty: resolved_ty.clone(),
                            mutable: false,
                        },
                    );
                    for stmt in &setter_block.stmts {
                        analyze_stmt(stmt, resolver, Some(ctx), diag);
                    }
                    resolver.pop_scope();

                    resolver.current_return_type = prev_return;
                    resolver.self_context = prev_self;
                }
            }
            StructMember::StoredProperty { .. } => {}
            StructMember::Method {
                visibility: _,
                name: mname,
                params,
                return_type,
                body,
            } => {
                let resolved_return = match resolve_type_checked(return_type, resolver) {
                    Ok(t) => t,
                    Err(e) => {
                        diag.emit(e);
                        continue;
                    }
                };
                let prev_self = resolver.self_context.clone();
                resolver.self_context = Some(SelfContext {
                    struct_name: struct_def.name.clone(),
                    mutable: false,
                });
                let prev_return = resolver.current_return_type.clone();
                resolver.current_return_type = Some(resolved_return);

                resolver.push_scope();
                for param in params {
                    let param_ty = match resolve_type_checked(&param.ty, resolver) {
                        Ok(t) => t,
                        Err(e) => {
                            diag.emit(e);
                            Type::Error
                        }
                    };
                    resolver.define_var(
                        param.name.clone(),
                        VarInfo {
                            ty: param_ty,
                            mutable: false,
                        },
                    );
                }

                if !block_always_returns(body) {
                    diag.emit(sem_err(format!(
                        "method `{}` must end with a `return` statement",
                        mname
                    )));
                    resolver.pop_scope();
                    resolver.current_return_type = prev_return;
                    resolver.self_context = prev_self;
                    continue;
                }
                let stmts = &body.stmts;
                for stmt in stmts {
                    if matches!(stmt, Stmt::Yield(_)) {
                        diag.emit(sem_err(
                            "`yield` cannot be used in method body (use `return` instead)",
                        ));
                        break;
                    }
                    analyze_stmt(stmt, resolver, Some(ctx), diag);
                }

                resolver.pop_scope();
                resolver.current_return_type = prev_return;
                resolver.self_context = prev_self;
            }
        }
    }
}

fn check_all_fields_initialized(
    struct_name: &str,
    body: &Block,
    resolver: &Resolver,
) -> Result<()> {
    let struct_info = resolver
        .lookup_struct(struct_name)
        .ok_or_else(|| {
            let help = find_suggestion(struct_name, resolver.all_struct_names())
                .map(|s| format!("did you mean '{s}'?"));
            sem_err_with_help(
                format!("undefined struct `{}`", struct_name),
                Span { start: 0, end: 0 },
                help,
            )
        })?
        .clone();

    let mut initialized: HashSet<String> = HashSet::new();
    for stmt in &body.stmts {
        if matches!(stmt, Stmt::Return(_)) {
            break;
        }
        if let Stmt::FieldAssign { object, field, .. } = stmt
            && matches!(object.kind, ExprKind::SelfRef)
        {
            initialized.insert(field.clone());
        }
    }

    for (field_name, _) in &struct_info.fields {
        if !initialized.contains(field_name) {
            return Err(sem_err(format!(
                "stored property `{}` not initialized in `{}` initializer",
                field_name, struct_name
            )));
        }
    }

    Ok(())
}

fn analyze_getter_block(
    block: &Block,
    resolver: &mut Resolver,
    ctx: &mut InferenceContext,
    diag: &mut DiagCtxt,
) {
    if !block_always_returns(block) {
        diag.emit(sem_err("getter must end with a `return` statement"));
        return;
    }
    for stmt in &block.stmts {
        analyze_stmt(stmt, resolver, Some(ctx), diag);
    }
}

fn check_assignment_target_mutable(expr: &Expr, resolver: &Resolver) -> Result<()> {
    match &expr.kind {
        ExprKind::Ident(name) => match resolver.lookup_var(name) {
            Some(info) if !info.mutable => Err(sem_err(format!(
                "cannot assign to field of immutable variable `{}`",
                name
            ))),
            Some(_) => Ok(()),
            None => {
                let help = find_suggestion(name, resolver.all_variable_names())
                    .map(|s| format!("did you mean '{s}'?"));
                Err(sem_err_with_help(
                    format!("undefined variable `{}`", name),
                    expr.span,
                    help,
                ))
            }
        },
        ExprKind::FieldAccess { object, .. } => check_assignment_target_mutable(object, resolver),
        ExprKind::SelfRef => match &resolver.self_context {
            Some(ctx) if ctx.mutable => Ok(()),
            _ => Err(sem_err("`self` is not mutable in this context")),
        },
        _ => Err(sem_err("invalid assignment target")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn analyze_str(input: &str) -> Result<SemanticInfo> {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        analyze_post_mono(&program)
    }

    // --- Phase 2 normal cases (maintained) ---

    #[test]
    fn ok_let_and_return() {
        assert!(analyze_str("func main() -> Int32 { let x: Int32 = 10; return x; }").is_ok());
    }

    #[test]
    fn ok_var_and_assign() {
        assert!(analyze_str("func main() -> Int32 { var x: Int32 = 1; x = 2; return x; }").is_ok());
    }

    #[test]
    fn ok_block_expr_yield() {
        assert!(
            analyze_str("func main() -> Int32 { let x: Int32 = { yield 10; }; return x; }").is_ok()
        );
    }

    // --- Phase 3 normal cases ---

    #[test]
    fn ok_if_else() {
        assert!(analyze_str(
            "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }"
        ).is_ok());
    }

    #[test]
    fn ok_while() {
        assert!(analyze_str("func main() -> Int32 { while false { }; return 0; }").is_ok());
    }

    #[test]
    fn ok_early_return() {
        assert!(
            analyze_str("func main() -> Int32 { if 1 < 2 { return 10; }; return 20; }").is_ok()
        );
    }

    #[test]
    fn ok_diverging_then() {
        assert!(analyze_str(
            "func main() -> Int32 { let x: Int32 = if true { return 1; } else { yield 2; }; return x; }"
        ).is_ok());
    }

    #[test]
    fn ok_diverging_else() {
        assert!(analyze_str(
            "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { return 2; }; return x; }"
        ).is_ok());
    }

    #[test]
    fn ok_unit_func() {
        assert!(
            analyze_str("func foo() { return; } func main() -> Int32 { foo(); return 0; }").is_ok()
        );
    }

    #[test]
    fn ok_bool_let() {
        assert!(analyze_str(
            "func main() -> Int32 { let b: Bool = true && false; if b { yield 1; } else { yield 0; }; return 0; }"
        ).is_ok());
    }

    // --- Phase 2 error cases (maintained) ---

    #[test]
    fn err_undefined_variable() {
        let err = analyze_str("func main() -> Int32 { return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_immutable_assign() {
        let err =
            analyze_str("func main() -> Int32 { let x: Int32 = 1; x = 2; return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_no_return() {
        let err = analyze_str("func main() -> Int32 { let x: Int32 = 1; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_no_yield_in_block() {
        let err =
            analyze_str("func main() -> Int32 { let x: Int32 = { let a: Int32 = 1; }; return x; }")
                .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_yield_in_function_body() {
        let err = analyze_str("func main() -> Int32 { yield 1; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_return_in_block_expr() {
        let err = analyze_str("func main() -> Int32 { let x: Int32 = { return 1; }; return x; }")
            .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_yield_not_last() {
        let err = analyze_str(
            "func main() -> Int32 { let x: Int32 = { yield 1; let y: Int32 = 2; }; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_undefined_function() {
        let err = analyze_str("func main() -> Int32 { return foo(1); }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_wrong_arg_count() {
        let err = analyze_str(
            "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(1); }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_no_main() {
        let err =
            analyze_str("func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_main_with_params() {
        let err = analyze_str("func main(x: Int32) -> Int32 { return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    // --- Phase 3 error cases ---

    #[test]
    fn err_if_non_bool_condition() {
        let err =
            analyze_str("func main() -> Int32 { if 1 { yield 1; } else { yield 2; }; return 0; }")
                .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_if_branch_type_mismatch() {
        let err = analyze_str(
            "func main() -> Int32 { if true { yield 1; } else { yield true; }; return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_while_non_bool_condition() {
        let err = analyze_str("func main() -> Int32 { while 1 { }; return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_yield_in_while() {
        let err =
            analyze_str("func main() -> Int32 { while true { yield 1; }; return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_let_type_mismatch_bool_to_i32() {
        let err =
            analyze_str("func main() -> Int32 { let x: Int32 = true; return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_let_type_mismatch_i32_to_bool() {
        let err = analyze_str("func main() -> Int32 { let x: Bool = 42; return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_assign_type_mismatch() {
        let err = analyze_str("func main() -> Int32 { var x: Int32 = 0; x = false; return x; }")
            .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    // --- Phase 4 normal cases ---

    #[test]
    fn ok_type_inference() {
        analyze_str("func main() -> Int32 { let x = 10; return x; }").unwrap();
    }

    #[test]
    fn ok_cast_i64() {
        analyze_str("func main() -> Int32 { let x: Int64 = 42 as Int64; return x as Int32; }")
            .unwrap();
    }

    #[test]
    fn ok_float_literal() {
        analyze_str("func main() -> Int32 { let x = 3.14; let y: Int32 = 0; return y; }").unwrap();
    }

    #[test]
    fn ok_break_in_if() {
        analyze_str(
            "func main() -> Int32 { var i: Int32 = 0; while i < 3 { if i == 1 { break; }; i = i + 1; }; return i; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_continue_in_if() {
        analyze_str(
            "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_break_in_if_else() {
        analyze_str(
            "func main() -> Int32 { var i: Int32 = 0; while i < 10 { let x: Int32 = if i == 5 { break; } else { yield i; }; i = i + 1; }; return i; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_i64_function() {
        analyze_str(
            "func add_i64(a: Int64, b: Int64) -> Int64 { return a + b; } func main() -> Int32 { return add_i64(1 as Int64, 2 as Int64) as Int32; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_while_true_break_value() {
        analyze_str("func main() -> Int32 { let x: Int32 = while true { break 10; }; return x; }")
            .unwrap();
    }

    #[test]
    fn ok_while_true_break_unit() {
        analyze_str("func main() -> Int32 { while true { break; }; return 42; }").unwrap();
    }

    #[test]
    fn ok_while_cond_nobreak() {
        analyze_str(
            "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { if i == 5 { break 99; }; i = i + 1; } nobreak { yield 0; }; return x; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_while_cond_unit_nobreak() {
        analyze_str(
            "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_i32_max() {
        analyze_str("func main() -> Int32 { let x = 2147483647; return x; }").unwrap();
    }

    // --- Phase 4 error cases ---

    #[test]
    fn err_break_outside_loop() {
        let err = analyze_str("func main() -> Int32 { break; return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_continue_outside_loop() {
        let err = analyze_str("func main() -> Int32 { continue; return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_nobreak_in_while_true() {
        let err = analyze_str(
            "func main() -> Int32 { let x: Int32 = while true { break 10; } nobreak { yield 20; }; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_non_unit_break_without_nobreak() {
        let err = analyze_str(
            "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; }; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_nobreak_type_mismatch() {
        let err = analyze_str(
            "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { break 1; } nobreak { yield true; }; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_cast_type_mismatch() {
        let err = analyze_str("func main() -> Int32 { let x: Int32 = 42 as Int64; return x; }")
            .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_arithmetic_type_mismatch() {
        let err = analyze_str(
            "func main() -> Int32 { let x: Int32 = 1; let y: Int64 = 2 as Int64; return x + y; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_cast_bool() {
        let err = analyze_str(
            "func main() -> Int32 { let b: Bool = true; let x = b as Int32; return x; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_float_to_i32() {
        let err =
            analyze_str("func main() -> Int32 { let x: Int32 = 3.14; return x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_integer_out_of_range() {
        let err =
            analyze_str("func main() -> Int32 { let x = 3000000000; return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_integer_out_of_range_with_cast() {
        let err = analyze_str("func main() -> Int32 { return 3000000000 as Int64; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    // --- Struct tests ---

    #[test]
    fn ok_struct_basic() {
        analyze_str(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1, y: 2); return p.x; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_struct_field_assign() {
        analyze_str(
            "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); p.x = 10; return p.x; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_struct_explicit_init() {
        analyze_str(
            "struct Foo { var x: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { var f = Foo(val: 42); return f.x; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_struct_computed_getter() {
        analyze_str(
            "struct Foo { var x: Int32; var double: Int32 { get { return self.x; } }; } func main() -> Int32 { var f = Foo(x: 1); return f.double; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_struct_computed_setter() {
        analyze_str(
            "struct Foo { var x: Int32; var bar: Int32 { get { return 0; } set { self.x = newValue; } }; } func main() -> Int32 { var f = Foo(x: 1); f.bar = 10; return f.x; }",
        )
        .unwrap();
    }

    #[test]
    fn ok_struct_empty_init() {
        analyze_str("struct Empty { } func main() -> Int32 { var e = Empty(); return 0; }")
            .unwrap();
    }

    #[test]
    fn err_undefined_struct() {
        let err = analyze_str("func main() -> Int32 { var f = Foo(x: 1); return 0; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_duplicate_field() {
        let err = analyze_str(
            "struct Foo { var x: Int32; var x: Int32; } func main() -> Int32 { return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_multiple_init() {
        let err = analyze_str(
            "struct Foo { var x: Int32; init(x: Int32) { self.x = x; } init(y: Int32) { self.x = y; } } func main() -> Int32 { return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_init_arg_label_mismatch() {
        let err = analyze_str(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(a: 1, b: 2); return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_init_arg_type_mismatch() {
        let err = analyze_str(
            "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: true); return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_init_arg_count_mismatch() {
        let err = analyze_str(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1); return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_no_such_field() {
        let err = analyze_str(
            "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); return p.y; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_field_access_on_non_struct() {
        let err =
            analyze_str("func main() -> Int32 { let x: Int32 = 1; return x.y; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_immutable_field_assign() {
        let err = analyze_str(
            "struct Point { var x: Int32; } func main() -> Int32 { let p = Point(x: 1); p.x = 10; return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_readonly_computed_assign() {
        let err = analyze_str(
            "struct Foo { var bar: Int32 { get { return 0; } }; } func main() -> Int32 { var f = Foo(); f.bar = 10; return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_self_outside_struct() {
        let err = analyze_str("func main() -> Int32 { return self.x; }").unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_duplicate_definition_struct_func() {
        let err = analyze_str(
            "struct Foo { var x: Int32; } func Foo() -> Int32 { return 0; } func main() -> Int32 { return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_memberwise_unavailable_with_explicit_init() {
        let err = analyze_str(
            "struct Foo { var x: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { var f = Foo(x: 1); return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn err_init_missing_field_initialization() {
        let err = analyze_str(
            "struct Foo { var x: Int32; var y: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { return 0; }",
        )
        .unwrap_err();
        assert!(matches!(err, BengalError::SemanticError { .. }));
    }

    #[test]
    fn multiple_errors_reported() {
        // Source with errors in two separate functions
        let source = r#"
            func foo() -> Int32 { return true; }
            func bar() -> Int32 { return true; }
            func main() -> Int32 { return 1; }
        "#;
        let tokens = crate::lexer::tokenize(source).unwrap();
        let program = crate::parser::parse(tokens).unwrap();
        let mut resolver = Resolver::new();
        let mut diag = DiagCtxt::new();

        let result = analyze_single_module(&program, &mut resolver, true, &mut diag);
        assert!(result.is_err(), "expected error due to type mismatches");
        // Both foo and bar have type errors — previously only foo's was reported
        assert!(
            diag.error_count() >= 2,
            "expected at least 2 errors, got {}",
            diag.error_count()
        );
    }

    #[test]
    fn multiple_errors_in_single_function() {
        // Source with multiple type errors within one function body
        let source = r#"
            func main() -> Int32 {
                let x: Int32 = true;
                let y: Bool = 42;
                return 0;
            }
        "#;
        let tokens = crate::lexer::tokenize(source).unwrap();
        let program = crate::parser::parse(tokens).unwrap();
        let mut resolver = Resolver::new();
        let mut diag = DiagCtxt::new();

        let result = analyze_single_module(&program, &mut resolver, true, &mut diag);
        assert!(result.is_err(), "expected error due to type mismatches");
        // Both let bindings have type errors — both should be reported
        assert!(
            diag.error_count() >= 2,
            "expected at least 2 errors, got {}",
            diag.error_count()
        );
    }
}

#[cfg(test)]
mod module_tests {
    use super::*;
    use crate::package::build_module_graph;
    use std::fs;
    use tempfile::TempDir;

    fn analyze_test_package(files: &[(&str, &str)]) -> Result<PackageSemanticInfo> {
        let dir = TempDir::new().unwrap();
        for (path, source) in files {
            let full_path = dir.path().join(path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full_path, source).unwrap();
        }
        let entry = dir.path().join(files[0].0);
        let graph = build_module_graph(&entry)?;
        let mut diag = DiagCtxt::new();
        let result = analyze_package(&graph, "test_pkg", &mut diag);
        if result.is_err() {
            // Return the first real error from diag instead of the sentinel
            let errors = diag.take_errors();
            if let Some(first) = errors.into_iter().next() {
                return Err(first);
            }
        }
        result
    }

    #[test]
    fn cross_module_function_import() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::add; func main() -> Int32 { return add(1, 2); }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn visibility_violation_internal() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::helper; func main() -> Int32 { return helper(); }",
            ),
            ("math.bengal", "func helper() -> Int32 { return 1; }"),
        ]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("cannot"),
            "expected 'cannot' in error: {}",
            msg
        );
    }

    #[test]
    fn glob_import() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::*; func main() -> Int32 { return add(1, 2); }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn cross_module_struct_import() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module shapes; import shapes::Point; func main() -> Int32 { let p = Point(x: 3, y: 4); return p.x; }",
            ),
            (
                "shapes.bengal",
                "public struct Point { public var x: Int32; public var y: Int32; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn glob_import_skips_internal() {
        // Internal symbols should NOT be imported by glob
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::*; func main() -> Int32 { return secret(); }",
            ),
            (
                "math.bengal",
                "func secret() -> Int32 { return 42; } public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("undefined function") || msg.contains("secret"),
            "expected undefined function error, got: {}",
            msg
        );
    }

    #[test]
    fn group_import() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::{add, sub}; func main() -> Int32 { return add(1, sub(3, 1)); }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; } public func sub(a: Int32, b: Int32) -> Int32 { return a - b; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn unresolved_import_symbol() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::nonexistent; func main() -> Int32 { return 0; }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("nonexistent"),
            "expected error about 'nonexistent', got: {}",
            msg
        );
    }

    #[test]
    fn package_visibility_accessible() {
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::helper; func main() -> Int32 { return helper(); }",
            ),
            (
                "math.bengal",
                "package func helper() -> Int32 { return 42; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn non_root_module_no_main_required() {
        // Child modules should not require a main function
        let result = analyze_test_package(&[
            (
                "main.bengal",
                "module math; import math::add; func main() -> Int32 { return add(1, 2); }",
            ),
            (
                "math.bengal",
                "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
            ),
        ]);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        // Verify the graph has 2 modules
        let info = result.unwrap();
        assert_eq!(info.module_infos.len(), 2);
    }

    #[test]
    fn super_at_root_is_error() {
        let result = analyze_test_package(&[(
            "main.bengal",
            "import super::foo; func main() -> Int32 { return 0; }",
        )]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("super"),
            "expected error about 'super', got: {}",
            msg
        );
    }

    #[test]
    fn unresolved_module_in_import() {
        let result = analyze_test_package(&[(
            "main.bengal",
            "import nonexistent::foo; func main() -> Int32 { return 0; }",
        )]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found") || msg.contains("nonexistent"),
            "expected error about unresolved module, got: {}",
            msg
        );
    }
}
