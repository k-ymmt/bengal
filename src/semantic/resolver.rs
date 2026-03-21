use std::collections::HashMap;

use super::types::Type;

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

pub struct Resolver {
    scopes: Vec<HashMap<String, VarInfo>>,
    functions: HashMap<String, FuncSig>,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            scopes: Vec::new(),
            functions: HashMap::new(),
        }
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
}
