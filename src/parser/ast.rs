#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: TypeAnnotation,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeAnnotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeAnnotation {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Unit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let { name: String, ty: Option<TypeAnnotation>, value: Expr },
    Var { name: String, ty: Option<TypeAnnotation>, value: Expr },
    Assign { name: String, value: Expr },
    Return(Option<Expr>),
    Yield(Expr),
    Break(Option<Expr>),
    Continue,
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, // +
    Sub, // -
    Mul, // *
    Div, // /
    Eq,  // ==
    Ne,  // !=
    Lt,  // <
    Gt,  // >
    Le,  // <=
    Ge,  // >=
    And, // &&
    Or,  // ||
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not, // !
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(i64),
    Float(f64),
    Ident(String),
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call { name: String, args: Vec<Expr> },
    Block(Block),
    Bool(bool),
    UnaryOp { op: UnaryOp, operand: Box<Expr> },
    If { condition: Box<Expr>, then_block: Block, else_block: Option<Block> },
    While { condition: Box<Expr>, body: Block, nobreak: Option<Block> },
    Cast { expr: Box<Expr>, target_type: TypeAnnotation },
}
