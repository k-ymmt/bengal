use super::*;
use crate::lexer::tokenize;

fn parse_source(source: &str) -> Program {
    let tokens = tokenize(source).unwrap();
    parse(tokens).unwrap()
}

#[test]
fn parse_module_decl() {
    let prog = parse_source("module math; func main() -> Int32 { return 0; }");
    assert_eq!(prog.module_decls.len(), 1);
    assert_eq!(prog.module_decls[0].name, "math");
    assert_eq!(prog.module_decls[0].visibility, Visibility::Internal);
}

#[test]
fn parse_public_module_decl() {
    let prog = parse_source("public module math; func main() -> Int32 { return 0; }");
    assert_eq!(prog.module_decls[0].visibility, Visibility::Public);
}

#[test]
fn parse_import_single() {
    let prog = parse_source("import math::Vector; func main() -> Int32 { return 0; }");
    assert_eq!(prog.import_decls.len(), 1);
    assert_eq!(
        prog.import_decls[0].prefix,
        PathPrefix::Named("math".to_string())
    );
    assert_eq!(
        prog.import_decls[0].tail,
        ImportTail::Single("Vector".to_string())
    );
}

#[test]
fn parse_import_group() {
    let prog = parse_source("import math::{Vector, Matrix}; func main() -> Int32 { return 0; }");
    assert_eq!(
        prog.import_decls[0].tail,
        ImportTail::Group(vec!["Vector".to_string(), "Matrix".to_string()])
    );
}

#[test]
fn parse_import_glob() {
    let prog = parse_source("import math::*; func main() -> Int32 { return 0; }");
    assert_eq!(prog.import_decls[0].tail, ImportTail::Glob);
}

#[test]
fn parse_import_self_path() {
    let prog = parse_source("import self::sub::helper; func main() -> Int32 { return 0; }");
    assert_eq!(prog.import_decls[0].prefix, PathPrefix::SelfKw);
    assert_eq!(prog.import_decls[0].path, vec!["sub".to_string()]);
    assert_eq!(
        prog.import_decls[0].tail,
        ImportTail::Single("helper".to_string())
    );
}

#[test]
fn parse_import_super_path() {
    let prog = parse_source("import super::common::Util; func main() -> Int32 { return 0; }");
    assert_eq!(prog.import_decls[0].prefix, PathPrefix::Super);
    assert_eq!(prog.import_decls[0].path, vec!["common".to_string()]);
    assert_eq!(
        prog.import_decls[0].tail,
        ImportTail::Single("Util".to_string())
    );
}

#[test]
fn parse_public_import_reexport() {
    let prog =
        parse_source("public import self::internal::Vector; func main() -> Int32 { return 0; }");
    assert_eq!(prog.import_decls[0].visibility, Visibility::Public);
}

#[test]
fn parse_visibility_on_func() {
    let prog = parse_source(
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return 0; }",
    );
    assert_eq!(prog.functions[0].visibility, Visibility::Public);
    assert_eq!(prog.functions[1].visibility, Visibility::Internal);
}

#[test]
fn parse_visibility_on_struct() {
    let prog = parse_source(
        "public struct Foo { private var x: Int32; } func main() -> Int32 { return 0; }",
    );
    assert_eq!(prog.structs[0].visibility, Visibility::Public);
    match &prog.structs[0].members[0] {
        StructMember::StoredProperty { visibility, .. } => {
            assert_eq!(*visibility, Visibility::Private);
        }
        _ => panic!("expected StoredProperty"),
    }
}

#[test]
fn parse_nested_import_path() {
    let prog =
        parse_source("import graphics::renderer::Shader; func main() -> Int32 { return 0; }");
    assert_eq!(
        prog.import_decls[0].prefix,
        PathPrefix::Named("graphics".to_string())
    );
    assert_eq!(prog.import_decls[0].path, vec!["renderer".to_string()]);
    assert_eq!(
        prog.import_decls[0].tail,
        ImportTail::Single("Shader".to_string())
    );
}

#[test]
fn parse_generic_function_def() {
    let tokens = tokenize("func identity<T>(value: T) -> T { return value; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
    let func = &program.functions[0];
    assert_eq!(func.type_params.len(), 1);
    assert_eq!(func.type_params[0].name, "T");
    assert_eq!(func.type_params[0].bound, None);
}

#[test]
fn parse_generic_function_with_bound() {
    let tokens = tokenize("func constrain<T: Summable>(item: T) -> Int32 { return 0; }").unwrap();
    let program = parse(tokens).unwrap();
    let func = &program.functions[0];
    assert_eq!(func.type_params.len(), 1);
    assert_eq!(func.type_params[0].name, "T");
    assert_eq!(func.type_params[0].bound, Some("Summable".to_string()));
}

#[test]
fn parse_generic_function_multi_params() {
    let tokens = tokenize("func pair<A, B>(a: A, b: B) -> A { return a; }").unwrap();
    let program = parse(tokens).unwrap();
    let func = &program.functions[0];
    assert_eq!(func.type_params.len(), 2);
    assert_eq!(func.type_params[0].name, "A");
    assert_eq!(func.type_params[1].name, "B");
}

#[test]
fn parse_generic_struct_def() {
    let tokens =
        tokenize("struct Box<T> { var value: T; } func main() -> Int32 { return 0; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.structs[0].type_params.len(), 1);
    assert_eq!(program.structs[0].type_params[0].name, "T");
}

#[test]
fn parse_generic_type_annotation() {
    let tokens =
        tokenize("func main() -> Int32 { let x: Box<Int32> = Box<Int32>(value: 1); return 0; }")
            .unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn parse_generic_call_with_type_args() {
    let tokens = tokenize(
        "func id<T>(v: T) -> T { return v; } func main() -> Int32 { return id<Int32>(3); }",
    )
    .unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 2);
    // Verify the call in main's return actually has type_args
    let main_fn = &program.functions[1];
    let ret_stmt = &main_fn.body.stmts[0];
    match ret_stmt {
        Stmt::Return(Some(expr)) => match &expr.kind {
            ExprKind::Call {
                name, type_args, ..
            } => {
                assert_eq!(name, "id");
                assert_eq!(type_args.len(), 1);
                assert_eq!(type_args[0], TypeAnnotation::I32);
            }
            other => panic!("expected Call, got {:?}", other),
        },
        other => panic!("expected Return, got {:?}", other),
    }
}

#[test]
fn parse_array_type() {
    let tokens =
        tokenize("func main() -> Int32 { let a: [Int32; 3] = [1, 2, 3]; return 0; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn parse_array_literal() {
    let tokens = tokenize("func main() -> Int32 { let a = [1, 2, 3]; return 0; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn parse_index_access() {
    let tokens = tokenize("func main() -> Int32 { let a = [1, 2, 3]; return a[0]; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn parse_index_assign() {
    let tokens =
        tokenize("func main() -> Int32 { var a = [1, 2, 3]; a[0] = 10; return a[0]; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}
