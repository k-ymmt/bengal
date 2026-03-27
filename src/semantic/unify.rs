use crate::error::BengalError;
use crate::semantic::types::Type;

use super::infer::{InferVarId, InferenceContext, VarState};

fn unify_err(message: impl Into<String>) -> BengalError {
    BengalError::SemanticError {
        message: message.into(),
        span: crate::error::Span { start: 0, end: 0 },
        help: None,
    }
}

impl InferenceContext {
    /// Unify two types, updating inference variable bindings as needed.
    /// Returns `Ok(())` on success, or an error if the types are incompatible.
    pub fn unify(&mut self, ty1: Type, ty2: Type) -> Result<(), BengalError> {
        // Error type unifies with anything — prevents cascading errors.
        if matches!(&ty1, Type::Error) || matches!(&ty2, Type::Error) {
            return Ok(());
        }

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
                match &other {
                    Type::IntegerLiteral(lit_id) => {
                        self.propagate_provenance(a, *lit_id);
                    }
                    Type::FloatLiteral(lit_id) => {
                        self.propagate_provenance(a, *lit_id);
                    }
                    _ => {}
                }
                self.set_resolved(a, other);
                Ok(())
            }

            // IntegerLiteral vs FloatLiteral — conflicting literal types
            (Type::IntegerLiteral(id1), Type::FloatLiteral(id2))
            | (Type::FloatLiteral(id2), Type::IntegerLiteral(id1)) => {
                let root1 = self.find(id1);
                let root2 = self.find(id2);
                let prov1 = self.provenance_at(root1).cloned();
                let prov2 = self.provenance_at(root2).cloned();
                if let (Some(p1), Some(p2)) = (&prov1, &prov2) {
                    let tp_prov = if !p1.type_param_name.is_empty() {
                        p1
                    } else {
                        p2
                    };
                    Err(BengalError::SemanticError {
                        message: format!(
                            "type parameter '{}' in function '{}' has conflicting constraints: \
                             integer literal (from argument '{}') vs float literal (from argument '{}')",
                            tp_prov.type_param_name,
                            tp_prov.def_name,
                            p1.arg_name.as_deref().unwrap_or("?"),
                            p2.arg_name.as_deref().unwrap_or("?"),
                        ),
                        span: tp_prov.span,
                        help: None,
                    })
                } else {
                    Err(unify_err("cannot unify integer literal with float literal"))
                }
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
                self.propagate_provenance(a, b);
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
                self.propagate_provenance(a, b);
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
    pub(super) fn find(&mut self, id: InferVarId) -> InferVarId {
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

#[cfg(test)]
#[path = "unify_tests.rs"]
mod unify_tests;
