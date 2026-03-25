pub mod resolver;
pub mod types;

use std::collections::{HashMap, HashSet};

use crate::error::{BengalError, Result, Span};
use crate::package::{ModuleGraph, ModulePath};
use crate::parser::ast::*;
use resolver::{FuncSig, ProtocolInfo, Resolver, StructInfo, VarInfo, is_accessible};
use types::{Type, resolve_type};

#[derive(Debug)]
pub struct SemanticInfo {
    pub struct_defs: HashMap<String, StructInfo>,
    pub struct_init_calls: HashSet<NodeId>,
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
    }
}

fn pkg_err(message: impl Into<String>) -> BengalError {
    BengalError::PackageError {
        message: message.into(),
    }
}

// ---------------------------------------------------------------------------
// Multi-module semantic analysis
// ---------------------------------------------------------------------------

/// Kinds of top-level symbols we track across modules.
#[derive(Debug, Clone)]
enum SymbolKind {
    Func(FuncSig),
    Struct(StructInfo),
    Protocol(ProtocolInfo),
}

/// A single entry in the global (cross-module) symbol table.
#[derive(Debug, Clone)]
struct GlobalSymbol {
    kind: SymbolKind,
    visibility: Visibility,
    module: ModulePath,
}

/// Global symbol table: module path -> (name -> GlobalSymbol)
type GlobalSymbolTable = HashMap<ModulePath, HashMap<String, GlobalSymbol>>;

/// Analyze an entire package represented by its `ModuleGraph`.
///
/// This is the multi-module entry point. It performs three phases:
///   1. Collect all top-level symbols from every module.
///   2. Resolve imports for each module and check visibility.
///   3. Run the existing single-module analysis passes with imported symbols.
pub fn analyze_package(graph: &ModuleGraph, _package_name: &str) -> Result<PackageSemanticInfo> {
    // ---------------------------------------------------------------
    // Phase 1: Collect all top-level symbols from all modules
    // ---------------------------------------------------------------
    let global_symbols = collect_global_symbols(graph)?;

    // ---------------------------------------------------------------
    // Phase 2 + 3: For each module, resolve imports then run analysis
    // ---------------------------------------------------------------
    let mut module_infos: HashMap<ModulePath, SemanticInfo> = HashMap::new();
    let mut import_sources: HashMap<(ModulePath, String), ModulePath> = HashMap::new();

    for (mod_path, mod_info) in &graph.modules {
        let mut resolver = Resolver::new();

        // Resolve imports and populate the resolver's import maps
        let module_import_sources = resolve_imports_for_module(
            mod_path,
            &mod_info.ast.import_decls,
            &global_symbols,
            graph,
            &mut resolver,
        )?;

        // Record import sources for this module
        for (name, source_module) in module_import_sources {
            import_sources.insert((mod_path.clone(), name), source_module);
        }

        // Run the standard single-module analysis (same as `analyze()` but
        // parameterised on whether to require `main`).
        let is_root = mod_path.is_root();
        let sem_info = analyze_single_module(&mod_info.ast, &mut resolver, is_root)?;
        module_infos.insert(mod_path.clone(), sem_info);
    }

    Ok(PackageSemanticInfo {
        module_infos,
        import_sources,
    })
}

/// Phase 1: Walk every module in the graph, register all top-level symbols,
/// and return the global symbol table.
fn collect_global_symbols(graph: &ModuleGraph) -> Result<GlobalSymbolTable> {
    let mut table: GlobalSymbolTable = HashMap::new();

    for (mod_path, mod_info) in &graph.modules {
        let mut symbols: HashMap<String, GlobalSymbol> = HashMap::new();
        let ast = &mod_info.ast;

        // We need a temporary resolver to resolve types for function signatures
        // and struct members. First do a two-pass approach: register all type
        // names, then resolve member types.

        let mut tmp_resolver = Resolver::new();

        // Register struct names (reserves)
        for s in &ast.structs {
            tmp_resolver.reserve_struct(s.name.clone());
        }
        // Register protocol placeholders
        for p in &ast.protocols {
            tmp_resolver.define_protocol(
                p.name.clone(),
                ProtocolInfo {
                    name: p.name.clone(),
                    methods: vec![],
                    properties: vec![],
                },
            );
        }
        // Register function signatures (need types already registered)
        for func in &ast.functions {
            let params: Vec<Type> = func
                .params
                .iter()
                .map(|p| resolve_type_checked(&p.ty, &tmp_resolver))
                .collect::<Result<Vec<_>>>()?;
            let return_type = resolve_type_checked(&func.return_type, &tmp_resolver)?;
            let sig = FuncSig {
                params,
                return_type,
            };
            tmp_resolver.define_func(func.name.clone(), sig.clone());
            symbols.insert(
                func.name.clone(),
                GlobalSymbol {
                    kind: SymbolKind::Func(sig),
                    visibility: func.visibility,
                    module: mod_path.clone(),
                },
            );
        }

        // Resolve struct member types
        for s in &ast.structs {
            resolve_struct_members(s, &mut tmp_resolver)?;
            let info = tmp_resolver.lookup_struct(&s.name).unwrap().clone();
            symbols.insert(
                s.name.clone(),
                GlobalSymbol {
                    kind: SymbolKind::Struct(info),
                    visibility: s.visibility,
                    module: mod_path.clone(),
                },
            );
        }

        // Resolve protocol member types
        for proto in &ast.protocols {
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
                            .map(|p| {
                                Ok((p.name.clone(), resolve_type_checked(&p.ty, &tmp_resolver)?))
                            })
                            .collect::<Result<Vec<_>>>()?;
                        let resolved_return = resolve_type_checked(return_type, &tmp_resolver)?;
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
                        let resolved_ty = resolve_type_checked(ty, &tmp_resolver)?;
                        properties.push(resolver::ProtocolPropertyReq {
                            name: name.clone(),
                            ty: resolved_ty,
                            has_setter: *has_setter,
                        });
                    }
                }
            }
            let info = ProtocolInfo {
                name: proto.name.clone(),
                methods,
                properties,
            };
            symbols.insert(
                proto.name.clone(),
                GlobalSymbol {
                    kind: SymbolKind::Protocol(info),
                    visibility: proto.visibility,
                    module: mod_path.clone(),
                },
            );
        }

        table.insert(mod_path.clone(), symbols);
    }

    Ok(table)
}

/// Phase 2: Resolve all import declarations for a given module and populate
/// the resolver's import maps. Returns a map of (local_name -> source_module_path)
/// for all imported symbols.
fn resolve_imports_for_module(
    current_module: &ModulePath,
    import_decls: &[ImportDecl],
    global_symbols: &GlobalSymbolTable,
    graph: &ModuleGraph,
    resolver: &mut Resolver,
) -> Result<Vec<(String, ModulePath)>> {
    let mut sources = Vec::new();

    for import in import_decls {
        // Resolve the target module path from the prefix + path segments
        let target_module =
            resolve_import_module_path(current_module, &import.prefix, &import.path, graph)?;

        let target_symbols = global_symbols.get(&target_module).ok_or_else(|| {
            pkg_err(format!(
                "unresolved import: module '{}' not found",
                target_module
            ))
        })?;

        match &import.tail {
            ImportTail::Single(name) => {
                import_single_symbol(
                    name,
                    &target_module,
                    target_symbols,
                    current_module,
                    resolver,
                )?;
                sources.push((name.clone(), target_module.clone()));
            }
            ImportTail::Group(names) => {
                for name in names {
                    import_single_symbol(
                        name,
                        &target_module,
                        target_symbols,
                        current_module,
                        resolver,
                    )?;
                    sources.push((name.clone(), target_module.clone()));
                }
            }
            ImportTail::Glob => {
                // Import all accessible symbols
                for (name, sym) in target_symbols {
                    if is_accessible(sym.visibility, &sym.module, current_module) {
                        import_symbol_to_resolver(name, sym, resolver);
                        sources.push((name.clone(), target_module.clone()));
                    }
                }
            }
        }
    }
    Ok(sources)
}

/// Import a single named symbol, checking visibility.
fn import_single_symbol(
    name: &str,
    target_module: &ModulePath,
    target_symbols: &HashMap<String, GlobalSymbol>,
    current_module: &ModulePath,
    resolver: &mut Resolver,
) -> Result<()> {
    let sym = target_symbols.get(name).ok_or_else(|| {
        sem_err(format!(
            "unresolved import: module '{}' has no item '{}'",
            target_module, name
        ))
    })?;

    if !is_accessible(sym.visibility, &sym.module, current_module) {
        return Err(sem_err(format!(
            "'{}' cannot be imported: it is not accessible from module '{}'",
            name, current_module
        )));
    }

    import_symbol_to_resolver(name, sym, resolver);
    Ok(())
}

/// Actually add a symbol to the resolver's import maps.
fn import_symbol_to_resolver(name: &str, sym: &GlobalSymbol, resolver: &mut Resolver) {
    match &sym.kind {
        SymbolKind::Func(sig) => {
            resolver.import_func(name.to_string(), sig.clone());
        }
        SymbolKind::Struct(info) => {
            resolver.import_struct(name.to_string(), info.clone());
        }
        SymbolKind::Protocol(info) => {
            resolver.import_protocol(name.to_string(), info.clone());
        }
    }
}

/// Resolve an import path prefix + intermediate segments to a `ModulePath`.
///
/// For `import math::sub::foo`, prefix = Named("math"), path = ["sub"], tail = Single("foo")
/// So we need to build module path from prefix + path segments.
fn resolve_import_module_path(
    current_module: &ModulePath,
    prefix: &PathPrefix,
    path_segments: &[String],
    graph: &ModuleGraph,
) -> Result<ModulePath> {
    let base = match prefix {
        PathPrefix::SelfKw => current_module.clone(),
        PathPrefix::Super => current_module
            .parent()
            .ok_or_else(|| sem_err("cannot use 'super' from the package root module"))?,
        PathPrefix::Named(name) => ModulePath(vec![name.clone()]),
    };

    // Append intermediate path segments
    let mut result = base;
    for seg in path_segments {
        result = result.child(seg);
    }

    // Verify the target module exists in the graph
    if !graph.modules.contains_key(&result) {
        return Err(pkg_err(format!(
            "unresolved import: module '{}' not found",
            result
        )));
    }

    Ok(result)
}

/// Analyze a single module's AST.
/// This is similar to the existing `analyze()` but:
///   - Takes an existing `resolver` (possibly pre-populated with imports).
///   - Only checks for `main()` when `require_main` is true.
fn analyze_single_module(
    program: &Program,
    resolver: &mut Resolver,
    require_main: bool,
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
        let params: Vec<Type> = func
            .params
            .iter()
            .map(|p| resolve_type_checked(&p.ty, resolver))
            .collect::<Result<Vec<_>>>()?;
        let return_type = resolve_type_checked(&func.return_type, resolver)?;
        resolver.define_func(
            func.name.clone(),
            FuncSig {
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
    }

    // Pass 3: analyze struct member bodies and function bodies
    for struct_def in &program.structs {
        analyze_struct_members(struct_def, resolver)?;
    }

    // Pass 3b: check protocol conformance
    for struct_def in &program.structs {
        for proto_name in &struct_def.conformances {
            let proto_info = resolver
                .lookup_protocol(proto_name)
                .ok_or_else(|| sem_err(format!("unknown protocol `{}`", proto_name)))?
                .clone();
            let struct_info = resolver
                .lookup_struct(&struct_def.name)
                .ok_or_else(|| sem_err(format!("undefined struct `{}`", struct_def.name)))?
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

    for func in &program.functions {
        analyze_function(func, resolver)?;
    }

    Ok(SemanticInfo {
        struct_defs: resolver.take_all_struct_defs(),
        struct_init_calls: resolver.take_struct_init_calls(),
    })
}

pub fn analyze(program: &Program) -> Result<SemanticInfo> {
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
        let params: Vec<Type> = func
            .params
            .iter()
            .map(|p| resolve_type_checked(&p.ty, &resolver))
            .collect::<Result<Vec<_>>>()?;
        let return_type = resolve_type_checked(&func.return_type, &resolver)?;
        resolver.define_func(
            func.name.clone(),
            FuncSig {
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
    for struct_def in &program.structs {
        analyze_struct_members(struct_def, &mut resolver)?;
    }

    // Pass 3b: check protocol conformance
    for struct_def in &program.structs {
        for proto_name in &struct_def.conformances {
            let proto_info = resolver
                .lookup_protocol(proto_name)
                .ok_or_else(|| sem_err(format!("unknown protocol `{}`", proto_name)))?
                .clone();
            let struct_info = resolver
                .lookup_struct(&struct_def.name)
                .ok_or_else(|| sem_err(format!("undefined struct `{}`", struct_def.name)))?
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

    for func in &program.functions {
        analyze_function(func, &mut resolver)?;
    }

    Ok(SemanticInfo {
        struct_defs: resolver.take_struct_defs(),
        struct_init_calls: resolver.take_struct_init_calls(),
    })
}

fn resolve_type_checked(ty: &TypeAnnotation, resolver: &Resolver) -> Result<Type> {
    match ty {
        TypeAnnotation::Named(name) => {
            if resolver.lookup_struct(name).is_none() {
                return Err(sem_err(format!("undefined type `{}`", name)));
            }
            Ok(Type::Struct(name.clone()))
        }
        other => Ok(resolve_type(other)),
    }
}

fn resolve_struct_members(struct_def: &StructDef, resolver: &mut Resolver) -> Result<()> {
    let name = &struct_def.name;

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

    resolver.define_struct(
        name.clone(),
        resolver::StructInfo {
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

fn analyze_function(func: &Function, resolver: &mut Resolver) -> Result<()> {
    let return_type = resolve_type_checked(&func.return_type, resolver)?;
    resolver.current_return_type = Some(return_type.clone());
    resolver.push_scope();

    // Register function parameters as immutable variables
    for param in &func.params {
        resolver.define_var(
            param.name.clone(),
            VarInfo {
                ty: resolve_type_checked(&param.ty, resolver)?,
                mutable: false,
            },
        );
    }

    let stmts = &func.body.stmts;

    // Check that all paths end with a return
    if !block_always_returns(&func.body) {
        return Err(sem_err(format!(
            "function `{}` must end with a `return` statement",
            func.name
        )));
    }

    for stmt in stmts.iter() {
        // Yield is not allowed in function bodies
        if matches!(stmt, Stmt::Yield(_)) {
            return Err(sem_err(
                "`yield` cannot be used in function body (use `return` instead)",
            ));
        }

        analyze_stmt(stmt, resolver)?;
    }

    resolver.pop_scope();
    resolver.current_return_type = None;
    Ok(())
}

/// Analyze a block expression (Expr::Block) — yield required, return forbidden
fn analyze_block_expr(block: &Block, resolver: &mut Resolver) -> Result<Type> {
    resolver.push_scope();

    let stmts = &block.stmts;

    if stmts.is_empty() {
        return Err(sem_err(
            "block expression must end with a `yield` statement",
        ));
    }

    // Check that the last statement is Yield
    if !matches!(stmts.last(), Some(Stmt::Yield(_))) {
        return Err(sem_err(
            "block expression must end with a `yield` statement",
        ));
    }

    let mut yield_type = Type::I32; // will be overwritten

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Return is not allowed in block expressions
        if matches!(stmt, Stmt::Return(_)) {
            return Err(sem_err("`return` cannot be used inside a block expression"));
        }

        // Yield is only allowed as the last statement
        if matches!(stmt, Stmt::Yield(_)) && !is_last {
            return Err(sem_err(
                "`yield` must be the last statement in the block expression",
            ));
        }

        analyze_stmt(stmt, resolver)?;

        // If this is the Yield statement, get the type
        if let Stmt::Yield(expr) = stmt {
            yield_type = analyze_expr(expr, resolver)?;
        }
    }

    resolver.pop_scope();
    Ok(yield_type)
}

/// Analyze a control block (if then/else) — yield and return both allowed.
/// Returns Some(type) if block yields a value, None if block diverges via return.
fn analyze_control_block(block: &Block, resolver: &mut Resolver) -> Result<Option<Type>> {
    resolver.push_scope();

    let stmts = &block.stmts;

    if stmts.is_empty() {
        resolver.pop_scope();
        return Ok(Some(Type::Unit));
    }

    let mut result: Option<Type> = None;

    for (i, stmt) in stmts.iter().enumerate() {
        let is_last = i == stmts.len() - 1;

        // Yield is only allowed as the last statement
        if matches!(stmt, Stmt::Yield(_)) && !is_last {
            return Err(sem_err("`yield` must be the last statement in the block"));
        }

        analyze_stmt(stmt, resolver)?;

        if is_last {
            match stmt {
                Stmt::Yield(expr) => {
                    let ty = analyze_expr(expr, resolver)?;
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
    Ok(result)
}

/// Analyze a loop body block — return allowed, yield forbidden.
fn analyze_loop_block(block: &Block, resolver: &mut Resolver) -> Result<()> {
    resolver.push_scope();

    for stmt in &block.stmts {
        if matches!(stmt, Stmt::Yield(_)) {
            return Err(sem_err("`yield` cannot be used in a while loop body"));
        }
        analyze_stmt(stmt, resolver)?;
    }

    resolver.pop_scope();
    Ok(())
}

fn analyze_stmt(stmt: &Stmt, resolver: &mut Resolver) -> Result<()> {
    match stmt {
        Stmt::Let { name, ty, value } => {
            let val_ty = analyze_expr(value, resolver)?;
            let var_ty = match ty {
                Some(ann) => {
                    let declared = resolve_type_checked(ann, resolver)?;
                    if val_ty != declared {
                        return Err(sem_err(format!(
                            "type mismatch: expected `{}`, found `{}`",
                            declared, val_ty
                        )));
                    }
                    declared
                }
                None => val_ty,
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
            let val_ty = analyze_expr(value, resolver)?;
            let var_ty = match ty {
                Some(ann) => {
                    let declared = resolve_type_checked(ann, resolver)?;
                    if val_ty != declared {
                        return Err(sem_err(format!(
                            "type mismatch: expected `{}`, found `{}`",
                            declared, val_ty
                        )));
                    }
                    declared
                }
                None => val_ty,
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
            let val_ty = analyze_expr(value, resolver)?;
            match resolver.lookup_var(name) {
                None => {
                    return Err(sem_err(format!("undefined variable `{}`", name)));
                }
                Some(info) => {
                    if !info.mutable {
                        return Err(sem_err(format!(
                            "cannot assign to immutable variable `{}`",
                            name
                        )));
                    }
                    if val_ty != info.ty {
                        return Err(sem_err(format!(
                            "type mismatch in assignment: expected `{}`, found `{}`",
                            info.ty, val_ty
                        )));
                    }
                }
            }
        }
        Stmt::Return(Some(expr)) => {
            let ty = analyze_expr(expr, resolver)?;
            if let Some(ref return_type) = resolver.current_return_type
                && ty != *return_type
            {
                return Err(sem_err(format!(
                    "return type mismatch: expected `{}`, found `{}`",
                    return_type, ty
                )));
            }
        }
        Stmt::Return(None) => {
            if let Some(ref return_type) = resolver.current_return_type
                && *return_type != Type::Unit
            {
                return Err(sem_err(format!(
                    "return type mismatch: expected `{}`, found `()`",
                    return_type
                )));
            }
        }
        Stmt::Yield(expr) => {
            let _ty = analyze_expr(expr, resolver)?;
        }
        Stmt::Expr(expr) => {
            let _ty = analyze_expr(expr, resolver)?;
        }
        Stmt::Break(opt_expr) => {
            if !resolver.in_loop() {
                return Err(sem_err("break outside of loop"));
            }
            let break_ty = match opt_expr {
                Some(expr) => analyze_expr(expr, resolver)?,
                None => Type::Unit,
            };
            resolver.set_break_type(break_ty)?;
        }
        Stmt::Continue => {
            if !resolver.in_loop() {
                return Err(sem_err("continue outside of loop"));
            }
        }
        Stmt::FieldAssign {
            object,
            field,
            value,
        } => {
            let obj_ty = analyze_expr(object, resolver)?;
            let val_ty = analyze_expr(value, resolver)?;
            match &obj_ty {
                Type::Struct(struct_name) => {
                    let struct_info = resolver
                        .lookup_struct(struct_name)
                        .ok_or_else(|| sem_err(format!("undefined struct `{}`", struct_name)))?
                        .clone();
                    let field_ty = if let Some(&idx) = struct_info.field_index.get(field.as_str()) {
                        struct_info.fields[idx].1.clone()
                    } else if let Some(&idx) = struct_info.computed_index.get(field.as_str()) {
                        let prop = &struct_info.computed[idx];
                        if !prop.has_setter {
                            return Err(sem_err(format!(
                                "computed property `{}` is read-only (no setter)",
                                field
                            )));
                        }
                        prop.ty.clone()
                    } else {
                        return Err(sem_err(format!(
                            "struct `{}` has no field `{}`",
                            struct_name, field
                        )));
                    };
                    if val_ty != field_ty {
                        return Err(sem_err(format!(
                            "type mismatch in field assignment: expected `{}`, found `{}`",
                            field_ty, val_ty
                        )));
                    }
                    check_assignment_target_mutable(object, resolver)?;
                }
                _ => {
                    return Err(sem_err(format!(
                        "field assignment on non-struct type `{}`",
                        obj_ty
                    )));
                }
            }
        }
    }
    Ok(())
}

fn analyze_expr(expr: &Expr, resolver: &mut Resolver) -> Result<Type> {
    match &expr.kind {
        ExprKind::Number(n) => {
            if *n < i32::MIN as i64 || *n > i32::MAX as i64 {
                return Err(sem_err(format!(
                    "integer literal `{}` is out of range for `Int32`",
                    n
                )));
            }
            Ok(Type::I32)
        }
        ExprKind::Bool(_) => Ok(Type::Bool),
        ExprKind::Ident(name) => match resolver.lookup_var(name) {
            Some(info) => Ok(info.ty.clone()),
            None => Err(sem_err(format!("undefined variable `{}`", name))),
        },
        ExprKind::UnaryOp { op, operand } => {
            let operand_ty = analyze_expr(operand, resolver)?;
            match op {
                UnaryOp::Not => {
                    if operand_ty != Type::Bool {
                        return Err(sem_err("operand of `!` must be `Bool`"));
                    }
                    Ok(Type::Bool)
                }
            }
        }
        ExprKind::BinaryOp { op, left, right } => {
            let left_ty = analyze_expr(left, resolver)?;
            let right_ty = analyze_expr(right, resolver)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    if !left_ty.is_numeric() || left_ty != right_ty {
                        return Err(sem_err(format!(
                            "arithmetic operation requires matching numeric operands, found `{}` and `{}`",
                            left_ty, right_ty
                        )));
                    }
                    Ok(left_ty)
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    if !left_ty.is_numeric() || left_ty != right_ty {
                        return Err(sem_err(format!(
                            "comparison requires matching numeric operands, found `{}` and `{}`",
                            left_ty, right_ty
                        )));
                    }
                    Ok(Type::Bool)
                }
                // Logical: bool x bool → bool
                BinOp::And | BinOp::Or => {
                    if left_ty != Type::Bool || right_ty != Type::Bool {
                        return Err(sem_err("logical operation requires `Bool` operands"));
                    }
                    Ok(Type::Bool)
                }
            }
        }
        ExprKind::Call { name, args } => {
            // Empty-arg call may be a struct init
            if args.is_empty()
                && let Some(struct_info) = resolver.lookup_struct(name)
            {
                let struct_info = struct_info.clone();
                if struct_info.init.params.is_empty() {
                    resolver.record_struct_init_call(expr.id);
                    return Ok(Type::Struct(name.clone()));
                } else {
                    return Err(sem_err(format!(
                        "struct `{}` initializer expects {} arguments, but 0 were given",
                        name,
                        struct_info.init.params.len()
                    )));
                }
            }
            let sig = resolver
                .lookup_func(name)
                .ok_or_else(|| sem_err(format!("undefined function `{}`", name)))?
                .clone();
            if args.len() != sig.params.len() {
                return Err(sem_err(format!(
                    "function `{}` expects {} arguments, but {} were given",
                    name,
                    sig.params.len(),
                    args.len()
                )));
            }
            for (arg, expected_ty) in args.iter().zip(sig.params.iter()) {
                let arg_ty = analyze_expr(arg, resolver)?;
                if arg_ty != *expected_ty {
                    return Err(sem_err(format!(
                        "argument type mismatch: expected `{}`, found `{}`",
                        expected_ty, arg_ty
                    )));
                }
            }
            Ok(sig.return_type.clone())
        }
        ExprKind::Block(block) => analyze_block_expr(block, resolver),
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            let cond_ty = analyze_expr(condition, resolver)?;
            if cond_ty != Type::Bool {
                return Err(sem_err("if condition must be `Bool`"));
            }

            let then_ty = analyze_control_block(then_block, resolver)?;

            match else_block {
                Some(else_blk) => {
                    let else_ty = analyze_control_block(else_blk, resolver)?;
                    // Type merging with divergence
                    match (then_ty, else_ty) {
                        (Some(t1), Some(t2)) => {
                            if t1 != t2 {
                                return Err(sem_err(format!(
                                    "if/else branch type mismatch: `{}` vs `{}`",
                                    t1, t2
                                )));
                            }
                            Ok(t1)
                        }
                        (None, Some(t)) => Ok(t), // then diverges, use else type
                        (Some(t), None) => Ok(t), // else diverges, use then type
                        (None, None) => Ok(Type::Unit), // both diverge
                    }
                }
                None => {
                    // if without else: type is Unit
                    if let Some(ref ty) = then_ty
                        && *ty != Type::Unit
                    {
                        return Err(sem_err(
                            "if without else must have unit type (use `yield` in both branches for a value)",
                        ));
                    }
                    Ok(Type::Unit)
                }
            }
        }
        ExprKind::While {
            condition,
            body,
            nobreak,
        } => {
            let cond_ty = analyze_expr(condition, resolver)?;
            if cond_ty != Type::Bool {
                return Err(sem_err("while condition must be `Bool`"));
            }
            let is_while_true = condition.kind == ExprKind::Bool(true);

            resolver.enter_loop();
            analyze_loop_block(body, resolver)?;
            let break_ty = resolver.exit_loop();

            let while_ty = break_ty.unwrap_or(Type::Unit);

            match (is_while_true, nobreak) {
                (true, Some(_)) => {
                    return Err(sem_err("`nobreak` is unreachable in `while true`"));
                }
                (false, None) if while_ty != Type::Unit => {
                    return Err(sem_err(
                        "`while` with non-unit break requires `nobreak` block",
                    ));
                }
                (false, Some(nobreak_block)) => {
                    let nobreak_ty = analyze_control_block(nobreak_block, resolver)?;
                    match nobreak_ty {
                        Some(t) if t != while_ty => {
                            return Err(sem_err(format!(
                                "nobreak type `{}` does not match while type `{}`",
                                t, while_ty
                            )));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }

            Ok(while_ty)
        }
        ExprKind::Float(_) => Ok(Type::F64),
        ExprKind::StructInit { name, args } => {
            let struct_info = resolver
                .lookup_struct(name)
                .ok_or_else(|| sem_err(format!("undefined struct `{}`", name)))?
                .clone();
            let init = &struct_info.init;
            if args.len() != init.params.len() {
                return Err(sem_err(format!(
                    "struct `{}` initializer expects {} arguments, but {} were given",
                    name,
                    init.params.len(),
                    args.len()
                )));
            }
            for ((label, arg_expr), (param_name, param_ty)) in args.iter().zip(init.params.iter()) {
                if label != param_name {
                    return Err(sem_err(format!(
                        "expected argument label `{}`, found `{}`",
                        param_name, label
                    )));
                }
                let arg_ty = analyze_expr(arg_expr, resolver)?;
                if arg_ty != *param_ty {
                    return Err(sem_err(format!(
                        "argument type mismatch: expected `{}`, found `{}`",
                        param_ty, arg_ty
                    )));
                }
            }
            Ok(Type::Struct(name.clone()))
        }
        ExprKind::FieldAccess { object, field } => {
            let obj_ty = analyze_expr(object, resolver)?;
            match &obj_ty {
                Type::Struct(struct_name) => {
                    let struct_info = resolver
                        .lookup_struct(struct_name)
                        .ok_or_else(|| sem_err(format!("undefined struct `{}`", struct_name)))?
                        .clone();
                    if let Some(&idx) = struct_info.field_index.get(field.as_str()) {
                        Ok(struct_info.fields[idx].1.clone())
                    } else if let Some(&idx) = struct_info.computed_index.get(field.as_str()) {
                        Ok(struct_info.computed[idx].ty.clone())
                    } else {
                        Err(sem_err(format!(
                            "struct `{}` has no field `{}`",
                            struct_name, field
                        )))
                    }
                }
                _ => Err(sem_err(format!(
                    "field access on non-struct type `{}`",
                    obj_ty
                ))),
            }
        }
        ExprKind::SelfRef => match &resolver.self_context {
            Some(ctx) => Ok(Type::Struct(ctx.struct_name.clone())),
            None => Err(sem_err(
                "`self` can only be used inside struct initializers, computed properties, or methods",
            )),
        },
        ExprKind::Cast { expr, target_type } => {
            let source_ty = analyze_expr(expr, resolver)?;
            let target_ty = resolve_type_checked(target_type, resolver)?;
            if !source_ty.is_numeric() || !target_ty.is_numeric() {
                return Err(sem_err(format!(
                    "cannot cast `{}` to `{}`",
                    source_ty, target_ty
                )));
            }
            Ok(target_ty)
        }
        ExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            let obj_ty = analyze_expr(object, resolver)?;
            match &obj_ty {
                Type::Struct(struct_name) => {
                    let struct_info = resolver
                        .lookup_struct(struct_name)
                        .ok_or_else(|| sem_err(format!("undefined struct `{}`", struct_name)))?
                        .clone();
                    let method_info = match struct_info.method_index.get(method.as_str()) {
                        Some(&idx) => struct_info.methods[idx].clone(),
                        None => {
                            return Err(sem_err(format!(
                                "type `{}` has no method `{}`",
                                struct_name, method
                            )));
                        }
                    };
                    if args.len() != method_info.params.len() {
                        return Err(sem_err(format!(
                            "method `{}` expects {} argument(s) but {} were given",
                            method,
                            method_info.params.len(),
                            args.len()
                        )));
                    }
                    for (arg, (param_name, param_ty)) in args.iter().zip(method_info.params.iter())
                    {
                        let arg_ty = analyze_expr(arg, resolver)?;
                        if arg_ty != *param_ty {
                            return Err(sem_err(format!(
                                "expected `{}` but got `{}` in argument `{}` of method `{}`",
                                param_ty, arg_ty, param_name, method
                            )));
                        }
                    }
                    Ok(method_info.return_type)
                }
                _ => Err(sem_err(format!(
                    "method call on non-struct type `{}`",
                    obj_ty
                ))),
            }
        }
    }
}

fn analyze_struct_members(struct_def: &StructDef, resolver: &mut Resolver) -> Result<()> {
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
                    resolver.define_var(
                        param.name.clone(),
                        VarInfo {
                            ty: resolve_type_checked(&param.ty, resolver)?,
                            mutable: false,
                        },
                    );
                }
                for stmt in &body.stmts {
                    analyze_stmt(stmt, resolver)?;
                }
                resolver.pop_scope();

                check_all_fields_initialized(&struct_def.name, body, resolver)?;

                resolver.current_return_type = prev_return;
                resolver.self_context = prev_self;
            }
            StructMember::ComputedProperty {
                ty, getter, setter, ..
            } => {
                let resolved_ty = resolve_type_checked(ty, resolver)?;

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
                    analyze_getter_block(getter, resolver)?;
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
                        analyze_stmt(stmt, resolver)?;
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
                let resolved_return = resolve_type_checked(return_type, resolver)?;
                let prev_self = resolver.self_context.clone();
                resolver.self_context = Some(SelfContext {
                    struct_name: struct_def.name.clone(),
                    mutable: false,
                });
                let prev_return = resolver.current_return_type.clone();
                resolver.current_return_type = Some(resolved_return);

                resolver.push_scope();
                for param in params {
                    resolver.define_var(
                        param.name.clone(),
                        VarInfo {
                            ty: resolve_type_checked(&param.ty, resolver)?,
                            mutable: false,
                        },
                    );
                }

                if !block_always_returns(body) {
                    return Err(sem_err(format!(
                        "method `{}` must end with a `return` statement",
                        mname
                    )));
                }
                let stmts = &body.stmts;
                for stmt in stmts {
                    if matches!(stmt, Stmt::Yield(_)) {
                        return Err(sem_err(
                            "`yield` cannot be used in method body (use `return` instead)",
                        ));
                    }
                    analyze_stmt(stmt, resolver)?;
                }

                resolver.pop_scope();
                resolver.current_return_type = prev_return;
                resolver.self_context = prev_self;
            }
        }
    }
    Ok(())
}

fn check_all_fields_initialized(
    struct_name: &str,
    body: &Block,
    resolver: &Resolver,
) -> Result<()> {
    let struct_info = resolver
        .lookup_struct(struct_name)
        .ok_or_else(|| sem_err(format!("undefined struct `{}`", struct_name)))?
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

fn analyze_getter_block(block: &Block, resolver: &mut Resolver) -> Result<()> {
    if !block_always_returns(block) {
        return Err(sem_err("getter must end with a `return` statement"));
    }
    for stmt in &block.stmts {
        analyze_stmt(stmt, resolver)?;
    }
    Ok(())
}

fn check_assignment_target_mutable(expr: &Expr, resolver: &Resolver) -> Result<()> {
    match &expr.kind {
        ExprKind::Ident(name) => match resolver.lookup_var(name) {
            Some(info) if !info.mutable => Err(sem_err(format!(
                "cannot assign to field of immutable variable `{}`",
                name
            ))),
            Some(_) => Ok(()),
            None => Err(sem_err(format!("undefined variable `{}`", name))),
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
        analyze(&program)
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
        analyze_package(&graph, "test_pkg")
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
