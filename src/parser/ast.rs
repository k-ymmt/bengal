#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, // +
    Sub, // -
    Mul, // *
    Div, // /
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Number(i32),
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}
