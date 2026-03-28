use std::collections::HashMap;

use crate::error::{BengalError, Result, Span};
use crate::parser::ast::*;
use crate::suggest::find_suggestion;

use super::infer::{InferenceContext, InferredTypeArgs};
use super::resolver::{self, FuncSig, Resolver};
use super::types::Type;
use super::{
    DiagCtxt, SemanticInfo, analyze_function, analyze_struct_members, collect_visibilities,
    is_builtin_type, resolve_struct_members, resolve_type_checked, sem_err, sem_err_with_help,
    type_annotation_display_name,
};

/// Pre-mono analysis pass (runs before BIR lowering and BIR-level monomorphization).
///
/// Runs the same setup phases as `analyze_post_mono` (register symbols, resolve
/// types, validate main) and then analyzes function/struct bodies. After each
/// body, it calls `apply_defaults` and `record_inferred_type_args` on the
/// `InferenceContext`. For the initial implementation the context is created but
/// not yet used by analyze_expr/analyze_stmt (that comes in Task 8), so the
/// returned `InferredTypeArgs` will always be empty.
pub fn analyze_pre_mono(program: &Program) -> Result<(InferredTypeArgs, SemanticInfo)> {
    analyze_pre_mono_inner(program, false)
}

/// Lenient variant of analyze_pre_mono for per-module analysis in the package pipeline.
/// Swallows non-inference errors that may be caused by cross-module imports.
pub fn analyze_pre_mono_lenient(program: &Program) -> Result<(InferredTypeArgs, SemanticInfo)> {
    analyze_pre_mono_inner(program, true)
}

fn analyze_pre_mono_inner(
    program: &Program,
    lenient: bool,
) -> Result<(InferredTypeArgs, SemanticInfo)> {
    let mut inferred = InferredTypeArgs::new();
    let mut ctx = InferenceContext::new();
    let mut resolver = Resolver::new();

    // --- Phase 1a: register struct / protocol / function names ---

    for struct_def in &program.structs {
        if resolver.lookup_func(&struct_def.name).is_some()
            || resolver.lookup_struct(&struct_def.name).is_some()
        {
            return Err(sem_err(format!(
                "duplicate definition `{}`",
                struct_def.name
            )));
        }
        resolver.reserve_struct(struct_def.name.clone());
    }
    for proto in &program.protocols {
        if resolver.lookup_struct(&proto.name).is_some()
            || resolver.lookup_func(&proto.name).is_some()
            || resolver.lookup_protocol(&proto.name).is_some()
        {
            return Err(sem_err(format!("duplicate definition `{}`", proto.name)));
        }
        resolver.define_protocol(
            proto.name.clone(),
            resolver::ProtocolInfo {
                name: proto.name.clone(),
                methods: vec![],
                properties: vec![],
            },
        );
    }

    for func in &program.functions {
        if resolver.lookup_struct(&func.name).is_some()
            || resolver.lookup_func(&func.name).is_some()
        {
            return Err(sem_err(format!("duplicate definition `{}`", func.name)));
        }
        resolver.push_type_params(&func.type_params);
        let params: Vec<(String, Type)> = func
            .params
            .iter()
            .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, &resolver)?)))
            .collect::<Result<Vec<_>>>()?;
        let return_type = resolve_type_checked(&func.return_type, &resolver)?;
        resolver.pop_type_params(func.type_params.len());
        resolver.define_func(
            func.name.clone(),
            FuncSig {
                type_params: func.type_params.clone(),
                params,
                return_type,
            },
        );
    }

    // --- Phase 1b: resolve struct member types ---

    for struct_def in &program.structs {
        resolve_struct_members(struct_def, &mut resolver)?;
    }

    for struct_def in &program.structs {
        if let Some(struct_info) = resolver.lookup_struct(&struct_def.name) {
            let struct_info = struct_info.clone();
            for method in &struct_info.methods {
                let mangled = format!("{}_{}", struct_def.name, method.name);
                if resolver.lookup_func(&mangled).is_some() {
                    return Err(sem_err(format!(
                        "function `{}` conflicts with method `{}.{}`",
                        mangled, struct_def.name, method.name
                    )));
                }
            }
        }
    }

    // Phase 1b (continued): resolve protocol member types
    for proto in &program.protocols {
        let mut methods = Vec::new();
        let mut properties = Vec::new();
        for member in &proto.members {
            match member {
                ProtocolMember::MethodSig {
                    name,
                    params,
                    return_type,
                } => {
                    let resolved_params: Vec<(String, Type)> = params
                        .iter()
                        .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, &resolver)?)))
                        .collect::<Result<Vec<_>>>()?;
                    let resolved_return = resolve_type_checked(return_type, &resolver)?;
                    methods.push(resolver::ProtocolMethodSig {
                        name: name.clone(),
                        params: resolved_params,
                        return_type: resolved_return,
                    });
                }
                ProtocolMember::PropertyReq {
                    name,
                    ty,
                    has_setter,
                } => {
                    let resolved_ty = resolve_type_checked(ty, &resolver)?;
                    properties.push(resolver::ProtocolPropertyReq {
                        name: name.clone(),
                        ty: resolved_ty,
                        has_setter: *has_setter,
                    });
                }
            }
        }
        resolver.define_protocol(
            proto.name.clone(),
            resolver::ProtocolInfo {
                name: proto.name.clone(),
                methods,
                properties,
            },
        );
    }

    let mut all_errors: Vec<BengalError> = Vec::new();

    // --- Phase 2b: analyze struct member bodies (init, methods, computed) ---
    // Skip generic structs — their member bodies contain unsubstituted type
    // parameters that would cause spurious errors.
    {
        let mut struct_ctx = InferenceContext::new();
        let mut struct_diag = DiagCtxt::new();
        for struct_def in &program.structs {
            if !struct_def.type_params.is_empty() {
                continue;
            }
            analyze_struct_members(struct_def, &mut resolver, &mut struct_ctx, &mut struct_diag);
            let errs = struct_ctx.apply_defaults();
            for e in errs {
                struct_diag.emit(e);
            }
            struct_ctx.reset();
        }
        all_errors.extend(struct_diag.take_errors());
    }

    // --- Phase 2c: check protocol conformance ---
    for struct_def in &program.structs {
        for proto_name in &struct_def.conformances {
            let proto_info = match resolver.lookup_protocol(proto_name) {
                Some(info) => info.clone(),
                None if lenient => {
                    // In lenient mode, skip conformance checks for unknown
                    // protocols — they may come from cross-module imports
                    // that aren't resolved until package-level analysis.
                    continue;
                }
                None => {
                    let help = find_suggestion(proto_name, resolver.all_protocol_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    return Err(sem_err_with_help(
                        format!("unknown protocol `{}`", proto_name),
                        Span { start: 0, end: 0 },
                        help,
                    ));
                }
            };
            let struct_info = resolver
                .lookup_struct(&struct_def.name)
                .ok_or_else(|| {
                    let help = find_suggestion(&struct_def.name, resolver.all_struct_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    sem_err_with_help(
                        format!("undefined struct `{}`", struct_def.name),
                        Span { start: 0, end: 0 },
                        help,
                    )
                })?
                .clone();

            // Check methods
            for req_method in &proto_info.methods {
                match struct_info.method_index.get(&req_method.name) {
                    Some(&idx) => {
                        let impl_method = &struct_info.methods[idx];
                        if impl_method.params.len() != req_method.params.len() {
                            return Err(sem_err(format!(
                                "method `{}` expects {} parameter(s) but protocol `{}` requires {}",
                                req_method.name,
                                impl_method.params.len(),
                                proto_name,
                                req_method.params.len()
                            )));
                        }
                        for ((impl_name, impl_ty), (req_name, req_ty)) in
                            impl_method.params.iter().zip(req_method.params.iter())
                        {
                            if impl_ty != req_ty {
                                return Err(sem_err(format!(
                                    "method `{}` has parameter `{}` of type `{}` but protocol `{}` requires `{}`",
                                    req_method.name, impl_name, impl_ty, proto_name, req_ty
                                )));
                            }
                            if impl_name != req_name {
                                return Err(sem_err(format!(
                                    "method `{}` has parameter `{}` but protocol `{}` requires `{}`",
                                    req_method.name, impl_name, proto_name, req_name
                                )));
                            }
                        }
                        if impl_method.return_type != req_method.return_type {
                            return Err(sem_err(format!(
                                "method `{}` has return type `{}` but protocol `{}` requires `{}`",
                                req_method.name,
                                impl_method.return_type,
                                proto_name,
                                req_method.return_type
                            )));
                        }
                    }
                    None => {
                        return Err(sem_err(format!(
                            "type `{}` does not implement method `{}` required by protocol `{}`",
                            struct_def.name, req_method.name, proto_name
                        )));
                    }
                }
            }

            // Check properties
            for req_prop in &proto_info.properties {
                if let Some(&idx) = struct_info.field_index.get(&req_prop.name) {
                    let (_, field_ty) = &struct_info.fields[idx];
                    if *field_ty != req_prop.ty {
                        return Err(sem_err(format!(
                            "property `{}` has type `{}` but protocol `{}` requires `{}`",
                            req_prop.name, field_ty, proto_name, req_prop.ty
                        )));
                    }
                    continue;
                }
                if let Some(&idx) = struct_info.computed_index.get(&req_prop.name) {
                    let computed = &struct_info.computed[idx];
                    if computed.ty != req_prop.ty {
                        return Err(sem_err(format!(
                            "property `{}` has type `{}` but protocol `{}` requires `{}`",
                            req_prop.name, computed.ty, proto_name, req_prop.ty
                        )));
                    }
                    if req_prop.has_setter && !computed.has_setter {
                        return Err(sem_err(format!(
                            "property `{}` requires a setter to conform to protocol `{}`",
                            req_prop.name, proto_name
                        )));
                    }
                    continue;
                }
                return Err(sem_err(format!(
                    "type `{}` does not implement property `{}` required by protocol `{}`",
                    struct_def.name, req_prop.name, proto_name
                )));
            }
        }
    }

    // --- Phase 3: analyze function bodies with inference ---
    //
    // For each non-generic function, analyze the body with the InferenceContext
    // so that numeric literal types can be inferred from context. Generic
    // Generic functions are skipped because their signatures contain unsubstituted
    // type parameters that would cause spurious errors; they are checked at the
    // BIR level after monomorphization resolves them to concrete types.

    for func in &program.functions {
        // Skip generic functions — checked after BIR-level monomorphization
        if !func.type_params.is_empty() {
            continue;
        }

        let mut func_diag = DiagCtxt::new();
        analyze_function(func, &mut resolver, Some(&mut ctx), &mut func_diag);
        let func_errors = func_diag.take_errors();

        if func_errors.is_empty() {
            let default_errors = ctx.apply_defaults();
            if default_errors.is_empty() {
                ctx.record_inferred_type_args(&mut inferred);
            } else {
                all_errors.extend(default_errors);
            }
        } else if lenient {
            // In lenient mode, only propagate type-inference errors.
            // Other errors (e.g. undefined symbols) may be caused by
            // cross-module imports that aren't yet available.
            for e in func_errors {
                let msg = e.to_string();
                if msg.contains("conflicting constraints")
                    || msg.contains("cannot unify")
                    || msg.contains("cannot infer type parameter")
                {
                    all_errors.push(e);
                }
            }
        } else {
            all_errors.extend(func_errors);
        }
        ctx.reset();
    }

    if let Err(e) = validate_inferred_constraints(&inferred, program) {
        all_errors.push(e);
    }

    if !all_errors.is_empty() {
        return Err(all_errors.remove(0));
    }

    let sem_info = SemanticInfo {
        struct_defs: resolver.take_struct_defs(),
        struct_init_calls: resolver.take_struct_init_calls(),
        protocols: resolver.take_protocols(),
        functions: resolver.take_functions(),
        visibilities: collect_visibilities(program),
    };
    Ok((inferred, sem_info))
}

/// Check that inferred type arguments satisfy protocol constraints.
///
/// This is Stage 2 of constraint validation. Stage 1 (`validate_generics`)
/// checks explicit type args at call sites. Stage 2 checks inferred type args
/// that were resolved during `analyze_pre_mono`. Stage 3 (constraint checking
/// after BIR-level monomorphization substitutes TypeParam args into concrete
/// types) is handled implicitly: any constraint violation will surface as a
/// type mismatch when the protocol method is called on a non-conforming type.
fn validate_inferred_constraints(inferred: &InferredTypeArgs, program: &Program) -> Result<()> {
    let struct_map: HashMap<String, &StructDef> = program
        .structs
        .iter()
        .map(|s| (s.name.clone(), s))
        .collect();

    for site in inferred.map.values() {
        for (param, arg) in site.type_params.iter().zip(&site.type_args) {
            // Skip args that are still TypeParam references (deferred to Stage 3 / post-mono)
            if let TypeAnnotation::Named(name) = arg
                && !struct_map.contains_key(name)
                && !is_builtin_type(name)
            {
                continue;
            }

            if let Some(ref bound) = param.bound {
                let arg_name = match arg {
                    TypeAnnotation::Named(name) => name.as_str(),
                    TypeAnnotation::Generic { name, .. } => name.as_str(),
                    // Primitives cannot conform to protocols (for now)
                    _ => {
                        let type_name = type_annotation_display_name(arg);
                        return Err(sem_err(format!(
                            "type `{}` does not conform to protocol `{}`",
                            type_name, bound
                        )));
                    }
                };
                if let Some(struct_def) = struct_map.get(arg_name) {
                    if !struct_def.conformances.contains(bound) {
                        return Err(sem_err(format!(
                            "type `{}` does not conform to protocol `{}` (required by {} type parameter `{}`)",
                            arg_name, bound, site.def_name, param.name
                        )));
                    }
                } else {
                    // Not a known struct — builtins / primitives don't conform to protocols
                    return Err(sem_err(format!(
                        "type `{}` does not conform to protocol `{}`",
                        arg_name, bound
                    )));
                }
            }
        }
    }
    Ok(())
}
