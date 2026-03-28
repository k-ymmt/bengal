use std::collections::HashMap;

use crate::error::Result;
use crate::interface::ModuleInterface;
use crate::package::{ModuleGraph, ModulePath};
use crate::parser::ast::*;

use super::resolver::{self, FuncSig, ProtocolInfo, Resolver, is_accessible};
use super::types::Type;
use super::{
    DiagCtxt, PackageSemanticInfo, SemanticInfo, analyze_single_module, pkg_err,
    resolve_struct_members, resolve_type_checked, sem_err,
};

// ---------------------------------------------------------------------------
// Multi-module semantic analysis
// ---------------------------------------------------------------------------

/// Kinds of top-level symbols we track across modules.
#[derive(Debug, Clone)]
pub enum SymbolKind {
    Func(FuncSig),
    Struct(resolver::StructInfo),
    Protocol(ProtocolInfo),
}

/// A single entry in the global (cross-module) symbol table.
#[derive(Debug, Clone)]
pub struct GlobalSymbol {
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub module: ModulePath,
}

/// Global symbol table: module path -> (name -> GlobalSymbol)
pub type GlobalSymbolTable = HashMap<ModulePath, HashMap<String, GlobalSymbol>>;

/// Analyze an entire package represented by its `ModuleGraph`.
///
/// This is the multi-module entry point. It performs three phases:
///   1. Collect all top-level symbols from every module.
///   2. Resolve imports for each module and check visibility.
///   3. Run the existing single-module analysis passes with imported symbols.
pub fn analyze_package(
    graph: &ModuleGraph,
    _package_name: &str,
    external_deps: &[crate::pipeline::ExternalDep],
    diag: &mut DiagCtxt,
) -> Result<PackageSemanticInfo> {
    // ---------------------------------------------------------------
    // Phase 1: Collect all top-level symbols from all modules
    // ---------------------------------------------------------------
    let mut global_symbols = collect_global_symbols(graph)?;

    // Inject external dep symbols into GlobalSymbolTable
    let mut external_dep_names: HashMap<ModulePath, String> = HashMap::new();
    for dep in external_deps {
        for (mod_path, iface) in &dep.interfaces {
            let ext_path = dep_module_path(&dep.name, mod_path);
            let symbols = interface_to_global_symbols(iface, &ext_path);
            global_symbols.insert(ext_path.clone(), symbols);
            external_dep_names.insert(ext_path, dep.package_name.clone());
        }
    }

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
        match analyze_single_module(&mod_info.ast, &mut resolver, is_root, diag) {
            Ok(sem_info) => {
                module_infos.insert(mod_path.clone(), sem_info);
            }
            Err(_) => {
                // Errors already emitted to diag; continue to next module
            }
        }
    }

    if diag.has_errors() {
        return Err(sem_err(format!("{} error(s) found", diag.error_count())));
    }

    Ok(PackageSemanticInfo {
        module_infos,
        import_sources,
        external_dep_names,
    })
}

/// Convert a `ModuleInterface` into a map of `GlobalSymbol` entries suitable
/// for injection into a `GlobalSymbolTable`.
///
/// This is the bridge that allows pre-compiled module interfaces (`.bengalmod`)
/// to participate in cross-module name resolution alongside source modules.
pub fn interface_to_global_symbols(
    iface: &ModuleInterface,
    module_path: &ModulePath,
) -> HashMap<String, GlobalSymbol> {
    let mut symbols = HashMap::new();
    for func in &iface.functions {
        symbols.insert(
            func.name.clone(),
            GlobalSymbol {
                kind: SymbolKind::Func(func.to_func_sig()),
                visibility: func.visibility,
                module: module_path.clone(),
            },
        );
    }
    for s in &iface.structs {
        symbols.insert(
            s.name.clone(),
            GlobalSymbol {
                kind: SymbolKind::Struct(s.to_struct_info()),
                visibility: s.visibility,
                module: module_path.clone(),
            },
        );
    }
    for p in &iface.protocols {
        symbols.insert(
            p.name.clone(),
            GlobalSymbol {
                kind: SymbolKind::Protocol(p.to_protocol_info()),
                visibility: p.visibility,
                module: module_path.clone(),
            },
        );
    }
    symbols
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
            tmp_resolver.push_type_params(&func.type_params);
            let params: Vec<(String, Type)> = func
                .params
                .iter()
                .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, &tmp_resolver)?)))
                .collect::<Result<Vec<_>>>()?;
            let return_type = resolve_type_checked(&func.return_type, &tmp_resolver)?;
            tmp_resolver.pop_type_params(func.type_params.len());
            let sig = FuncSig {
                type_params: func.type_params.clone(),
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

/// Build a module path for an external dependency module.
/// Prefixes the dep name to the internal module path to avoid collisions.
pub fn dep_module_path(dep_name: &str, internal_path: &ModulePath) -> ModulePath {
    let mut segments = vec![dep_name.to_string()];
    segments.extend(internal_path.0.iter().cloned());
    ModulePath(segments)
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
        let target_module = resolve_import_module_path(
            current_module,
            &import.prefix,
            &import.path,
            graph,
            global_symbols,
        )?;

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
    global_symbols: &GlobalSymbolTable,
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

    // Local module — preferred
    if graph.modules.contains_key(&result) {
        return Ok(result);
    }
    // External dependency — fallback
    if global_symbols.contains_key(&result) {
        return Ok(result);
    }
    Err(pkg_err(format!(
        "unresolved import: module '{}' not found",
        result
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dep_module_path_root() {
        let result = dep_module_path("math", &ModulePath::root());
        assert_eq!(result, ModulePath(vec!["math".to_string()]));
    }

    #[test]
    fn dep_module_path_submodule() {
        let result = dep_module_path("math", &ModulePath(vec!["utils".to_string()]));
        assert_eq!(
            result,
            ModulePath(vec!["math".to_string(), "utils".to_string()])
        );
    }

    #[test]
    fn analyze_package_injects_external_dep_symbols() {
        use crate::interface::ModuleInterface;
        use crate::parser::ast::Visibility;
        use crate::pipeline::ExternalDep;

        let iface = ModuleInterface {
            functions: vec![crate::interface::InterfaceFuncEntry {
                visibility: Visibility::Public,
                name: "ext_add".to_string(),
                sig: crate::interface::InterfaceFuncSig {
                    type_params: vec![],
                    params: vec![
                        ("a".to_string(), crate::interface::InterfaceType::I32),
                        ("b".to_string(), crate::interface::InterfaceType::I32),
                    ],
                    return_type: crate::interface::InterfaceType::I32,
                },
            }],
            structs: vec![],
            protocols: vec![],
        };

        let dep = ExternalDep {
            name: "extlib".to_string(),
            package_name: "extlib".to_string(),
            interfaces: HashMap::from([(ModulePath::root(), iface)]),
            bir_modules: HashMap::new(),
        };

        let graph = crate::package::ModuleGraph::from_source(
            "app",
            "import extlib::ext_add;\nfunc main() -> Int32 { return ext_add(1, 2); }",
        )
        .unwrap();

        let mut diag = crate::error::DiagCtxt::new();
        let result = analyze_package(&graph, "app", &[dep], &mut diag);
        assert!(
            result.is_ok(),
            "analyze_package should succeed with external dep: {:?}",
            result.err()
        );

        let pkg_info = result.unwrap();
        let source = pkg_info
            .import_sources
            .get(&(ModulePath::root(), "ext_add".to_string()));
        assert!(source.is_some(), "ext_add should be in import_sources");
        assert_eq!(source.unwrap(), &ModulePath(vec!["extlib".to_string()]));

        let dep_name = pkg_info
            .external_dep_names
            .get(&ModulePath(vec!["extlib".to_string()]));
        assert_eq!(dep_name, Some(&"extlib".to_string()));
    }
}
