use std::collections::HashMap;

use super::types::Type;
use crate::error::{BengalError, Result, Span};

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

#[derive(Default)]
pub struct Resolver {
    scopes: Vec<HashMap<String, VarInfo>>,
    functions: HashMap<String, FuncSig>,
    pub current_return_type: Option<Type>,
    loop_depth: u32,
    loop_break_types: Vec<Option<Type>>,
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
