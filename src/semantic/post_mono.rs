use crate::error::{Result, Span};
use crate::parser::ast::*;
use crate::suggest::find_suggestion;

use super::infer::InferenceContext;
use super::resolver::{self, FuncSig, Resolver};
use super::types::Type;
use super::{
    DiagCtxt, SemanticInfo, analyze_function, analyze_struct_members, collect_visibilities,
    resolve_struct_members, resolve_type_checked, sem_err, sem_err_with_help,
};

pub fn analyze_post_mono(program: &Program) -> Result<SemanticInfo> {
    let mut resolver = Resolver::new();

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

    // Pass 1b: resolve struct member types (all names now registered)
    for struct_def in &program.structs {
        resolve_struct_members(struct_def, &mut resolver)?;
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

    // Pass 2: verify main function exists with correct signature
    match resolver.lookup_func("main") {
        None => return Err(sem_err("no `main` function found")),
        Some(sig) => {
            if !sig.params.is_empty() {
                return Err(sem_err("`main` function must have no parameters"));
            }
            if sig.return_type != Type::I32 {
                return Err(sem_err("`main` function must return `Int32`"));
            }
        }
    }

    // Pass 3: analyze struct member bodies and function bodies
    {
        let mut struct_ctx = InferenceContext::new();
        let mut struct_diag = DiagCtxt::new();
        for struct_def in &program.structs {
            analyze_struct_members(struct_def, &mut resolver, &mut struct_ctx, &mut struct_diag);
            let errs = struct_ctx.apply_defaults();
            for e in errs {
                struct_diag.emit(e);
            }
            struct_ctx.reset();
        }
        let struct_errors = struct_diag.take_errors();
        if let Some(e) = struct_errors.into_iter().next() {
            return Err(e);
        }
    }

    // Pass 3b: check protocol conformance
    for struct_def in &program.structs {
        for proto_name in &struct_def.conformances {
            let proto_info = resolver
                .lookup_protocol(proto_name)
                .ok_or_else(|| {
                    let help = find_suggestion(proto_name, resolver.all_protocol_names())
                        .map(|s| format!("did you mean '{s}'?"));
                    sem_err_with_help(
                        format!("unknown protocol `{}`", proto_name),
                        Span { start: 0, end: 0 },
                        help,
                    )
                })?
                .clone();
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
                // Check stored properties first
                if let Some(&idx) = struct_info.field_index.get(&req_prop.name) {
                    let (_, field_ty) = &struct_info.fields[idx];
                    if *field_ty != req_prop.ty {
                        return Err(sem_err(format!(
                            "property `{}` has type `{}` but protocol `{}` requires `{}`",
                            req_prop.name, field_ty, proto_name, req_prop.ty
                        )));
                    }
                    // stored var always satisfies { get } and { get set }
                    continue;
                }
                // Check computed properties
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

    {
        let mut ctx = InferenceContext::new();
        let mut func_diag = DiagCtxt::new();
        for func in &program.functions {
            analyze_function(func, &mut resolver, Some(&mut ctx), &mut func_diag);
            let errs = ctx.apply_defaults();
            for e in errs {
                func_diag.emit(e);
            }
            ctx.reset();
        }
        let func_errors = func_diag.take_errors();
        if let Some(e) = func_errors.into_iter().next() {
            return Err(e);
        }
    }

    Ok(SemanticInfo {
        struct_defs: resolver.take_struct_defs(),
        struct_init_calls: resolver.take_struct_init_calls(),
        protocols: resolver.take_protocols(),
        functions: resolver.take_functions(),
        visibilities: collect_visibilities(program),
    })
}
