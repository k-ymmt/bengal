use std::collections::{HashMap, HashSet};

use super::types::Type;
use crate::error::{BengalError, Result, Span};
use crate::package::ModulePath;
use crate::parser::ast::{Block, NodeId, TypeParam, Visibility};

#[derive(Debug, Clone)]
pub struct VarInfo {
    pub ty: Type,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct FuncSig {
    pub type_params: Vec<TypeParam>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub type_params: Vec<TypeParam>,
    pub fields: Vec<(String, Type)>,
    pub field_index: HashMap<String, usize>,
    pub computed: Vec<ComputedPropInfo>,
    pub computed_index: HashMap<String, usize>,
    pub init: InitializerInfo,
    pub methods: Vec<MethodInfo>,
    pub method_index: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct ComputedPropInfo {
    pub name: String,
    pub ty: Type,
    pub has_setter: bool,
    pub getter: Block,
    pub setter: Option<Block>,
}

#[derive(Debug, Clone)]
pub struct InitializerInfo {
    pub params: Vec<(String, Type)>,
    pub body: Option<Block>,
}

#[derive(Debug, Clone)]
pub struct SelfContext {
    pub struct_name: String,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct ProtocolMethodSig {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct ProtocolPropertyReq {
    pub name: String,
    pub ty: Type,
    pub has_setter: bool,
}

#[derive(Debug, Clone)]
pub struct ProtocolInfo {
    pub name: String,
    pub methods: Vec<ProtocolMethodSig>,
    pub properties: Vec<ProtocolPropertyReq>,
}

#[derive(Default)]
pub struct Resolver {
    scopes: Vec<HashMap<String, VarInfo>>,
    functions: HashMap<String, FuncSig>,
    pub current_return_type: Option<Type>,
    loop_depth: u32,
    loop_break_types: Vec<Option<Type>>,
    struct_defs: HashMap<String, StructInfo>,
    protocol_defs: HashMap<String, ProtocolInfo>,
    pub self_context: Option<SelfContext>,
    pub struct_init_calls: HashSet<NodeId>,
    // Type parameters currently in scope (for generic functions/structs)
    current_type_params: Vec<TypeParam>,
    // Import maps: symbols brought in from other modules
    imported_funcs: HashMap<String, FuncSig>,
    imported_structs: HashMap<String, StructInfo>,
    imported_protocols: HashMap<String, ProtocolInfo>,
}

impl Resolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn define_var(&mut self, name: String, info: VarInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, info);
        }
    }

    pub fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    pub fn define_func(&mut self, name: String, sig: FuncSig) {
        self.functions.insert(name, sig);
    }

    pub fn lookup_func(&self, name: &str) -> Option<&FuncSig> {
        self.functions
            .get(name)
            .or_else(|| self.imported_funcs.get(name))
    }

    pub fn define_struct(&mut self, name: String, info: StructInfo) {
        self.struct_defs.insert(name, info);
    }

    pub fn lookup_struct(&self, name: &str) -> Option<&StructInfo> {
        self.struct_defs
            .get(name)
            .or_else(|| self.imported_structs.get(name))
    }

    pub fn define_protocol(&mut self, name: String, info: ProtocolInfo) {
        self.protocol_defs.insert(name, info);
    }

    pub fn lookup_protocol(&self, name: &str) -> Option<&ProtocolInfo> {
        self.protocol_defs
            .get(name)
            .or_else(|| self.imported_protocols.get(name))
    }

    pub fn reserve_struct(&mut self, name: String) {
        self.struct_defs.insert(
            name,
            StructInfo {
                type_params: vec![],
                fields: vec![],
                field_index: HashMap::new(),
                computed: vec![],
                computed_index: HashMap::new(),
                init: InitializerInfo {
                    params: vec![],
                    body: None,
                },
                methods: vec![],
                method_index: HashMap::new(),
            },
        );
    }

    pub fn record_struct_init_call(&mut self, id: NodeId) {
        self.struct_init_calls.insert(id);
    }

    pub fn take_struct_defs(&mut self) -> HashMap<String, StructInfo> {
        std::mem::take(&mut self.struct_defs)
    }

    /// Take all struct definitions (local + imported) for use in BIR lowering.
    pub fn take_all_struct_defs(&mut self) -> HashMap<String, StructInfo> {
        let mut all = std::mem::take(&mut self.struct_defs);
        for (name, info) in std::mem::take(&mut self.imported_structs) {
            all.entry(name).or_insert(info);
        }
        all
    }

    pub fn take_struct_init_calls(&mut self) -> HashSet<NodeId> {
        std::mem::take(&mut self.struct_init_calls)
    }

    pub fn take_protocols(&mut self) -> HashMap<String, ProtocolInfo> {
        std::mem::take(&mut self.protocol_defs)
    }

    /// Take all protocol definitions (local + imported) for use in BIR lowering.
    pub fn take_all_protocols(&mut self) -> HashMap<String, ProtocolInfo> {
        let mut all = std::mem::take(&mut self.protocol_defs);
        for (name, info) in std::mem::take(&mut self.imported_protocols) {
            all.entry(name).or_insert(info);
        }
        all
    }

    pub fn enter_loop(&mut self) {
        self.loop_depth += 1;
        self.loop_break_types.push(None);
    }

    pub fn exit_loop(&mut self) -> Option<Type> {
        self.loop_depth -= 1;
        self.loop_break_types.pop().flatten()
    }

    pub fn in_loop(&self) -> bool {
        self.loop_depth > 0
    }

    pub fn set_break_type(&mut self, ty: Type) -> Result<()> {
        let current = self.loop_break_types.last_mut().unwrap();
        match current {
            Some(existing) if *existing != ty => Err(BengalError::SemanticError {
                message: format!(
                    "break type mismatch: expected `{}`, found `{}`",
                    existing, ty
                ),
                span: Span { start: 0, end: 0 },
                help: None,
            }),
            Some(_) => Ok(()),
            None => {
                *current = Some(ty);
                Ok(())
            }
        }
    }

    /// Like `set_break_type`, but uses `InferenceContext::unify` when there's
    /// already a break type, so inference variables are handled correctly.
    pub fn set_or_unify_break_type(
        &mut self,
        ty: Type,
        ctx: &mut crate::semantic::infer::InferenceContext,
    ) -> Result<()> {
        let current = self.loop_break_types.last_mut().unwrap();
        match current {
            Some(existing) => {
                ctx.unify(ty, existing.clone())?;
                Ok(())
            }
            None => {
                *current = Some(ty);
                Ok(())
            }
        }
    }

    // Type parameter scope management

    pub fn push_type_params(&mut self, params: &[TypeParam]) {
        self.current_type_params.extend(params.iter().cloned());
    }

    pub fn pop_type_params(&mut self, count: usize) {
        let new_len = self.current_type_params.len().saturating_sub(count);
        self.current_type_params.truncate(new_len);
    }

    pub fn lookup_type_param(&self, name: &str) -> Option<&TypeParam> {
        self.current_type_params
            .iter()
            .rev()
            .find(|tp| tp.name == name)
    }

    // Import methods for cross-module analysis

    pub fn import_func(&mut self, name: String, sig: FuncSig) {
        self.imported_funcs.insert(name, sig);
    }

    pub fn import_struct(&mut self, name: String, info: StructInfo) {
        self.imported_structs.insert(name, info);
    }

    pub fn import_protocol(&mut self, name: String, info: ProtocolInfo) {
        self.imported_protocols.insert(name, info);
    }

    /// Return all variable names currently in scope (all scope levels).
    pub fn all_variable_names(&self) -> impl Iterator<Item = &str> {
        self.scopes
            .iter()
            .flat_map(|scope| scope.keys().map(|s| s.as_str()))
    }

    /// Return all function names (local + imported).
    pub fn all_function_names(&self) -> impl Iterator<Item = &str> {
        self.functions
            .keys()
            .chain(self.imported_funcs.keys())
            .map(|s| s.as_str())
    }

    /// Return all struct/type names (local + imported).
    pub fn all_struct_names(&self) -> impl Iterator<Item = &str> {
        self.struct_defs
            .keys()
            .chain(self.imported_structs.keys())
            .map(|s| s.as_str())
    }

    /// Return all protocol names (local + imported).
    pub fn all_protocol_names(&self) -> impl Iterator<Item = &str> {
        self.protocol_defs
            .keys()
            .chain(self.imported_protocols.keys())
            .map(|s| s.as_str())
    }
}

/// Check whether a symbol with the given visibility in `symbol_module` is
/// accessible from `accessor_module`.
pub fn is_accessible(
    symbol_visibility: Visibility,
    _symbol_module: &ModulePath,
    _accessor_module: &ModulePath,
) -> bool {
    match symbol_visibility {
        Visibility::Public => true,
        Visibility::Package => true, // same package — always true within a package
        Visibility::Internal => false, // cross-module access disallowed
        Visibility::Fileprivate => false,
        Visibility::Private => false,
    }
}
