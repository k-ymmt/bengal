mod lower_control_flow;
mod lower_expr;
mod lower_short_circuit;
mod lower_stmt;
mod lower_struct;
mod lower_struct_field;

mod lower_program;

#[cfg(test)]
mod tests;
pub(crate) use lower_program::lower_module_with_inferred;
pub use lower_program::{lower_module, lower_program};

use std::collections::{HashMap, HashSet};

use crate::error::{BengalError, Result, Span};
use crate::parser::ast::*;
use crate::semantic::resolver;

use super::instruction::*;

enum StmtResult {
    None,
    Return(Value),
    ReturnVoid,
    Yield(Value),
    Break,
    Continue,
}

struct LoopContext {
    header_bb: u32,
    exit_bb: u32,
    break_ty: Option<BirType>,
}

struct SemInfoRef {
    struct_defs: HashMap<String, resolver::StructInfo>,
    struct_init_calls: HashSet<NodeId>,
    protocols: HashMap<String, resolver::ProtocolInfo>,
}

struct ReceiverInfo {
    value: Value,
    struct_name: String,
    var_name: String,
}

struct Lowering {
    next_value: u32,
    next_block: u32,
    scopes: Vec<HashMap<String, Value>>,
    // Block management
    blocks: Vec<BasicBlock>,
    current_instructions: Vec<Instruction>,
    current_block_label: u32,
    current_block_params: Vec<(Value, BirType)>,
    // CfgRegion tracking -- regions generated during expression lowering
    pending_regions: Vec<CfgRegion>,
    // Function signatures for Call return type lookup
    func_sigs: HashMap<String, BirType>,
    // Function type parameter names (func_name -> [param_names])
    func_type_param_names: HashMap<String, Vec<String>>,
    // Track which variables are mutable (for while loop block args)
    mutable_vars: Vec<(String, BirType)>,
    // Loop context stack for break/continue
    loop_stack: Vec<LoopContext>,
    // Track Value types for cast/multi-numeric support
    value_types: HashMap<Value, BirType>,
    // Struct support
    sem_info: Option<SemInfoRef>,
    self_var_name: Option<String>,
    in_init_body: bool,
    init_struct_name: Option<String>,
    lowering_errors: Vec<BengalError>,
    // Name mangling map for per-module lowering: local name -> mangled name
    name_map: Option<HashMap<String, String>>,
    // Set by lower_if when both branches diverge (all paths return/break/continue)
    last_expr_diverged: bool,
    // When inlining a getter, redirect `return expr` to this continuation block
    // instead of emitting Terminator::Return.  The block has one param for the value.
    getter_return_bb: Option<(u32, Value, BirType)>,
    // Current function's type params (for protocol constraint lookup)
    current_type_params: Vec<TypeParam>,
    // Inferred type args from type inference (for omitted type args at call sites)
    inferred_type_args: HashMap<NodeId, Vec<TypeAnnotation>>,
}

impl Lowering {
    fn new(func_sigs: HashMap<String, BirType>, sem_info: SemInfoRef) -> Self {
        Self {
            next_value: 0,
            next_block: 0,
            scopes: Vec::new(),
            blocks: Vec::new(),
            current_instructions: Vec::new(),
            current_block_label: 0,
            current_block_params: Vec::new(),
            pending_regions: Vec::new(),
            func_sigs,
            func_type_param_names: HashMap::new(),
            mutable_vars: Vec::new(),
            loop_stack: Vec::new(),
            value_types: HashMap::new(),
            sem_info: Some(sem_info),
            self_var_name: None,
            in_init_body: false,
            init_struct_name: None,
            lowering_errors: Vec::new(),
            name_map: None,
            last_expr_diverged: false,
            getter_return_bb: None,
            current_type_params: Vec::new(),
            inferred_type_args: HashMap::new(),
        }
    }

    fn convert_type_with_structs(&self, ty: &TypeAnnotation) -> BirType {
        match ty {
            TypeAnnotation::Named(name) => {
                // Check if this name refers to a type parameter of the current function
                if self.current_type_params.iter().any(|tp| tp.name == *name) {
                    BirType::TypeParam(name.clone())
                } else {
                    BirType::struct_simple(name.clone())
                }
            }
            TypeAnnotation::Generic { name, args } => {
                // Generic struct instantiation (e.g. Box<Int32>)
                let bir_args: Vec<BirType> = args
                    .iter()
                    .map(|a| self.convert_type_with_structs(a))
                    .collect();
                BirType::Struct {
                    name: name.clone(),
                    type_args: bir_args,
                }
            }
            TypeAnnotation::Array { element, size } => BirType::Array {
                element: Box::new(self.convert_type_with_structs(element)),
                size: *size,
            },
            other => convert_type(other),
        }
    }

    /// Resolve a function/method name through the name_map (if present).
    /// Returns the mangled name if a mapping exists, otherwise returns the original name.
    fn resolve_name(&self, name: &str) -> String {
        if let Some(map) = &self.name_map {
            map.get(name).cloned().unwrap_or_else(|| name.to_string())
        } else {
            name.to_string()
        }
    }

    fn record_error(&mut self, message: impl Into<String>, span: Option<Span>) -> Value {
        self.lowering_errors.push(BengalError::LoweringError {
            message: message.into(),
            span,
        });
        let dummy = self.fresh_value();
        self.emit(Instruction::Literal {
            result: dummy,
            value: 0,
            ty: BirType::Unit,
        });
        self.value_types.insert(dummy, BirType::Unit);
        dummy
    }

    fn fresh_value(&mut self) -> Value {
        let v = Value(self.next_value);
        self.next_value += 1;
        v
    }

    fn fresh_block(&mut self) -> u32 {
        let label = self.next_block;
        self.next_block += 1;
        label
    }

    fn seal_block(&mut self, terminator: Terminator) {
        let block = BasicBlock {
            label: self.current_block_label,
            params: std::mem::take(&mut self.current_block_params),
            instructions: std::mem::take(&mut self.current_instructions),
            terminator,
        };
        self.blocks.push(block);
    }

    fn start_block(&mut self, label: u32, params: Vec<(Value, BirType)>) {
        self.current_block_label = label;
        self.current_block_params = params;
        self.current_instructions = Vec::new();
    }

    fn emit(&mut self, inst: Instruction) {
        self.current_instructions.push(inst);
    }

    fn take_pending_regions(&mut self) -> Vec<CfgRegion> {
        std::mem::take(&mut self.pending_regions)
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_var(&mut self, name: String, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    fn lookup_var(&self, name: &str) -> Value {
        for scope in self.scopes.iter().rev() {
            if let Some(&value) = scope.get(name) {
                return value;
            }
        }
        unreachable!(
            "undefined variable `{}` (should be caught by semantic analysis)",
            name
        )
    }

    fn assign_var(&mut self, name: &str, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return;
            }
        }
        unreachable!(
            "undefined variable `{}` (should be caught by semantic analysis)",
            name
        )
    }

    fn try_lookup_var(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(&value) = scope.get(name) {
                return Some(value);
            }
        }
        None
    }

    // ========== Function ==========

    fn lower_function(&mut self, func: &Function) -> BirFunction {
        self.next_value = 0;
        self.next_block = 0;
        self.scopes.clear();
        self.blocks.clear();
        self.pending_regions.clear();
        self.mutable_vars.clear();
        self.value_types.clear();
        self.current_type_params = func.type_params.clone();

        let bb0 = self.fresh_block();
        self.start_block(bb0, vec![]);
        self.push_scope();

        // Register parameters
        let mut params = Vec::new();
        for param in &func.params {
            let val = self.fresh_value();
            let bir_ty = self.convert_type_with_structs(&param.ty);
            params.push((val, bir_ty.clone()));
            self.value_types.insert(val, bir_ty);
            self.define_var(param.name.clone(), val);
        }

        let (result, body_regions) = self.lower_block_stmts(func.body.as_ref().unwrap());

        self.pop_scope();

        // Seal the final block with terminator
        match result {
            Some(StmtResult::Return(v)) => {
                self.seal_block(Terminator::Return(v));
            }
            Some(StmtResult::ReturnVoid) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            _ => unreachable!("function must end with return (semantic analysis guarantees this)"),
        }

        // Build CfgRegion body: body_regions + final block (which was just sealed)
        let final_label = self.blocks.last().unwrap().label;
        let mut body = body_regions;
        body.push(CfgRegion::Block(final_label));

        let blocks = std::mem::take(&mut self.blocks);

        BirFunction {
            name: self.resolve_name(&func.name),
            type_params: func.type_params.iter().map(|tp| tp.name.clone()).collect(),
            params,
            return_type: self.convert_type_with_structs(&func.return_type),
            blocks,
            body,
        }
    }

    // ========== Block ==========

    /// Lower a block's statements. Returns (StmtResult, collected CfgRegions).
    /// Does NOT push/pop scope -- caller handles that.
    fn lower_block_stmts(&mut self, block: &Block) -> (Option<StmtResult>, Vec<CfgRegion>) {
        let mut regions = Vec::new();
        let mut result = None;

        for stmt in &block.stmts {
            let sr = self.lower_stmt(stmt);

            // Collect any CfgRegions generated during this statement
            regions.append(&mut self.take_pending_regions());

            match sr {
                StmtResult::Return(v) => {
                    result = Some(StmtResult::Return(v));
                    break;
                }
                StmtResult::ReturnVoid => {
                    result = Some(StmtResult::ReturnVoid);
                    break;
                }
                StmtResult::Yield(v) => {
                    result = Some(StmtResult::Yield(v));
                    break;
                }
                StmtResult::Break => {
                    result = Some(StmtResult::Break);
                    break;
                }
                StmtResult::Continue => {
                    result = Some(StmtResult::Continue);
                    break;
                }
                StmtResult::None => {}
            }
        }

        (result, regions)
    }
}

fn convert_binop(op: BinOp) -> BirBinOp {
    match op {
        BinOp::Add => BirBinOp::Add,
        BinOp::Sub => BirBinOp::Sub,
        BinOp::Mul => BirBinOp::Mul,
        BinOp::Div => BirBinOp::Div,
        _ => unreachable!("non-arithmetic BinOp should be handled separately"),
    }
}

fn convert_compare_op(op: BinOp) -> BirCompareOp {
    match op {
        BinOp::Eq => BirCompareOp::Eq,
        BinOp::Ne => BirCompareOp::Ne,
        BinOp::Lt => BirCompareOp::Lt,
        BinOp::Gt => BirCompareOp::Gt,
        BinOp::Le => BirCompareOp::Le,
        BinOp::Ge => BirCompareOp::Ge,
        _ => unreachable!("non-comparison BinOp"),
    }
}

pub fn semantic_type_to_bir(ty: &crate::semantic::types::Type) -> BirType {
    match ty {
        crate::semantic::types::Type::I32 => BirType::I32,
        crate::semantic::types::Type::I64 => BirType::I64,
        crate::semantic::types::Type::F32 => BirType::F32,
        crate::semantic::types::Type::F64 => BirType::F64,
        crate::semantic::types::Type::Bool => BirType::Bool,
        crate::semantic::types::Type::Unit => BirType::Unit,
        crate::semantic::types::Type::Struct(name) => BirType::struct_simple(name.clone()),
        crate::semantic::types::Type::TypeParam { name, .. } => BirType::TypeParam(name.clone()),
        crate::semantic::types::Type::Generic { name, .. } => {
            panic!("unresolved generic type `{}` in BIR lowering", name)
        }
        crate::semantic::types::Type::Array { element, size } => BirType::Array {
            element: Box::new(semantic_type_to_bir(element)),
            size: *size,
        },
        crate::semantic::types::Type::InferVar(_)
        | crate::semantic::types::Type::IntegerLiteral(_)
        | crate::semantic::types::Type::FloatLiteral(_) => {
            unreachable!("inference type in post-mono pass")
        }
        crate::semantic::types::Type::Error => BirType::Error,
    }
}

fn check_acyclic_structs(
    layouts: &HashMap<String, Vec<(String, BirType)>>,
    struct_spans: &HashMap<&str, Span>,
) -> Result<()> {
    fn visit(
        name: &str,
        layouts: &HashMap<String, Vec<(String, BirType)>>,
        struct_spans: &HashMap<&str, Span>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
        if visited.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name.to_string()) {
            return Err(BengalError::LoweringError {
                message: format!(
                    "recursive struct `{}` is not supported (infinitely sized)",
                    name
                ),
                span: struct_spans.get(name).copied(),
            });
        }
        if let Some(fields) = layouts.get(name) {
            for (_, ty) in fields {
                if let BirType::Struct { name: dep, .. } = ty {
                    visit(dep, layouts, struct_spans, visiting, visited)?;
                }
            }
        }
        visiting.remove(name);
        visited.insert(name.to_string());
        Ok(())
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for name in layouts.keys() {
        visit(name, layouts, struct_spans, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn convert_type(ty: &TypeAnnotation) -> BirType {
    match ty {
        TypeAnnotation::I32 => BirType::I32,
        TypeAnnotation::I64 => BirType::I64,
        TypeAnnotation::F32 => BirType::F32,
        TypeAnnotation::F64 => BirType::F64,
        TypeAnnotation::Bool => BirType::Bool,
        TypeAnnotation::Unit => BirType::Unit,
        TypeAnnotation::Named(name) => {
            panic!("Named type `{}` not yet supported in convert_type", name)
        }
        TypeAnnotation::Generic { name, .. } => {
            panic!("Generic type `{}` not yet supported in convert_type", name)
        }
        TypeAnnotation::Array { element, size } => BirType::Array {
            element: Box::new(convert_type(element)),
            size: *size,
        },
    }
}
