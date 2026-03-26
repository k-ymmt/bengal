use std::collections::{HashMap, HashSet};

use super::instruction::{BasicBlock, BirFunction, BirModule, BirType, Instruction, Terminator};

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

// ---------------------------------------------------------------------------
// mono_collect implementation
// ---------------------------------------------------------------------------

pub fn mono_collect(bir: &BirModule, _entry: &str) -> MonoCollectResult {
    // Build function lookup map.
    let func_map: HashMap<&str, &BirFunction> =
        bir.functions.iter().map(|f| (f.name.as_str(), f)).collect();

    let mut func_instances: Vec<Instance> = Vec::new();
    let mut seen_func: HashSet<Instance> = HashSet::new();
    let mut struct_instances: HashSet<(String, Vec<BirType>)> = HashSet::new();
    let mut worklist: Vec<Instance> = Vec::new();

    // Seed: scan all non-generic functions for calls with type_args.
    let empty_subst: HashMap<String, BirType> = HashMap::new();
    for func in &bir.functions {
        if !func.type_params.is_empty() {
            continue;
        }
        scan_function(
            func,
            &empty_subst,
            &mut worklist,
            &mut seen_func,
            &mut struct_instances,
        );
    }

    // Process worklist.
    while let Some(inst) = worklist.pop() {
        if let Some(func) = func_map.get(inst.func_name.as_str()) {
            let subst = inst.substitution_map(&func.type_params);
            scan_function(
                func,
                &subst,
                &mut worklist,
                &mut seen_func,
                &mut struct_instances,
            );
        }
        func_instances.push(inst);
    }

    MonoCollectResult {
        func_instances,
        struct_instances,
    }
}

fn scan_function(
    func: &BirFunction,
    subst: &HashMap<String, BirType>,
    worklist: &mut Vec<Instance>,
    seen_func: &mut HashSet<Instance>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    // Scan parameter types and return type.
    for (_, ty) in &func.params {
        let resolved = resolve_bir_type_lenient(ty, subst);
        collect_struct_instances(&resolved, struct_instances);
    }
    let resolved_ret = resolve_bir_type_lenient(&func.return_type, subst);
    collect_struct_instances(&resolved_ret, struct_instances);

    // Scan all basic blocks.
    for block in &func.blocks {
        scan_block(block, subst, worklist, seen_func, struct_instances);
    }
}

fn scan_block(
    block: &BasicBlock,
    subst: &HashMap<String, BirType>,
    worklist: &mut Vec<Instance>,
    seen_func: &mut HashSet<Instance>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    // Block params.
    for (_, ty) in &block.params {
        let resolved = resolve_bir_type_lenient(ty, subst);
        collect_struct_instances(&resolved, struct_instances);
    }

    for inst in &block.instructions {
        scan_instruction(inst, subst, worklist, seen_func, struct_instances);
    }

    scan_terminator(&block.terminator, subst, struct_instances);
}

fn scan_instruction(
    inst: &Instruction,
    subst: &HashMap<String, BirType>,
    worklist: &mut Vec<Instance>,
    seen_func: &mut HashSet<Instance>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    // Collect struct instances from all BirType fields.
    for ty in instruction_types(inst) {
        let resolved = resolve_bir_type_lenient(ty, subst);
        collect_struct_instances(&resolved, struct_instances);
    }

    // Handle generic calls.
    if let Instruction::Call {
        func_name,
        type_args,
        ..
    } = inst
        && !type_args.is_empty()
    {
        let resolved_args: Vec<BirType> = type_args
            .iter()
            .map(|t| resolve_bir_type_lenient(t, subst))
            .collect();
        let new_inst = Instance {
            func_name: func_name.clone(),
            type_args: resolved_args,
        };
        if !seen_func.contains(&new_inst) {
            seen_func.insert(new_inst.clone());
            worklist.push(new_inst);
        }
    }
}

fn instruction_types(inst: &Instruction) -> Vec<&BirType> {
    match inst {
        Instruction::Literal { ty, .. } => vec![ty],
        Instruction::BinaryOp { ty, .. } => vec![ty],
        Instruction::Call { ty, type_args, .. } => {
            let mut v: Vec<&BirType> = type_args.iter().collect();
            v.push(ty);
            v
        }
        Instruction::Compare { ty, .. } => vec![ty],
        Instruction::Not { .. } => vec![],
        Instruction::Cast { from_ty, to_ty, .. } => vec![from_ty, to_ty],
        Instruction::StructInit { ty, type_args, .. } => {
            let mut v: Vec<&BirType> = type_args.iter().collect();
            v.push(ty);
            v
        }
        Instruction::FieldGet { object_ty, ty, .. } => vec![object_ty, ty],
        Instruction::FieldSet { ty, .. } => vec![ty],
        Instruction::ArrayInit { ty, .. } => vec![ty],
        Instruction::ArrayGet { ty, .. } => vec![ty],
        Instruction::ArraySet { ty, .. } => vec![ty],
    }
}

fn scan_terminator(
    term: &Terminator,
    subst: &HashMap<String, BirType>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    match term {
        Terminator::Br { args, .. } => {
            for (_, ty) in args {
                let resolved = resolve_bir_type_lenient(ty, subst);
                collect_struct_instances(&resolved, struct_instances);
            }
        }
        Terminator::BrBreak { args, value, .. } => {
            for (_, ty) in args {
                let resolved = resolve_bir_type_lenient(ty, subst);
                collect_struct_instances(&resolved, struct_instances);
            }
            if let Some((_, ty)) = value {
                let resolved = resolve_bir_type_lenient(ty, subst);
                collect_struct_instances(&resolved, struct_instances);
            }
        }
        Terminator::BrContinue { args, .. } => {
            for (_, ty) in args {
                let resolved = resolve_bir_type_lenient(ty, subst);
                collect_struct_instances(&resolved, struct_instances);
            }
        }
        Terminator::Return(_) | Terminator::ReturnVoid | Terminator::CondBr { .. } => {}
    }
}

fn collect_struct_instances(ty: &BirType, set: &mut HashSet<(String, Vec<BirType>)>) {
    match ty {
        BirType::Struct { name, type_args } if !type_args.is_empty() => {
            set.insert((name.clone(), type_args.clone()));
            // Recurse into type_args.
            for arg in type_args {
                collect_struct_instances(arg, set);
            }
        }
        BirType::Array { element, .. } => {
            collect_struct_instances(element, set);
        }
        _ => {}
    }
}

/// Like `resolve_bir_type` but passes through TypeParams that are not in subst
/// (used when scanning non-generic callers where subst is empty and types are concrete).
fn resolve_bir_type_lenient(ty: &BirType, subst: &HashMap<String, BirType>) -> BirType {
    match ty {
        BirType::TypeParam(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        BirType::Array { element, size } => BirType::Array {
            element: Box::new(resolve_bir_type_lenient(element, subst)),
            size: *size,
        },
        BirType::Struct { name, type_args } => BirType::Struct {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|t| resolve_bir_type_lenient(t, subst))
                .collect(),
        },
        other => other.clone(),
    }
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

    // ------------------------------------------------------------------
    // Helper to build a minimal BirModule
    // ------------------------------------------------------------------
    fn make_module(functions: Vec<BirFunction>) -> BirModule {
        BirModule {
            struct_layouts: HashMap::new(),
            functions,
            conformance_map: HashMap::new(),
        }
    }

    fn make_block(
        label: u32,
        params: Vec<(super::super::instruction::Value, BirType)>,
        instructions: Vec<Instruction>,
        terminator: Terminator,
    ) -> BasicBlock {
        BasicBlock {
            label,
            params,
            instructions,
            terminator,
        }
    }

    // ------------------------------------------------------------------
    // Test 1: mono_collect_identity
    // ------------------------------------------------------------------
    #[test]
    fn mono_collect_identity() {
        use super::super::instruction::Value;

        // @identity<T>(%0: TypeParam("T")) -> TypeParam("T") { bb0: return %0 }
        let identity = BirFunction {
            name: "identity".into(),
            type_params: vec!["T".into()],
            params: vec![(Value(0), BirType::TypeParam("T".into()))],
            return_type: BirType::TypeParam("T".into()),
            blocks: vec![make_block(0, vec![], vec![], Terminator::Return(Value(0)))],
            body: vec![],
        };

        // @main() -> I32 {
        //   %0 = literal 42 : I32
        //   %1 = call @identity(%0) type_args=[I32] : I32
        //   return %1
        // }
        let main_func = BirFunction {
            name: "main".into(),
            type_params: vec![],
            params: vec![],
            return_type: BirType::I32,
            blocks: vec![make_block(
                0,
                vec![],
                vec![
                    Instruction::Literal {
                        result: Value(0),
                        value: 42,
                        ty: BirType::I32,
                    },
                    Instruction::Call {
                        result: Value(1),
                        func_name: "identity".into(),
                        args: vec![Value(0)],
                        type_args: vec![BirType::I32],
                        ty: BirType::I32,
                    },
                ],
                Terminator::Return(Value(1)),
            )],
            body: vec![],
        };

        let bir = make_module(vec![identity, main_func]);
        let result = mono_collect(&bir, "main");

        assert_eq!(result.func_instances.len(), 1);
        assert_eq!(result.func_instances[0].func_name, "identity");
        assert_eq!(result.func_instances[0].type_args, vec![BirType::I32]);
        assert!(result.struct_instances.is_empty());
    }

    // ------------------------------------------------------------------
    // Test 2: mono_collect_generic_struct
    // ------------------------------------------------------------------
    #[test]
    fn mono_collect_generic_struct() {
        use super::super::instruction::Value;

        let pair_ty = BirType::Struct {
            name: "Pair".into(),
            type_args: vec![
                BirType::TypeParam("T".into()),
                BirType::TypeParam("U".into()),
            ],
        };
        let pair_concrete = BirType::Struct {
            name: "Pair".into(),
            type_args: vec![BirType::I32, BirType::Bool],
        };

        // @idPair<T, U>(%0: Pair<T,U>) -> Pair<T,U> { return %0 }
        let id_pair = BirFunction {
            name: "idPair".into(),
            type_params: vec!["T".into(), "U".into()],
            params: vec![(Value(0), pair_ty.clone())],
            return_type: pair_ty.clone(),
            blocks: vec![make_block(0, vec![], vec![], Terminator::Return(Value(0)))],
            body: vec![],
        };

        // @main() -> I32 {
        //   %0 = struct_init @Pair { ... } type_args=[I32, Bool] : Pair<I32, Bool>
        //   %1 = call @idPair(%0) type_args=[I32, Bool] : Pair<I32, Bool>
        //   return %1
        // }
        let main_func = BirFunction {
            name: "main".into(),
            type_params: vec![],
            params: vec![],
            return_type: BirType::I32,
            blocks: vec![make_block(
                0,
                vec![],
                vec![
                    Instruction::StructInit {
                        result: Value(0),
                        struct_name: "Pair".into(),
                        fields: vec![("first".into(), Value(10)), ("second".into(), Value(11))],
                        type_args: vec![BirType::I32, BirType::Bool],
                        ty: pair_concrete.clone(),
                    },
                    Instruction::Call {
                        result: Value(1),
                        func_name: "idPair".into(),
                        args: vec![Value(0)],
                        type_args: vec![BirType::I32, BirType::Bool],
                        ty: pair_concrete.clone(),
                    },
                ],
                Terminator::Return(Value(1)),
            )],
            body: vec![],
        };

        let bir = make_module(vec![id_pair, main_func]);
        let result = mono_collect(&bir, "main");

        assert_eq!(result.func_instances.len(), 1);
        assert_eq!(result.func_instances[0].func_name, "idPair");
        assert_eq!(
            result.func_instances[0].type_args,
            vec![BirType::I32, BirType::Bool]
        );
        assert!(
            result
                .struct_instances
                .contains(&("Pair".into(), vec![BirType::I32, BirType::Bool]))
        );
    }

    // ------------------------------------------------------------------
    // Test 3: mono_collect_transitive
    // ------------------------------------------------------------------
    #[test]
    fn mono_collect_transitive() {
        use super::super::instruction::Value;

        // @bar<T>(%0: TypeParam("T")) -> TypeParam("T") { return %0 }
        let bar = BirFunction {
            name: "bar".into(),
            type_params: vec!["T".into()],
            params: vec![(Value(0), BirType::TypeParam("T".into()))],
            return_type: BirType::TypeParam("T".into()),
            blocks: vec![make_block(0, vec![], vec![], Terminator::Return(Value(0)))],
            body: vec![],
        };

        // @foo<T>(%0: TypeParam("T")) -> TypeParam("T") {
        //   %1 = call @bar(%0) type_args=[TypeParam("T")] : TypeParam("T")
        //   return %1
        // }
        let foo = BirFunction {
            name: "foo".into(),
            type_params: vec!["T".into()],
            params: vec![(Value(0), BirType::TypeParam("T".into()))],
            return_type: BirType::TypeParam("T".into()),
            blocks: vec![make_block(
                0,
                vec![],
                vec![Instruction::Call {
                    result: Value(1),
                    func_name: "bar".into(),
                    args: vec![Value(0)],
                    type_args: vec![BirType::TypeParam("T".into())],
                    ty: BirType::TypeParam("T".into()),
                }],
                Terminator::Return(Value(1)),
            )],
            body: vec![],
        };

        // @main() -> I32 {
        //   %0 = literal 42 : I32
        //   %1 = call @foo(%0) type_args=[I32] : I32
        //   return %1
        // }
        let main_func = BirFunction {
            name: "main".into(),
            type_params: vec![],
            params: vec![],
            return_type: BirType::I32,
            blocks: vec![make_block(
                0,
                vec![],
                vec![
                    Instruction::Literal {
                        result: Value(0),
                        value: 42,
                        ty: BirType::I32,
                    },
                    Instruction::Call {
                        result: Value(1),
                        func_name: "foo".into(),
                        args: vec![Value(0)],
                        type_args: vec![BirType::I32],
                        ty: BirType::I32,
                    },
                ],
                Terminator::Return(Value(1)),
            )],
            body: vec![],
        };

        let bir = make_module(vec![bar, foo, main_func]);
        let result = mono_collect(&bir, "main");

        assert_eq!(result.func_instances.len(), 2);
        let names: HashSet<&str> = result
            .func_instances
            .iter()
            .map(|i| i.func_name.as_str())
            .collect();
        assert!(names.contains("foo"));
        assert!(names.contains("bar"));
        for inst in &result.func_instances {
            assert_eq!(inst.type_args, vec![BirType::I32]);
        }
    }
}
