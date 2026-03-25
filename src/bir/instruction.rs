use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BirType {
    Unit,
    I32,
    I64,
    F32,
    F64,
    Bool,
    Struct(String),
    Array { element: Box<BirType>, size: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BirBinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BirCompareOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
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
    /// %result = compare <op> %lhs, %rhs : <type>
    Compare {
        result: Value,
        op: BirCompareOp,
        lhs: Value,
        rhs: Value,
        ty: BirType,
    },
    /// %result = not %operand : bool
    Not { result: Value, operand: Value },
    /// %result = cast %operand : <from_ty> -> <to_ty>
    Cast {
        result: Value,
        operand: Value,
        from_ty: BirType,
        to_ty: BirType,
    },
    /// %result = struct_init @StructName { field0: %v0, ... } : Struct
    StructInit {
        result: Value,
        struct_name: String,
        fields: Vec<(String, Value)>,
        ty: BirType,
    },
    /// %result = field_get %object, "field_name" : FieldType
    FieldGet {
        result: Value,
        object: Value,
        field: String,
        object_ty: BirType,
        ty: BirType,
    },
    /// %result = field_set %object, "field_name", %value : StructType
    FieldSet {
        result: Value,
        object: Value,
        field: String,
        value: Value,
        ty: BirType,
    },
    /// %result = array_init [%e0, %e1, ...] : [ElemType; N]
    ArrayInit {
        result: Value,
        ty: BirType,
        elements: Vec<Value>,
    },
    /// %result = array_get %array, %index : ElemType (array_size for runtime bounds check)
    ArrayGet {
        result: Value,
        ty: BirType,
        array: Value,
        index: Value,
        array_size: u64,
    },
    /// %result = array_set %array, %index, %value : [ElemType; N] (array_size for runtime bounds check)
    ArraySet {
        result: Value,
        ty: BirType,
        array: Value,
        index: Value,
        value: Value,
        array_size: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminator {
    /// return %value
    Return(Value),
    /// return (void)
    ReturnVoid,
    /// br bb_target(%arg0: type, %arg1: type, ...)
    Br {
        target: u32,
        args: Vec<(Value, BirType)>,
    },
    /// cond_br %cond, bb_then, bb_else
    CondBr {
        cond: Value,
        then_bb: u32,
        else_bb: u32,
    },
    /// break — exit while loop, passing updated mutable vars + optional break value
    BrBreak {
        header_bb: u32,
        exit_bb: u32,
        args: Vec<(Value, BirType)>,
        value: Option<(Value, BirType)>,
    },
    /// continue — jump to while header, passing updated mutable vars
    BrContinue {
        header_bb: u32,
        args: Vec<(Value, BirType)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicBlock {
    pub label: u32,
    pub params: Vec<(Value, BirType)>,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgRegion {
    /// Single basic block — emit instructions + terminator
    Block(u32),
    /// if/else branch
    IfElse {
        cond_region: Vec<CfgRegion>,
        cond_bb: u32,
        cond_value: Value,
        then_region: Vec<CfgRegion>,
        else_region: Vec<CfgRegion>,
        merge_bb: u32,
    },
    /// if without else
    IfOnly {
        cond_region: Vec<CfgRegion>,
        cond_bb: u32,
        cond_value: Value,
        then_region: Vec<CfgRegion>,
        merge_bb: u32,
    },
    /// while loop
    While {
        entry_bb: u32,
        header_region: Vec<CfgRegion>,
        header_bb: u32,
        cond_value: Value,
        body_region: Vec<CfgRegion>,
        nobreak_region: Vec<CfgRegion>,
        exit_bb: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BirFunction {
    pub name: String,
    pub params: Vec<(Value, BirType)>,
    pub return_type: BirType,
    pub blocks: Vec<BasicBlock>,
    pub body: Vec<CfgRegion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BirModule {
    pub struct_layouts: HashMap<String, Vec<(String, BirType)>>,
    pub functions: Vec<BirFunction>,
}
