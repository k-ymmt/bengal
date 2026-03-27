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
mod tests {
    use super::*;
    use crate::error::Span;
    use crate::parser::ast::{NodeId, TypeAnnotation, TypeParam};
    use crate::semantic::infer::{
        InferenceContext, InferredTypeArgs, VarProvenance, type_to_annotation,
    };

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

    #[test]
    fn unify_integer_float_literal_error_with_provenance() {
        let mut ctx = InferenceContext::new();
        let t_var = ctx.fresh_var_with_provenance(VarProvenance {
            type_param_name: "T".into(),
            def_name: "choose".into(),
            arg_name: None,
            span: Span { start: 0, end: 10 },
        });
        let int_var = ctx.fresh_integer();
        ctx.set_provenance(
            int_var,
            VarProvenance {
                type_param_name: String::new(),
                def_name: "choose".into(),
                arg_name: Some("a".into()),
                span: Span { start: 0, end: 10 },
            },
        );
        let float_var = ctx.fresh_float();
        ctx.set_provenance(
            float_var,
            VarProvenance {
                type_param_name: String::new(),
                def_name: "choose".into(),
                arg_name: Some("b".into()),
                span: Span { start: 0, end: 10 },
            },
        );
        assert!(
            ctx.unify(Type::IntegerLiteral(int_var), Type::InferVar(t_var))
                .is_ok()
        );
        let result = ctx.unify(Type::FloatLiteral(float_var), Type::InferVar(t_var));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("conflicting constraints"), "got: {}", msg);
        assert!(
            msg.contains("'T'"),
            "expected type param name, got: {}",
            msg
        );
        assert!(msg.contains("'choose'"), "expected func name, got: {}", msg);
    }

    #[test]
    fn unify_error_with_i32_ok() {
        let mut ctx = InferenceContext::new();
        assert!(ctx.unify(Type::Error, Type::I32).is_ok());
    }

    #[test]
    fn unify_i32_with_error_ok() {
        let mut ctx = InferenceContext::new();
        assert!(ctx.unify(Type::I32, Type::Error).is_ok());
    }

    #[test]
    fn unify_error_with_error_ok() {
        let mut ctx = InferenceContext::new();
        assert!(ctx.unify(Type::Error, Type::Error).is_ok());
    }

    // --- VarProvenance tests ---

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

    // --- apply_defaults error collection tests ---

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

    // --- type_to_annotation and register/record type args ---

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
        ctx.set_resolved(var_id, Type::I32);
        ctx.record_inferred_type_args(&mut inferred);
        let site = inferred.map.get(&node_id).unwrap();
        assert_eq!(site.type_args, vec![TypeAnnotation::I32]);
        assert_eq!(site.def_name, "identity");
    }
}
