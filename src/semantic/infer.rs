use std::collections::HashMap;

use crate::error::{BengalError, Span};
use crate::parser::ast::{NodeId, TypeAnnotation, TypeParam};
use crate::semantic::types::Type;

fn unify_err(message: impl Into<String>) -> BengalError {
    BengalError::SemanticError {
        message: message.into(),
        span: Span { start: 0, end: 0 },
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

/// Convert a fully-resolved `Type` to `TypeAnnotation`.
/// Called after `apply_defaults`, so no InferVar/IntegerLiteral/FloatLiteral should remain.
pub fn type_to_annotation(ty: &Type) -> TypeAnnotation {
    match ty {
        Type::I32 => TypeAnnotation::I32,
        Type::I64 => TypeAnnotation::I64,
        Type::F32 => TypeAnnotation::F32,
        Type::F64 => TypeAnnotation::F64,
        Type::Bool => TypeAnnotation::Bool,
        Type::Unit => TypeAnnotation::Unit,
        Type::Struct(name) => TypeAnnotation::Named(name.clone()),
        Type::TypeParam { name, .. } => TypeAnnotation::Named(name.clone()),
        Type::Generic { name, args } => TypeAnnotation::Generic {
            name: name.clone(),
            args: args.iter().map(type_to_annotation).collect(),
        },
        Type::Array { element, size } => TypeAnnotation::Array {
            element: Box::new(type_to_annotation(element)),
            size: *size,
        },
        Type::InferVar(_) | Type::IntegerLiteral(_) | Type::FloatLiteral(_) => {
            unreachable!("unresolved type variable in type_to_annotation")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VarKind {
    General,
    IntegerLiteral,
    FloatLiteral,
}

#[derive(Debug, Clone)]
enum VarState {
    Unbound,
    Linked(InferVarId),
    Resolved(Type),
}

#[derive(Debug)]
pub struct InferenceContext {
    var_states: Vec<VarState>,
    var_kinds: Vec<VarKind>,
    var_provenance: Vec<Option<VarProvenance>>,
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

    /// Copy provenance from `from` to `to` if `to` currently has none.
    pub fn propagate_provenance(&mut self, from: InferVarId, to: InferVarId) {
        let from_root = self.find(from);
        let to_root = self.find(to);
        if self.var_provenance[to_root as usize].is_none()
            && let Some(prov) = self.var_provenance[from_root as usize].clone()
        {
            self.var_provenance[to_root as usize] = Some(prov);
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

    /// Unify two types, updating inference variable bindings as needed.
    /// Returns `Ok(())` on success, or an error if the types are incompatible.
    pub fn unify(&mut self, ty1: Type, ty2: Type) -> Result<(), BengalError> {
        let ty1 = self.deep_resolve(ty1);
        let ty2 = self.deep_resolve(ty2);

        if ty1 == ty2 {
            return Ok(());
        }

        match (ty1, ty2) {
            // TypeParam is opaque in pre-mono — treat as compatible with anything.
            // The real check happens post-monomorphization.
            (Type::TypeParam { .. }, _) | (_, Type::TypeParam { .. }) => Ok(()),

            // InferVar binds to anything
            (Type::InferVar(a), other) | (other, Type::InferVar(a)) => {
                self.set_resolved(a, other);
                Ok(())
            }

            // IntegerLiteral with integer concrete types
            (Type::IntegerLiteral(a), ref concrete @ Type::I32)
            | (Type::IntegerLiteral(a), ref concrete @ Type::I64)
            | (ref concrete @ Type::I32, Type::IntegerLiteral(a))
            | (ref concrete @ Type::I64, Type::IntegerLiteral(a)) => {
                self.set_resolved(a, concrete.clone());
                Ok(())
            }

            // IntegerLiteral with IntegerLiteral
            (Type::IntegerLiteral(a), Type::IntegerLiteral(b)) => {
                self.link(a, b);
                Ok(())
            }

            // FloatLiteral with float concrete types
            (Type::FloatLiteral(a), ref concrete @ Type::F32)
            | (Type::FloatLiteral(a), ref concrete @ Type::F64)
            | (ref concrete @ Type::F32, Type::FloatLiteral(a))
            | (ref concrete @ Type::F64, Type::FloatLiteral(a)) => {
                self.set_resolved(a, concrete.clone());
                Ok(())
            }

            // FloatLiteral with FloatLiteral
            (Type::FloatLiteral(a), Type::FloatLiteral(b)) => {
                self.link(a, b);
                Ok(())
            }

            // Array: recursive unification
            (
                Type::Array {
                    element: e1,
                    size: s1,
                },
                Type::Array {
                    element: e2,
                    size: s2,
                },
            ) => {
                if s1 != s2 {
                    return Err(unify_err(format!(
                        "array size mismatch: expected {}, found {}",
                        s1, s2
                    )));
                }
                self.unify(*e1, *e2)
            }

            // Generic: pairwise unification
            (Type::Generic { name: n1, args: a1 }, Type::Generic { name: n2, args: a2 }) => {
                if n1 != n2 {
                    return Err(unify_err(format!(
                        "cannot unify generic types: {} and {}",
                        n1, n2
                    )));
                }
                if a1.len() != a2.len() {
                    return Err(unify_err(format!(
                        "generic arity mismatch for {}: expected {}, found {}",
                        n1,
                        a1.len(),
                        a2.len()
                    )));
                }
                for (arg1, arg2) in a1.into_iter().zip(a2) {
                    self.unify(arg1, arg2)?;
                }
                Ok(())
            }

            // Everything else is an error
            (t1, t2) => Err(unify_err(format!("cannot unify {} with {}", t1, t2))),
        }
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

    #[test]
    fn unify_same_concrete() {
        let mut ctx = InferenceContext::new();
        assert!(ctx.unify(Type::I32, Type::I32).is_ok());
        assert!(ctx.unify(Type::Bool, Type::Bool).is_ok());
    }

    #[test]
    fn unify_infer_var_with_concrete() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_var();
        assert!(ctx.unify(Type::InferVar(a), Type::I32).is_ok());
        assert_eq!(ctx.resolve(a), Type::I32);
    }

    #[test]
    fn unify_integer_literal_with_i32() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_integer();
        assert!(ctx.unify(Type::IntegerLiteral(a), Type::I32).is_ok());
        assert_eq!(ctx.resolve(a), Type::I32);
    }

    #[test]
    fn unify_integer_literal_with_i64() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_integer();
        assert!(ctx.unify(Type::IntegerLiteral(a), Type::I64).is_ok());
        assert_eq!(ctx.resolve(a), Type::I64);
    }

    #[test]
    fn unify_integer_literal_with_infer_var() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_integer();
        let b = ctx.fresh_var();
        assert!(
            ctx.unify(Type::IntegerLiteral(a), Type::InferVar(b))
                .is_ok()
        );
        // b should resolve to IntegerLiteral(a)
        assert_eq!(ctx.resolve(b), Type::IntegerLiteral(a));
    }

    #[test]
    fn unify_two_integer_literals() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_integer();
        let b = ctx.fresh_integer();
        assert!(
            ctx.unify(Type::IntegerLiteral(a), Type::IntegerLiteral(b))
                .is_ok()
        );
        // Resolving b to I32 should also resolve a
        ctx.set_resolved(b, Type::I32);
        assert_eq!(ctx.resolve(a), Type::I32);
    }

    #[test]
    fn unify_integer_literal_with_float_literal_error() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_integer();
        let b = ctx.fresh_float();
        assert!(
            ctx.unify(Type::IntegerLiteral(a), Type::FloatLiteral(b))
                .is_err()
        );
    }

    #[test]
    fn unify_integer_literal_with_bool_error() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_integer();
        assert!(ctx.unify(Type::IntegerLiteral(a), Type::Bool).is_err());
    }

    #[test]
    fn unify_float_literal_with_f64() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_float();
        assert!(ctx.unify(Type::FloatLiteral(a), Type::F64).is_ok());
        assert_eq!(ctx.resolve(a), Type::F64);
    }

    #[test]
    fn unify_array_recursive() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_var();
        let arr1 = Type::Array {
            element: Box::new(Type::InferVar(a)),
            size: 3,
        };
        let arr2 = Type::Array {
            element: Box::new(Type::I32),
            size: 3,
        };
        assert!(ctx.unify(arr1, arr2).is_ok());
        assert_eq!(ctx.resolve(a), Type::I32);
    }

    #[test]
    fn unify_array_size_mismatch_error() {
        let mut ctx = InferenceContext::new();
        let arr1 = Type::Array {
            element: Box::new(Type::I32),
            size: 3,
        };
        let arr2 = Type::Array {
            element: Box::new(Type::I32),
            size: 5,
        };
        assert!(ctx.unify(arr1, arr2).is_err());
    }

    #[test]
    fn unify_generic_pairwise() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_var();
        let g1 = Type::Generic {
            name: "Box".to_string(),
            args: vec![Type::InferVar(a)],
        };
        let g2 = Type::Generic {
            name: "Box".to_string(),
            args: vec![Type::I64],
        };
        assert!(ctx.unify(g1, g2).is_ok());
        assert_eq!(ctx.resolve(a), Type::I64);
    }

    #[test]
    fn unify_generic_name_mismatch_error() {
        let mut ctx = InferenceContext::new();
        let g1 = Type::Generic {
            name: "Box".to_string(),
            args: vec![Type::I32],
        };
        let g2 = Type::Generic {
            name: "Pair".to_string(),
            args: vec![Type::I32],
        };
        assert!(ctx.unify(g1, g2).is_err());
    }

    #[test]
    fn unify_type_param_same() {
        let mut ctx = InferenceContext::new();
        let t1 = Type::TypeParam {
            name: "T".to_string(),
            bound: None,
        };
        let t2 = Type::TypeParam {
            name: "T".to_string(),
            bound: None,
        };
        assert!(ctx.unify(t1, t2).is_ok());
    }

    #[test]
    fn unify_type_param_with_concrete_ok() {
        // TypeParam is treated as opaque in pre-mono; unification should succeed.
        let mut ctx = InferenceContext::new();
        let t = Type::TypeParam {
            name: "T".to_string(),
            bound: None,
        };
        assert!(ctx.unify(t, Type::I32).is_ok());
    }

    #[test]
    fn unify_symmetry() {
        // unify(A, B) should give same result as unify(B, A)
        let mut ctx1 = InferenceContext::new();
        let a1 = ctx1.fresh_integer();
        assert!(ctx1.unify(Type::IntegerLiteral(a1), Type::I64).is_ok());
        assert_eq!(ctx1.resolve(a1), Type::I64);

        let mut ctx2 = InferenceContext::new();
        let a2 = ctx2.fresh_integer();
        assert!(ctx2.unify(Type::I64, Type::IntegerLiteral(a2)).is_ok());
        assert_eq!(ctx2.resolve(a2), Type::I64);
    }

    #[test]
    fn unify_different_concrete_error() {
        let mut ctx = InferenceContext::new();
        assert!(ctx.unify(Type::I32, Type::Bool).is_err());
        assert!(ctx.unify(Type::I32, Type::I64).is_err());
    }

    #[test]
    fn unify_struct_same() {
        let mut ctx = InferenceContext::new();
        assert!(
            ctx.unify(Type::Struct("Foo".into()), Type::Struct("Foo".into()))
                .is_ok()
        );
    }

    #[test]
    fn unify_struct_different_error() {
        let mut ctx = InferenceContext::new();
        assert!(
            ctx.unify(Type::Struct("Foo".into()), Type::Struct("Bar".into()))
                .is_err()
        );
    }

    // --- Task 4: apply_defaults, type_to_annotation, register/record ---

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

    #[test]
    fn type_to_annotation_primitives() {
        assert_eq!(type_to_annotation(&Type::I32), TypeAnnotation::I32);
        assert_eq!(type_to_annotation(&Type::I64), TypeAnnotation::I64);
        assert_eq!(type_to_annotation(&Type::F32), TypeAnnotation::F32);
        assert_eq!(type_to_annotation(&Type::F64), TypeAnnotation::F64);
        assert_eq!(type_to_annotation(&Type::Bool), TypeAnnotation::Bool);
        assert_eq!(type_to_annotation(&Type::Unit), TypeAnnotation::Unit);
    }

    #[test]
    fn type_to_annotation_struct() {
        assert_eq!(
            type_to_annotation(&Type::Struct("Foo".into())),
            TypeAnnotation::Named("Foo".into())
        );
    }

    #[test]
    fn type_to_annotation_type_param() {
        assert_eq!(
            type_to_annotation(&Type::TypeParam {
                name: "T".into(),
                bound: Some("Proto".into())
            }),
            TypeAnnotation::Named("T".into())
        );
    }

    #[test]
    fn type_to_annotation_generic() {
        let ty = Type::Generic {
            name: "Box".into(),
            args: vec![Type::I32],
        };
        assert_eq!(
            type_to_annotation(&ty),
            TypeAnnotation::Generic {
                name: "Box".into(),
                args: vec![TypeAnnotation::I32],
            }
        );
    }

    #[test]
    fn register_and_record_type_args() {
        let mut ctx = InferenceContext::new();
        let mut inferred = InferredTypeArgs::new();
        let node_id = NodeId(42);
        let var_id = ctx.fresh_var();
        ctx.register_call_site(
            node_id,
            vec![var_id],
            vec![TypeParam {
                name: "T".into(),
                bound: None,
            }],
            "identity".into(),
        );
        // Resolve the var
        ctx.set_resolved(var_id, Type::I32);
        ctx.record_inferred_type_args(&mut inferred);
        let site = inferred.map.get(&node_id).unwrap();
        assert_eq!(site.type_args, vec![TypeAnnotation::I32]);
        assert_eq!(site.def_name, "identity");
    }

    // --- Task 3: VarProvenance tests ---

    #[test]
    fn fresh_var_with_provenance_records() {
        let mut ctx = InferenceContext::new();
        let id = ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "T".into(),
            def_name: "foo".into(),
            arg_name: None,
            span: Span { start: 10, end: 20 },
        });
        let prov = ctx.get_provenance(id).unwrap();
        assert_eq!(prov.type_param_name, "T");
        assert_eq!(prov.def_name, "foo");
        assert!(prov.arg_name.is_none());
    }

    #[test]
    fn set_provenance_replaces() {
        let mut ctx = InferenceContext::new();
        let id = ctx.fresh_var();
        assert!(ctx.get_provenance(id).is_none());
        ctx.set_provenance(
            id,
            VarProvenance {
                type_param_name: "U".into(),
                def_name: "bar".into(),
                arg_name: Some("x".into()),
                span: Span { start: 0, end: 5 },
            },
        );
        assert_eq!(ctx.get_provenance(id).unwrap().type_param_name, "U");
    }

    #[test]
    fn update_arg_name_sets_name() {
        let mut ctx = InferenceContext::new();
        let id = ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "T".into(),
            def_name: "f".into(),
            arg_name: None,
            span: Span { start: 0, end: 0 },
        });
        ctx.update_arg_name(id, "x".into());
        assert_eq!(
            ctx.get_provenance(id).unwrap().arg_name.as_deref(),
            Some("x")
        );
    }

    #[test]
    fn propagate_provenance_copies_to_empty() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "T".into(),
            def_name: "f".into(),
            arg_name: Some("a".into()),
            span: Span { start: 0, end: 5 },
        });
        let b = ctx.fresh_var();
        assert!(ctx.get_provenance(b).is_none());
        ctx.propagate_provenance(a, b);
        assert!(ctx.get_provenance(b).is_some());
        assert_eq!(ctx.get_provenance(b).unwrap().type_param_name, "T");
    }

    #[test]
    fn propagate_provenance_does_not_overwrite() {
        let mut ctx = InferenceContext::new();
        let a = ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "T".into(),
            def_name: "f".into(),
            arg_name: None,
            span: Span { start: 0, end: 0 },
        });
        let b = ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "U".into(),
            def_name: "g".into(),
            arg_name: None,
            span: Span { start: 0, end: 0 },
        });
        ctx.propagate_provenance(a, b);
        assert_eq!(ctx.get_provenance(b).unwrap().type_param_name, "U");
    }

    #[test]
    fn reset_clears_provenance() {
        let mut ctx = InferenceContext::new();
        ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "T".into(),
            def_name: "f".into(),
            arg_name: None,
            span: Span { start: 0, end: 0 },
        });
        ctx.reset();
        let id = ctx.fresh_var();
        assert!(ctx.get_provenance(id).is_none());
    }

    // --- Task 4: apply_defaults error collection tests ---

    #[test]
    fn apply_defaults_unresolved_with_provenance() {
        let mut ctx = InferenceContext::new();
        let _id = ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "T".into(),
            def_name: "foo".into(),
            arg_name: None,
            span: Span { start: 10, end: 20 },
        });
        let errors = ctx.apply_defaults();
        assert_eq!(errors.len(), 1);
        let msg = errors[0].to_string();
        assert!(
            msg.contains("'T'"),
            "expected type param name, got: {}",
            msg
        );
        assert!(msg.contains("'foo'"), "expected func name, got: {}", msg);
    }

    #[test]
    fn apply_defaults_multiple_errors_collected() {
        let mut ctx = InferenceContext::new();
        ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "A".into(),
            def_name: "f".into(),
            arg_name: None,
            span: Span { start: 0, end: 5 },
        });
        ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "B".into(),
            def_name: "f".into(),
            arg_name: None,
            span: Span { start: 0, end: 5 },
        });
        let errors = ctx.apply_defaults();
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn apply_defaults_integer_literal_returns_empty_errors() {
        let mut ctx = InferenceContext::new();
        ctx.fresh_integer();
        let errors = ctx.apply_defaults();
        assert!(errors.is_empty());
    }
}
