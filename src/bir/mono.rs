use std::collections::{HashMap, HashSet};

use super::instruction::BirType;

pub fn resolve_bir_type(ty: &BirType, subst: &HashMap<String, BirType>) -> BirType {
    match ty {
        BirType::TypeParam(name) => subst
            .get(name)
            .unwrap_or_else(|| panic!("unresolved TypeParam: {name}"))
            .clone(),
        BirType::Array { element, size } => BirType::Array {
            element: Box::new(resolve_bir_type(element, subst)),
            size: *size,
        },
        BirType::Struct { name, type_args } => BirType::Struct {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|t| resolve_bir_type(t, subst))
                .collect(),
        },
        other => other.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Instance {
    pub func_name: String,
    pub type_args: Vec<BirType>,
}

impl Instance {
    pub fn mangled_name(&self) -> String {
        if self.type_args.is_empty() {
            self.func_name.clone()
        } else {
            let args: Vec<String> = self.type_args.iter().map(mangle_bir_type).collect();
            format!("{}_{}", self.func_name, args.join("_"))
        }
    }

    pub fn substitution_map(&self, type_params: &[String]) -> HashMap<String, BirType> {
        type_params
            .iter()
            .zip(&self.type_args)
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect()
    }
}

fn mangle_bir_type(ty: &BirType) -> String {
    match ty {
        BirType::I32 => "Int32".into(),
        BirType::I64 => "Int64".into(),
        BirType::F32 => "Float32".into(),
        BirType::F64 => "Float64".into(),
        BirType::Bool => "Bool".into(),
        BirType::Unit => "Unit".into(),
        BirType::Struct { name, type_args } => {
            if type_args.is_empty() {
                name.clone()
            } else {
                let args: Vec<String> = type_args.iter().map(mangle_bir_type).collect();
                format!("{}_{}", name, args.join("_"))
            }
        }
        BirType::Array { element, size } => {
            format!("Array_{}_{}", mangle_bir_type(element), size)
        }
        BirType::TypeParam(name) => panic!("cannot mangle unresolved TypeParam: {name}"),
    }
}

pub struct MonoCollectResult {
    pub func_instances: Vec<Instance>,
    pub struct_instances: HashSet<(String, Vec<BirType>)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_type_param() {
        let subst: HashMap<String, BirType> = [("T".into(), BirType::I32)].into();
        assert_eq!(
            resolve_bir_type(&BirType::TypeParam("T".into()), &subst),
            BirType::I32
        );
    }

    #[test]
    fn resolve_nested_array() {
        let subst: HashMap<String, BirType> = [("T".into(), BirType::Bool)].into();
        let input = BirType::Array {
            element: Box::new(BirType::TypeParam("T".into())),
            size: 3,
        };
        let expected = BirType::Array {
            element: Box::new(BirType::Bool),
            size: 3,
        };
        assert_eq!(resolve_bir_type(&input, &subst), expected);
    }

    #[test]
    fn resolve_generic_struct() {
        let subst: HashMap<String, BirType> =
            [("T".into(), BirType::I32), ("U".into(), BirType::Bool)].into();
        let input = BirType::Struct {
            name: "Pair".into(),
            type_args: vec![
                BirType::TypeParam("T".into()),
                BirType::TypeParam("U".into()),
            ],
        };
        let expected = BirType::Struct {
            name: "Pair".into(),
            type_args: vec![BirType::I32, BirType::Bool],
        };
        assert_eq!(resolve_bir_type(&input, &subst), expected);
    }

    #[test]
    fn resolve_concrete_passthrough() {
        let subst: HashMap<String, BirType> = HashMap::new();
        assert_eq!(resolve_bir_type(&BirType::I32, &subst), BirType::I32);
    }

    #[test]
    fn instance_mangle_single() {
        let inst = Instance {
            func_name: "identity".into(),
            type_args: vec![BirType::I32],
        };
        assert_eq!(inst.mangled_name(), "identity_Int32");
    }

    #[test]
    fn instance_mangle_multi() {
        let inst = Instance {
            func_name: "swap".into(),
            type_args: vec![BirType::I32, BirType::Bool],
        };
        assert_eq!(inst.mangled_name(), "swap_Int32_Bool");
    }

    #[test]
    fn instance_mangle_struct_arg() {
        let inst = Instance {
            func_name: "getFirst".into(),
            type_args: vec![BirType::struct_simple("Point".into()), BirType::I32],
        };
        assert_eq!(inst.mangled_name(), "getFirst_Point_Int32");
    }

    #[test]
    fn instance_no_type_args() {
        let inst = Instance {
            func_name: "main".into(),
            type_args: vec![],
        };
        assert_eq!(inst.mangled_name(), "main");
    }
}
