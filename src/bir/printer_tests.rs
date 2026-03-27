use super::super::lowering::lower_program;
use super::*;
use crate::lexer::tokenize;
use crate::parser::parse;

fn print_str(input: &str) -> String {
    let tokens = tokenize(input).unwrap();
    let program = parse(tokens).unwrap();
    let sem_info = crate::semantic::analyze_post_mono(&program).unwrap();
    let module = lower_program(&program, &sem_info).unwrap();
    print_module(&module)
}

#[test]
fn print_literal() {
    let output = print_str("func main() -> Int32 { return 42; }");
    let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 42 : Int32
    return %0
}
";
    assert_eq!(output, expected);
}

#[test]
fn print_binary_expr() {
    let output = print_str("2 + 3 * 4");
    let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 2 : Int32
    %1 = literal 3 : Int32
    %2 = literal 4 : Int32
    %3 = binary_op mul %1, %2 : Int32
    %4 = binary_op add %0, %3 : Int32
    return %4
}
";
    assert_eq!(output, expected);
}

#[test]
fn print_call() {
    let output = print_str(
        "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(1, 2); }",
    );
    assert!(output.contains("call @add(%0, %1) : Int32"));
}

#[test]
fn format_type_struct() {
    assert_eq!(
        format_type(&BirType::struct_simple("Foo".to_string())),
        "Foo"
    );
}

#[test]
fn print_struct_instructions() {
    use std::collections::HashMap;

    let module = BirModule {
        struct_layouts: HashMap::from([(
            "Point".to_string(),
            vec![
                ("x".to_string(), BirType::I32),
                ("y".to_string(), BirType::I32),
            ],
        )]),
        struct_type_params: HashMap::new(),
        conformance_map: HashMap::new(),
        functions: vec![BirFunction {
            name: "test".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: BirType::Unit,
            blocks: vec![BasicBlock {
                label: 0,
                params: vec![],
                instructions: vec![
                    Instruction::Literal {
                        result: Value(0),
                        value: 1,
                        ty: BirType::I32,
                    },
                    Instruction::Literal {
                        result: Value(1),
                        value: 2,
                        ty: BirType::I32,
                    },
                    Instruction::StructInit {
                        result: Value(2),
                        struct_name: "Point".to_string(),
                        fields: vec![("x".to_string(), Value(0)), ("y".to_string(), Value(1))],
                        type_args: vec![],
                        ty: BirType::struct_simple("Point".to_string()),
                    },
                    Instruction::FieldGet {
                        result: Value(3),
                        object: Value(2),
                        field: "x".to_string(),
                        object_ty: BirType::struct_simple("Point".to_string()),
                        ty: BirType::I32,
                    },
                    Instruction::FieldSet {
                        result: Value(4),
                        object: Value(2),
                        field: "x".to_string(),
                        value: Value(3),
                        ty: BirType::struct_simple("Point".to_string()),
                    },
                ],
                terminator: Terminator::ReturnVoid,
            }],
            body: vec![CfgRegion::Block(0)],
        }],
    };
    let output = print_module(&module);
    assert!(
        output.contains(r#"%2 = struct_init @Point { x: %0, y: %1 } : Point"#),
        "StructInit not found in:\n{}",
        output
    );
    assert!(
        output.contains(r#"%3 = field_get %2, "x" : Int32"#),
        "FieldGet not found in:\n{}",
        output
    );
    assert!(
        output.contains(r#"%4 = field_set %2, "x", %3 : Point"#),
        "FieldSet not found in:\n{}",
        output
    );
}
