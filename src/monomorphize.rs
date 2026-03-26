use std::collections::HashMap;

use crate::parser::ast::{
    Block, Expr, ExprKind, Function, Param, Program, Stmt, StructDef, StructMember, TypeAnnotation,
    TypeParam,
};
use crate::semantic::infer::InferredTypeArgs;

/// Monomorphize a program: collect all generic instantiations, generate
/// specialized (concrete) versions, rewrite call sites, and remove generic
/// definitions.
///
/// This pass runs BEFORE semantic analysis so the analyzer only sees concrete types.
/// The `inferred` side table provides type arguments that were inferred during
/// `analyze_pre_mono`; when a call site has no explicit type args but appears in
/// the side table, the inferred args are used.
pub fn monomorphize(program: &Program, inferred: &InferredTypeArgs) -> Program {
    let mut mono = Monomorphizer::new(program, inferred);
    mono.collect_instantiations();
    mono.generate_specializations();
    mono.build_output()
}

/// Mangle a name with concrete type arguments.
/// e.g. `identity` + `[Int32]` -> `identity_Int32`
fn mangle_name(name: &str, type_args: &[TypeAnnotation]) -> String {
    let mut mangled = name.to_string();
    for ta in type_args {
        mangled.push('_');
        mangled.push_str(&type_annotation_to_string(ta));
    }
    mangled
}

fn type_annotation_to_string(ta: &TypeAnnotation) -> String {
    match ta {
        TypeAnnotation::I32 => "Int32".to_string(),
        TypeAnnotation::I64 => "Int64".to_string(),
        TypeAnnotation::F32 => "Float32".to_string(),
        TypeAnnotation::F64 => "Float64".to_string(),
        TypeAnnotation::Bool => "Bool".to_string(),
        TypeAnnotation::Unit => "Unit".to_string(),
        TypeAnnotation::Named(s) => s.clone(),
        TypeAnnotation::Generic { name, args } => {
            let mut result = name.clone();
            for arg in args {
                result.push('_');
                result.push_str(&type_annotation_to_string(arg));
            }
            result
        }
        TypeAnnotation::Array { element, size } => {
            format!("Array_{}_{}", type_annotation_to_string(element), size)
        }
    }
}

/// A unique instantiation key: (original name, concrete type args).
type InstKey = (String, Vec<TypeAnnotation>);

struct Monomorphizer<'a> {
    program: &'a Program,
    /// Inferred type arguments from pre-mono analysis.
    inferred: &'a InferredTypeArgs,
    /// All unique function instantiations found.
    func_insts: Vec<InstKey>,
    /// All unique struct instantiations found.
    struct_insts: Vec<InstKey>,
    /// Map from original generic func name to its definition.
    generic_funcs: HashMap<String, &'a Function>,
    /// Map from original generic struct name to its definition.
    generic_structs: HashMap<String, &'a StructDef>,
    /// Generated specialized functions.
    specialized_funcs: Vec<Function>,
    /// Generated specialized structs.
    specialized_structs: Vec<StructDef>,
}

impl<'a> Monomorphizer<'a> {
    fn new(program: &'a Program, inferred: &'a InferredTypeArgs) -> Self {
        let generic_funcs: HashMap<String, &Function> = program
            .functions
            .iter()
            .filter(|f| !f.type_params.is_empty())
            .map(|f| (f.name.clone(), f))
            .collect();

        let generic_structs: HashMap<String, &StructDef> = program
            .structs
            .iter()
            .filter(|s| !s.type_params.is_empty())
            .map(|s| (s.name.clone(), s))
            .collect();

        Monomorphizer {
            program,
            inferred,
            func_insts: Vec::new(),
            struct_insts: Vec::new(),
            generic_funcs,
            generic_structs,
            specialized_funcs: Vec::new(),
            specialized_structs: Vec::new(),
        }
    }

    /// Walk the entire program AST to find all generic call sites and struct inits.
    fn collect_instantiations(&mut self) {
        for func in &self.program.functions {
            self.collect_from_block(&func.body);
        }
        for struct_def in &self.program.structs {
            for member in &struct_def.members {
                match member {
                    StructMember::Initializer { body, .. } => {
                        self.collect_from_block(body);
                    }
                    StructMember::Method { body, .. } => {
                        self.collect_from_block(body);
                    }
                    StructMember::ComputedProperty { getter, setter, .. } => {
                        self.collect_from_block(getter);
                        if let Some(s) = setter {
                            self.collect_from_block(s);
                        }
                    }
                    StructMember::StoredProperty { .. } => {}
                }
            }
        }
    }

    fn collect_from_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.collect_from_stmt(stmt);
        }
    }

    fn collect_from_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { value, .. } | Stmt::Var { value, .. } | Stmt::Assign { value, .. } => {
                self.collect_from_expr(value);
            }
            Stmt::Return(Some(expr)) | Stmt::Yield(expr) | Stmt::Break(Some(expr)) => {
                self.collect_from_expr(expr);
            }
            Stmt::Expr(expr) => {
                self.collect_from_expr(expr);
            }
            Stmt::FieldAssign { object, value, .. } => {
                self.collect_from_expr(object);
                self.collect_from_expr(value);
            }
            Stmt::IndexAssign {
                object,
                index,
                value,
            } => {
                self.collect_from_expr(object);
                self.collect_from_expr(index);
                self.collect_from_expr(value);
            }
            Stmt::Return(None) | Stmt::Break(None) | Stmt::Continue => {}
        }
    }

    fn collect_from_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Call {
                name,
                type_args,
                args,
            } => {
                // Determine the effective type args: explicit ones, or inferred
                // from the side table when none were written.
                let effective_type_args = if !type_args.is_empty() {
                    type_args.clone()
                } else if let Some(site) = self.inferred.map.get(&expr.id) {
                    site.type_args.clone()
                } else {
                    vec![]
                };

                if !effective_type_args.is_empty() {
                    // Could be either a function call or struct init parsed as Call
                    if self.generic_funcs.contains_key(name) {
                        let key = (name.clone(), effective_type_args.clone());
                        if !self.func_insts.contains(&key) {
                            self.func_insts.push(key);
                        }
                    }
                    if self.generic_structs.contains_key(name) {
                        let key = (name.clone(), effective_type_args.clone());
                        if !self.struct_insts.contains(&key) {
                            self.struct_insts.push(key);
                        }
                    }
                }
                for arg in args {
                    self.collect_from_expr(arg);
                }
            }
            ExprKind::StructInit {
                name,
                type_args,
                args,
            } => {
                // Determine the effective type args
                let effective_type_args = if !type_args.is_empty() {
                    type_args.clone()
                } else if let Some(site) = self.inferred.map.get(&expr.id) {
                    site.type_args.clone()
                } else {
                    vec![]
                };

                if !effective_type_args.is_empty() && self.generic_structs.contains_key(name) {
                    let key = (name.clone(), effective_type_args.clone());
                    if !self.struct_insts.contains(&key) {
                        self.struct_insts.push(key);
                    }
                }
                for (_, arg_expr) in args {
                    self.collect_from_expr(arg_expr);
                }
            }
            ExprKind::BinaryOp { left, right, .. } => {
                self.collect_from_expr(left);
                self.collect_from_expr(right);
            }
            ExprKind::UnaryOp { operand, .. } => {
                self.collect_from_expr(operand);
            }
            ExprKind::Block(block) => {
                self.collect_from_block(block);
            }
            ExprKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.collect_from_expr(condition);
                self.collect_from_block(then_block);
                if let Some(eb) = else_block {
                    self.collect_from_block(eb);
                }
            }
            ExprKind::While {
                condition,
                body,
                nobreak,
            } => {
                self.collect_from_expr(condition);
                self.collect_from_block(body);
                if let Some(nb) = nobreak {
                    self.collect_from_block(nb);
                }
            }
            ExprKind::Cast { expr, .. } => {
                self.collect_from_expr(expr);
            }
            ExprKind::FieldAccess { object, .. } => {
                self.collect_from_expr(object);
            }
            ExprKind::MethodCall { object, args, .. } => {
                self.collect_from_expr(object);
                for arg in args {
                    self.collect_from_expr(arg);
                }
            }
            ExprKind::ArrayLiteral { elements } => {
                for elem in elements {
                    self.collect_from_expr(elem);
                }
            }
            ExprKind::IndexAccess { object, index } => {
                self.collect_from_expr(object);
                self.collect_from_expr(index);
            }
            ExprKind::Number(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Ident(_)
            | ExprKind::SelfRef => {}
        }
    }

    /// For each collected instantiation, clone the generic definition and substitute types.
    fn generate_specializations(&mut self) {
        // Build a map of all struct specialization renames so we can rewrite references
        // inside method bodies.
        let mut struct_rename_map: HashMap<String, Vec<(Vec<TypeAnnotation>, String)>> =
            HashMap::new();
        for (name, type_args) in &self.struct_insts {
            let mangled = mangle_name(name, type_args);
            struct_rename_map
                .entry(name.clone())
                .or_default()
                .push((type_args.clone(), mangled));
        }

        // Specialize functions
        for (name, type_args) in &self.func_insts {
            let generic_def = self.generic_funcs[name];
            let mangled = mangle_name(name, type_args);
            let subst = build_substitution(&generic_def.type_params, type_args);
            let specialized =
                specialize_function(generic_def, &mangled, &subst, &struct_rename_map);
            self.specialized_funcs.push(specialized);
        }

        // Specialize structs
        for (name, type_args) in &self.struct_insts {
            let generic_def = self.generic_structs[name];
            let mangled = mangle_name(name, type_args);
            let subst = build_substitution(&generic_def.type_params, type_args);
            let specialized = specialize_struct(generic_def, &mangled, &subst, &struct_rename_map);
            self.specialized_structs.push(specialized);
        }
    }

    /// Build the final program with specialized versions, rewritten call sites,
    /// and generic definitions removed.
    fn build_output(&self) -> Program {
        // Build rename maps for rewriting call sites
        let mut func_rename: HashMap<(String, Vec<TypeAnnotation>), String> = HashMap::new();
        for (name, type_args) in &self.func_insts {
            let mangled = mangle_name(name, type_args);
            func_rename.insert((name.clone(), type_args.clone()), mangled);
        }

        let mut struct_rename: HashMap<(String, Vec<TypeAnnotation>), String> = HashMap::new();
        for (name, type_args) in &self.struct_insts {
            let mangled = mangle_name(name, type_args);
            struct_rename.insert((name.clone(), type_args.clone()), mangled);
        }

        let rename_map = RenameMap {
            func_rename,
            struct_rename,
            inferred: self.inferred,
        };

        // Keep non-generic functions, rewrite their bodies
        let mut functions: Vec<Function> = self
            .program
            .functions
            .iter()
            .filter(|f| f.type_params.is_empty())
            .map(|f| rewrite_function(f, &rename_map))
            .collect();

        // Add specialized functions
        for sf in &self.specialized_funcs {
            functions.push(rewrite_function(sf, &rename_map));
        }

        // Keep non-generic structs, rewrite their bodies
        let mut structs: Vec<StructDef> = self
            .program
            .structs
            .iter()
            .filter(|s| s.type_params.is_empty())
            .map(|s| rewrite_struct(s, &rename_map))
            .collect();

        // Add specialized structs
        for ss in &self.specialized_structs {
            structs.push(rewrite_struct(ss, &rename_map));
        }

        Program {
            module_decls: self.program.module_decls.clone(),
            import_decls: self.program.import_decls.clone(),
            structs,
            protocols: self.program.protocols.clone(),
            functions,
        }
    }
}

/// Build a substitution map from type param names to concrete types.
fn build_substitution(
    type_params: &[TypeParam],
    type_args: &[TypeAnnotation],
) -> HashMap<String, TypeAnnotation> {
    type_params
        .iter()
        .zip(type_args.iter())
        .map(|(param, arg)| (param.name.clone(), arg.clone()))
        .collect()
}

/// Substitute type annotations according to the substitution map.
fn substitute_type(ta: &TypeAnnotation, subst: &HashMap<String, TypeAnnotation>) -> TypeAnnotation {
    match ta {
        TypeAnnotation::Named(name) => {
            if let Some(concrete) = subst.get(name) {
                concrete.clone()
            } else {
                ta.clone()
            }
        }
        TypeAnnotation::Generic { name, args } => {
            let new_args: Vec<TypeAnnotation> =
                args.iter().map(|a| substitute_type(a, subst)).collect();
            TypeAnnotation::Generic {
                name: name.clone(),
                args: new_args,
            }
        }
        TypeAnnotation::Array { element, size } => TypeAnnotation::Array {
            element: Box::new(substitute_type(element, subst)),
            size: *size,
        },
        _ => ta.clone(),
    }
}

fn substitute_param(param: &Param, subst: &HashMap<String, TypeAnnotation>) -> Param {
    Param {
        name: param.name.clone(),
        ty: substitute_type(&param.ty, subst),
    }
}

fn substitute_block(
    block: &Block,
    subst: &HashMap<String, TypeAnnotation>,
    struct_rename_map: &HashMap<String, Vec<(Vec<TypeAnnotation>, String)>>,
) -> Block {
    Block {
        stmts: block
            .stmts
            .iter()
            .map(|s| substitute_stmt(s, subst, struct_rename_map))
            .collect(),
    }
}

fn substitute_stmt(
    stmt: &Stmt,
    subst: &HashMap<String, TypeAnnotation>,
    struct_rename_map: &HashMap<String, Vec<(Vec<TypeAnnotation>, String)>>,
) -> Stmt {
    match stmt {
        Stmt::Let { name, ty, value } => Stmt::Let {
            name: name.clone(),
            ty: ty.as_ref().map(|t| substitute_type(t, subst)),
            value: substitute_expr(value, subst, struct_rename_map),
        },
        Stmt::Var { name, ty, value } => Stmt::Var {
            name: name.clone(),
            ty: ty.as_ref().map(|t| substitute_type(t, subst)),
            value: substitute_expr(value, subst, struct_rename_map),
        },
        Stmt::Assign { name, value } => Stmt::Assign {
            name: name.clone(),
            value: substitute_expr(value, subst, struct_rename_map),
        },
        Stmt::Return(Some(expr)) => {
            Stmt::Return(Some(substitute_expr(expr, subst, struct_rename_map)))
        }
        Stmt::Return(None) => Stmt::Return(None),
        Stmt::Yield(expr) => Stmt::Yield(substitute_expr(expr, subst, struct_rename_map)),
        Stmt::Break(Some(expr)) => {
            Stmt::Break(Some(substitute_expr(expr, subst, struct_rename_map)))
        }
        Stmt::Break(None) => Stmt::Break(None),
        Stmt::Continue => Stmt::Continue,
        Stmt::Expr(expr) => Stmt::Expr(substitute_expr(expr, subst, struct_rename_map)),
        Stmt::FieldAssign {
            object,
            field,
            value,
        } => Stmt::FieldAssign {
            object: Box::new(substitute_expr(object, subst, struct_rename_map)),
            field: field.clone(),
            value: substitute_expr(value, subst, struct_rename_map),
        },
        Stmt::IndexAssign {
            object,
            index,
            value,
        } => Stmt::IndexAssign {
            object: Box::new(substitute_expr(object, subst, struct_rename_map)),
            index: Box::new(substitute_expr(index, subst, struct_rename_map)),
            value: substitute_expr(value, subst, struct_rename_map),
        },
    }
}

fn substitute_expr(
    expr: &Expr,
    subst: &HashMap<String, TypeAnnotation>,
    struct_rename_map: &HashMap<String, Vec<(Vec<TypeAnnotation>, String)>>,
) -> Expr {
    let kind = match &expr.kind {
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            let new_args: Vec<Expr> = args
                .iter()
                .map(|a| substitute_expr(a, subst, struct_rename_map))
                .collect();
            let new_type_args: Vec<TypeAnnotation> = type_args
                .iter()
                .map(|t| substitute_type(t, subst))
                .collect();

            // If this call references a generic struct, rewrite name to the
            // specialized version (within a generic body being specialized).
            if !new_type_args.is_empty()
                && let Some(renames) = struct_rename_map.get(name)
            {
                for (args_key, mangled) in renames {
                    if *args_key == new_type_args {
                        return Expr {
                            id: expr.id,
                            kind: ExprKind::Call {
                                name: mangled.clone(),
                                type_args: vec![],
                                args: new_args,
                            },
                            span: expr.span,
                        };
                    }
                }
            }

            ExprKind::Call {
                name: name.clone(),
                type_args: new_type_args,
                args: new_args,
            }
        }
        ExprKind::StructInit {
            name,
            type_args,
            args,
        } => {
            let new_args: Vec<(String, Expr)> = args
                .iter()
                .map(|(n, e)| (n.clone(), substitute_expr(e, subst, struct_rename_map)))
                .collect();
            let new_type_args: Vec<TypeAnnotation> = type_args
                .iter()
                .map(|t| substitute_type(t, subst))
                .collect();

            if !new_type_args.is_empty()
                && let Some(renames) = struct_rename_map.get(name)
            {
                for (args_key, mangled) in renames {
                    if *args_key == new_type_args {
                        return Expr {
                            id: expr.id,
                            kind: ExprKind::StructInit {
                                name: mangled.clone(),
                                type_args: vec![],
                                args: new_args,
                            },
                            span: expr.span,
                        };
                    }
                }
            }

            ExprKind::StructInit {
                name: name.clone(),
                type_args: new_type_args,
                args: new_args,
            }
        }
        ExprKind::BinaryOp { op, left, right } => ExprKind::BinaryOp {
            op: *op,
            left: Box::new(substitute_expr(left, subst, struct_rename_map)),
            right: Box::new(substitute_expr(right, subst, struct_rename_map)),
        },
        ExprKind::UnaryOp { op, operand } => ExprKind::UnaryOp {
            op: *op,
            operand: Box::new(substitute_expr(operand, subst, struct_rename_map)),
        },
        ExprKind::Block(block) => {
            ExprKind::Block(substitute_block(block, subst, struct_rename_map))
        }
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => ExprKind::If {
            condition: Box::new(substitute_expr(condition, subst, struct_rename_map)),
            then_block: substitute_block(then_block, subst, struct_rename_map),
            else_block: else_block
                .as_ref()
                .map(|eb| substitute_block(eb, subst, struct_rename_map)),
        },
        ExprKind::While {
            condition,
            body,
            nobreak,
        } => ExprKind::While {
            condition: Box::new(substitute_expr(condition, subst, struct_rename_map)),
            body: substitute_block(body, subst, struct_rename_map),
            nobreak: nobreak
                .as_ref()
                .map(|nb| substitute_block(nb, subst, struct_rename_map)),
        },
        ExprKind::Cast { expr, target_type } => ExprKind::Cast {
            expr: Box::new(substitute_expr(expr, subst, struct_rename_map)),
            target_type: substitute_type(target_type, subst),
        },
        ExprKind::FieldAccess { object, field } => ExprKind::FieldAccess {
            object: Box::new(substitute_expr(object, subst, struct_rename_map)),
            field: field.clone(),
        },
        ExprKind::MethodCall {
            object,
            method,
            args,
        } => ExprKind::MethodCall {
            object: Box::new(substitute_expr(object, subst, struct_rename_map)),
            method: method.clone(),
            args: args
                .iter()
                .map(|a| substitute_expr(a, subst, struct_rename_map))
                .collect(),
        },
        ExprKind::ArrayLiteral { elements } => ExprKind::ArrayLiteral {
            elements: elements
                .iter()
                .map(|e| substitute_expr(e, subst, struct_rename_map))
                .collect(),
        },
        ExprKind::IndexAccess { object, index } => ExprKind::IndexAccess {
            object: Box::new(substitute_expr(object, subst, struct_rename_map)),
            index: Box::new(substitute_expr(index, subst, struct_rename_map)),
        },
        ExprKind::Number(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Ident(_)
        | ExprKind::SelfRef => expr.kind.clone(),
    };
    Expr {
        id: expr.id,
        kind,
        span: expr.span,
    }
}

/// Create a specialized function from a generic one.
fn specialize_function(
    generic: &Function,
    mangled_name: &str,
    subst: &HashMap<String, TypeAnnotation>,
    struct_rename_map: &HashMap<String, Vec<(Vec<TypeAnnotation>, String)>>,
) -> Function {
    Function {
        visibility: generic.visibility,
        name: mangled_name.to_string(),
        type_params: vec![], // no longer generic
        params: generic
            .params
            .iter()
            .map(|p| substitute_param(p, subst))
            .collect(),
        return_type: substitute_type(&generic.return_type, subst),
        body: substitute_block(&generic.body, subst, struct_rename_map),
    }
}

/// Create a specialized struct from a generic one.
fn specialize_struct(
    generic: &StructDef,
    mangled_name: &str,
    subst: &HashMap<String, TypeAnnotation>,
    struct_rename_map: &HashMap<String, Vec<(Vec<TypeAnnotation>, String)>>,
) -> StructDef {
    let members = generic
        .members
        .iter()
        .map(|m| specialize_member(m, subst, struct_rename_map))
        .collect();

    StructDef {
        visibility: generic.visibility,
        name: mangled_name.to_string(),
        type_params: vec![], // no longer generic
        conformances: generic.conformances.clone(),
        members,
    }
}

fn specialize_member(
    member: &StructMember,
    subst: &HashMap<String, TypeAnnotation>,
    struct_rename_map: &HashMap<String, Vec<(Vec<TypeAnnotation>, String)>>,
) -> StructMember {
    match member {
        StructMember::StoredProperty {
            visibility,
            name,
            ty,
        } => StructMember::StoredProperty {
            visibility: *visibility,
            name: name.clone(),
            ty: substitute_type(ty, subst),
        },
        StructMember::ComputedProperty {
            visibility,
            name,
            ty,
            getter,
            setter,
        } => StructMember::ComputedProperty {
            visibility: *visibility,
            name: name.clone(),
            ty: substitute_type(ty, subst),
            getter: substitute_block(getter, subst, struct_rename_map),
            setter: setter
                .as_ref()
                .map(|s| substitute_block(s, subst, struct_rename_map)),
        },
        StructMember::Initializer {
            visibility,
            params,
            body,
        } => StructMember::Initializer {
            visibility: *visibility,
            params: params.iter().map(|p| substitute_param(p, subst)).collect(),
            body: substitute_block(body, subst, struct_rename_map),
        },
        StructMember::Method {
            visibility,
            name,
            params,
            return_type,
            body,
        } => StructMember::Method {
            visibility: *visibility,
            name: name.clone(),
            params: params.iter().map(|p| substitute_param(p, subst)).collect(),
            return_type: substitute_type(return_type, subst),
            body: substitute_block(body, subst, struct_rename_map),
        },
    }
}

// ---------------------------------------------------------------------------
// Rewriting call sites in the output program
// ---------------------------------------------------------------------------

struct RenameMap<'a> {
    /// (original_name, type_args) -> mangled_name for functions
    func_rename: HashMap<(String, Vec<TypeAnnotation>), String>,
    /// (original_name, type_args) -> mangled_name for structs
    struct_rename: HashMap<(String, Vec<TypeAnnotation>), String>,
    /// Inferred type arguments from pre-mono analysis (for call sites with omitted type args)
    inferred: &'a InferredTypeArgs,
}

fn rewrite_function(func: &Function, rename_map: &RenameMap) -> Function {
    Function {
        visibility: func.visibility,
        name: func.name.clone(),
        type_params: func.type_params.clone(),
        params: func.params.clone(),
        return_type: func.return_type.clone(),
        body: rewrite_block(&func.body, rename_map),
    }
}

fn rewrite_struct(s: &StructDef, rename_map: &RenameMap) -> StructDef {
    let members = s
        .members
        .iter()
        .map(|m| rewrite_member(m, rename_map))
        .collect();

    StructDef {
        visibility: s.visibility,
        name: s.name.clone(),
        type_params: s.type_params.clone(),
        conformances: s.conformances.clone(),
        members,
    }
}

fn rewrite_member(member: &StructMember, rename_map: &RenameMap) -> StructMember {
    match member {
        StructMember::StoredProperty { .. } => member.clone(),
        StructMember::ComputedProperty {
            visibility,
            name,
            ty,
            getter,
            setter,
        } => StructMember::ComputedProperty {
            visibility: *visibility,
            name: name.clone(),
            ty: ty.clone(),
            getter: rewrite_block(getter, rename_map),
            setter: setter.as_ref().map(|s| rewrite_block(s, rename_map)),
        },
        StructMember::Initializer {
            visibility,
            params,
            body,
        } => StructMember::Initializer {
            visibility: *visibility,
            params: params.clone(),
            body: rewrite_block(body, rename_map),
        },
        StructMember::Method {
            visibility,
            name,
            params,
            return_type,
            body,
        } => StructMember::Method {
            visibility: *visibility,
            name: name.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
            body: rewrite_block(body, rename_map),
        },
    }
}

fn rewrite_block(block: &Block, rename_map: &RenameMap) -> Block {
    Block {
        stmts: block
            .stmts
            .iter()
            .map(|s| rewrite_stmt(s, rename_map))
            .collect(),
    }
}

fn rewrite_type_annotation(ta: &TypeAnnotation, rename_map: &RenameMap) -> TypeAnnotation {
    match ta {
        TypeAnnotation::Generic { name, args } => {
            let key = (name.clone(), args.clone());
            if let Some(mangled) = rename_map.struct_rename.get(&key) {
                TypeAnnotation::Named(mangled.clone())
            } else {
                TypeAnnotation::Generic {
                    name: name.clone(),
                    args: args
                        .iter()
                        .map(|a| rewrite_type_annotation(a, rename_map))
                        .collect(),
                }
            }
        }
        TypeAnnotation::Array { element, size } => TypeAnnotation::Array {
            element: Box::new(rewrite_type_annotation(element, rename_map)),
            size: *size,
        },
        _ => ta.clone(),
    }
}

fn rewrite_stmt(stmt: &Stmt, rename_map: &RenameMap) -> Stmt {
    match stmt {
        Stmt::Let { name, ty, value } => Stmt::Let {
            name: name.clone(),
            ty: ty.as_ref().map(|t| rewrite_type_annotation(t, rename_map)),
            value: rewrite_expr(value, rename_map),
        },
        Stmt::Var { name, ty, value } => Stmt::Var {
            name: name.clone(),
            ty: ty.as_ref().map(|t| rewrite_type_annotation(t, rename_map)),
            value: rewrite_expr(value, rename_map),
        },
        Stmt::Assign { name, value } => Stmt::Assign {
            name: name.clone(),
            value: rewrite_expr(value, rename_map),
        },
        Stmt::Return(Some(expr)) => Stmt::Return(Some(rewrite_expr(expr, rename_map))),
        Stmt::Return(None) => Stmt::Return(None),
        Stmt::Yield(expr) => Stmt::Yield(rewrite_expr(expr, rename_map)),
        Stmt::Break(Some(expr)) => Stmt::Break(Some(rewrite_expr(expr, rename_map))),
        Stmt::Break(None) => Stmt::Break(None),
        Stmt::Continue => Stmt::Continue,
        Stmt::Expr(expr) => Stmt::Expr(rewrite_expr(expr, rename_map)),
        Stmt::FieldAssign {
            object,
            field,
            value,
        } => Stmt::FieldAssign {
            object: Box::new(rewrite_expr(object, rename_map)),
            field: field.clone(),
            value: rewrite_expr(value, rename_map),
        },
        Stmt::IndexAssign {
            object,
            index,
            value,
        } => Stmt::IndexAssign {
            object: Box::new(rewrite_expr(object, rename_map)),
            index: Box::new(rewrite_expr(index, rename_map)),
            value: rewrite_expr(value, rename_map),
        },
    }
}

fn rewrite_expr(expr: &Expr, rename_map: &RenameMap) -> Expr {
    let kind = match &expr.kind {
        ExprKind::Call {
            name,
            type_args,
            args,
        } => {
            let new_args: Vec<Expr> = args.iter().map(|a| rewrite_expr(a, rename_map)).collect();

            // Determine effective type args: explicit, or inferred from side table
            let effective_type_args = if !type_args.is_empty() {
                type_args.clone()
            } else if let Some(site) = rename_map.inferred.map.get(&expr.id) {
                site.type_args.clone()
            } else {
                vec![]
            };

            if !effective_type_args.is_empty() {
                // Check function rename
                let key = (name.clone(), effective_type_args.clone());
                if let Some(mangled) = rename_map.func_rename.get(&key) {
                    return Expr {
                        id: expr.id,
                        kind: ExprKind::Call {
                            name: mangled.clone(),
                            type_args: vec![],
                            args: new_args,
                        },
                        span: expr.span,
                    };
                }
                // Check struct rename (parser may emit Call for struct constructors)
                if let Some(mangled) = rename_map.struct_rename.get(&key) {
                    return Expr {
                        id: expr.id,
                        kind: ExprKind::Call {
                            name: mangled.clone(),
                            type_args: vec![],
                            args: new_args,
                        },
                        span: expr.span,
                    };
                }
            }

            ExprKind::Call {
                name: name.clone(),
                type_args: type_args.clone(),
                args: new_args,
            }
        }
        ExprKind::StructInit {
            name,
            type_args,
            args,
        } => {
            let new_args: Vec<(String, Expr)> = args
                .iter()
                .map(|(n, e)| (n.clone(), rewrite_expr(e, rename_map)))
                .collect();

            // Determine effective type args: explicit, or inferred from side table
            let effective_type_args = if !type_args.is_empty() {
                type_args.clone()
            } else if let Some(site) = rename_map.inferred.map.get(&expr.id) {
                site.type_args.clone()
            } else {
                vec![]
            };

            if !effective_type_args.is_empty() {
                let key = (name.clone(), effective_type_args.clone());
                if let Some(mangled) = rename_map.struct_rename.get(&key) {
                    return Expr {
                        id: expr.id,
                        kind: ExprKind::StructInit {
                            name: mangled.clone(),
                            type_args: vec![],
                            args: new_args,
                        },
                        span: expr.span,
                    };
                }
            }

            ExprKind::StructInit {
                name: name.clone(),
                type_args: type_args.clone(),
                args: new_args,
            }
        }
        ExprKind::BinaryOp { op, left, right } => ExprKind::BinaryOp {
            op: *op,
            left: Box::new(rewrite_expr(left, rename_map)),
            right: Box::new(rewrite_expr(right, rename_map)),
        },
        ExprKind::UnaryOp { op, operand } => ExprKind::UnaryOp {
            op: *op,
            operand: Box::new(rewrite_expr(operand, rename_map)),
        },
        ExprKind::Block(block) => ExprKind::Block(rewrite_block(block, rename_map)),
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => ExprKind::If {
            condition: Box::new(rewrite_expr(condition, rename_map)),
            then_block: rewrite_block(then_block, rename_map),
            else_block: else_block.as_ref().map(|eb| rewrite_block(eb, rename_map)),
        },
        ExprKind::While {
            condition,
            body,
            nobreak,
        } => ExprKind::While {
            condition: Box::new(rewrite_expr(condition, rename_map)),
            body: rewrite_block(body, rename_map),
            nobreak: nobreak.as_ref().map(|nb| rewrite_block(nb, rename_map)),
        },
        ExprKind::Cast { expr, target_type } => ExprKind::Cast {
            expr: Box::new(rewrite_expr(expr, rename_map)),
            target_type: target_type.clone(),
        },
        ExprKind::FieldAccess { object, field } => ExprKind::FieldAccess {
            object: Box::new(rewrite_expr(object, rename_map)),
            field: field.clone(),
        },
        ExprKind::MethodCall {
            object,
            method,
            args,
        } => ExprKind::MethodCall {
            object: Box::new(rewrite_expr(object, rename_map)),
            method: method.clone(),
            args: args.iter().map(|a| rewrite_expr(a, rename_map)).collect(),
        },
        ExprKind::ArrayLiteral { elements } => ExprKind::ArrayLiteral {
            elements: elements
                .iter()
                .map(|e| rewrite_expr(e, rename_map))
                .collect(),
        },
        ExprKind::IndexAccess { object, index } => ExprKind::IndexAccess {
            object: Box::new(rewrite_expr(object, rename_map)),
            index: Box::new(rewrite_expr(index, rename_map)),
        },
        ExprKind::Number(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Ident(_)
        | ExprKind::SelfRef => expr.kind.clone(),
    };
    Expr {
        id: expr.id,
        kind,
        span: expr.span,
    }
}
