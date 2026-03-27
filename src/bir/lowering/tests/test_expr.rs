use super::lower_str;

#[test]
fn lower_simple_return() {
    let output = lower_str("func main() -> Int32 { return 42; }");
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
fn lower_let_return() {
    let output = lower_str("func main() -> Int32 { let x: Int32 = 10; return x; }");
    let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 10 : Int32
    return %0
}
";
    assert_eq!(output, expected);
}

#[test]
fn lower_call() {
    let output = lower_str(
        "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }",
    );
    let expected = "\
bir @add(%0: Int32, %1: Int32) -> Int32 {
bb0:
    %2 = binary_op add %0, %1 : Int32
    return %2
}
bir @main() -> Int32 {
bb0:
    %0 = literal 3 : Int32
    %1 = literal 4 : Int32
    %2 = call @add(%0, %1) : Int32
    return %2
}
";
    assert_eq!(output, expected);
}

#[test]
fn lower_block_scope() {
    let output = lower_str(
        "func main() -> Int32 { let x: Int32 = 1; let y: Int32 = { let x: Int32 = 10; yield x + 1; }; return x + y; }",
    );
    let expected = "\
bir @main() -> Int32 {
bb0:
    %0 = literal 1 : Int32
    %1 = literal 10 : Int32
    %2 = literal 1 : Int32
    %3 = binary_op add %1, %2 : Int32
    %4 = binary_op add %0, %3 : Int32
    return %4
}
";
    assert_eq!(output, expected);
}
