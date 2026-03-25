#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub structs: Vec<StructDef>,
    pub protocols: Vec<ProtocolDef>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: String,
    pub conformances: Vec<String>,
    pub members: Vec<StructMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDef {
    pub name: String,
    pub members: Vec<ProtocolMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolMember {
    MethodSig {
        name: String,
        params: Vec<Param>,
        return_type: TypeAnnotation,
    },
    PropertyReq {
        name: String,
        ty: TypeAnnotation,
        has_setter: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructMember {
    StoredProperty {
        name: String,
        ty: TypeAnnotation,
    },
    ComputedProperty {
        name: String,
        ty: TypeAnnotation,
        getter: Block,
        setter: Option<Block>,
    },
    Initializer {
        params: Vec<Param>,
        body: Block,
    },
    Method {
        name: String,
        params: Vec<Param>,
        return_type: TypeAnnotation,
        body: Block,
    },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeAnnotation {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Unit,
    Named(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<TypeAnnotation>,
        value: Expr,
    },
    Var {
        name: String,
        ty: Option<TypeAnnotation>,
        value: Expr,
    },
    Assign {
        name: String,
        value: Expr,
    },
    Return(Option<Expr>),
    Yield(Expr),
    Break(Option<Expr>),
    Continue,
    Expr(Expr),
    FieldAssign {
        object: Box<Expr>,
        field: String,
        value: Expr,
    },
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
pub enum ExprKind {
    Number(i64),
    Float(f64),
    Ident(String),
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Block(Block),
    Bool(bool),
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    If {
        condition: Box<Expr>,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Box<Expr>,
        body: Block,
        nobreak: Option<Block>,
    },
    Cast {
        expr: Box<Expr>,
        target_type: TypeAnnotation,
    },
    // Struct: expressions
    StructInit {
        name: String,
        args: Vec<(String, Expr)>,
    },
    FieldAccess {
        object: Box<Expr>,
        field: String,
    },
    SelfRef,
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
}
