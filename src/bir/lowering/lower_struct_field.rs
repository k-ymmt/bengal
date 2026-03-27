use std::collections::HashMap;

use super::super::instruction::*;
use super::{ReceiverInfo, semantic_type_to_bir};
use crate::parser::ast::*;

impl super::Lowering {
    pub(super) fn lower_field_access_on_receiver(
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

    pub(super) fn resolve_field_type_params(
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
