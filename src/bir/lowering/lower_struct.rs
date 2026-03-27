use std::collections::HashMap;

use super::super::instruction::*;
use super::{ReceiverInfo, StmtResult, semantic_type_to_bir};
use crate::parser::ast::*;

impl super::Lowering {
    // ========== Struct helpers ==========

    pub(super) fn emit_struct_init(
        &mut self,
        struct_name: &str,
        field_values: &[(String, Value)],
        type_args: &[BirType],
    ) -> Value {
        let bir_ty = BirType::Struct {
            name: struct_name.to_string(),
            type_args: type_args.to_vec(),
        };
        let result = self.fresh_value();
        self.emit(Instruction::StructInit {
            result,
            struct_name: struct_name.to_string(),
            fields: field_values.to_vec(),
            type_args: type_args.to_vec(),
            ty: bir_ty.clone(),
        });
        self.value_types.insert(result, bir_ty);
        result
    }

    pub(super) fn lower_receiver(&mut self, object: &Expr) -> Option<ReceiverInfo> {
        match &object.kind {
            ExprKind::Ident(name) => {
                let val = self.lookup_var(name);
                let struct_name = match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => n.clone(),
                    _ => return None,
                };
                Some(ReceiverInfo {
                    value: val,
                    struct_name,
                    var_name: name.clone(),
                })
            }
            ExprKind::SelfRef => {
                let self_name = self.self_var_name.as_ref()?.clone();
                let val = self.lookup_var(&self_name);
                let struct_name = match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => n.clone(),
                    _ => return None,
                };
                Some(ReceiverInfo {
                    value: val,
                    struct_name,
                    var_name: self_name,
                })
            }
            _ => {
                let val = self.lower_expr(object);
                let struct_name = match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => n.clone(),
                    _ => return None,
                };
                let tmp_name = format!("__tmp_{}", self.next_value);
                self.define_var(tmp_name.clone(), val);
                Some(ReceiverInfo {
                    value: val,
                    struct_name,
                    var_name: tmp_name,
                })
            }
        }
    }

    pub(super) fn infer_struct_name_no_lower(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => {
                let val = self.try_lookup_var(name)?;
                match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => Some(n.clone()),
                    _ => None,
                }
            }
            ExprKind::SelfRef => {
                let self_name = self.self_var_name.as_ref()?;
                let val = self.try_lookup_var(self_name)?;
                match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => Some(n.clone()),
                    _ => None,
                }
            }
            ExprKind::StructInit { name, .. } => Some(name.clone()),
            ExprKind::Call { name, .. } => {
                let sem = self.sem_info.as_ref().unwrap();
                if sem.struct_init_calls.contains(&expr.id) {
                    Some(name.clone())
                } else {
                    match self.func_sigs.get(name)? {
                        BirType::Struct { name: sn, .. } => Some(sn.clone()),
                        _ => None,
                    }
                }
            }
            ExprKind::FieldAccess { object, field } => {
                let parent_struct = self.infer_struct_name_no_lower(object)?;
                let sem = self.sem_info.as_ref().unwrap();
                let info = sem.struct_defs.get(&parent_struct)?;
                // Check stored fields
                if let Some((_, ty)) = info.fields.iter().find(|(n, _)| n == field) {
                    return match ty {
                        crate::semantic::types::Type::Struct(name) => Some(name.clone()),
                        _ => None,
                    };
                }
                // Check computed properties
                if let Some(prop) = info.computed.iter().find(|p| p.name == *field) {
                    return match &prop.ty {
                        crate::semantic::types::Type::Struct(name) => Some(name.clone()),
                        _ => None,
                    };
                }
                None
            }
            _ => None,
        }
    }

    pub(super) fn inline_getter(
        &mut self,
        self_var_name: &str,
        getter_block: &Block,
        return_ty: BirType,
    ) -> Value {
        let prev_self_var = self.self_var_name.clone();
        let prev_in_init = self.in_init_body;
        let prev_getter_return_bb = self.getter_return_bb.take();

        // Allocate a continuation block that accepts the getter's return value as a param.
        let cont_bb = self.fresh_block();
        let cont_val = self.fresh_value();
        self.value_types.insert(cont_val, return_ty.clone());
        self.getter_return_bb = Some((cont_bb, cont_val, return_ty.clone()));

        self.self_var_name = Some(self_var_name.to_string());
        self.in_init_body = false;
        self.push_scope();
        let (result, mut getter_regions) = self.lower_block_stmts(getter_block);
        self.pending_regions.append(&mut getter_regions);
        self.pop_scope();
        self.self_var_name = prev_self_var;
        self.in_init_body = prev_in_init;
        self.getter_return_bb = prev_getter_return_bb;

        match result {
            Some(StmtResult::Return(v)) => {
                // Simple trailing-return getter: seal the current block by branching to cont_bb,
                // then start cont_bb so the caller continues from there.
                // NOTE: with getter_return_bb set, Stmt::Return redirects to cont_bb and returns
                // ReturnVoid, so this arm is normally unreachable -- but kept as a safety fallback.
                self.seal_block(Terminator::Br {
                    target: cont_bb,
                    args: vec![(v, return_ty.clone())],
                });
                self.start_block(cont_bb, vec![(cont_val, return_ty)]);
                cont_val
            }
            Some(StmtResult::ReturnVoid) | None => {
                // Exhaustive control-flow returns: all paths branched to cont_bb already.
                // The current block is a dead/unreachable block -- abandon it and switch to cont_bb.
                self.start_block(cont_bb, vec![(cont_val, return_ty)]);
                cont_val
            }
            _ => unreachable!("getter body produced unexpected StmtResult"),
        }
    }

    pub(super) fn try_lower_computed_setter(
        &mut self,
        object: &Expr,
        field: &str,
        value: &Expr,
    ) -> bool {
        let struct_name = match self.infer_struct_name_no_lower(object) {
            Some(n) => n,
            None => return false,
        };
        let sem = self.sem_info.as_ref().unwrap();
        let struct_info = match sem.struct_defs.get(&struct_name) {
            Some(i) => i.clone(),
            None => return false,
        };
        let prop = match struct_info.computed.iter().find(|p| p.name == field) {
            Some(p) => p.clone(),
            None => return false,
        };
        if !prop.has_setter {
            return false;
        }

        // Only support Ident/SelfRef receivers for setters
        let var_name = match &object.kind {
            ExprKind::Ident(name) => name.clone(),
            ExprKind::SelfRef => match &self.self_var_name {
                Some(n) => n.clone(),
                None => return false,
            },
            _ => return false,
        };

        let val = self.lower_expr(value);
        let setter_block = prop.setter.unwrap();

        // Save and switch context -- setter body runs as normal struct method,
        // not as initializer.
        let prev_self_var = self.self_var_name.clone();
        let prev_in_init = self.in_init_body;
        let prev_init_struct = self.init_struct_name.clone();
        self.self_var_name = Some(var_name);
        self.in_init_body = false;
        self.init_struct_name = None;

        self.push_scope();
        self.define_var("newValue".to_string(), val);
        let (_, mut setter_regions) = self.lower_block_stmts(&setter_block);
        self.pending_regions.append(&mut setter_regions);
        self.pop_scope();

        self.self_var_name = prev_self_var;
        self.in_init_body = prev_in_init;
        self.init_struct_name = prev_init_struct;
        true
    }

    pub(super) fn expr_refers_to_self(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::SelfRef => true,
            ExprKind::FieldAccess { object, .. } => self.expr_refers_to_self(object),
            _ => false,
        }
    }

    pub(super) fn lower_field_assign_recursive(
        &mut self,
        object: &Expr,
        field: &str,
        new_val: Value,
    ) {
        // Guard: check that field is a stored field, not computed
        if let Some(struct_name) = self.infer_struct_name_no_lower(object) {
            let sem = self.sem_info.as_ref().unwrap();
            if let Some(info) = sem.struct_defs.get(&struct_name)
                && info.computed.iter().any(|p| p.name == field)
            {
                self.record_error(
                    format!(
                        "computed property setter `{}` on non-direct receiver is not yet supported",
                        field
                    ),
                    Some(object.span),
                );
                return;
            }
        }

        match &object.kind {
            ExprKind::Ident(var_name) => {
                let obj_val = self.lookup_var(var_name);
                let obj_ty = self.value_types.get(&obj_val).cloned().unwrap();
                let result = self.fresh_value();
                self.emit(Instruction::FieldSet {
                    result,
                    object: obj_val,
                    field: field.to_string(),
                    value: new_val,
                    ty: obj_ty.clone(),
                });
                self.value_types.insert(result, obj_ty);
                self.assign_var(var_name, result);
            }
            ExprKind::SelfRef => {
                let self_name = self.self_var_name.as_ref().unwrap().clone();
                let obj_val = self.lookup_var(&self_name);
                let obj_ty = self.value_types.get(&obj_val).cloned().unwrap();
                let result = self.fresh_value();
                self.emit(Instruction::FieldSet {
                    result,
                    object: obj_val,
                    field: field.to_string(),
                    value: new_val,
                    ty: obj_ty.clone(),
                });
                self.value_types.insert(result, obj_ty);
                self.assign_var(&self_name, result);
            }
            ExprKind::FieldAccess {
                object: parent,
                field: parent_field,
            } => {
                // Guard: parent_field must be stored, not computed
                if let Some(parent_struct) = self.infer_struct_name_no_lower(parent) {
                    let sem = self.sem_info.as_ref().unwrap();
                    if let Some(info) = sem.struct_defs.get(&parent_struct)
                        && info.computed.iter().any(|p| p.name == *parent_field)
                    {
                        self.record_error(
                            format!(
                                "assignment through computed property `{}` is not yet supported",
                                parent_field
                            ),
                            Some(parent.span),
                        );
                        return;
                    }
                }
                // 1. Get the intermediate struct
                let parent_val = self.lower_expr(parent);
                let parent_ty = self.value_types.get(&parent_val).cloned().unwrap();
                let parent_struct_name = match &parent_ty {
                    BirType::Struct { name, .. } => name.clone(),
                    _ => unreachable!(),
                };
                // 2. Get the inner struct field
                let sem = self.sem_info.as_ref().unwrap();
                let inner_sem_ty = &sem
                    .struct_defs
                    .get(&parent_struct_name)
                    .unwrap()
                    .fields
                    .iter()
                    .find(|(n, _)| n == parent_field)
                    .unwrap()
                    .1;
                let inner_ty = semantic_type_to_bir(inner_sem_ty);
                let inner_val = self.fresh_value();
                self.emit(Instruction::FieldGet {
                    result: inner_val,
                    object: parent_val,
                    field: parent_field.clone(),
                    object_ty: parent_ty.clone(),
                    ty: inner_ty.clone(),
                });
                self.value_types.insert(inner_val, inner_ty.clone());
                // 3. Update the inner struct's field
                let updated_inner = self.fresh_value();
                self.emit(Instruction::FieldSet {
                    result: updated_inner,
                    object: inner_val,
                    field: field.to_string(),
                    value: new_val,
                    ty: inner_ty.clone(),
                });
                self.value_types.insert(updated_inner, inner_ty);
                // 4. Recurse: write the updated inner back into parent
                self.lower_field_assign_recursive(parent, parent_field, updated_inner);
            }
            _ => unreachable!("FieldAssign on unsupported object expression"),
        }
    }

    /// Collect current values of all mutable variables for while loop block args
    pub(super) fn collect_mutable_var_values(&self) -> Vec<(String, Value, BirType)> {
        self.mutable_vars
            .iter()
            .map(|(name, ty)| (name.clone(), self.lookup_var(name), ty.clone()))
            .collect()
    }

    /// Lower a FieldAccess expression. Extracted from lower_expr for maintainability.
    pub(super) fn lower_field_access(
        &mut self,
        expr: &Expr,
        object: &Expr,
        field: &str, // field name from AST
    ) -> Value {
        // Special case: self.field during init body
        if let ExprKind::SelfRef = &object.kind
            && self.in_init_body
            && let Some(self_name) = &self.self_var_name.clone()
        {
            // Reject computed property access during init
            let struct_name = self.init_struct_name.as_ref().unwrap().clone();
            let sem = self.sem_info.as_ref().unwrap();
            if let Some(info) = sem.struct_defs.get(&struct_name)
                && info.computed.iter().any(|p| p.name == field)
            {
                return self.record_error(
                    format!(
                        "computed property `{}` access on `self` in initializer body \
                         is not supported (self is not fully materialized during init)",
                        field
                    ),
                    Some(expr.span),
                );
            }
            // Stored field -- read from per-field variable
            let key = format!("{}.{}", self_name, field);
            if let Some(val) = self.try_lookup_var(&key) {
                return val;
            }
            return self.record_error(
                format!(
                    "read-before-init: field `{}` read before initialization",
                    field
                ),
                Some(expr.span),
            );
        }

        // General case: lower the receiver, then dispatch on lowered type
        let receiver = self.lower_receiver(object);
        match receiver {
            Some(recv) => self.lower_field_access_on_receiver(expr, &recv, field),
            None => unreachable!("field access on non-struct (semantic guarantees this)"),
        }
    }

    fn lower_field_access_on_receiver(
        &mut self,
        _expr: &Expr,
        recv: &ReceiverInfo,
        field: &str,
    ) -> Value {
        let sem = self.sem_info.as_ref().unwrap();
        let struct_info = sem.struct_defs.get(&recv.struct_name).unwrap().clone();

        // Check if field is a computed property
        if let Some(prop) = struct_info.computed.iter().find(|p| p.name == field) {
            // Inline the getter with receiver as self
            let prop_ty = semantic_type_to_bir(&prop.ty);
            return self.inline_getter(&recv.var_name, &prop.getter.clone(), prop_ty);
        }

        // Stored field -- emit FieldGet
        let field_sem_ty = &struct_info
            .fields
            .iter()
            .find(|(n, _)| n == field)
            .unwrap()
            .1;
        let field_ty = semantic_type_to_bir(field_sem_ty);
        // Use the object's actual type (preserves type_args for generics)
        let obj_ty = self
            .value_types
            .get(&recv.value)
            .cloned()
            .unwrap_or_else(|| BirType::struct_simple(recv.struct_name.clone()));
        // Resolve TypeParam field types using the object's type_args
        let resolved_field_ty =
            self.resolve_field_type_params(&obj_ty, field, &field_ty, &recv.struct_name);
        let result = self.fresh_value();
        self.emit(Instruction::FieldGet {
            result,
            object: recv.value,
            field: field.to_string(),
            object_ty: obj_ty,
            ty: field_ty.clone(),
        });
        self.value_types.insert(result, resolved_field_ty);
        result
    }

    fn resolve_field_type_params(
        &self,
        obj_ty: &BirType,
        field: &str,
        field_ty: &BirType,
        _struct_name: &str,
    ) -> BirType {
        if let BirType::Struct {
            name: sname,
            type_args: ta,
        } = obj_ty
        {
            if !ta.is_empty() {
                use crate::bir::mono::resolve_bir_type_lenient;
                let mangled = format!("{}_{}", sname, field);
                let subst: HashMap<String, BirType> = self
                    .func_type_param_names
                    .get(&mangled)
                    .map(|params| {
                        params
                            .iter()
                            .zip(ta.iter())
                            .map(|(p, a)| (p.clone(), a.clone()))
                            .collect()
                    })
                    .unwrap_or_else(|| {
                        // Fall back to deriving type params from sem_info
                        let sem = self.sem_info.as_ref().unwrap();
                        sem.struct_defs
                            .get(sname)
                            .map(|info| {
                                info.type_params
                                    .iter()
                                    .zip(ta.iter())
                                    .map(|(tp, a)| (tp.name.clone(), a.clone()))
                                    .collect()
                            })
                            .unwrap_or_default()
                    });
                resolve_bir_type_lenient(field_ty, &subst)
            } else {
                field_ty.clone()
            }
        } else {
            field_ty.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::bir::lowering::lower_program;
    use crate::bir::printer::print_module;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::semantic;

    fn lower_str(input: &str) -> String {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let module = lower_program(&program, &sem_info).unwrap();
        print_module(&module)
    }

    #[test]
    fn lower_struct_init_basic() {
        let output = lower_str(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 1, y: 2); return p.x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_field_get() {
        let output = lower_str(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1, y: 2); return p.x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains(r#"field_get"#));
        assert!(output.contains(r#""x""#));
    }

    #[test]
    fn lower_struct_field_set() {
        let output = lower_str(
            "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); p.x = 10; return p.x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains("field_set"));
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_as_function_arg() {
        let output = lower_str(
            "struct Point { var x: Int32; } func get_x(p: Point) -> Int32 { return p.x; } func main() -> Int32 { return get_x(Point(x: 42)); }",
        );
        assert!(output.contains("@get_x(%0: Point)"));
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_as_return_value() {
        let output = lower_str(
            "struct Point { var x: Int32; } func make() -> Point { return Point(x: 5); } func main() -> Int32 { let p = make(); return p.x; }",
        );
        assert!(output.contains("@make() -> Point"));
        assert!(output.contains("struct_init @Point"));
    }

    #[test]
    fn lower_struct_in_if_expr() {
        let output = lower_str(
            "struct Point { var x: Int32; } func main() -> Int32 { let p = if true { yield Point(x: 1); } else { yield Point(x: 2); }; return p.x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_computed_property() {
        let output = lower_str(
            "struct Foo { var x: Int32; var double: Int32 { get { return self.x; } }; } func main() -> Int32 { var f = Foo(x: 5); return f.double; }",
        );
        assert!(output.contains("struct_init @Foo"));
        // Getter is inlined -- field_get on self.x
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_explicit_init() {
        let output = lower_str(
            "struct Foo { var x: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { var f = Foo(val: 42); return f.x; }",
        );
        assert!(output.contains("struct_init @Foo"));
        assert!(output.contains("literal 42 : Int32"));
    }

    #[test]
    fn lower_struct_nested_field_assign() {
        let output = lower_str(
            "struct Inner { var x: Int32; } struct Outer { var inner: Inner; } func main() -> Int32 { var o = Outer(inner: Inner(x: 1)); o.inner.x = 10; return o.inner.x; }",
        );
        assert!(output.contains("field_get"));
        assert!(output.contains("field_set"));
    }

    #[test]
    fn lower_struct_mutable_in_loop() {
        let output = lower_str(
            "struct Acc { var val: Int32; } func main() -> Int32 { var a = Acc(val: 0); var i: Int32 = 0; while i < 3 { a.val = a.val + 1; i = i + 1; }; return a.val; }",
        );
        assert!(output.contains("struct_init @Acc"));
        assert!(output.contains("field_get"));
        assert!(output.contains("field_set"));
    }

    #[test]
    fn lower_struct_computed_setter() {
        let output = lower_str(
            "struct Foo { var x: Int32; var bar: Int32 { get { return 0; } set { self.x = newValue; } }; } func main() -> Int32 { var f = Foo(x: 1); f.bar = 10; return f.x; }",
        );
        // Setter is inlined -- field_set on self.x via setter body
        assert!(output.contains("field_set"));
    }

    #[test]
    fn lower_struct_init_field_access() {
        // Point(x: 1).x should now work (struct in expression position)
        let output = lower_str(
            "struct Point { var x: Int32; } func main() -> Int32 { return Point(x: 1).x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains("field_get"));
    }
}
