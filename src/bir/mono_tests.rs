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
        func_name: "_BGFN3pkg8identityE".into(),
        type_args: vec![BirType::I32],
    };
    assert_eq!(inst.mangled_name(), "_BGFN3pkg8identityEIiE");
}

#[test]
fn instance_mangle_multi() {
    let inst = Instance {
        func_name: "_BGFN3pkg4swapE".into(),
        type_args: vec![BirType::I32, BirType::Bool],
    };
    assert_eq!(inst.mangled_name(), "_BGFN3pkg4swapEIibE");
}

#[test]
fn instance_mangle_struct_arg() {
    let inst = Instance {
        func_name: "_BGFN3pkg8getFirstE".into(),
        type_args: vec![BirType::struct_simple("Point".into()), BirType::I32],
    };
    assert_eq!(inst.mangled_name(), "_BGFN3pkg8getFirstEIS5PointiE");
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
        struct_type_params: HashMap::new(),
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
