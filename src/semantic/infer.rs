use crate::parser::ast::NodeId;
use crate::semantic::types::Type;

pub type InferVarId = u32;

#[derive(Debug, Clone)]
enum VarState {
    Unbound,
    Linked(InferVarId),
    Resolved(Type),
}

#[derive(Debug)]
pub struct InferenceContext {
    var_states: Vec<VarState>,
    pub pending_type_args: Vec<(NodeId, Vec<InferVarId>)>,
}

impl InferenceContext {
    pub fn new() -> Self {
        Self {
            var_states: Vec::new(),
            pending_type_args: Vec::new(),
        }
    }

    /// Create a fresh unbound inference variable.
    pub fn fresh_var(&mut self) -> InferVarId {
        let id = self.var_states.len() as InferVarId;
        self.var_states.push(VarState::Unbound);
        id
    }

    /// Create a fresh variable for an integer literal.
    /// The IntegerLiteral vs InferVar distinction is in the Type enum, not VarState.
    pub fn fresh_integer(&mut self) -> InferVarId {
        self.fresh_var()
    }

    /// Create a fresh variable for a float literal.
    /// The FloatLiteral vs InferVar distinction is in the Type enum, not VarState.
    pub fn fresh_float(&mut self) -> InferVarId {
        self.fresh_var()
    }

    /// Follow the Union-Find chain with path compression and return the resolved type.
    /// If unbound, returns `Type::InferVar(id)`.
    pub fn resolve(&mut self, id: InferVarId) -> Type {
        let root = self.find(id);
        match self.var_states[root as usize].clone() {
            VarState::Unbound => Type::InferVar(root),
            VarState::Resolved(ty) => ty,
            VarState::Linked(_) => unreachable!("find() should return the root"),
        }
    }

    /// Set the root variable to a resolved concrete type.
    pub fn set_resolved(&mut self, id: InferVarId, ty: Type) {
        let root = self.find(id);
        self.var_states[root as usize] = VarState::Resolved(ty);
    }

    /// Link variable `a` to variable `b` (Union-Find union).
    pub fn link(&mut self, a: InferVarId, b: InferVarId) {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a != root_b {
            self.var_states[root_a as usize] = VarState::Linked(root_b);
        }
    }

    /// Clear all state.
    pub fn reset(&mut self) {
        self.var_states.clear();
        self.pending_type_args.clear();
    }

    /// Find the root of the Union-Find chain for `id`, with path compression.
    fn find(&mut self, id: InferVarId) -> InferVarId {
        let state = self.var_states[id as usize].clone();
        match state {
            VarState::Linked(parent) => {
                let root = self.find(parent);
                // Path compression
                if root != parent {
                    self.var_states[id as usize] = VarState::Linked(root);
                }
                root
            }
            VarState::Unbound | VarState::Resolved(_) => id,
        }
    }
}

impl Default for InferenceContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::types::Type;

    #[test]
    fn fresh_var_increments() {
        let mut ctx = InferenceContext::new();
        assert_eq!(ctx.fresh_var(), 0);
        assert_eq!(ctx.fresh_var(), 1);
        assert_eq!(ctx.fresh_var(), 2);
    }

    #[test]
    fn resolve_unbound() {
        let mut ctx = InferenceContext::new();
        let id = ctx.fresh_var();
        assert_eq!(ctx.resolve(id), Type::InferVar(0));
    }

    #[test]
    fn resolve_follows_link() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_var();
        let b = ctx.fresh_var();
        ctx.link(a, b);
        ctx.set_resolved(b, Type::I32);
        assert_eq!(ctx.resolve(a), Type::I32);
    }

    #[test]
    fn resolve_concrete() {
        let mut ctx = InferenceContext::new();
        let id = ctx.fresh_var();
        ctx.set_resolved(id, Type::Bool);
        assert_eq!(ctx.resolve(id), Type::Bool);
    }

    #[test]
    fn reset_clears_state() {
        let mut ctx = InferenceContext::new();
        ctx.fresh_var();
        ctx.fresh_var();
        ctx.reset();
        assert_eq!(ctx.fresh_var(), 0);
    }
}
