use crate::error::{Result, Span};
use crate::parser::ast::*;
use crate::suggest::find_suggestion;

use super::infer::InferenceContext;
use super::resolver::{self, FuncSig, ProtocolInfo, Resolver};
use super::types::Type;
use super::{
    DiagCtxt, SemanticInfo, analyze_function, analyze_struct_members, resolve_struct_members,
    resolve_type_checked, sem_err, sem_err_with_help,
};

/// Analyze a single module's AST.
/// This is similar to the existing `analyze()` but:
///   - Takes an existing `resolver` (possibly pre-populated with imports).
///   - Only checks for `main()` when `require_main` is true.
pub(super) fn analyze_single_module(
    program: &Program,
    resolver: &mut Resolver,
    require_main: bool,
    diag: &mut DiagCtxt,
) -> Result<SemanticInfo> {
    // Pass 1a: register all struct and function names (for forward reference)
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
    // Pass 1a (continued): register protocol names
    for proto in &program.protocols {
        if resolver.lookup_struct(&proto.name).is_some()
            || resolver.lookup_func(&proto.name).is_some()
            || resolver.lookup_protocol(&proto.name).is_some()
        {
            return Err(sem_err(format!("duplicate definition `{}`", proto.name)));
        }
        resolver.define_protocol(
            proto.name.clone(),
            ProtocolInfo {
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
        // Push type params into scope so that parameter/return types can reference them
        resolver.push_type_params(&func.type_params);
        let params: Vec<(String, Type)> = func
            .params
            .iter()
            .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, resolver)?)))
            .collect::<Result<Vec<_>>>()?;
        let return_type = resolve_type_checked(&func.return_type, resolver)?;
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

    // Pass 1b: resolve struct member types
    for struct_def in &program.structs {
        resolve_struct_members(struct_def, resolver)?;
    }

    // Check for name collisions between mangled method names and top-level functions
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

    // Pass 1b (continued): resolve protocol member types
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
                        .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, resolver)?)))
                        .collect::<Result<Vec<_>>>()?;
                    let resolved_return = resolve_type_checked(return_type, resolver)?;
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
                    let resolved_ty = resolve_type_checked(ty, resolver)?;
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
            ProtocolInfo {
                name: proto.name.clone(),
                methods,
                properties,
            },
        );
    }

    // Pass 2: verify main function exists with correct signature (only for root module)
    if require_main {
        match resolver.lookup_func("main") {
            None => {
                diag.emit(sem_err("no `main` function found"));
            }
            Some(sig) => {
                if !sig.params.is_empty() {
                    diag.emit(sem_err("`main` function must have no parameters"));
                }
                if sig.return_type != Type::I32 {
                    diag.emit(sem_err("`main` function must return `Int32`"));
                }
            }
        }
    }

    // Pass 3: analyze struct member bodies and function bodies
    // Skip generic structs — their member bodies contain unsubstituted type
    // parameters that would cause spurious errors.
    let mut infer_ctx = InferenceContext::new();
    for struct_def in &program.structs {
        if !struct_def.type_params.is_empty() {
            continue;
        }
        analyze_struct_members(struct_def, resolver, &mut infer_ctx, diag);
        let _ = infer_ctx.apply_defaults();
        infer_ctx.reset();
    }

    // Pass 3b: check protocol conformance
    for struct_def in &program.structs {
        'proto: for proto_name in &struct_def.conformances {
            let proto_info = match resolver.lookup_protocol(proto_name) {
                Some(info) => info.clone(),
                None => {
                    let help = find_suggestion(proto_name, resolver.all_protocol_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("unknown protocol `{}`", proto_name),
                        Span { start: 0, end: 0 },
                        help,
                    ));
                    continue 'proto;
                }
            };
            let struct_info = match resolver.lookup_struct(&struct_def.name) {
                Some(info) => info.clone(),
                None => {
                    let help = find_suggestion(&struct_def.name, resolver.all_struct_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    diag.emit(sem_err_with_help(
                        format!("undefined struct `{}`", struct_def.name),
                        Span { start: 0, end: 0 },
                        help,
                    ));
                    continue 'proto;
                }
            };

            // Check methods
            for req_method in &proto_info.methods {
                match struct_info.method_index.get(&req_method.name) {
                    Some(&idx) => {
                        let impl_method = &struct_info.methods[idx];
                        if impl_method.params.len() != req_method.params.len() {
                            diag.emit(sem_err(format!(
                                "method `{}` expects {} parameter(s) but protocol `{}` requires {}",
                                req_method.name,
                                impl_method.params.len(),
                                proto_name,
                                req_method.params.len()
                            )));
                            continue;
                        }
                        for ((impl_name, impl_ty), (req_name, req_ty)) in
                            impl_method.params.iter().zip(req_method.params.iter())
                        {
                            if impl_ty != req_ty {
                                diag.emit(sem_err(format!(
                                    "method `{}` has parameter `{}` of type `{}` but protocol `{}` requires `{}`",
                                    req_method.name, impl_name, impl_ty, proto_name, req_ty
                                )));
                            }
                            if impl_name != req_name {
                                diag.emit(sem_err(format!(
                                    "method `{}` has parameter `{}` but protocol `{}` requires `{}`",
                                    req_method.name, impl_name, proto_name, req_name
                                )));
                            }
                        }
                        if impl_method.return_type != req_method.return_type {
                            diag.emit(sem_err(format!(
                                "method `{}` has return type `{}` but protocol `{}` requires `{}`",
                                req_method.name,
                                impl_method.return_type,
                                proto_name,
                                req_method.return_type
                            )));
                        }
                    }
                    None => {
                        diag.emit(sem_err(format!(
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
                        diag.emit(sem_err(format!(
                            "property `{}` has type `{}` but protocol `{}` requires `{}`",
                            req_prop.name, field_ty, proto_name, req_prop.ty
                        )));
                    }
                    continue;
                }
                if let Some(&idx) = struct_info.computed_index.get(&req_prop.name) {
                    let computed = &struct_info.computed[idx];
                    if computed.ty != req_prop.ty {
                        diag.emit(sem_err(format!(
                            "property `{}` has type `{}` but protocol `{}` requires `{}`",
                            req_prop.name, computed.ty, proto_name, req_prop.ty
                        )));
                    }
                    if req_prop.has_setter && !computed.has_setter {
                        diag.emit(sem_err(format!(
                            "property `{}` requires a setter to conform to protocol `{}`",
                            req_prop.name, proto_name
                        )));
                    }
                    continue;
                }
                diag.emit(sem_err(format!(
                    "type `{}` does not implement property `{}` required by protocol `{}`",
                    struct_def.name, req_prop.name, proto_name
                )));
            }
        }
    }

    {
        let mut ctx = InferenceContext::new();
        for func in &program.functions {
            analyze_function(func, resolver, Some(&mut ctx), diag);
            let errs = ctx.apply_defaults();
            for e in errs {
                diag.emit(e);
            }
            ctx.reset();
        }
    }

    if diag.has_errors() {
        return Err(sem_err(format!("{} error(s) found", diag.error_count())));
    }

    Ok(SemanticInfo {
        struct_defs: resolver.take_all_struct_defs(),
        struct_init_calls: resolver.take_struct_init_calls(),
        protocols: resolver.take_all_protocols(),
    })
}
