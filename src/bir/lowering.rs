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
    // CfgRegion tracking — regions generated during expression lowering
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
    lowering_error: Option<BengalError>,
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
            lowering_error: None,
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
        if self.lowering_error.is_none() {
            self.lowering_error = Some(BengalError::LoweringError {
                message: message.into(),
                span,
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

    // ========== Struct helpers ==========

    fn emit_struct_init(
        &mut self,
        struct_name: &str,
        field_values: &[(String, Value)],
        type_args: &[BirType],
    ) -> Value {
        let bir_ty = BirType::Struct {
            name: struct_name.to_string(),
            type_args: type_args.to_vec(),
        };
        let result = self.fresh_value();
        self.emit(Instruction::StructInit {
            result,
            struct_name: struct_name.to_string(),
            fields: field_values.to_vec(),
            type_args: type_args.to_vec(),
            ty: bir_ty.clone(),
        });
        self.value_types.insert(result, bir_ty);
        result
    }

    fn lower_receiver(&mut self, object: &Expr) -> Option<ReceiverInfo> {
        match &object.kind {
            ExprKind::Ident(name) => {
                let val = self.lookup_var(name);
                let struct_name = match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => n.clone(),
                    _ => return None,
                };
                Some(ReceiverInfo {
                    value: val,
                    struct_name,
                    var_name: name.clone(),
                })
            }
            ExprKind::SelfRef => {
                let self_name = self.self_var_name.as_ref()?.clone();
                let val = self.lookup_var(&self_name);
                let struct_name = match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => n.clone(),
                    _ => return None,
                };
                Some(ReceiverInfo {
                    value: val,
                    struct_name,
                    var_name: self_name,
                })
            }
            _ => {
                let val = self.lower_expr(object);
                let struct_name = match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => n.clone(),
                    _ => return None,
                };
                let tmp_name = format!("__tmp_{}", self.next_value);
                self.define_var(tmp_name.clone(), val);
                Some(ReceiverInfo {
                    value: val,
                    struct_name,
                    var_name: tmp_name,
                })
            }
        }
    }

    fn infer_struct_name_no_lower(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Ident(name) => {
                let val = self.try_lookup_var(name)?;
                match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => Some(n.clone()),
                    _ => None,
                }
            }
            ExprKind::SelfRef => {
                let self_name = self.self_var_name.as_ref()?;
                let val = self.try_lookup_var(self_name)?;
                match self.value_types.get(&val)? {
                    BirType::Struct { name: n, .. } => Some(n.clone()),
                    _ => None,
                }
            }
            ExprKind::StructInit { name, .. } => Some(name.clone()),
            ExprKind::Call { name, .. } => {
                let sem = self.sem_info.as_ref().unwrap();
                if sem.struct_init_calls.contains(&expr.id) {
                    Some(name.clone())
                } else {
                    match self.func_sigs.get(name)? {
                        BirType::Struct { name: sn, .. } => Some(sn.clone()),
                        _ => None,
                    }
                }
            }
            ExprKind::FieldAccess { object, field } => {
                let parent_struct = self.infer_struct_name_no_lower(object)?;
                let sem = self.sem_info.as_ref().unwrap();
                let info = sem.struct_defs.get(&parent_struct)?;
                // Check stored fields
                if let Some((_, ty)) = info.fields.iter().find(|(n, _)| n == field) {
                    return match ty {
                        crate::semantic::types::Type::Struct(name) => Some(name.clone()),
                        _ => None,
                    };
                }
                // Check computed properties
                if let Some(prop) = info.computed.iter().find(|p| p.name == *field) {
                    return match &prop.ty {
                        crate::semantic::types::Type::Struct(name) => Some(name.clone()),
                        _ => None,
                    };
                }
                None
            }
            _ => None,
        }
    }

    fn inline_getter(
        &mut self,
        self_var_name: &str,
        getter_block: &Block,
        return_ty: BirType,
    ) -> Value {
        let prev_self_var = self.self_var_name.clone();
        let prev_in_init = self.in_init_body;
        let prev_getter_return_bb = self.getter_return_bb.take();

        // Allocate a continuation block that accepts the getter's return value as a param.
        let cont_bb = self.fresh_block();
        let cont_val = self.fresh_value();
        self.value_types.insert(cont_val, return_ty.clone());
        self.getter_return_bb = Some((cont_bb, cont_val, return_ty.clone()));

        self.self_var_name = Some(self_var_name.to_string());
        self.in_init_body = false;
        self.push_scope();
        let (result, mut getter_regions) = self.lower_block_stmts(getter_block);
        self.pending_regions.append(&mut getter_regions);
        self.pop_scope();
        self.self_var_name = prev_self_var;
        self.in_init_body = prev_in_init;
        self.getter_return_bb = prev_getter_return_bb;

        match result {
            Some(StmtResult::Return(v)) => {
                // Simple trailing-return getter: seal the current block by branching to cont_bb,
                // then start cont_bb so the caller continues from there.
                // NOTE: with getter_return_bb set, Stmt::Return redirects to cont_bb and returns
                // ReturnVoid, so this arm is normally unreachable — but kept as a safety fallback.
                self.seal_block(Terminator::Br {
                    target: cont_bb,
                    args: vec![(v, return_ty.clone())],
                });
                self.start_block(cont_bb, vec![(cont_val, return_ty)]);
                cont_val
            }
            Some(StmtResult::ReturnVoid) | None => {
                // Exhaustive control-flow returns: all paths branched to cont_bb already.
                // The current block is a dead/unreachable block — abandon it and switch to cont_bb.
                self.start_block(cont_bb, vec![(cont_val, return_ty)]);
                cont_val
            }
            _ => unreachable!("getter body produced unexpected StmtResult"),
        }
    }

    fn try_lower_computed_setter(&mut self, object: &Expr, field: &str, value: &Expr) -> bool {
        let struct_name = match self.infer_struct_name_no_lower(object) {
            Some(n) => n,
            None => return false,
        };
        let sem = self.sem_info.as_ref().unwrap();
        let struct_info = match sem.struct_defs.get(&struct_name) {
            Some(i) => i.clone(),
            None => return false,
        };
        let prop = match struct_info.computed.iter().find(|p| p.name == field) {
            Some(p) => p.clone(),
            None => return false,
        };
        if !prop.has_setter {
            return false;
        }

        // Only support Ident/SelfRef receivers for setters
        let var_name = match &object.kind {
            ExprKind::Ident(name) => name.clone(),
            ExprKind::SelfRef => match &self.self_var_name {
                Some(n) => n.clone(),
                None => return false,
            },
            _ => return false,
        };

        let val = self.lower_expr(value);
        let setter_block = prop.setter.unwrap();

        // Save and switch context — setter body runs as normal struct method,
        // not as initializer.
        let prev_self_var = self.self_var_name.clone();
        let prev_in_init = self.in_init_body;
        let prev_init_struct = self.init_struct_name.clone();
        self.self_var_name = Some(var_name);
        self.in_init_body = false;
        self.init_struct_name = None;

        self.push_scope();
        self.define_var("newValue".to_string(), val);
        let (_, mut setter_regions) = self.lower_block_stmts(&setter_block);
        self.pending_regions.append(&mut setter_regions);
        self.pop_scope();

        self.self_var_name = prev_self_var;
        self.in_init_body = prev_in_init;
        self.init_struct_name = prev_init_struct;
        true
    }

    fn expr_refers_to_self(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::SelfRef => true,
            ExprKind::FieldAccess { object, .. } => self.expr_refers_to_self(object),
            _ => false,
        }
    }

    fn lower_field_assign_recursive(&mut self, object: &Expr, field: &str, new_val: Value) {
        // Guard: check that field is a stored field, not computed
        if let Some(struct_name) = self.infer_struct_name_no_lower(object) {
            let sem = self.sem_info.as_ref().unwrap();
            if let Some(info) = sem.struct_defs.get(&struct_name)
                && info.computed.iter().any(|p| p.name == field)
            {
                self.record_error(
                    format!(
                        "computed property setter `{}` on non-direct receiver is not yet supported",
                        field
                    ),
                    Some(object.span),
                );
                return;
            }
        }

        match &object.kind {
            ExprKind::Ident(var_name) => {
                let obj_val = self.lookup_var(var_name);
                let obj_ty = self.value_types.get(&obj_val).cloned().unwrap();
                let result = self.fresh_value();
                self.emit(Instruction::FieldSet {
                    result,
                    object: obj_val,
                    field: field.to_string(),
                    value: new_val,
                    ty: obj_ty.clone(),
                });
                self.value_types.insert(result, obj_ty);
                self.assign_var(var_name, result);
            }
            ExprKind::SelfRef => {
                let self_name = self.self_var_name.as_ref().unwrap().clone();
                let obj_val = self.lookup_var(&self_name);
                let obj_ty = self.value_types.get(&obj_val).cloned().unwrap();
                let result = self.fresh_value();
                self.emit(Instruction::FieldSet {
                    result,
                    object: obj_val,
                    field: field.to_string(),
                    value: new_val,
                    ty: obj_ty.clone(),
                });
                self.value_types.insert(result, obj_ty);
                self.assign_var(&self_name, result);
            }
            ExprKind::FieldAccess {
                object: parent,
                field: parent_field,
            } => {
                // Guard: parent_field must be stored, not computed
                if let Some(parent_struct) = self.infer_struct_name_no_lower(parent) {
                    let sem = self.sem_info.as_ref().unwrap();
                    if let Some(info) = sem.struct_defs.get(&parent_struct)
                        && info.computed.iter().any(|p| p.name == *parent_field)
                    {
                        self.record_error(
                            format!(
                                "assignment through computed property `{}` is not yet supported",
                                parent_field
                            ),
                            Some(parent.span),
                        );
                        return;
                    }
                }
                // 1. Get the intermediate struct
                let parent_val = self.lower_expr(parent);
                let parent_ty = self.value_types.get(&parent_val).cloned().unwrap();
                let parent_struct_name = match &parent_ty {
                    BirType::Struct { name, .. } => name.clone(),
                    _ => unreachable!(),
                };
                // 2. Get the inner struct field
                let sem = self.sem_info.as_ref().unwrap();
                let inner_sem_ty = &sem
                    .struct_defs
                    .get(&parent_struct_name)
                    .unwrap()
                    .fields
                    .iter()
                    .find(|(n, _)| n == parent_field)
                    .unwrap()
                    .1;
                let inner_ty = semantic_type_to_bir(inner_sem_ty);
                let inner_val = self.fresh_value();
                self.emit(Instruction::FieldGet {
                    result: inner_val,
                    object: parent_val,
                    field: parent_field.clone(),
                    object_ty: parent_ty.clone(),
                    ty: inner_ty.clone(),
                });
                self.value_types.insert(inner_val, inner_ty.clone());
                // 3. Update the inner struct's field
                let updated_inner = self.fresh_value();
                self.emit(Instruction::FieldSet {
                    result: updated_inner,
                    object: inner_val,
                    field: field.to_string(),
                    value: new_val,
                    ty: inner_ty.clone(),
                });
                self.value_types.insert(updated_inner, inner_ty);
                // 4. Recurse: write the updated inner back into parent
                self.lower_field_assign_recursive(parent, parent_field, updated_inner);
            }
            _ => unreachable!("FieldAssign on unsupported object expression"),
        }
    }

    /// Collect current values of all mutable variables for while loop block args
    fn collect_mutable_var_values(&self) -> Vec<(String, Value, BirType)> {
        self.mutable_vars
            .iter()
            .map(|(name, ty)| (name.clone(), self.lookup_var(name), ty.clone()))
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

        // Set up self context — per-field variable tracking (hybrid model)
        let temp_self = format!("__init_{}", self.next_value);
        let prev_self_var = self.self_var_name.clone();
        let prev_in_init = self.in_init_body;
        let prev_init_struct = self.init_struct_name.clone();
        self.self_var_name = Some(temp_self.clone());
        self.in_init_body = true;
        self.init_struct_name = Some(struct_name.to_string());

        self.push_scope();

        // Define init parameters as local variables
        for (i, (param_name, _)) in init_info.params.iter().enumerate() {
            self.define_var(param_name.clone(), arg_values[i]);
        }

        // Execute init body — self.field = val goes through FieldAssign init-body path
        let (_, mut init_regions) = self.lower_block_stmts(&body);
        self.pending_regions.append(&mut init_regions);

        // Collect resulting field values (all fields should be initialized)
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
        self.in_init_body = prev_in_init;
        self.init_struct_name = prev_init_struct;

        result
    }

    // ========== Stmt ==========

    fn lower_stmt(&mut self, stmt: &Stmt) -> StmtResult {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let val = self.lower_expr(value);
                self.define_var(name.clone(), val);
                StmtResult::None
            }
            Stmt::Var { name, value, .. } => {
                let val = self.lower_expr(value);
                self.define_var(name.clone(), val);
                let ty = self.value_types.get(&val).cloned().unwrap_or(BirType::I32);
                if !self.mutable_vars.iter().any(|(n, _)| n == name) {
                    self.mutable_vars.push((name.clone(), ty));
                }
                StmtResult::None
            }
            Stmt::Assign { name, value } => {
                let val = self.lower_expr(value);
                self.assign_var(name, val);
                StmtResult::None
            }
            Stmt::Return(Some(expr)) => {
                let val = self.lower_expr(expr);
                if let Some((cont_bb, _param, ty)) = self.getter_return_bb.clone() {
                    self.seal_block(Terminator::Br {
                        target: cont_bb,
                        args: vec![(val, ty)],
                    });
                    let dead_bb = self.fresh_block();
                    self.start_block(dead_bb, vec![]);
                    StmtResult::ReturnVoid
                } else {
                    StmtResult::Return(val)
                }
            }
            Stmt::Return(None) => StmtResult::ReturnVoid,
            Stmt::Yield(expr) => {
                let val = self.lower_expr(expr);
                StmtResult::Yield(val)
            }
            Stmt::Expr(expr) => {
                self.last_expr_diverged = false;
                let _val = self.lower_expr(expr);
                if self.last_expr_diverged {
                    StmtResult::ReturnVoid
                } else {
                    StmtResult::None
                }
            }
            Stmt::Break(opt_expr) => {
                let loop_ctx = self.loop_stack.last().unwrap();
                let header_bb = loop_ctx.header_bb;
                let exit_bb = loop_ctx.exit_bb;
                let mutable_vars = self.collect_mutable_var_values();
                let args: Vec<(Value, BirType)> = mutable_vars
                    .iter()
                    .map(|(_, v, t)| (*v, t.clone()))
                    .collect();
                let value = match opt_expr {
                    Some(expr) => {
                        let val = self.lower_expr(expr);
                        let ty = self.value_types.get(&val).cloned().unwrap_or(BirType::I32);
                        self.loop_stack.last_mut().unwrap().break_ty = Some(ty.clone());
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
                let args: Vec<(Value, BirType)> = mutable_vars
                    .iter()
                    .map(|(_, v, t)| (*v, t.clone()))
                    .collect();
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
                // 1. Init body + SelfRef: per-field variable path
                if self.in_init_body {
                    if let ExprKind::SelfRef = &object.kind {
                        // Reject computed property setter on self during init
                        let struct_name = self.init_struct_name.as_ref().unwrap().clone();
                        let sem = self.sem_info.as_ref().unwrap();
                        if let Some(info) = sem.struct_defs.get(&struct_name)
                            && info.computed.iter().any(|p| p.name == *field)
                        {
                            self.record_error(
                                format!(
                                    "computed property setter `{}` on `self` in initializer body \
                                     is not supported (self is not fully materialized during init)",
                                    field
                                ),
                                Some(object.span),
                            );
                            return StmtResult::None;
                        }
                        // Stored field — write to per-field variable
                        let new_val = self.lower_expr(value);
                        let self_name = self.self_var_name.as_ref().unwrap().clone();
                        let key = format!("{}.{}", self_name, field);
                        if self.try_lookup_var(&key).is_some() {
                            self.assign_var(&key, new_val);
                        } else {
                            self.define_var(key, new_val);
                        }
                        return StmtResult::None;
                    }
                    // 2. Init body + nested self path: error
                    if self.expr_refers_to_self(object) {
                        self.record_error(
                            "nested field assignment through `self` in initializer body is not supported",
                            Some(object.span),
                        );
                        return StmtResult::None;
                    }
                }

                // 3. General case: try computed setter, then recursive field assign
                if self.try_lower_computed_setter(object, field, value) {
                    return StmtResult::None;
                }

                let new_val = self.lower_expr(value);
                self.lower_field_assign_recursive(object, field, new_val);
                StmtResult::None
            }
            Stmt::IndexAssign {
                object,
                index,
                value,
            } => {
                let idx_val = self.lower_expr(index);
                let new_val = self.lower_expr(value);

                // The object must be a variable (semantic analysis ensures this)
                let var_name = match &object.kind {
                    ExprKind::Ident(name) => name.clone(),
                    _ => unreachable!("IndexAssign on non-ident (semantic guarantees this)"),
                };
                let arr_val = self.lookup_var(&var_name);
                let arr_ty = self
                    .value_types
                    .get(&arr_val)
                    .cloned()
                    .unwrap_or(BirType::I32);
                let size = match &arr_ty {
                    BirType::Array { size, .. } => *size,
                    _ => unreachable!("IndexAssign on non-array type (semantic guarantees this)"),
                };
                let result = self.fresh_value();
                self.emit(Instruction::ArraySet {
                    result,
                    ty: arr_ty.clone(),
                    array: arr_val,
                    index: idx_val,
                    value: new_val,
                    array_size: size,
                });
                self.value_types.insert(result, arr_ty);
                self.assign_var(&var_name, result);
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
            ExprKind::Ident(name) => self.lookup_var(name),
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
                    let ty = self.value_types.get(&lhs).cloned().unwrap_or(BirType::I32);
                    let result = self.fresh_value();
                    self.emit(Instruction::BinaryOp {
                        result,
                        op: convert_binop(*op),
                        lhs,
                        rhs,
                        ty: ty.clone(),
                    });
                    self.value_types.insert(result, ty);
                    result
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    let lhs = self.lower_expr(left);
                    let rhs = self.lower_expr(right);
                    let operand_ty = self.value_types.get(&lhs).cloned().unwrap_or(BirType::I32);
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
            ExprKind::Call {
                name,
                args,
                type_args,
            } => {
                // Use explicit type_args if present, otherwise check inferred
                let effective_type_args: Vec<TypeAnnotation> = if type_args.is_empty() {
                    self.inferred_type_args
                        .get(&expr.id)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    type_args.clone()
                };
                let bir_type_args: Vec<BirType> = effective_type_args
                    .iter()
                    .map(|ta| self.convert_type_with_structs(ta))
                    .collect();
                let sem = self.sem_info.as_ref().unwrap();
                if sem.struct_init_calls.contains(&expr.id) {
                    let struct_info = sem.struct_defs.get(name).unwrap().clone();
                    if struct_info.init.body.is_some() {
                        let field_values = self.lower_explicit_init(name, &struct_info, expr);
                        return self.emit_struct_init(name, &field_values, &bir_type_args);
                    }
                    // No-arg memberwise init (call syntax with no custom init body)
                    return self.emit_struct_init(name, &[], &bir_type_args);
                }
                let arg_vals: Vec<Value> = args.iter().map(|a| self.lower_expr(a)).collect();
                let resolved = self.resolve_name(name);
                let ty = self
                    .func_sigs
                    .get(&resolved)
                    .cloned()
                    .unwrap_or(BirType::I32);
                // Resolve TypeParam return types for the local value_types map
                // so that subsequent instructions (BinaryOp, FieldGet, etc.)
                // get the resolved concrete type.
                let resolved_ty = if !bir_type_args.is_empty() {
                    use crate::bir::mono::resolve_bir_type_lenient;
                    let subst: HashMap<String, BirType> = self
                        .func_type_param_names
                        .get(&resolved)
                        .map(|params| {
                            params
                                .iter()
                                .zip(bir_type_args.iter())
                                .map(|(p, a)| (p.clone(), a.clone()))
                                .collect()
                        })
                        .unwrap_or_default();
                    if subst.is_empty() {
                        ty.clone()
                    } else {
                        resolve_bir_type_lenient(&ty, &subst)
                    }
                } else {
                    ty.clone()
                };
                let result = self.fresh_value();
                self.emit(Instruction::Call {
                    result,
                    func_name: resolved,
                    args: arg_vals,
                    type_args: bir_type_args,
                    ty: ty.clone(),
                });
                self.value_types.insert(result, resolved_ty);
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
                    .cloned()
                    .unwrap_or(BirType::I32);
                let to_ty = self.convert_type_with_structs(target_type);
                let result = self.fresh_value();
                self.emit(Instruction::Cast {
                    result,
                    operand,
                    from_ty,
                    to_ty: to_ty.clone(),
                });
                self.value_types.insert(result, to_ty);
                result
            }
            ExprKind::StructInit {
                name,
                args,
                type_args,
            } => {
                // Use explicit type_args if present, otherwise check inferred
                let effective_type_args: Vec<TypeAnnotation> = if type_args.is_empty() {
                    self.inferred_type_args
                        .get(&expr.id)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    type_args.clone()
                };
                let bir_type_args: Vec<BirType> = effective_type_args
                    .iter()
                    .map(|ta| self.convert_type_with_structs(ta))
                    .collect();
                let sem = self.sem_info.as_ref().unwrap();
                let struct_info = sem.struct_defs.get(name).unwrap().clone();
                if struct_info.init.body.is_some() {
                    let field_values = self.lower_explicit_init(name, &struct_info, expr);
                    return self.emit_struct_init(name, &field_values, &bir_type_args);
                }
                let field_values: Vec<(String, Value)> = args
                    .iter()
                    .map(|(label, arg_expr)| (label.clone(), self.lower_expr(arg_expr)))
                    .collect();
                self.emit_struct_init(name, &field_values, &bir_type_args)
            }
            ExprKind::FieldAccess { object, field } => {
                // Special case: self.field during init body
                if let ExprKind::SelfRef = &object.kind
                    && self.in_init_body
                    && let Some(self_name) = &self.self_var_name.clone()
                {
                    // Reject computed property access during init
                    let struct_name = self.init_struct_name.as_ref().unwrap().clone();
                    let sem = self.sem_info.as_ref().unwrap();
                    if let Some(info) = sem.struct_defs.get(&struct_name)
                        && info.computed.iter().any(|p| p.name == *field)
                    {
                        return self.record_error(
                            format!(
                                "computed property `{}` access on `self` in initializer body \
                                 is not supported (self is not fully materialized during init)",
                                field
                            ),
                            Some(expr.span),
                        );
                    }
                    // Stored field — read from per-field variable
                    let key = format!("{}.{}", self_name, field);
                    if let Some(val) = self.try_lookup_var(&key) {
                        return val;
                    }
                    return self.record_error(
                        format!(
                            "read-before-init: field `{}` read before initialization",
                            field
                        ),
                        Some(expr.span),
                    );
                }

                // General case: lower the receiver, then dispatch on lowered type
                let receiver = self.lower_receiver(object);
                match receiver {
                    Some(recv) => {
                        let sem = self.sem_info.as_ref().unwrap();
                        let struct_info = sem.struct_defs.get(&recv.struct_name).unwrap().clone();

                        // Check if field is a computed property
                        if let Some(prop) = struct_info.computed.iter().find(|p| p.name == *field) {
                            // Inline the getter with receiver as self
                            let prop_ty = semantic_type_to_bir(&prop.ty);
                            return self.inline_getter(
                                &recv.var_name,
                                &prop.getter.clone(),
                                prop_ty,
                            );
                        }

                        // Stored field — emit FieldGet
                        let field_sem_ty = &struct_info
                            .fields
                            .iter()
                            .find(|(n, _)| n == field)
                            .unwrap()
                            .1;
                        let field_ty = semantic_type_to_bir(field_sem_ty);
                        // Use the object's actual type (preserves type_args for generics)
                        let obj_ty = self
                            .value_types
                            .get(&recv.value)
                            .cloned()
                            .unwrap_or_else(|| BirType::struct_simple(recv.struct_name.clone()));
                        // Resolve TypeParam field types using the object's type_args
                        let resolved_field_ty = if let BirType::Struct {
                            name: ref sname,
                            type_args: ref ta,
                        } = obj_ty
                        {
                            if !ta.is_empty() {
                                use crate::bir::mono::resolve_bir_type_lenient;
                                let mangled = format!("{}_{}", sname, field);
                                let subst: HashMap<String, BirType> = self
                                    .func_type_param_names
                                    .get(&mangled)
                                    .map(|params| {
                                        params
                                            .iter()
                                            .zip(ta.iter())
                                            .map(|(p, a)| (p.clone(), a.clone()))
                                            .collect()
                                    })
                                    .unwrap_or_else(|| {
                                        // Fall back to deriving type params from sem_info
                                        let sem = self.sem_info.as_ref().unwrap();
                                        sem.struct_defs
                                            .get(sname)
                                            .map(|info| {
                                                info.type_params
                                                    .iter()
                                                    .zip(ta.iter())
                                                    .map(|(tp, a)| (tp.name.clone(), a.clone()))
                                                    .collect()
                                            })
                                            .unwrap_or_default()
                                    });
                                resolve_bir_type_lenient(&field_ty, &subst)
                            } else {
                                field_ty.clone()
                            }
                        } else {
                            field_ty.clone()
                        };
                        let result = self.fresh_value();
                        self.emit(Instruction::FieldGet {
                            result,
                            object: recv.value,
                            field: field.clone(),
                            object_ty: obj_ty,
                            ty: field_ty.clone(),
                        });
                        self.value_types.insert(result, resolved_field_ty);
                        result
                    }
                    None => unreachable!("field access on non-struct (semantic guarantees this)"),
                }
            }
            ExprKind::SelfRef => {
                let self_name = self.self_var_name.as_ref().unwrap();
                if self.in_init_body {
                    return self.record_error(
                        "bare `self` in initializer body is not supported; use self.field instead",
                        Some(expr.span),
                    );
                }
                self.lookup_var(self_name)
            }
            ExprKind::MethodCall {
                object,
                method,
                args,
            } => {
                let obj_val = self.lower_expr(object);
                let (struct_name, struct_type_args) = match self.value_types.get(&obj_val) {
                    Some(BirType::Struct {
                        name: n,
                        type_args: ta,
                    }) => (n.clone(), ta.clone()),
                    Some(BirType::TypeParam(type_param_name)) => {
                        // Protocol method call on constrained type parameter.
                        // Look up the type param's protocol constraint, then emit
                        // a Call to "{Protocol}_{method}" with type_args.
                        let type_param_name = type_param_name.clone();
                        let bound = self
                            .current_type_params
                            .iter()
                            .find(|tp| tp.name == type_param_name)
                            .and_then(|tp| tp.bound.clone());
                        let proto_name = match bound {
                            Some(b) => b,
                            None => {
                                return self.record_error(
                                    format!(
                                        "type parameter `{}` has no protocol constraint for method `{}`",
                                        type_param_name, method
                                    ),
                                    Some(expr.span),
                                );
                            }
                        };
                        // Look up the protocol's method signature for the return type
                        let ret_ty = self
                            .sem_info
                            .as_ref()
                            .and_then(|si| si.protocols.get(&proto_name))
                            .and_then(|pi| pi.methods.iter().find(|m| m.name == *method))
                            .map(|m| semantic_type_to_bir(&m.return_type))
                            .unwrap_or(BirType::Unit);
                        let func_name = format!("{}_{}", proto_name, method);
                        let mut call_args = vec![obj_val];
                        for arg in args {
                            call_args.push(self.lower_expr(arg));
                        }
                        let result = self.fresh_value();
                        self.emit(Instruction::Call {
                            result,
                            func_name,
                            args: call_args,
                            type_args: vec![BirType::TypeParam(type_param_name)],
                            ty: ret_ty.clone(),
                        });
                        self.value_types.insert(result, ret_ty);
                        return result;
                    }
                    _ => {
                        return self
                            .record_error("method call on non-struct value", Some(expr.span));
                    }
                };
                let local_mangled = format!("{}_{}", struct_name, method);
                let resolved = self.resolve_name(&local_mangled);
                let ret_ty = self
                    .func_sigs
                    .get(&resolved)
                    .cloned()
                    .unwrap_or(BirType::Unit);
                let mut call_args = vec![obj_val];
                for arg in args {
                    call_args.push(self.lower_expr(arg));
                }
                // Resolve TypeParam return type for value_types
                let resolved_ret_ty = if !struct_type_args.is_empty() {
                    use crate::bir::mono::resolve_bir_type_lenient;
                    let subst: HashMap<String, BirType> = self
                        .func_type_param_names
                        .get(&resolved)
                        .map(|params| {
                            params
                                .iter()
                                .zip(struct_type_args.iter())
                                .map(|(p, a)| (p.clone(), a.clone()))
                                .collect()
                        })
                        .unwrap_or_default();
                    if subst.is_empty() {
                        ret_ty.clone()
                    } else {
                        resolve_bir_type_lenient(&ret_ty, &subst)
                    }
                } else {
                    ret_ty.clone()
                };
                let result = self.fresh_value();
                self.emit(Instruction::Call {
                    result,
                    func_name: resolved,
                    args: call_args,
                    type_args: struct_type_args,
                    ty: ret_ty.clone(),
                });
                self.value_types.insert(result, resolved_ret_ty);
                result
            }
            ExprKind::ArrayLiteral { elements } => {
                let elem_values: Vec<Value> = elements.iter().map(|e| self.lower_expr(e)).collect();
                // Determine element BirType from first element
                let elem_ty = if let Some(first) = elem_values.first() {
                    self.value_types.get(first).cloned().unwrap_or(BirType::I32)
                } else {
                    BirType::I32
                };
                let arr_ty = BirType::Array {
                    element: Box::new(elem_ty),
                    size: elem_values.len() as u64,
                };
                let result = self.fresh_value();
                self.emit(Instruction::ArrayInit {
                    result,
                    ty: arr_ty.clone(),
                    elements: elem_values,
                });
                self.value_types.insert(result, arr_ty);
                result
            }
            ExprKind::IndexAccess { object, index } => {
                let arr_val = self.lower_expr(object);
                let idx_val = self.lower_expr(index);
                let arr_ty = self
                    .value_types
                    .get(&arr_val)
                    .cloned()
                    .unwrap_or(BirType::I32);
                let (elem_ty, size) = match &arr_ty {
                    BirType::Array { element, size } => (*element.clone(), *size),
                    _ => unreachable!("IndexAccess on non-array type (semantic guarantees this)"),
                };
                let result = self.fresh_value();
                self.emit(Instruction::ArrayGet {
                    result,
                    ty: elem_ty.clone(),
                    array: arr_val,
                    index: idx_val,
                    array_size: size,
                });
                self.value_types.insert(result, elem_ty);
                result
            }
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
                        self.value_types.get(v).cloned().unwrap_or(BirType::I32)
                    }
                    (_, Some(StmtResult::Yield(v))) => {
                        self.value_types.get(v).cloned().unwrap_or(BirType::I32)
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
                            args: vec![(*v, merge_type.clone())],
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
                            args: vec![(*v, merge_type.clone())],
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

                // Detect whether both branches diverge (all paths return/break/continue)
                let then_diverges = matches!(
                    &then_result,
                    Some(StmtResult::Return(_))
                        | Some(StmtResult::ReturnVoid)
                        | Some(StmtResult::Break)
                        | Some(StmtResult::Continue)
                );
                let else_diverges = matches!(
                    &else_result,
                    Some(StmtResult::Return(_))
                        | Some(StmtResult::ReturnVoid)
                        | Some(StmtResult::Break)
                        | Some(StmtResult::Continue)
                );
                let both_diverge = then_diverges && else_diverges;

                // merge block
                let merge_params = if has_merge_value {
                    let result = self.fresh_value();
                    self.value_types.insert(result, merge_type.clone());
                    self.start_block(bb_merge, vec![(result, merge_type)]);
                    result
                } else {
                    let result = self.fresh_value(); // dummy for Unit
                    self.start_block(bb_merge, vec![]);
                    if both_diverge {
                        // merge block is unreachable; signal caller to seal it
                        self.last_expr_diverged = true;
                    }
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
            .map(|(_, val, ty)| (*val, ty.clone()))
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
                self.value_types.insert(v, ty.clone());
                (v, ty.clone())
            })
            .collect();

        self.start_block(bb_header, header_params.clone());

        // Remap mutable vars to header params
        for (i, (name, _, _)) in mutable_vars.iter().enumerate() {
            let param_val = header_params[i].0;
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
                    .map(|(name, _, ty)| (self.lookup_var(name), ty.clone()))
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

        // Restore mutable var scope to header params.
        // The body may have reassigned vars (e.g., `a = b` makes a point to b's value).
        // After the loop, mutable vars should map to their header block parameters,
        // which hold the correct values at loop exit.
        for (i, (name, _, _)) in mutable_vars.iter().enumerate() {
            let param_val = header_params[i].0;
            self.assign_var(name, param_val);
        }

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
                        let ty = self.value_types.get(&v).cloned().unwrap_or(BirType::I32);
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
            self.start_block(bb_exit, vec![(result_val, bty.clone())]);
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

pub fn lower_program(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
) -> Result<BirModule> {
    lower_program_with_inferred(program, sem_info, &HashMap::new())
}

/// Lower program with inferred type args for call sites with omitted type arguments.
pub(crate) fn lower_program_with_inferred(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
    inferred_type_args: &HashMap<NodeId, Vec<TypeAnnotation>>,
) -> Result<BirModule> {
    // Build struct_layouts from semantic StructInfo
    let mut struct_layouts: HashMap<String, Vec<(String, BirType)>> = HashMap::new();
    for (name, info) in &sem_info.struct_defs {
        let fields: Vec<(String, BirType)> = info
            .fields
            .iter()
            .map(|(n, t)| (n.clone(), semantic_type_to_bir(t)))
            .collect();
        struct_layouts.insert(name.clone(), fields);
    }

    // Build name-to-span lookup from AST StructDefs
    let struct_spans: HashMap<&str, Span> = program
        .structs
        .iter()
        .map(|s| (s.name.as_str(), s.span))
        .collect();

    // Reject Unit-typed stored fields
    for (name, fields) in &struct_layouts {
        for (fname, fty) in fields {
            if matches!(fty, BirType::Unit) {
                return Err(BengalError::LoweringError {
                    message: format!(
                        "struct `{}` has Unit-typed stored field `{}`; Unit fields are not supported",
                        name, fname
                    ),
                    span: struct_spans.get(name.as_str()).copied(),
                });
            }
        }
    }

    // Reject recursive structs (infinitely sized)
    check_acyclic_structs(&struct_layouts, &struct_spans)?;

    let sem_info_ref = SemInfoRef {
        struct_defs: sem_info.struct_defs.clone(),
        struct_init_calls: sem_info.struct_init_calls.clone(),
        protocols: sem_info.protocols.clone(),
    };
    let mut lowering = Lowering::new(HashMap::new(), sem_info_ref);
    lowering.inferred_type_args = inferred_type_args.clone();

    // Build func_sigs using convert_type_with_structs (supports Named types)
    for func in &program.functions {
        // Push type params so generic return types resolve to TypeParam, not Struct
        lowering.current_type_params = func.type_params.clone();
        let bir_ty = lowering.convert_type_with_structs(&func.return_type);
        lowering.current_type_params.clear();
        lowering.func_sigs.insert(func.name.clone(), bir_ty);
        if !func.type_params.is_empty() {
            lowering.func_type_param_names.insert(
                func.name.clone(),
                func.type_params.iter().map(|tp| tp.name.clone()).collect(),
            );
        }
    }

    // Register mangled method signatures
    for (struct_name, info) in &sem_info.struct_defs {
        for method in &info.methods {
            let mangled = format!("{}_{}", struct_name, method.name);
            let bir_ret = semantic_type_to_bir(&method.return_type);
            lowering.func_sigs.insert(mangled.clone(), bir_ret);
            if !info.type_params.is_empty() {
                lowering.func_type_param_names.insert(
                    mangled,
                    info.type_params.iter().map(|tp| tp.name.clone()).collect(),
                );
            }
        }
    }

    let mut functions: Vec<BirFunction> = program
        .functions
        .iter()
        .map(|f| lowering.lower_function(f))
        .collect();

    // Lower methods as flattened functions
    for struct_def in &program.structs {
        for member in &struct_def.members {
            if let StructMember::Method {
                visibility: _,
                name: mname,
                params,
                return_type,
                body,
            } = member
            {
                let mangled_name = format!("{}_{}", struct_def.name, mname);
                // Build self type — for generic structs, include type params
                let self_ty = if struct_def.type_params.is_empty() {
                    TypeAnnotation::Named(struct_def.name.clone())
                } else {
                    TypeAnnotation::Generic {
                        name: struct_def.name.clone(),
                        args: struct_def
                            .type_params
                            .iter()
                            .map(|tp| TypeAnnotation::Named(tp.name.clone()))
                            .collect(),
                    }
                };
                let mut all_params = vec![Param {
                    name: "self".to_string(),
                    ty: self_ty,
                }];
                all_params.extend(params.clone());
                let func = Function {
                    visibility: Visibility::Internal,
                    name: mangled_name,
                    type_params: struct_def.type_params.clone(),
                    params: all_params,
                    return_type: return_type.clone(),
                    body: body.clone(),
                    span: struct_def.span,
                };

                // Set up self context for lowering
                lowering.self_var_name = Some("self".to_string());
                let bir_func = lowering.lower_function(&func);
                lowering.self_var_name = None;
                functions.push(bir_func);
            }
        }
    }

    if let Some(err) = lowering.lowering_error {
        return Err(err);
    }

    // Build conformance_map: (protocol_method, concrete_type) -> impl_name
    let mut conformance_map: HashMap<(String, BirType), String> = HashMap::new();
    for struct_def in &program.structs {
        for proto_name in &struct_def.conformances {
            if let Some(proto_info) = sem_info.protocols.get(proto_name) {
                for method in &proto_info.methods {
                    let key = (
                        format!("{}_{}", proto_name, method.name),
                        BirType::struct_simple(struct_def.name.clone()),
                    );
                    let impl_name = format!("{}_{}", struct_def.name, method.name);
                    conformance_map.insert(key, impl_name);
                }
            }
        }
    }

    // Build struct_type_params from semantic StructInfo
    let struct_type_params: HashMap<String, Vec<String>> = sem_info
        .struct_defs
        .iter()
        .filter(|(_, info)| !info.type_params.is_empty())
        .map(|(name, info)| {
            (
                name.clone(),
                info.type_params.iter().map(|tp| tp.name.clone()).collect(),
            )
        })
        .collect();

    Ok(BirModule {
        struct_layouts,
        struct_type_params,
        functions,
        conformance_map,
    })
}

/// Lower a single module's AST to BIR with name mangling.
///
/// `name_map` maps local names (function names, `StructName_method` method names)
/// to their mangled equivalents. The caller is responsible for building this map
/// using `mangle::mangle_function()` and `mangle::mangle_method()`.
///
/// For the entry module's `main` function, the name_map should map "main" -> "main"
/// (i.e., not mangled) so that the linker can find the entry point.
pub fn lower_module(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
    name_map: &HashMap<String, String>,
) -> Result<BirModule> {
    lower_module_with_inferred(program, sem_info, name_map, &HashMap::new())
}

/// Lower a single module's AST to BIR with name mangling and inferred type args.
///
/// Like `lower_module`, but also accepts `inferred_type_args` for call sites with
/// omitted type arguments (needed for BIR-level monomorphization).
pub(crate) fn lower_module_with_inferred(
    program: &Program,
    sem_info: &crate::semantic::SemanticInfo,
    name_map: &HashMap<String, String>,
    inferred_type_args: &HashMap<NodeId, Vec<TypeAnnotation>>,
) -> Result<BirModule> {
    // Build struct_layouts from semantic StructInfo
    let mut struct_layouts: HashMap<String, Vec<(String, BirType)>> = HashMap::new();
    for (name, info) in &sem_info.struct_defs {
        let fields: Vec<(String, BirType)> = info
            .fields
            .iter()
            .map(|(n, t)| (n.clone(), semantic_type_to_bir(t)))
            .collect();
        struct_layouts.insert(name.clone(), fields);
    }

    // Build name-to-span lookup from AST StructDefs
    let struct_spans: HashMap<&str, Span> = program
        .structs
        .iter()
        .map(|s| (s.name.as_str(), s.span))
        .collect();

    // Reject Unit-typed stored fields
    for (name, fields) in &struct_layouts {
        for (fname, fty) in fields {
            if matches!(fty, BirType::Unit) {
                return Err(BengalError::LoweringError {
                    message: format!(
                        "struct `{}` has Unit-typed stored field `{}`; Unit fields are not supported",
                        name, fname
                    ),
                    span: struct_spans.get(name.as_str()).copied(),
                });
            }
        }
    }

    // Reject recursive structs (infinitely sized)
    check_acyclic_structs(&struct_layouts, &struct_spans)?;

    let sem_info_ref = SemInfoRef {
        struct_defs: sem_info.struct_defs.clone(),
        struct_init_calls: sem_info.struct_init_calls.clone(),
        protocols: sem_info.protocols.clone(),
    };
    let mut lowering = Lowering::new(HashMap::new(), sem_info_ref);
    lowering.name_map = Some(name_map.clone());
    lowering.inferred_type_args = inferred_type_args.clone();

    // Build func_sigs using mangled names
    for func in &program.functions {
        // Push type params so generic return types resolve to TypeParam, not Struct
        lowering.current_type_params = func.type_params.clone();
        let bir_ty = lowering.convert_type_with_structs(&func.return_type);
        lowering.current_type_params.clear();
        let resolved = lowering.resolve_name(&func.name);
        if !func.type_params.is_empty() {
            lowering.func_type_param_names.insert(
                resolved.clone(),
                func.type_params.iter().map(|tp| tp.name.clone()).collect(),
            );
        }
        lowering.func_sigs.insert(resolved, bir_ty);
    }

    // Register mangled method signatures
    for (struct_name, info) in &sem_info.struct_defs {
        for method in &info.methods {
            let local_mangled = format!("{}_{}", struct_name, method.name);
            let resolved = lowering.resolve_name(&local_mangled);
            let bir_ret = semantic_type_to_bir(&method.return_type);
            if !info.type_params.is_empty() {
                lowering.func_type_param_names.insert(
                    resolved.clone(),
                    info.type_params.iter().map(|tp| tp.name.clone()).collect(),
                );
            }
            lowering.func_sigs.insert(resolved, bir_ret);
        }
    }

    // Also register imported function signatures under their mangled names.
    // These are already in the name_map; we need to register their return types
    // in func_sigs so that Call instructions can look up the return type.
    // (imported funcs' sigs are in sem_info via the resolver, but we need them
    // under their mangled names in func_sigs.)

    let mut functions: Vec<BirFunction> = program
        .functions
        .iter()
        .map(|f| lowering.lower_function(f))
        .collect();

    // Lower methods as flattened functions
    for struct_def in &program.structs {
        for member in &struct_def.members {
            if let StructMember::Method {
                visibility: _,
                name: mname,
                params,
                return_type,
                body,
            } = member
            {
                let local_mangled_name = format!("{}_{}", struct_def.name, mname);
                // Build self type — for generic structs, include type params
                let self_ty = if struct_def.type_params.is_empty() {
                    TypeAnnotation::Named(struct_def.name.clone())
                } else {
                    TypeAnnotation::Generic {
                        name: struct_def.name.clone(),
                        args: struct_def
                            .type_params
                            .iter()
                            .map(|tp| TypeAnnotation::Named(tp.name.clone()))
                            .collect(),
                    }
                };
                let mut all_params = vec![Param {
                    name: "self".to_string(),
                    ty: self_ty,
                }];
                all_params.extend(params.clone());
                // Use the local_mangled_name so resolve_name can map it
                let func = Function {
                    visibility: Visibility::Internal,
                    name: local_mangled_name,
                    type_params: struct_def.type_params.clone(),
                    params: all_params,
                    return_type: return_type.clone(),
                    body: body.clone(),
                    span: struct_def.span,
                };

                // Set up self context for lowering
                lowering.self_var_name = Some("self".to_string());
                let bir_func = lowering.lower_function(&func);
                lowering.self_var_name = None;
                functions.push(bir_func);
            }
        }
    }

    if let Some(err) = lowering.lowering_error {
        return Err(err);
    }

    // Build conformance_map: (protocol_method, concrete_type) -> impl_name
    // Use resolve_name so the impl_name matches the mangled BIR function name.
    let mut conformance_map: HashMap<(String, BirType), String> = HashMap::new();
    for struct_def in &program.structs {
        for proto_name in &struct_def.conformances {
            if let Some(proto_info) = sem_info.protocols.get(proto_name) {
                for method in &proto_info.methods {
                    let key = (
                        format!("{}_{}", proto_name, method.name),
                        BirType::struct_simple(struct_def.name.clone()),
                    );
                    let local_impl_name = format!("{}_{}", struct_def.name, method.name);
                    let impl_name = lowering.resolve_name(&local_impl_name);
                    conformance_map.insert(key, impl_name);
                }
            }
        }
    }

    // Build struct_type_params from semantic StructInfo
    let struct_type_params: HashMap<String, Vec<String>> = sem_info
        .struct_defs
        .iter()
        .filter(|(_, info)| !info.type_params.is_empty())
        .map(|(name, info)| {
            (
                name.clone(),
                info.type_params.iter().map(|tp| tp.name.clone()).collect(),
            )
        })
        .collect();

    Ok(BirModule {
        struct_layouts,
        struct_type_params,
        functions,
        conformance_map,
    })
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
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
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

    // --- Phase 5: Struct lowering tests (updated for first-class struct instructions) ---

    #[test]
    fn lower_struct_init_basic() {
        let output = lower_str(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { let p = Point(x: 1, y: 2); return p.x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_field_get() {
        let output = lower_str(
            "struct Point { var x: Int32; var y: Int32; } func main() -> Int32 { var p = Point(x: 1, y: 2); return p.x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains(r#"field_get"#));
        assert!(output.contains(r#""x""#));
    }

    #[test]
    fn lower_struct_field_set() {
        let output = lower_str(
            "struct Point { var x: Int32; } func main() -> Int32 { var p = Point(x: 1); p.x = 10; return p.x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains("field_set"));
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_as_function_arg() {
        let output = lower_str(
            "struct Point { var x: Int32; } func get_x(p: Point) -> Int32 { return p.x; } func main() -> Int32 { return get_x(Point(x: 42)); }",
        );
        assert!(output.contains("@get_x(%0: Point)"));
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_as_return_value() {
        let output = lower_str(
            "struct Point { var x: Int32; } func make() -> Point { return Point(x: 5); } func main() -> Int32 { let p = make(); return p.x; }",
        );
        assert!(output.contains("@make() -> Point"));
        assert!(output.contains("struct_init @Point"));
    }

    #[test]
    fn lower_struct_in_if_expr() {
        let output = lower_str(
            "struct Point { var x: Int32; } func main() -> Int32 { let p = if true { yield Point(x: 1); } else { yield Point(x: 2); }; return p.x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_computed_property() {
        let output = lower_str(
            "struct Foo { var x: Int32; var double: Int32 { get { return self.x; } }; } func main() -> Int32 { var f = Foo(x: 5); return f.double; }",
        );
        assert!(output.contains("struct_init @Foo"));
        // Getter is inlined — field_get on self.x
        assert!(output.contains("field_get"));
    }

    #[test]
    fn lower_struct_explicit_init() {
        let output = lower_str(
            "struct Foo { var x: Int32; init(val: Int32) { self.x = val; } } func main() -> Int32 { var f = Foo(val: 42); return f.x; }",
        );
        assert!(output.contains("struct_init @Foo"));
        assert!(output.contains("literal 42 : Int32"));
    }

    #[test]
    fn lower_struct_nested_field_assign() {
        let output = lower_str(
            "struct Inner { var x: Int32; } struct Outer { var inner: Inner; } func main() -> Int32 { var o = Outer(inner: Inner(x: 1)); o.inner.x = 10; return o.inner.x; }",
        );
        assert!(output.contains("field_get"));
        assert!(output.contains("field_set"));
    }

    #[test]
    fn lower_struct_mutable_in_loop() {
        let output = lower_str(
            "struct Acc { var val: Int32; } func main() -> Int32 { var a = Acc(val: 0); var i: Int32 = 0; while i < 3 { a.val = a.val + 1; i = i + 1; }; return a.val; }",
        );
        assert!(output.contains("struct_init @Acc"));
        assert!(output.contains("field_get"));
        assert!(output.contains("field_set"));
    }

    #[test]
    fn lower_struct_computed_setter() {
        let output = lower_str(
            "struct Foo { var x: Int32; var bar: Int32 { get { return 0; } set { self.x = newValue; } }; } func main() -> Int32 { var f = Foo(x: 1); f.bar = 10; return f.x; }",
        );
        // Setter is inlined — field_set on self.x via setter body
        assert!(output.contains("field_set"));
    }

    #[test]
    fn lower_struct_init_field_access() {
        // Point(x: 1).x should now work (struct in expression position)
        let output = lower_str(
            "struct Point { var x: Int32; } func main() -> Int32 { return Point(x: 1).x; }",
        );
        assert!(output.contains("struct_init @Point"));
        assert!(output.contains("field_get"));
    }

    // --- Error tests ---

    #[test]
    fn lower_err_read_before_init() {
        let tokens = tokenize(
            "struct Foo { var x: Int32; init(val: Int32) { let y: Int32 = self.x; self.x = val; } } func main() -> Int32 { var f = Foo(val: 1); return f.x; }",
        ).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("read-before-init"));
    }

    #[test]
    fn lower_err_recursive_struct() {
        let tokens =
            tokenize("struct Node { var next: Node; } func main() -> Int32 { return 0; }").unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("recursive struct"));
    }

    #[test]
    fn lower_err_recursive_struct_has_span() {
        let source =
            "struct A { var b: B; } struct B { var a: A; } func main() -> Int32 { return 0; }";
        let tokens = crate::lexer::tokenize(source).unwrap();
        let program = crate::parser::parse(tokens).unwrap();
        let sem_info = crate::semantic::analyze_post_mono(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        match result {
            Err(crate::error::BengalError::LoweringError { span, .. }) => {
                assert!(span.is_some(), "recursive struct error should include span");
            }
            other => panic!("expected LoweringError, got {:?}", other),
        }
    }

    #[test]
    fn lower_err_bare_self_in_init() {
        let tokens = tokenize(
            "struct Foo { var x: Int32; init(val: Int32) { self.x = val; let s = self; } } func main() -> Int32 { var f = Foo(val: 1); return f.x; }",
        ).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("bare `self`"));
    }

    #[test]
    fn lower_err_computed_on_self_in_init() {
        let tokens = tokenize(
            "struct Foo { var x: Int32; var double: Int32 { get { return self.x; } }; init(val: Int32) { self.x = val; let d: Int32 = self.double; } } func main() -> Int32 { var f = Foo(val: 1); return f.x; }",
        ).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("computed property")
        );
    }

    #[test]
    fn lower_err_nested_self_field_assign_in_init() {
        let tokens = tokenize(
            "struct Inner { var x: Int32; } struct Outer { var inner: Inner; init() { self.inner = Inner(x: 0); self.inner.x = 10; } } func main() -> Int32 { var o = Outer(); return o.inner.x; }",
        ).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();
        let result = lower_program(&program, &sem_info);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("nested field assignment")
        );
    }

    // --- Per-module lowering tests ---

    #[test]
    fn lower_module_mangles_function_names() {
        let input = "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }";
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();

        // Build name_map: main stays as "main", add gets mangled
        let mut name_map = HashMap::new();
        name_map.insert("main".to_string(), "main".to_string());
        name_map.insert(
            "add".to_string(),
            crate::mangle::mangle_function("my_app", &[""], "add"),
        );

        let module = lower_module(&program, &sem_info, &name_map).unwrap();
        let output = print_module(&module);

        // "main" function name should NOT be mangled
        assert!(output.contains("@main("));
        // "add" function name should be mangled
        let mangled_add = crate::mangle::mangle_function("my_app", &[""], "add");
        assert!(
            output.contains(&format!("@{}(", mangled_add)),
            "expected mangled add function, got:\n{}",
            output
        );
        // Call to add should also use the mangled name
        assert!(
            output.contains(&format!("call @{}", mangled_add)),
            "expected mangled call target, got:\n{}",
            output
        );
    }

    #[test]
    fn lower_module_mangles_method_names() {
        let input = "struct Point { var x: Int32; func get_x() -> Int32 { return self.x; } } func main() -> Int32 { var p = Point(x: 42); return p.get_x(); }";
        let tokens = tokenize(input).unwrap();
        let program = parse(tokens).unwrap();
        let sem_info = semantic::analyze_post_mono(&program).unwrap();

        let mut name_map = HashMap::new();
        name_map.insert("main".to_string(), "main".to_string());
        name_map.insert(
            "Point_get_x".to_string(),
            crate::mangle::mangle_method("my_app", &[""], "Point", "get_x"),
        );

        let module = lower_module(&program, &sem_info, &name_map).unwrap();
        let output = print_module(&module);

        let mangled_method = crate::mangle::mangle_method("my_app", &[""], "Point", "get_x");
        // The method function should have the mangled name
        assert!(
            output.contains(&format!("@{}", mangled_method)),
            "expected mangled method name, got:\n{}",
            output
        );
        // The call to the method should also use the mangled name
        assert!(
            output.contains(&format!("call @{}", mangled_method)),
            "expected mangled method call, got:\n{}",
            output
        );
    }
}
