use std::collections::{HashMap, HashSet};

use super::types::Type;
use crate::error::{BengalError, Result, Span};
use crate::parser::ast::{Block, NodeId};

#[derive(Debug, Clone)]
pub struct VarInfo {
    pub ty: Type,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<Type>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub fields: Vec<(String, Type)>,
    pub field_index: HashMap<String, usize>,
    pub computed: Vec<ComputedPropInfo>,
    pub computed_index: HashMap<String, usize>,
    pub init: InitializerInfo,
}

#[derive(Debug, Clone)]
pub struct ComputedPropInfo {
    pub name: String,
    pub ty: Type,
    pub has_setter: bool,
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

#[derive(Default)]
pub struct Resolver {
    scopes: Vec<HashMap<String, VarInfo>>,
    functions: HashMap<String, FuncSig>,
    pub current_return_type: Option<Type>,
    loop_depth: u32,
    loop_break_types: Vec<Option<Type>>,
    struct_defs: HashMap<String, StructInfo>,
    pub self_context: Option<SelfContext>,
    pub struct_init_calls: HashSet<NodeId>,
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
        self.functions.get(name)
    }

    pub fn define_struct(&mut self, name: String, info: StructInfo) {
        self.struct_defs.insert(name, info);
    }

    pub fn lookup_struct(&self, name: &str) -> Option<&StructInfo> {
        self.struct_defs.get(name)
    }

    pub fn reserve_struct(&mut self, name: String) {
        self.struct_defs.insert(
            name,
            StructInfo {
                fields: vec![],
                field_index: HashMap::new(),
                computed: vec![],
                computed_index: HashMap::new(),
                init: InitializerInfo {
                    params: vec![],
                    body: None,
                },
            },
        );
    }

    pub fn record_struct_init_call(&mut self, id: NodeId) {
        self.struct_init_calls.insert(id);
    }

    pub fn take_struct_defs(&mut self) -> HashMap<String, StructInfo> {
        std::mem::take(&mut self.struct_defs)
    }

    pub fn take_struct_init_calls(&mut self) -> HashSet<NodeId> {
        std::mem::take(&mut self.struct_init_calls)
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
            }),
            Some(_) => Ok(()),
            None => {
                *current = Some(ty);
                Ok(())
            }
        }
    }
}
