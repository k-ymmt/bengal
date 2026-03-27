use super::lower_str;

#[test]
fn lower_if_else() {
    let output = lower_str(
        "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }",
    );
    // Should have 4 blocks with cond_br
    assert!(output.contains("cond_br"));
    assert!(output.contains("bb1"));
    assert!(output.contains("bb2"));
    assert!(output.contains("bb3"));
}

#[test]
fn lower_while() {
    let output = lower_str(
        "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; }; return i; }",
    );
    // Should have blocks for entry, header, body, exit
    assert!(output.contains("cond_br"));
    assert!(output.contains("compare lt"));
}
