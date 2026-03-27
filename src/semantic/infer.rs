use std::collections::HashMap;

use crate::error::{BengalError, Span};
use crate::parser::ast::{NodeId, TypeAnnotation, TypeParam};
use crate::semantic::types::Type;

fn unify_err(message: impl Into<String>) -> BengalError {
    BengalError::SemanticError {
        message: message.into(),
        span: Span { start: 0, end: 0 },
        help: None,
    }
}

pub type InferVarId = u32;

/// Provenance metadata for an inference variable — tracks where it came from.
#[derive(Debug, Clone)]
pub struct VarProvenance {
    pub type_param_name: String,
    pub def_name: String,
    pub arg_name: Option<String>,
    pub span: Span,
}

/// Stores inferred type arguments for call sites, indexed by NodeId.
pub struct InferredTypeArgs {
    pub map: HashMap<NodeId, InferredCallSite>,
}

/// One call site's inferred type args + definition info for constraint checking.
pub struct InferredCallSite {
    pub type_args: Vec<TypeAnnotation>,
    pub type_params: Vec<TypeParam>,
    pub def_name: String,
}

impl InferredTypeArgs {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl Default for InferredTypeArgs {
    fn default() -> Self {
        Self::new()
    }
}

/// Try to convert a `Type` to `TypeAnnotation`, returning an error for unresolved variables.
pub fn try_type_to_annotation(ty: &Type) -> std::result::Result<TypeAnnotation, BengalError> {
    match ty {
        Type::I32 => Ok(TypeAnnotation::I32),
        Type::I64 => Ok(TypeAnnotation::I64),
        Type::F32 => Ok(TypeAnnotation::F32),
        Type::F64 => Ok(TypeAnnotation::F64),
        Type::Bool => Ok(TypeAnnotation::Bool),
        Type::Unit => Ok(TypeAnnotation::Unit),
        Type::Struct(name) => Ok(TypeAnnotation::Named(name.clone())),
        Type::TypeParam { name, .. } => Ok(TypeAnnotation::Named(name.clone())),
        Type::Generic { name, args } => {
            let converted: std::result::Result<Vec<_>, _> =
                args.iter().map(try_type_to_annotation).collect();
            Ok(TypeAnnotation::Generic {
                name: name.clone(),
                args: converted?,
            })
        }
        Type::Array { element, size } => Ok(TypeAnnotation::Array {
            element: Box::new(try_type_to_annotation(element)?),
            size: *size,
        }),
        Type::InferVar(_) | Type::IntegerLiteral(_) | Type::FloatLiteral(_) => {
            Err(unify_err("unresolved type variable in type_to_annotation"))
        }
        Type::Error => Err(unify_err("error type in type_to_annotation")),
    }
}

/// Convert a fully-resolved `Type` to `TypeAnnotation`.
/// Called after `apply_defaults`, so no InferVar/IntegerLiteral/FloatLiteral should remain.
pub fn type_to_annotation(ty: &Type) -> TypeAnnotation {
    try_type_to_annotation(ty).expect("unresolved type variable in type_to_annotation")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VarKind {
    General,
    IntegerLiteral,
    FloatLiteral,
}

#[derive(Debug, Clone)]
pub(super) enum VarState {
    Unbound,
    Linked(InferVarId),
    Resolved(Type),
}

#[derive(Debug)]
pub struct InferenceContext {
    pub(super) var_states: Vec<VarState>,
    pub(super) var_kinds: Vec<VarKind>,
    pub(super) var_provenance: Vec<Option<VarProvenance>>,
    pub pending_type_args: Vec<(NodeId, Vec<InferVarId>, Vec<TypeParam>, String)>,
    /// Integer literal values pending range checks after type resolution.
    pending_int_range_checks: Vec<(InferVarId, i64)>,
}

impl InferenceContext {
    pub fn new() -> Self {
        Self {
            var_states: Vec::new(),
            var_kinds: Vec::new(),
            var_provenance: Vec::new(),
            pending_type_args: Vec::new(),
            pending_int_range_checks: Vec::new(),
        }
    }

    /// Create a fresh unbound inference variable.
    pub fn fresh_var(&mut self) -> InferVarId {
        let id = self.var_states.len() as InferVarId;
        self.var_states.push(VarState::Unbound);
        self.var_kinds.push(VarKind::General);
        self.var_provenance.push(None);
        id
    }

    /// Create a fresh variable for an integer literal.
    pub fn fresh_integer(&mut self) -> InferVarId {
        let id = self.var_states.len() as InferVarId;
        self.var_states.push(VarState::Unbound);
        self.var_kinds.push(VarKind::IntegerLiteral);
        self.var_provenance.push(None);
        id
    }

    /// Register a pending range check for an integer literal.
    /// Called when creating an IntegerLiteral variable so we can verify
    /// the value fits after the concrete type is resolved.
    pub fn register_int_range_check(&mut self, id: InferVarId, value: i64) {
        self.pending_int_range_checks.push((id, value));
    }

    /// Create a fresh variable for a float literal.
    pub fn fresh_float(&mut self) -> InferVarId {
        let id = self.var_states.len() as InferVarId;
        self.var_states.push(VarState::Unbound);
        self.var_kinds.push(VarKind::FloatLiteral);
        self.var_provenance.push(None);
        id
    }

    /// Create a fresh var and immediately attach provenance.
    pub fn fresh_var_with_provenance(&mut self, prov: VarProvenance) -> InferVarId {
        let id = self.fresh_var();
        self.set_provenance(id, prov);
        id
    }

    /// Set (or replace) the provenance for an inference variable.
    pub fn set_provenance(&mut self, id: InferVarId, prov: VarProvenance) {
        let root = self.find(id);
        self.var_provenance[root as usize] = Some(prov);
    }

    /// If provenance exists for this variable, update its arg_name.
    pub fn update_arg_name(&mut self, id: InferVarId, name: String) {
        let root = self.find(id);
        if let Some(prov) = self.var_provenance[root as usize].as_mut() {
            prov.arg_name = Some(name);
        }
    }

    /// Copy provenance from `from` to `to`:
    /// - If `to` has no provenance, copies fully.
    /// - If `to` already has provenance but an empty `type_param_name`, copies the
    ///   `type_param_name` and `def_name` from `from` so errors can name the parameter.
    pub fn propagate_provenance(&mut self, from: InferVarId, to: InferVarId) {
        let from_root = self.find(from);
        let to_root = self.find(to);
        if from_root == to_root {
            return;
        }
        let from_prov = self.var_provenance[from_root as usize].clone();
        let Some(from_prov) = from_prov else { return };
        match self.var_provenance[to_root as usize].as_mut() {
            None => {
                self.var_provenance[to_root as usize] = Some(from_prov);
            }
            Some(to_prov) if to_prov.type_param_name.is_empty() => {
                to_prov.type_param_name = from_prov.type_param_name;
                to_prov.def_name = from_prov.def_name;
            }
            _ => {}
        }
    }

    /// Get the provenance for an inference variable, if any.
    pub fn get_provenance(&mut self, id: InferVarId) -> Option<&VarProvenance> {
        let root = self.find(id);
        self.var_provenance[root as usize].as_ref()
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
        self.var_kinds.clear();
        self.var_provenance.clear();
        self.pending_type_args.clear();
        self.pending_int_range_checks.clear();
    }

    /// Deeply resolve a type by following all inference variable chains and
    /// recursively resolving structural types (Array, Generic).
    pub fn deep_resolve(&mut self, ty: Type) -> Type {
        match ty {
            Type::InferVar(id) => {
                let resolved = self.resolve(id);
                match &resolved {
                    Type::InferVar(rid) if *rid == id => resolved,
                    _ => self.deep_resolve(resolved),
                }
            }
            Type::IntegerLiteral(id) => {
                let resolved = self.resolve(id);
                match &resolved {
                    // Unbound: preserve IntegerLiteral wrapper
                    Type::InferVar(rid) if *rid == id => Type::IntegerLiteral(id),
                    _ => self.deep_resolve(resolved),
                }
            }
            Type::FloatLiteral(id) => {
                let resolved = self.resolve(id);
                match &resolved {
                    // Unbound: preserve FloatLiteral wrapper
                    Type::InferVar(rid) if *rid == id => Type::FloatLiteral(id),
                    _ => self.deep_resolve(resolved),
                }
            }
            Type::Array { element, size } => {
                let resolved_elem = self.deep_resolve(*element);
                Type::Array {
                    element: Box::new(resolved_elem),
                    size,
                }
            }
            Type::Generic { name, args } => {
                let resolved_args = args.into_iter().map(|a| self.deep_resolve(a)).collect();
                Type::Generic {
                    name,
                    args: resolved_args,
                }
            }
            _ => ty,
        }
    }

    /// After analyzing a function body, resolve remaining type variables to defaults:
    /// - `IntegerLiteral` -> `I32`
    /// - `FloatLiteral` -> `F64`
    /// - `InferVar` still unbound -> error (collected, not early-returned)
    pub fn apply_defaults(&mut self) -> Vec<BengalError> {
        let mut errors: Vec<BengalError> = Vec::new();

        for id in 0..self.var_states.len() {
            let id = id as InferVarId;
            let resolved = self.deep_resolve(Type::InferVar(id));
            match resolved {
                Type::InferVar(_) => {
                    // Variable is still unbound; check its kind for defaulting
                    match self.var_kinds[id as usize] {
                        VarKind::IntegerLiteral => {
                            self.set_resolved(id, Type::I32);
                        }
                        VarKind::FloatLiteral => {
                            self.set_resolved(id, Type::F64);
                        }
                        VarKind::General => {
                            let root = self.find(id);
                            let err =
                                if let Some(prov) = self.var_provenance[root as usize].as_ref() {
                                    BengalError::SemanticError {
                                        message: format!(
                                            "cannot infer type parameter '{}' for function '{}'; \
                                         add explicit type annotation",
                                            prov.type_param_name, prov.def_name
                                        ),
                                        span: prov.span,
                                        help: None,
                                    }
                                } else {
                                    unify_err("cannot infer type; add explicit type annotation")
                                };
                            errors.push(err);
                        }
                    }
                }
                Type::IntegerLiteral(_) => {
                    self.set_resolved(id, Type::I32);
                }
                Type::FloatLiteral(_) => {
                    self.set_resolved(id, Type::F64);
                }
                _ => {} // already resolved to concrete type
            }
        }

        // Check that integer literal values fit in their resolved type
        let range_checks: Vec<_> = self.pending_int_range_checks.clone();
        for &(id, value) in &range_checks {
            let resolved = self.deep_resolve(Type::InferVar(id));
            match resolved {
                Type::I32 => {
                    if value < i32::MIN as i64 || value > i32::MAX as i64 {
                        errors.push(unify_err(format!(
                            "integer literal `{}` is out of range for `Int32`",
                            value
                        )));
                    }
                }
                Type::I64 => {
                    // i64 is the widest type we have; value is already i64
                }
                _ => {} // resolved to non-integer — will be caught by type checking
            }
        }

        errors
    }

    /// Record that a call site needs inferred type args.
    pub fn register_call_site(
        &mut self,
        node_id: NodeId,
        var_ids: Vec<InferVarId>,
        type_params: Vec<TypeParam>,
        def_name: String,
    ) {
        self.pending_type_args
            .push((node_id, var_ids, type_params, def_name));
    }

    /// After `apply_defaults`, convert pending type args to TypeAnnotation
    /// and store in InferredTypeArgs.
    pub fn record_inferred_type_args(&mut self, inferred: &mut InferredTypeArgs) {
        let pending: Vec<_> = self.pending_type_args.drain(..).collect();
        for (node_id, var_ids, type_params, def_name) in pending {
            let type_args: Vec<TypeAnnotation> = var_ids
                .iter()
                .map(|&id| {
                    let resolved = self.deep_resolve(Type::InferVar(id));
                    type_to_annotation(&resolved)
                })
                .collect();
            inferred.map.insert(
                node_id,
                InferredCallSite {
                    type_args,
                    type_params,
                    def_name,
                },
            );
        }
    }

    /// Return the provenance at a raw index (used by the unify module).
    pub(super) fn provenance_at(&self, idx: InferVarId) -> Option<&VarProvenance> {
        self.var_provenance.get(idx as usize)?.as_ref()
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

    // --- apply_defaults, type_to_annotation, register/record ---

    #[test]
    fn apply_defaults_integer_literal_to_i32() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_integer();
        // a is unbound, should default to I32
        assert!(ctx.apply_defaults().is_empty());
        assert_eq!(ctx.resolve(a), Type::I32);
    }

    #[test]
    fn apply_defaults_float_literal_to_f64() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_float();
        assert!(ctx.apply_defaults().is_empty());
        assert_eq!(ctx.resolve(a), Type::F64);
    }

    #[test]
    fn apply_defaults_already_resolved() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_integer();
        ctx.set_resolved(a, Type::I64);
        assert!(ctx.apply_defaults().is_empty());
        assert_eq!(ctx.resolve(a), Type::I64); // stays I64, not defaulted to I32
    }

    #[test]
    fn apply_defaults_unresolved_infer_var_error() {
        let mut ctx = InferenceContext::new();
        let _a = ctx.fresh_var();
        assert!(!ctx.apply_defaults().is_empty());
    }
}
