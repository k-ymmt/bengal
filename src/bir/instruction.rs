#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BirType {
    Unit,
    I32,
    I64,
    F32,
    F64,
    Bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BirBinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    /// %result = literal <value> : <type>
    Literal {
        result: Value,
        value: i64,
        ty: BirType,
    },
    /// %result = binary_op <op> %lhs, %rhs : <type>
    BinaryOp {
        result: Value,
        op: BirBinOp,
        lhs: Value,
        rhs: Value,
        ty: BirType,
    },
    /// %result = call @func_name(%arg0, %arg1, ...) : <type>
    Call {
        result: Value,
        func_name: String,
        args: Vec<Value>,
        ty: BirType,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminator {
    /// return %value
    Return(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicBlock {
    pub label: u32,
    pub params: Vec<(Value, BirType)>,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BirFunction {
    pub name: String,
    pub params: Vec<(Value, BirType)>,
    pub return_type: BirType,
    pub blocks: Vec<BasicBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BirModule {
    pub functions: Vec<BirFunction>,
}
