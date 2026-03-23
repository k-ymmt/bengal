use std::collections::{HashMap, HashSet};

use crate::error::{BengalError, Result};
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

#[derive(Clone)]
struct StructMeta {
    struct_name: String,
    fields: Vec<String>,
}

struct SemInfoRef {
    struct_defs: HashMap<String, resolver::StructInfo>,
    struct_init_calls: HashSet<NodeId>,
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
    // CfgRegion tracking — regions generated during expression lowering
    pending_regions: Vec<CfgRegion>,
    // Function signatures for Call return type lookup
    func_sigs: HashMap<String, BirType>,
    // Track which variables are mutable (for while loop block args)
    mutable_vars: Vec<(String, BirType)>,
    // Loop context stack for break/continue
    loop_stack: Vec<LoopContext>,
    // Track Value types for cast/multi-numeric support
    value_types: HashMap<Value, BirType>,
    // Struct support
    struct_meta_scopes: Vec<HashMap<String, StructMeta>>,
    sem_info: Option<SemInfoRef>,
    self_var_name: Option<String>,
    lowering_error: Option<BengalError>,
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
            mutable_vars: Vec::new(),
            loop_stack: Vec::new(),
            value_types: HashMap::new(),
            struct_meta_scopes: Vec::new(),
            sem_info: Some(sem_info),
            self_var_name: None,
            lowering_error: None,
        }
    }

    fn record_error(&mut self, message: impl Into<String>) -> Value {
        if self.lowering_error.is_none() {
            self.lowering_error = Some(BengalError::LoweringError {
                message: message.into(),
            });
        }
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
        self.struct_meta_scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.struct_meta_scopes.pop();
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

    fn define_struct_var(
        &mut self,
        var_name: &str,
        struct_name: &str,
        field_values: Vec<(String, Value)>,
    ) {
        let struct_info = self
            .sem_info
            .as_ref()
            .unwrap()
            .struct_defs
            .get(struct_name)
            .unwrap();
        let fields: Vec<String> = struct_info.fields.iter().map(|(n, _)| n.clone()).collect();
        for (fname, val) in &field_values {
            let key = format!("{}.{}", var_name, fname);
            self.define_var(key, *val);
        }
        if let Some(scope) = self.struct_meta_scopes.last_mut() {
            scope.insert(
                var_name.to_string(),
                StructMeta {
                    struct_name: struct_name.to_string(),
                    fields,
                },
            );
        }
    }

    fn lookup_struct_meta(&self, var_name: &str) -> Option<&StructMeta> {
        for scope in self.struct_meta_scopes.iter().rev() {
            if let Some(meta) = scope.get(var_name) {
                return Some(meta);
            }
        }
        None
    }

    fn resolve_field_access_key(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.clone()),
            ExprKind::SelfRef => Some(
                self.self_var_name
                    .clone()
                    .unwrap_or_else(|| "self".to_string()),
            ),
            ExprKind::FieldAccess { object, field } => self
                .resolve_field_access_key(object)
                .map(|base| format!("{}.{}", base, field)),
            _ => None,
        }
    }

    /// Collect current values of all mutable variables for while loop block args
    fn collect_mutable_var_values(&self) -> Vec<(String, Value, BirType)> {
        self.mutable_vars
            .iter()
            .map(|(name, ty)| (name.clone(), self.lookup_var(name), *ty))
            .collect()
    }

    // ========== Function ==========

    fn lower_function(&mut self, func: &Function) -> BirFunction {
        self.next_value = 0;
        self.next_block = 0;
        self.scopes.clear();
        self.blocks.clear();
        self.pending_regions.clear();
        self.mutable_vars.clear();
        self.struct_meta_scopes.clear();

        let bb0 = self.fresh_block();
        self.start_block(bb0, vec![]);
        self.push_scope();

        // Register parameters
        let mut params = Vec::new();
        for param in &func.params {
            let val = self.fresh_value();
            let bir_ty = convert_type(&param.ty);
            params.push((val, bir_ty));
            self.define_var(param.name.clone(), val);
        }

        let (result, body_regions) = self.lower_block_stmts(&func.body);

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
            name: func.name.clone(),
            params,
            return_type: convert_type(&func.return_type),
            blocks,
            body,
        }
    }

    // ========== Block ==========

    /// Lower a block's statements. Returns (StmtResult, collected CfgRegions).
    /// Does NOT push/pop scope — caller handles that.
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

    // ========== Struct helpers ==========

    fn struct_type_of_expr(&self, expr: &Expr) -> Option<String> {
        let sem = self.sem_info.as_ref().unwrap();
        match &expr.kind {
            ExprKind::StructInit { name, .. } => Some(name.clone()),
            ExprKind::Call { name, .. } if sem.struct_init_calls.contains(&expr.id) => {
                Some(name.clone())
            }
            ExprKind::Ident(name) => self.lookup_struct_meta(name).map(|m| m.struct_name.clone()),
            _ => None,
        }
    }

    fn lower_struct_producing_expr(
        &mut self,
        expr: &Expr,
        struct_name: &str,
    ) -> Vec<(String, Value)> {
        let sem = self.sem_info.as_ref().unwrap();
        let struct_info = sem.struct_defs.get(struct_name).unwrap().clone();

        match &expr.kind {
            ExprKind::StructInit { .. } | ExprKind::Call { .. }
                if matches!(&expr.kind, ExprKind::StructInit { .. })
                    || self
                        .sem_info
                        .as_ref()
                        .unwrap()
                        .struct_init_calls
                        .contains(&expr.id) =>
            {
                self.lower_struct_init_fields(struct_name, &struct_info, expr)
            }
            ExprKind::Ident(name) => {
                let meta = self.lookup_struct_meta(name).unwrap().clone();
                meta.fields
                    .iter()
                    .map(|fname| {
                        let key = format!("{}.{}", name, fname);
                        let val = self.lookup_var(&key);
                        (fname.clone(), val)
                    })
                    .collect()
            }
            _ => unreachable!("not a struct-producing expression"),
        }
    }

    fn lower_struct_init_fields(
        &mut self,
        struct_name: &str,
        struct_info: &resolver::StructInfo,
        expr: &Expr,
    ) -> Vec<(String, Value)> {
        if struct_info.init.body.is_none() {
            // Memberwise init: args map directly to stored fields
            let args = match &expr.kind {
                ExprKind::StructInit { args, .. } => args,
                ExprKind::Call { args, .. } if args.is_empty() => {
                    return vec![];
                }
                _ => unreachable!(),
            };
            args.iter()
                .map(|(label, arg_expr)| {
                    let val = self.lower_expr(arg_expr);
                    (label.clone(), val)
                })
                .collect()
        } else {
            self.lower_explicit_init(struct_name, struct_info, expr)
        }
    }

    fn lower_explicit_init(
        &mut self,
        struct_name: &str,
        struct_info: &resolver::StructInfo,
        expr: &Expr,
    ) -> Vec<(String, Value)> {
        let init_info = &struct_info.init;
        let body = init_info.body.as_ref().unwrap().clone();

        // Evaluate init arguments
        let arg_values: Vec<Value> = match &expr.kind {
            ExprKind::StructInit { args, .. } => {
                args.iter().map(|(_, e)| self.lower_expr(e)).collect()
            }
            ExprKind::Call { args, .. } => args.iter().map(|e| self.lower_expr(e)).collect(),
            _ => unreachable!(),
        };

        // Set up self context
        let temp_self = format!("__init_{}", self.next_value);
        let prev_self_var = self.self_var_name.clone();
        self.self_var_name = Some(temp_self.clone());

        self.push_scope();

        // Register StructMeta for temp_self
        let fields: Vec<String> = struct_info.fields.iter().map(|(n, _)| n.clone()).collect();
        if let Some(scope) = self.struct_meta_scopes.last_mut() {
            scope.insert(
                temp_self.clone(),
                StructMeta {
                    struct_name: struct_name.to_string(),
                    fields,
                },
            );
        }

        // Define init parameters as local variables
        for (i, (param_name, _)) in init_info.params.iter().enumerate() {
            self.define_var(param_name.clone(), arg_values[i]);
        }

        // Execute init body
        let (_, mut init_regions) = self.lower_block_stmts(&body);
        self.pending_regions.append(&mut init_regions);

        // Collect resulting field values from self
        let result: Vec<(String, Value)> = struct_info
            .fields
            .iter()
            .map(|(fname, _)| {
                let key = format!("{}.{}", temp_self, fname);
                let val = self.lookup_var(&key);
                (fname.clone(), val)
            })
            .collect();

        self.pop_scope();
        self.self_var_name = prev_self_var;

        result
    }

    // ========== Stmt ==========

    fn lower_stmt(&mut self, stmt: &Stmt) -> StmtResult {
        match stmt {
            Stmt::Let { name, value, .. } => {
                if let Some(struct_name) = self.struct_type_of_expr(value) {
                    let field_values = self.lower_struct_producing_expr(value, &struct_name);
                    self.define_struct_var(name, &struct_name, field_values);
                } else {
                    let val = self.lower_expr(value);
                    self.define_var(name.clone(), val);
                }
                StmtResult::None
            }
            Stmt::Var { name, value, .. } => {
                if let Some(struct_name) = self.struct_type_of_expr(value) {
                    let field_values = self.lower_struct_producing_expr(value, &struct_name);
                    self.define_struct_var(name, &struct_name, field_values.clone());
                    for (fname, val) in &field_values {
                        let key = format!("{}.{}", name, fname);
                        let ty = self.value_types.get(val).copied().unwrap_or(BirType::I32);
                        if !self.mutable_vars.iter().any(|(n, _)| n == &key) {
                            self.mutable_vars.push((key, ty));
                        }
                    }
                } else {
                    let val = self.lower_expr(value);
                    self.define_var(name.clone(), val);
                    let ty = self.value_types.get(&val).copied().unwrap_or(BirType::I32);
                    if !self.mutable_vars.iter().any(|(n, _)| n == name) {
                        self.mutable_vars.push((name.clone(), ty));
                    }
                }
                StmtResult::None
            }
            Stmt::Assign { name, value } => {
                if let Some(struct_name) = self.struct_type_of_expr(value) {
                    let field_values = self.lower_struct_producing_expr(value, &struct_name);
                    for (fname, val) in &field_values {
                        let key = format!("{}.{}", name, fname);
                        self.assign_var(&key, *val);
                    }
                } else {
                    let val = self.lower_expr(value);
                    self.assign_var(name, val);
                }
                StmtResult::None
            }
            Stmt::Return(Some(expr)) => {
                let val = self.lower_expr(expr);
                StmtResult::Return(val)
            }
            Stmt::Return(None) => StmtResult::ReturnVoid,
            Stmt::Yield(expr) => {
                let val = self.lower_expr(expr);
                StmtResult::Yield(val)
            }
            Stmt::Expr(expr) => {
                let _val = self.lower_expr(expr);
                StmtResult::None
            }
            Stmt::Break(opt_expr) => {
                let loop_ctx = self.loop_stack.last().unwrap();
                let header_bb = loop_ctx.header_bb;
                let exit_bb = loop_ctx.exit_bb;
                let mutable_vars = self.collect_mutable_var_values();
                let args: Vec<(Value, BirType)> =
                    mutable_vars.iter().map(|(_, v, t)| (*v, *t)).collect();
                let value = match opt_expr {
                    Some(expr) => {
                        let val = self.lower_expr(expr);
                        let ty = self.value_types.get(&val).copied().unwrap_or(BirType::I32);
                        self.loop_stack.last_mut().unwrap().break_ty = Some(ty);
                        Some((val, ty))
                    }
                    None => None,
                };
                self.seal_block(Terminator::BrBreak {
                    header_bb,
                    exit_bb,
                    args,
                    value,
                });
                let dummy_bb = self.fresh_block();
                self.start_block(dummy_bb, vec![]);
                StmtResult::Break
            }
            Stmt::Continue => {
                let loop_ctx = self.loop_stack.last().unwrap();
                let header_bb = loop_ctx.header_bb;
                let mutable_vars = self.collect_mutable_var_values();
                let args: Vec<(Value, BirType)> =
                    mutable_vars.iter().map(|(_, v, t)| (*v, *t)).collect();
                self.seal_block(Terminator::BrContinue { header_bb, args });
                let dummy_bb = self.fresh_block();
                self.start_block(dummy_bb, vec![]);
                StmtResult::Continue
            }
            Stmt::FieldAssign {
                object,
                field,
                value,
            } => {
                let Some(base_key) = self.resolve_field_access_key(object) else {
                    self.record_error(format!(
                        "struct value in expression position is not yet supported (FieldAssign on {:?})",
                        object.kind
                    ));
                    return StmtResult::None;
                };
                let stored_key = format!("{}.{}", base_key, field);

                if self.try_lookup_var(&stored_key).is_some() {
                    // 1. Stored property already defined — reassignment
                    let val = self.lower_expr(value);
                    self.assign_var(&stored_key, val);
                } else {
                    // Check if it's a computed property
                    let is_computed = if let Some(meta) = self.lookup_struct_meta(&base_key) {
                        let meta = meta.clone();
                        let sem = self.sem_info.as_ref().unwrap();
                        let struct_info = sem.struct_defs.get(&meta.struct_name).unwrap().clone();
                        struct_info
                            .computed
                            .iter()
                            .find(|p| p.name == *field)
                            .cloned()
                    } else {
                        None
                    };

                    if let Some(prop) = is_computed {
                        // 2. Computed property — inline setter
                        let val = self.lower_expr(value);
                        let setter_block = prop.setter.as_ref().unwrap().clone();

                        let prev_self_var = self.self_var_name.clone();
                        self.self_var_name = Some(base_key.clone());
                        self.push_scope();

                        self.define_var("newValue".to_string(), val);

                        let (_, mut setter_regions) = self.lower_block_stmts(&setter_block);
                        self.pending_regions.append(&mut setter_regions);

                        self.pop_scope();
                        self.self_var_name = prev_self_var;
                    } else {
                        // 3. Stored property not yet defined — first assignment in init body
                        let val = self.lower_expr(value);
                        self.define_var(stored_key, val);
                    }
                }
                StmtResult::None
            }
        }
    }

    // ========== Expr ==========

    fn lower_expr(&mut self, expr: &Expr) -> Value {
        match &expr.kind {
            ExprKind::Number(n) => {
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: *n,
                    ty: BirType::I32,
                });
                self.value_types.insert(result, BirType::I32);
                result
            }
            ExprKind::Float(f) => {
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: f.to_bits() as i64,
                    ty: BirType::F64,
                });
                self.value_types.insert(result, BirType::F64);
                result
            }
            ExprKind::Bool(b) => {
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: if *b { 1 } else { 0 },
                    ty: BirType::Bool,
                });
                self.value_types.insert(result, BirType::Bool);
                result
            }
            ExprKind::Ident(name) => {
                if self.lookup_struct_meta(name).is_some() {
                    return self.record_error(format!(
                        "struct variable `{}` in expression position is not yet supported",
                        name
                    ));
                }
                self.lookup_var(name)
            }
            ExprKind::UnaryOp { op, operand } => {
                let operand_val = self.lower_expr(operand);
                match op {
                    UnaryOp::Not => {
                        let result = self.fresh_value();
                        self.emit(Instruction::Not {
                            result,
                            operand: operand_val,
                        });
                        self.value_types.insert(result, BirType::Bool);
                        result
                    }
                }
            }
            ExprKind::BinaryOp { op, left, right } => match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    let lhs = self.lower_expr(left);
                    let rhs = self.lower_expr(right);
                    let ty = self.value_types.get(&lhs).copied().unwrap_or(BirType::I32);
                    let result = self.fresh_value();
                    self.emit(Instruction::BinaryOp {
                        result,
                        op: convert_binop(*op),
                        lhs,
                        rhs,
                        ty,
                    });
                    self.value_types.insert(result, ty);
                    result
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    let lhs = self.lower_expr(left);
                    let rhs = self.lower_expr(right);
                    let operand_ty = self.value_types.get(&lhs).copied().unwrap_or(BirType::I32);
                    let result = self.fresh_value();
                    self.emit(Instruction::Compare {
                        result,
                        op: convert_compare_op(*op),
                        lhs,
                        rhs,
                        ty: operand_ty,
                    });
                    self.value_types.insert(result, BirType::Bool);
                    result
                }
                BinOp::And => {
                    let result = self.lower_short_circuit_and(left, right);
                    self.value_types.insert(result, BirType::Bool);
                    result
                }
                BinOp::Or => {
                    let result = self.lower_short_circuit_or(left, right);
                    self.value_types.insert(result, BirType::Bool);
                    result
                }
            },
            ExprKind::Call { name, args } => {
                let sem = self.sem_info.as_ref().unwrap();
                if sem.struct_init_calls.contains(&expr.id) {
                    return self.record_error(
                        "struct value in expression position is not yet supported (Call as StructInit)",
                    );
                }
                let arg_vals: Vec<Value> = args.iter().map(|a| self.lower_expr(a)).collect();
                let ty = self.func_sigs.get(name).copied().unwrap_or(BirType::I32);
                let result = self.fresh_value();
                self.emit(Instruction::Call {
                    result,
                    func_name: name.clone(),
                    args: arg_vals,
                    ty,
                });
                self.value_types.insert(result, ty);
                result
            }
            ExprKind::Block(block) => {
                self.push_scope();
                let (result, mut inner_regions) = self.lower_block_stmts(block);
                self.pop_scope();
                self.pending_regions.append(&mut inner_regions);
                match result {
                    Some(StmtResult::Yield(v)) => v,
                    _ => unreachable!(
                        "block expression must yield (semantic analysis guarantees this)"
                    ),
                }
            }
            ExprKind::If {
                condition,
                then_block,
                else_block,
            } => self.lower_if(condition, then_block, else_block.as_ref()),
            ExprKind::While {
                condition,
                body,
                nobreak,
            } => self.lower_while(condition, body, nobreak.as_ref()),
            ExprKind::Cast { expr, target_type } => {
                let operand = self.lower_expr(expr);
                let from_ty = self
                    .value_types
                    .get(&operand)
                    .copied()
                    .unwrap_or(BirType::I32);
                let to_ty = convert_type(target_type);
                let result = self.fresh_value();
                self.emit(Instruction::Cast {
                    result,
                    operand,
                    from_ty,
                    to_ty,
                });
                self.value_types.insert(result, to_ty);
                result
            }
            ExprKind::StructInit { .. } => self.record_error(
                "struct value in expression position is not yet supported (StructInit)",
            ),
            ExprKind::FieldAccess { object, field } => {
                let Some(base_key) = self.resolve_field_access_key(object) else {
                    return self.record_error(format!(
                        "struct value in expression position is not yet supported (FieldAccess on {:?})",
                        object.kind
                    ));
                };
                let key = format!("{}.{}", base_key, field);

                // 1. Stored field — already defined in scopes
                if self.try_lookup_var(&key).is_some() {
                    return self.lookup_var(&key);
                }

                // 2. Computed property — inline getter
                if let Some(meta) = self.lookup_struct_meta(&base_key) {
                    let meta = meta.clone();
                    let sem = self.sem_info.as_ref().unwrap();
                    let struct_info = sem.struct_defs.get(&meta.struct_name).unwrap().clone();

                    if let Some(prop) = struct_info.computed.iter().find(|p| p.name == *field) {
                        let getter_block = prop.getter.clone();

                        let prev_self_var = self.self_var_name.clone();
                        self.self_var_name = Some(base_key.clone());
                        self.push_scope();

                        let (result, mut getter_regions) = self.lower_block_stmts(&getter_block);
                        self.pending_regions.append(&mut getter_regions);
                        let getter_val = match result {
                            Some(StmtResult::Return(v)) => v,
                            _ => unreachable!("getter must return a value"),
                        };

                        self.pop_scope();
                        self.self_var_name = prev_self_var;

                        return getter_val;
                    }
                }

                // 3. Read-before-init in initializer
                if self.self_var_name.is_some() {
                    return self.record_error(format!(
                        "read-before-init: field `{}` read before initialization in initializer",
                        field
                    ));
                }

                self.lookup_var(&key)
            }
            ExprKind::SelfRef => self
                .record_error("struct value in expression position is not yet supported (SelfRef)"),
        }
    }

    // ========== Short-circuit &&  ==========

    fn lower_short_circuit_and(&mut self, left: &Expr, right: &Expr) -> Value {
        let lhs = self.lower_expr(left);
        let cond_regions = self.take_pending_regions();

        let cond_bb = self.current_block_label;
        let bb_rhs = self.fresh_block();
        let bb_false = self.fresh_block();
        let bb_merge = self.fresh_block();

        self.seal_block(Terminator::CondBr {
            cond: lhs,
            then_bb: bb_rhs,
            else_bb: bb_false,
        });

        // bb_rhs: evaluate right operand
        self.start_block(bb_rhs, vec![]);
        let rhs = self.lower_expr(right);
        let rhs_regions = self.take_pending_regions();
        self.seal_block(Terminator::Br {
            target: bb_merge,
            args: vec![(rhs, BirType::Bool)],
        });

        // bb_false: literal false
        self.start_block(bb_false, vec![]);
        let false_val = self.fresh_value();
        self.emit(Instruction::Literal {
            result: false_val,
            value: 0,
            ty: BirType::Bool,
        });
        self.seal_block(Terminator::Br {
            target: bb_merge,
            args: vec![(false_val, BirType::Bool)],
        });

        // bb_merge: result
        let result = self.fresh_value();
        self.start_block(bb_merge, vec![(result, BirType::Bool)]);

        // Build then_region with any nested regions from rhs
        let mut then_region = rhs_regions;
        then_region.push(CfgRegion::Block(bb_rhs));

        self.pending_regions.push(CfgRegion::IfElse {
            cond_region: cond_regions,
            cond_bb,
            cond_value: lhs,
            then_region,
            else_region: vec![CfgRegion::Block(bb_false)],
            merge_bb: bb_merge,
        });

        result
    }

    // ========== Short-circuit ||  ==========

    fn lower_short_circuit_or(&mut self, left: &Expr, right: &Expr) -> Value {
        let lhs = self.lower_expr(left);
        let cond_regions = self.take_pending_regions();

        let cond_bb = self.current_block_label;
        let bb_true = self.fresh_block();
        let bb_rhs = self.fresh_block();
        let bb_merge = self.fresh_block();

        self.seal_block(Terminator::CondBr {
            cond: lhs,
            then_bb: bb_true,
            else_bb: bb_rhs,
        });

        // bb_true: literal true
        self.start_block(bb_true, vec![]);
        let true_val = self.fresh_value();
        self.emit(Instruction::Literal {
            result: true_val,
            value: 1,
            ty: BirType::Bool,
        });
        self.seal_block(Terminator::Br {
            target: bb_merge,
            args: vec![(true_val, BirType::Bool)],
        });

        // bb_rhs: evaluate right operand
        self.start_block(bb_rhs, vec![]);
        let rhs = self.lower_expr(right);
        let rhs_regions = self.take_pending_regions();
        self.seal_block(Terminator::Br {
            target: bb_merge,
            args: vec![(rhs, BirType::Bool)],
        });

        // bb_merge: result
        let result = self.fresh_value();
        self.start_block(bb_merge, vec![(result, BirType::Bool)]);

        let mut else_region = rhs_regions;
        else_region.push(CfgRegion::Block(bb_rhs));

        self.pending_regions.push(CfgRegion::IfElse {
            cond_region: cond_regions,
            cond_bb,
            cond_value: lhs,
            then_region: vec![CfgRegion::Block(bb_true)],
            else_region,
            merge_bb: bb_merge,
        });

        result
    }

    // ========== If/Else ==========

    fn lower_if(
        &mut self,
        condition: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Value {
        let cond_val = self.lower_expr(condition);
        let cond_regions = self.take_pending_regions();
        let cond_bb = self.current_block_label;

        let bb_then = self.fresh_block();
        let bb_merge = self.fresh_block();

        match else_block {
            Some(else_blk) => {
                let bb_else = self.fresh_block();

                self.seal_block(Terminator::CondBr {
                    cond: cond_val,
                    then_bb: bb_then,
                    else_bb: bb_else,
                });

                // then block
                self.start_block(bb_then, vec![]);
                self.push_scope();
                let (then_result, then_inner_regions) = self.lower_block_stmts(then_block);
                self.pop_scope();
                let mut then_region = then_inner_regions;
                // Don't seal yet — need to know merge type first

                // else block
                let then_block_label = self.current_block_label;
                let then_instructions = std::mem::take(&mut self.current_instructions);
                let then_params = std::mem::take(&mut self.current_block_params);

                self.start_block(bb_else, vec![]);
                self.push_scope();
                let (else_result, else_inner_regions) = self.lower_block_stmts(else_blk);
                self.pop_scope();
                let mut else_region = else_inner_regions;

                let else_block_label = self.current_block_label;
                let else_instructions = std::mem::take(&mut self.current_instructions);
                let else_params = std::mem::take(&mut self.current_block_params);

                // Determine if merge block needs a value parameter
                let then_yields = matches!(&then_result, Some(StmtResult::Yield(_)));
                let else_yields = matches!(&else_result, Some(StmtResult::Yield(_)));
                let has_merge_value = then_yields || else_yields;
                // Infer merge type from yield values
                let merge_type = match (&then_result, &else_result) {
                    (Some(StmtResult::Yield(v)), _) => {
                        self.value_types.get(v).copied().unwrap_or(BirType::I32)
                    }
                    (_, Some(StmtResult::Yield(v))) => {
                        self.value_types.get(v).copied().unwrap_or(BirType::I32)
                    }
                    _ => BirType::I32,
                };

                // Seal then block
                self.current_block_label = then_block_label;
                self.current_instructions = then_instructions;
                self.current_block_params = then_params;
                match &then_result {
                    Some(StmtResult::Yield(v)) => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![(*v, merge_type)],
                        });
                    }
                    Some(StmtResult::Return(v)) => {
                        self.seal_block(Terminator::Return(*v));
                    }
                    Some(StmtResult::ReturnVoid) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    Some(StmtResult::Break) | Some(StmtResult::Continue) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    _ => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![],
                        });
                    }
                }
                then_region.push(CfgRegion::Block(bb_then));

                // Seal else block
                self.current_block_label = else_block_label;
                self.current_instructions = else_instructions;
                self.current_block_params = else_params;
                match &else_result {
                    Some(StmtResult::Yield(v)) => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![(*v, merge_type)],
                        });
                    }
                    Some(StmtResult::Return(v)) => {
                        self.seal_block(Terminator::Return(*v));
                    }
                    Some(StmtResult::ReturnVoid) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    Some(StmtResult::Break) | Some(StmtResult::Continue) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    _ => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![],
                        });
                    }
                }
                else_region.push(CfgRegion::Block(bb_else));

                // merge block
                let merge_params = if has_merge_value {
                    let result = self.fresh_value();
                    self.start_block(bb_merge, vec![(result, merge_type)]);
                    result
                } else {
                    let result = self.fresh_value(); // dummy for Unit
                    self.start_block(bb_merge, vec![]);
                    result
                };

                self.pending_regions.push(CfgRegion::IfElse {
                    cond_region: cond_regions,
                    cond_bb,
                    cond_value: cond_val,
                    then_region,
                    else_region,
                    merge_bb: bb_merge,
                });

                merge_params
            }
            None => {
                // if without else
                self.seal_block(Terminator::CondBr {
                    cond: cond_val,
                    then_bb: bb_then,
                    else_bb: bb_merge,
                });

                self.start_block(bb_then, vec![]);
                self.push_scope();
                let (then_result, then_inner_regions) = self.lower_block_stmts(then_block);
                self.pop_scope();

                let mut then_region = then_inner_regions;

                match then_result {
                    Some(StmtResult::Return(v)) => {
                        self.seal_block(Terminator::Return(v));
                    }
                    Some(StmtResult::ReturnVoid) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    Some(StmtResult::Break) | Some(StmtResult::Continue) => {
                        self.seal_block(Terminator::ReturnVoid);
                    }
                    _ => {
                        self.seal_block(Terminator::Br {
                            target: bb_merge,
                            args: vec![],
                        });
                    }
                }
                then_region.push(CfgRegion::Block(bb_then));

                self.start_block(bb_merge, vec![]);

                self.pending_regions.push(CfgRegion::IfOnly {
                    cond_region: cond_regions,
                    cond_bb,
                    cond_value: cond_val,
                    then_region,
                    merge_bb: bb_merge,
                });

                // If without else returns Unit — return dummy value
                let result = self.fresh_value();
                self.emit(Instruction::Literal {
                    result,
                    value: 0,
                    ty: BirType::Unit,
                });
                result
            }
        }
    }

    // ========== While ==========

    fn lower_while(&mut self, condition: &Expr, body: &Block, nobreak: Option<&Block>) -> Value {
        let entry_bb = self.current_block_label;

        // Collect mutable var values for block args
        let mutable_vars = self.collect_mutable_var_values();

        let bb_header = self.fresh_block();
        let bb_body = self.fresh_block();
        let bb_exit = self.fresh_block();

        // Seal entry block with Br to header
        let entry_args: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(_, val, ty)| (*val, *ty))
            .collect();
        self.seal_block(Terminator::Br {
            target: bb_header,
            args: entry_args,
        });

        // Header block: receives mutable vars as params
        let header_params: Vec<(Value, BirType)> = mutable_vars
            .iter()
            .map(|(_, _, ty)| {
                let v = self.fresh_value();
                (v, *ty)
            })
            .collect();

        self.start_block(bb_header, header_params.clone());

        // Remap mutable vars to header params
        for (i, (name, _, _)) in mutable_vars.iter().enumerate() {
            let (param_val, _) = header_params[i];
            self.assign_var(name, param_val);
        }

        // Push loop context
        self.loop_stack.push(LoopContext {
            header_bb: bb_header,
            exit_bb: bb_exit,
            break_ty: None,
        });

        // Evaluate condition
        let cond_val = self.lower_expr(condition);
        let header_regions = self.take_pending_regions();

        // Determine nobreak target
        let bb_nobreak = if nobreak.is_some() {
            self.fresh_block()
        } else {
            bb_exit
        };

        self.seal_block(Terminator::CondBr {
            cond: cond_val,
            then_bb: bb_body,
            else_bb: bb_nobreak,
        });

        // Body block
        self.start_block(bb_body, vec![]);
        self.push_scope();
        let (body_result, body_inner_regions) = self.lower_block_stmts(body);
        self.pop_scope();

        let has_inner_regions = !body_inner_regions.is_empty();
        let mut body_region = body_inner_regions;

        // Track the last block in the body (may differ from bb_body if if/while regions exist)
        let last_body_bb = self.current_block_label;

        // Only emit back-edge if body didn't diverge (break/continue/return)
        match body_result {
            Some(StmtResult::Break) | Some(StmtResult::Continue) => {
                // Already sealed by lower_stmt with BrBreak/BrContinue
                // Seal the dummy block that lower_stmt started
                self.seal_block(Terminator::ReturnVoid);
            }
            Some(StmtResult::Return(v)) => {
                self.seal_block(Terminator::Return(v));
            }
            Some(StmtResult::ReturnVoid) => {
                self.seal_block(Terminator::ReturnVoid);
            }
            _ => {
                // Normal fall-through: emit back-edge to header
                let updated_args: Vec<(Value, BirType)> = mutable_vars
                    .iter()
                    .map(|(name, _, ty)| (self.lookup_var(name), *ty))
                    .collect();
                self.seal_block(Terminator::Br {
                    target: bb_header,
                    args: updated_args,
                });
            }
        }

        // Add body blocks to body_region:
        // - If body_inner_regions is empty, bb_body IS last_body_bb (no if/while inside)
        // - If body_inner_regions is non-empty, bb_body is already used as cond_bb
        //   inside the first region (e.g., IfOnly), so we add last_body_bb instead
        let body_diverged = matches!(
            body_result,
            Some(StmtResult::Break)
                | Some(StmtResult::Continue)
                | Some(StmtResult::Return(_))
                | Some(StmtResult::ReturnVoid)
        );
        if has_inner_regions {
            // bb_body is used as cond_bb inside the first inner region (IfOnly etc.)
            // Add last_body_bb only if body didn't diverge (otherwise it's an unreachable dummy)
            if !body_diverged {
                body_region.push(CfgRegion::Block(last_body_bb));
            }
        } else {
            // No inner regions — bb_body contains all body instructions + terminator
            body_region.push(CfgRegion::Block(bb_body));
        }

        // Pop loop context and get break type
        let loop_ctx = self.loop_stack.pop().unwrap();
        let break_ty = loop_ctx.break_ty;

        // Nobreak block
        let nobreak_region = if let Some(nobreak_blk) = nobreak {
            self.start_block(bb_nobreak, vec![]);
            self.push_scope();
            let (nobreak_result, nobreak_inner_regions) = self.lower_block_stmts(nobreak_blk);
            self.pop_scope();

            let mut nb_region = nobreak_inner_regions;

            // Seal nobreak block → Br to exit_bb with yield value
            match nobreak_result {
                Some(StmtResult::Yield(v)) => {
                    if break_ty.is_some() {
                        let ty = self.value_types.get(&v).copied().unwrap_or(BirType::I32);
                        self.seal_block(Terminator::Br {
                            target: bb_exit,
                            args: vec![(v, ty)],
                        });
                    } else {
                        self.seal_block(Terminator::Br {
                            target: bb_exit,
                            args: vec![],
                        });
                    }
                }
                Some(StmtResult::Return(v)) => {
                    self.seal_block(Terminator::Return(v));
                }
                Some(StmtResult::ReturnVoid) => {
                    self.seal_block(Terminator::ReturnVoid);
                }
                _ => {
                    self.seal_block(Terminator::Br {
                        target: bb_exit,
                        args: vec![],
                    });
                }
            }

            nb_region.push(CfgRegion::Block(bb_nobreak));
            nb_region
        } else {
            vec![]
        };

        // Exit block
        let while_result = if let Some(bty) = break_ty {
            let result_val = self.fresh_value();
            self.start_block(bb_exit, vec![(result_val, bty)]);
            self.value_types.insert(result_val, bty);
            result_val
        } else {
            self.start_block(bb_exit, vec![]);
            let result = self.fresh_value();
            self.emit(Instruction::Literal {
                result,
                value: 0,
                ty: BirType::Unit,
            });
            self.value_types.insert(result, BirType::Unit);
            result
        };

        self.pending_regions.push(CfgRegion::While {
            entry_bb,
            header_region: header_regions,
            header_bb: bb_header,
            cond_value: cond_val,
            body_region,
            nobreak_region,
            exit_bb: bb_exit,
        });

        while_result
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
    }
}

pub fn lower_program(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
) -> Result<BirModule> {
    // Pre-collect function signatures
    let mut func_sigs = HashMap::new();
    for func in &program.functions {
        // Reject functions with struct params or return types
        if matches!(func.return_type, TypeAnnotation::Named(_)) {
            return Err(BengalError::LoweringError {
                message: format!(
                    "function `{}` returns a struct type, which is not yet supported in lowering",
                    func.name
                ),
            });
        }
        for param in &func.params {
            if matches!(param.ty, TypeAnnotation::Named(_)) {
                return Err(BengalError::LoweringError {
                    message: format!(
                        "function `{}` has struct parameter `{}`, which is not yet supported in lowering",
                        func.name, param.name
                    ),
                });
            }
        }
        func_sigs.insert(func.name.clone(), convert_type(&func.return_type));
    }

    let sem_info_ref = SemInfoRef {
        struct_defs: sem_info.struct_defs.clone(),
        struct_init_calls: sem_info.struct_init_calls.clone(),
    };
    let mut lowering = Lowering::new(func_sigs, sem_info_ref);
    let functions = program
        .functions
        .iter()
        .map(|f| lowering.lower_function(f))
        .collect();

    if let Some(err) = lowering.lowering_error {
        return Err(err);
    }

    Ok(BirModule { functions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bir::printer::print_module;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::semantic;

    fn lower_str(input: &str) -> String {
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let module = lower_program(&program, &sem_info).unwrap();
        print_module(&module)
    }

    // --- Phase 2 tests (maintained) ---

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

    // --- Phase 3 tests ---

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

    #[test]
    fn lower_short_circuit_and() {
        let output = lower_str(
            "func main() -> Int32 { let b: Bool = true && false; if b { yield 1; } else { yield 0; }; return 0; }",
        );
        // Should have cond_br for && short-circuit
        assert!(output.contains("cond_br"));
        assert!(output.contains("literal 0 : Bool"));
    }

    // --- Phase 5: Struct lowering tests ---

    #[test]
    fn lower_struct_field_expansion() {
        let output = lower_str(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1, y: 2); return p.x; }",
        );
        assert!(output.contains("literal 1 : Int32"));
        assert!(output.contains("literal 2 : Int32"));
        assert!(output.contains("return"));
    }

    #[test]
    fn lower_struct_field_write() {
        let output = lower_str(
            "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); p.x = 10; return p.x; }",
        );
        assert!(output.contains("literal 10 : Int32"));
    }

    #[test]
    fn lower_struct_value_copy() {
        let output = lower_str(
            "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); var q = p; q.x = 99; return p.x; }",
        );
        assert!(output.contains("literal 1 : Int32"));
        assert!(output.contains("literal 99 : Int32"));
    }

    #[test]
    fn lower_struct_explicit_init() {
        let output = lower_str(
            "struct Foo { var x: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { var f = Foo(val: 42); return f.x; }",
        );
        assert!(output.contains("literal 42 : Int32"));
        assert!(output.contains("return"));
    }

    #[test]
    fn lower_struct_computed_getter() {
        let output = lower_str(
            "struct Foo { var x: Int32; var double: Int32 { get { return self.x; } }; } func main() -> Int32 { var f = Foo(x: 5); return f.double; }",
        );
        assert!(output.contains("literal 5 : Int32"));
    }

    #[test]
    fn lower_struct_computed_setter() {
        let output = lower_str(
            "struct Foo { var x: Int32; var bar: Int32 { get { return 0; } set { self.x = newValue; } }; } func main() -> Int32 { var f = Foo(x: 1); f.bar = 10; return f.x; }",
        );
        assert!(output.contains("literal 10 : Int32"));
    }

    #[test]
    fn lower_err_struct_return_type() {
        let tokens = tokenize(
            "struct Point { var x: Int32; } func make() -> Point { return Point(x: 1); } func main() -> Int32 { return 0; }",
        ).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
    }

    #[test]
    fn lower_err_struct_param() {
        let tokens = tokenize(
            "struct Point { var x: Int32; } func use_point(p: Point) -> Int32 { return p.x; } func main() -> Int32 { return 0; }",
        ).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
    }

    #[test]
    fn lower_err_struct_var_in_expr_position() {
        let tokens = tokenize(
            "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); p; return 0; }",
        ).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
    }

    #[test]
    fn lower_err_struct_init_field_access() {
        let tokens = tokenize(
            "struct Point { var x: Int32; } func main() -> Int32 { return Point(x: 1).x; }",
        )
        .unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
    }

    #[test]
    fn lower_err_read_before_init() {
        let tokens = tokenize(
            "struct Foo { var x: Int32; init(val: Int32) { let y: Int32 = self.x; self.x = val; } } func main() -> Int32 { var f = Foo(val: 1); return f.x; }",
        ).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
    }
}
