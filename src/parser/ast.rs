#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    Public,
    Package,
    #[default]
    Internal,
    Fileprivate,
    Private,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    pub visibility: Visibility,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportTail {
    Single(String),
    Group(Vec<String>),
    Glob,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PathPrefix {
    SelfKw,
    Super,
    Named(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub visibility: Visibility,
    pub prefix: PathPrefix,
    pub path: Vec<String>,
    pub tail: ImportTail,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub module_decls: Vec<ModuleDecl>,
    pub import_decls: Vec<ImportDecl>,
    pub structs: Vec<StructDef>,
    pub protocols: Vec<ProtocolDef>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub visibility: Visibility,
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub conformances: Vec<String>,
    pub members: Vec<StructMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDef {
    pub visibility: Visibility,
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
        visibility: Visibility,
        name: String,
        ty: TypeAnnotation,
    },
    ComputedProperty {
        visibility: Visibility,
        name: String,
        ty: TypeAnnotation,
        getter: Block,
        setter: Option<Block>,
    },
    Initializer {
        visibility: Visibility,
        params: Vec<Param>,
        body: Block,
    },
    Method {
        visibility: Visibility,
        name: String,
        params: Vec<Param>,
        return_type: TypeAnnotation,
        body: Block,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub visibility: Visibility,
    pub name: String,
    pub type_params: Vec<TypeParam>,
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
pub struct TypeParam {
    pub name: String,
    pub bound: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeAnnotation {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Unit,
    Named(String),
    Generic {
        name: String,
        args: Vec<TypeAnnotation>,
    },
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
        type_args: Vec<TypeAnnotation>,
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
        type_args: Vec<TypeAnnotation>,
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
