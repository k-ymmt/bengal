use std::collections::HashMap;

use wasm_encoder::{
    BlockType, CodeSection, ExportKind, ExportSection, Function, FunctionSection, Module,
    TypeSection, ValType,
};

use crate::bir::instruction::*;
use crate::error::{BengalError, Result};

fn bir_type_to_val_type(ty: &BirType) -> Result<ValType> {
    match ty {
        BirType::I32 | BirType::Bool => Ok(ValType::I32),
        BirType::I64 => Ok(ValType::I64),
        BirType::F32 => Ok(ValType::F32),
        BirType::F64 => Ok(ValType::F64),
        _ => Err(BengalError::CodegenError {
            message: format!("unsupported type: {:?}", ty),
        }),
    }
}

struct LoopLabels {
    exit_depth: u32,
    loop_depth: u32,
    header_bb: u32,
}

fn collect_locals(func: &BirFunction) -> (HashMap<Value, u32>, HashMap<Value, BirType>) {
    let mut locals = HashMap::new();
    let mut value_types = HashMap::new();
    let param_count = func.params.len() as u32;

    // Parameters occupy local indices 0..param_count
    for (i, (val, ty)) in func.params.iter().enumerate() {
        locals.insert(*val, i as u32);
        value_types.insert(*val, *ty);
    }

    // Instruction results get indices starting at param_count
    let mut next_local = param_count;
    for block in &func.blocks {
        // Block params (merge block args, loop header args)
        for (val, ty) in &block.params {
            if !locals.contains_key(val) {
                locals.insert(*val, next_local);
                next_local += 1;
            }
            value_types.insert(*val, *ty);
        }
        for inst in &block.instructions {
            let (result, skip, ty) = match inst {
                Instruction::Literal { result, ty, .. } => (result, *ty == BirType::Unit, *ty),
                Instruction::BinaryOp { result, ty, .. } => (result, false, *ty),
                Instruction::Compare { result, .. } => (result, false, BirType::Bool),
                Instruction::Not { result, .. } => (result, false, BirType::Bool),
                Instruction::Cast { result, to_ty, .. } => (result, false, *to_ty),
                Instruction::Call { result, ty, .. } => (result, *ty == BirType::Unit, *ty),
            };
            if !skip && !locals.contains_key(result) {
                locals.insert(*result, next_local);
                next_local += 1;
            }
            value_types.insert(*result, ty);
        }
    }

    (locals, value_types)
}

fn emit_instruction(
    inst: &Instruction,
    locals: &HashMap<Value, u32>,
    func_index_map: &HashMap<String, u32>,
    func: &mut Function,
) {
    match inst {
        Instruction::Literal { result, value, ty } => {
            if *ty == BirType::Unit {
                return;
            }
            let wasm_const = match ty {
                BirType::I32 | BirType::Bool => wasm_encoder::Instruction::I32Const(*value as i32),
                BirType::I64 => wasm_encoder::Instruction::I64Const(*value),
                BirType::F32 => wasm_encoder::Instruction::F32Const(f32::from_bits(*value as u32)),
                BirType::F64 => wasm_encoder::Instruction::F64Const(f64::from_bits(*value as u64)),
                BirType::Unit => return,
            };
            func.instruction(&wasm_const);
            func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
        }
        Instruction::BinaryOp {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => {
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[lhs]));
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[rhs]));
            let wasm_op = match (op, ty) {
                (BirBinOp::Add, BirType::I32) => wasm_encoder::Instruction::I32Add,
                (BirBinOp::Add, BirType::I64) => wasm_encoder::Instruction::I64Add,
                (BirBinOp::Add, BirType::F32) => wasm_encoder::Instruction::F32Add,
                (BirBinOp::Add, BirType::F64) => wasm_encoder::Instruction::F64Add,
                (BirBinOp::Sub, BirType::I32) => wasm_encoder::Instruction::I32Sub,
                (BirBinOp::Sub, BirType::I64) => wasm_encoder::Instruction::I64Sub,
                (BirBinOp::Sub, BirType::F32) => wasm_encoder::Instruction::F32Sub,
                (BirBinOp::Sub, BirType::F64) => wasm_encoder::Instruction::F64Sub,
                (BirBinOp::Mul, BirType::I32) => wasm_encoder::Instruction::I32Mul,
                (BirBinOp::Mul, BirType::I64) => wasm_encoder::Instruction::I64Mul,
                (BirBinOp::Mul, BirType::F32) => wasm_encoder::Instruction::F32Mul,
                (BirBinOp::Mul, BirType::F64) => wasm_encoder::Instruction::F64Mul,
                (BirBinOp::Div, BirType::I32) => wasm_encoder::Instruction::I32DivS,
                (BirBinOp::Div, BirType::I64) => wasm_encoder::Instruction::I64DivS,
                (BirBinOp::Div, BirType::F32) => wasm_encoder::Instruction::F32Div,
                (BirBinOp::Div, BirType::F64) => wasm_encoder::Instruction::F64Div,
                _ => unreachable!(),
            };
            func.instruction(&wasm_op);
            func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
        }
        Instruction::Call {
            result,
            func_name,
            args,
            ty,
        } => {
            for arg in args {
                func.instruction(&wasm_encoder::Instruction::LocalGet(locals[arg]));
            }
            let idx = func_index_map[func_name.as_str()];
            func.instruction(&wasm_encoder::Instruction::Call(idx));
            if *ty != BirType::Unit {
                func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
            }
        }
        Instruction::Compare {
            result,
            op,
            lhs,
            rhs,
            ty,
        } => {
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[lhs]));
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[rhs]));
            let wasm_op = match (op, ty) {
                (BirCompareOp::Eq, BirType::I32) => wasm_encoder::Instruction::I32Eq,
                (BirCompareOp::Eq, BirType::I64) => wasm_encoder::Instruction::I64Eq,
                (BirCompareOp::Eq, BirType::F32) => wasm_encoder::Instruction::F32Eq,
                (BirCompareOp::Eq, BirType::F64) => wasm_encoder::Instruction::F64Eq,
                (BirCompareOp::Ne, BirType::I32) => wasm_encoder::Instruction::I32Ne,
                (BirCompareOp::Ne, BirType::I64) => wasm_encoder::Instruction::I64Ne,
                (BirCompareOp::Ne, BirType::F32) => wasm_encoder::Instruction::F32Ne,
                (BirCompareOp::Ne, BirType::F64) => wasm_encoder::Instruction::F64Ne,
                (BirCompareOp::Lt, BirType::I32) => wasm_encoder::Instruction::I32LtS,
                (BirCompareOp::Lt, BirType::I64) => wasm_encoder::Instruction::I64LtS,
                (BirCompareOp::Lt, BirType::F32) => wasm_encoder::Instruction::F32Lt,
                (BirCompareOp::Lt, BirType::F64) => wasm_encoder::Instruction::F64Lt,
                (BirCompareOp::Gt, BirType::I32) => wasm_encoder::Instruction::I32GtS,
                (BirCompareOp::Gt, BirType::I64) => wasm_encoder::Instruction::I64GtS,
                (BirCompareOp::Gt, BirType::F32) => wasm_encoder::Instruction::F32Gt,
                (BirCompareOp::Gt, BirType::F64) => wasm_encoder::Instruction::F64Gt,
                (BirCompareOp::Le, BirType::I32) => wasm_encoder::Instruction::I32LeS,
                (BirCompareOp::Le, BirType::I64) => wasm_encoder::Instruction::I64LeS,
                (BirCompareOp::Le, BirType::F32) => wasm_encoder::Instruction::F32Le,
                (BirCompareOp::Le, BirType::F64) => wasm_encoder::Instruction::F64Le,
                (BirCompareOp::Ge, BirType::I32) => wasm_encoder::Instruction::I32GeS,
                (BirCompareOp::Ge, BirType::I64) => wasm_encoder::Instruction::I64GeS,
                (BirCompareOp::Ge, BirType::F32) => wasm_encoder::Instruction::F32Ge,
                (BirCompareOp::Ge, BirType::F64) => wasm_encoder::Instruction::F64Ge,
                _ => unreachable!(),
            };
            func.instruction(&wasm_op);
            func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
        }
        Instruction::Not { result, operand } => {
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[operand]));
            func.instruction(&wasm_encoder::Instruction::I32Eqz);
            func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
        }
        Instruction::Cast {
            result,
            operand,
            from_ty,
            to_ty,
        } => {
            if from_ty == to_ty {
                // Same type — just copy
                func.instruction(&wasm_encoder::Instruction::LocalGet(locals[operand]));
                func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
                return;
            }
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[operand]));
            let wasm_op = match (from_ty, to_ty) {
                (BirType::I32, BirType::I64) => wasm_encoder::Instruction::I64ExtendI32S,
                (BirType::I64, BirType::I32) => wasm_encoder::Instruction::I32WrapI64,
                (BirType::I32, BirType::F32) => wasm_encoder::Instruction::F32ConvertI32S,
                (BirType::I32, BirType::F64) => wasm_encoder::Instruction::F64ConvertI32S,
                (BirType::I64, BirType::F32) => wasm_encoder::Instruction::F32ConvertI64S,
                (BirType::I64, BirType::F64) => wasm_encoder::Instruction::F64ConvertI64S,
                (BirType::F32, BirType::I32) => wasm_encoder::Instruction::I32TruncF32S,
                (BirType::F32, BirType::I64) => wasm_encoder::Instruction::I64TruncF32S,
                (BirType::F64, BirType::I32) => wasm_encoder::Instruction::I32TruncF64S,
                (BirType::F64, BirType::I64) => wasm_encoder::Instruction::I64TruncF64S,
                (BirType::F32, BirType::F64) => wasm_encoder::Instruction::F64PromoteF32,
                (BirType::F64, BirType::F32) => wasm_encoder::Instruction::F32DemoteF64,
                _ => return,
            };
            func.instruction(&wasm_op);
            func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
        }
    }
}

/// Emit block instructions only (no terminator)
fn emit_block_instructions(
    block: &BasicBlock,
    locals: &HashMap<Value, u32>,
    func_index_map: &HashMap<String, u32>,
    func: &mut Function,
) {
    for inst in &block.instructions {
        emit_instruction(inst, locals, func_index_map, func);
    }
}

/// Emit Br args as local.set (block argument passing)
fn emit_br_args(
    args: &[(Value, BirType)],
    target_block: &BasicBlock,
    locals: &HashMap<Value, u32>,
    func: &mut Function,
) {
    for (i, (val, _)) in args.iter().enumerate() {
        func.instruction(&wasm_encoder::Instruction::LocalGet(locals[val]));
        let target_param = target_block.params[i].0;
        func.instruction(&wasm_encoder::Instruction::LocalSet(locals[&target_param]));
    }
}

/// Find a BasicBlock by label
fn find_block(blocks: &[BasicBlock], label: u32) -> &BasicBlock {
    blocks.iter().find(|b| b.label == label).unwrap()
}

/// Emit a sequence of CfgRegions
fn emit_regions(
    regions: &[CfgRegion],
    blocks: &[BasicBlock],
    locals: &HashMap<Value, u32>,
    func_index_map: &HashMap<String, u32>,
    func: &mut Function,
    loop_labels: Option<&LoopLabels>,
) {
    for region in regions {
        emit_region(region, blocks, locals, func_index_map, func, loop_labels);
    }
}

/// Emit a single CfgRegion
fn emit_region(
    region: &CfgRegion,
    blocks: &[BasicBlock],
    locals: &HashMap<Value, u32>,
    func_index_map: &HashMap<String, u32>,
    func: &mut Function,
    loop_labels: Option<&LoopLabels>,
) {
    match region {
        CfgRegion::Block(label) => {
            let block = find_block(blocks, *label);
            emit_block_instructions(block, locals, func_index_map, func);

            // Handle terminator
            match &block.terminator {
                Terminator::Return(val) => {
                    func.instruction(&wasm_encoder::Instruction::LocalGet(locals[val]));
                    func.instruction(&wasm_encoder::Instruction::Return);
                }
                Terminator::ReturnVoid => {
                    func.instruction(&wasm_encoder::Instruction::Return);
                }
                Terminator::Br { args, target } => {
                    let target_block = find_block(blocks, *target);
                    emit_br_args(args, target_block, locals, func);
                }
                Terminator::CondBr { .. } => {
                    unreachable!("CondBr in CfgRegion::Block should not happen");
                }
                Terminator::BrBreak {
                    exit_bb,
                    args,
                    value,
                } => {
                    let ll = loop_labels.expect("BrBreak outside of loop context");
                    // Copy mutable var values → header_bb param locals (for post-loop reads)
                    let header_block = find_block(blocks, ll.header_bb);
                    emit_br_args(args, header_block, locals, func);
                    // Handle break value → exit_bb block arg local
                    if let Some((val, _)) = value {
                        func.instruction(&wasm_encoder::Instruction::LocalGet(locals[val]));
                        let exit_block = find_block(blocks, *exit_bb);
                        if !exit_block.params.is_empty() {
                            let exit_param = exit_block.params[0].0;
                            func.instruction(&wasm_encoder::Instruction::LocalSet(
                                locals[&exit_param],
                            ));
                        }
                    }
                    func.instruction(&wasm_encoder::Instruction::Br(ll.exit_depth));
                }
                Terminator::BrContinue { header_bb, args } => {
                    let ll = loop_labels.expect("BrContinue outside of loop context");
                    // Set header_bb param locals
                    let header_block = find_block(blocks, *header_bb);
                    emit_br_args(args, header_block, locals, func);
                    func.instruction(&wasm_encoder::Instruction::Br(ll.loop_depth));
                }
            }
        }

        CfgRegion::IfElse {
            cond_region,
            cond_bb,
            cond_value,
            then_region,
            else_region,
            merge_bb,
        } => {
            emit_regions(
                cond_region,
                blocks,
                locals,
                func_index_map,
                func,
                loop_labels,
            );

            let cond_block = find_block(blocks, *cond_bb);
            emit_block_instructions(cond_block, locals, func_index_map, func);

            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[cond_value]));
            func.instruction(&wasm_encoder::Instruction::If(BlockType::Empty));

            // Inside if: depth +1 for loop labels
            let inner_labels = loop_labels.map(|ll| LoopLabels {
                exit_depth: ll.exit_depth + 1,
                loop_depth: ll.loop_depth + 1,
                header_bb: ll.header_bb,
            });
            emit_regions(
                then_region,
                blocks,
                locals,
                func_index_map,
                func,
                inner_labels.as_ref(),
            );

            func.instruction(&wasm_encoder::Instruction::Else);
            emit_regions(
                else_region,
                blocks,
                locals,
                func_index_map,
                func,
                inner_labels.as_ref(),
            );

            func.instruction(&wasm_encoder::Instruction::End);

            let merge_block = find_block(blocks, *merge_bb);
            emit_block_instructions(merge_block, locals, func_index_map, func);
        }

        CfgRegion::IfOnly {
            cond_region,
            cond_bb,
            cond_value,
            then_region,
            merge_bb,
        } => {
            emit_regions(
                cond_region,
                blocks,
                locals,
                func_index_map,
                func,
                loop_labels,
            );

            let cond_block = find_block(blocks, *cond_bb);
            emit_block_instructions(cond_block, locals, func_index_map, func);

            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[cond_value]));
            func.instruction(&wasm_encoder::Instruction::If(BlockType::Empty));

            let inner_labels = loop_labels.map(|ll| LoopLabels {
                exit_depth: ll.exit_depth + 1,
                loop_depth: ll.loop_depth + 1,
                header_bb: ll.header_bb,
            });
            emit_regions(
                then_region,
                blocks,
                locals,
                func_index_map,
                func,
                inner_labels.as_ref(),
            );

            func.instruction(&wasm_encoder::Instruction::End);

            let merge_block = find_block(blocks, *merge_bb);
            emit_block_instructions(merge_block, locals, func_index_map, func);
        }

        CfgRegion::While {
            entry_bb,
            header_region,
            header_bb,
            cond_value,
            body_region,
            nobreak_region,
            exit_bb,
        } => {
            let has_nobreak = !nobreak_region.is_empty();

            // 1. entry_bb instructions
            let entry_block = find_block(blocks, *entry_bb);
            emit_block_instructions(entry_block, locals, func_index_map, func);

            // 2. entry_bb Br args → local.set (initialize header block args)
            if let Terminator::Br { args, target } = &entry_block.terminator {
                let header_block = find_block(blocks, *target);
                emit_br_args(args, header_block, locals, func);
            }

            if has_nobreak {
                // block $exit { block $nobreak { loop $loop { ... } } nobreak_region } exit_bb
                func.instruction(&wasm_encoder::Instruction::Block(BlockType::Empty)); // $exit
                func.instruction(&wasm_encoder::Instruction::Block(BlockType::Empty)); // $nobreak
                func.instruction(&wasm_encoder::Instruction::Loop(BlockType::Empty)); // $loop

                let while_labels = LoopLabels {
                    exit_depth: 2,
                    loop_depth: 0,
                    header_bb: *header_bb,
                };

                // 3. header_region
                emit_regions(
                    header_region,
                    blocks,
                    locals,
                    func_index_map,
                    func,
                    Some(&while_labels),
                );

                // 4. header_bb instructions
                let header_block = find_block(blocks, *header_bb);
                emit_block_instructions(header_block, locals, func_index_map, func);

                // 5. condition false → br $nobreak (depth 1 from inside loop)
                func.instruction(&wasm_encoder::Instruction::LocalGet(locals[cond_value]));
                func.instruction(&wasm_encoder::Instruction::I32Eqz);
                func.instruction(&wasm_encoder::Instruction::BrIf(1)); // → $nobreak end

                // 6. body_region
                emit_regions(
                    body_region,
                    blocks,
                    locals,
                    func_index_map,
                    func,
                    Some(&while_labels),
                );

                // 7. br $loop
                func.instruction(&wasm_encoder::Instruction::Br(0));

                // end loop
                func.instruction(&wasm_encoder::Instruction::End);
                // end $nobreak block — condition false lands here
                func.instruction(&wasm_encoder::Instruction::End);

                // nobreak_region (between $nobreak end and $exit end)
                // Inside nobreak, loop_labels should point to parent loop if nested
                emit_regions(
                    nobreak_region,
                    blocks,
                    locals,
                    func_index_map,
                    func,
                    loop_labels,
                );

                // end $exit block — break lands here
                func.instruction(&wasm_encoder::Instruction::End);
            } else {
                // block $exit { loop $loop { ... } } exit_bb
                func.instruction(&wasm_encoder::Instruction::Block(BlockType::Empty));
                func.instruction(&wasm_encoder::Instruction::Loop(BlockType::Empty));

                let while_labels = LoopLabels {
                    exit_depth: 1,
                    loop_depth: 0,
                    header_bb: *header_bb,
                };

                // 3. header_region
                emit_regions(
                    header_region,
                    blocks,
                    locals,
                    func_index_map,
                    func,
                    Some(&while_labels),
                );

                // 4. header_bb instructions
                let header_block = find_block(blocks, *header_bb);
                emit_block_instructions(header_block, locals, func_index_map, func);

                // 5. condition false → br $exit
                func.instruction(&wasm_encoder::Instruction::LocalGet(locals[cond_value]));
                func.instruction(&wasm_encoder::Instruction::I32Eqz);
                func.instruction(&wasm_encoder::Instruction::BrIf(1));

                // 6. body_region
                emit_regions(
                    body_region,
                    blocks,
                    locals,
                    func_index_map,
                    func,
                    Some(&while_labels),
                );

                // 7. br $loop
                func.instruction(&wasm_encoder::Instruction::Br(0));

                // end loop, end block
                func.instruction(&wasm_encoder::Instruction::End);
                func.instruction(&wasm_encoder::Instruction::End);
            }

            // 8. exit_bb instructions
            let exit_block = find_block(blocks, *exit_bb);
            emit_block_instructions(exit_block, locals, func_index_map, func);
        }
    }
}

pub fn compile(bir_module: &BirModule) -> Result<Vec<u8>> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    let mut functions = FunctionSection::new();
    let mut exports = ExportSection::new();
    let mut code = CodeSection::new();

    // Build function name → index mapping
    let mut func_index_map = HashMap::new();
    for (i, bir_func) in bir_module.functions.iter().enumerate() {
        func_index_map.insert(bir_func.name.clone(), i as u32);
    }

    for (i, bir_func) in bir_module.functions.iter().enumerate() {
        // Function signature
        let result_types: Vec<ValType> = if bir_func.return_type == BirType::Unit {
            vec![]
        } else {
            vec![bir_type_to_val_type(&bir_func.return_type)?]
        };
        let param_types: Vec<ValType> = bir_func
            .params
            .iter()
            .map(|(_, ty)| bir_type_to_val_type(ty))
            .collect::<Result<_>>()?;

        types.ty().function(param_types, result_types.clone());
        functions.function(i as u32);

        let (locals, value_types) = collect_locals(bir_func);
        let param_count = bir_func.params.len() as u32;

        // Build local types: each extra local declared individually in index order
        let mut max_local: u32 = param_count;
        for idx in locals.values() {
            if *idx >= param_count && *idx + 1 > max_local {
                max_local = *idx + 1;
            }
        }
        let extra_locals = max_local - param_count;
        // Build index → ValType map for extra locals
        let mut local_valtypes: Vec<ValType> = vec![ValType::I32; extra_locals as usize];
        for (val, idx) in &locals {
            if *idx >= param_count {
                let bir_ty = value_types.get(val).unwrap_or(&BirType::I32);
                let vt = bir_type_to_val_type(bir_ty).unwrap_or(ValType::I32);
                local_valtypes[(*idx - param_count) as usize] = vt;
            }
        }
        // Declare each local individually (1, type) to preserve index ordering
        let local_types_vec: Vec<(u32, ValType)> =
            local_valtypes.iter().map(|vt| (1, *vt)).collect();

        let mut func = Function::new(local_types_vec);

        emit_regions(
            &bir_func.body,
            &bir_func.blocks,
            &locals,
            &func_index_map,
            &mut func,
            None,
        );

        func.instruction(&wasm_encoder::Instruction::End);
        code.function(&func);

        if bir_func.name == "main" {
            exports.export(&bir_func.name, ExportKind::Func, i as u32);
        }
    }

    module.section(&types);
    module.section(&functions);
    module.section(&exports);
    module.section(&code);

    Ok(module.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bir::lowering::lower_program;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::semantic;

    fn compile_and_run(source: &str) -> i32 {
        let tokens = tokenize(source).unwrap();
        let program = parse(tokens).unwrap();
        semantic::analyze(&program).unwrap();
        let bir_module = lower_program(&program).unwrap();
        let wasm_bytes = compile(&bir_module).unwrap();

        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let main = instance
            .get_typed_func::<(), i32>(&mut store, "main")
            .unwrap();
        main.call(&mut store, ()).unwrap()
    }

    // --- Phase 2 tests (maintained) ---

    #[test]
    fn compile_simple_return() {
        assert_eq!(compile_and_run("func main() -> Int32 { return 42; }"), 42);
    }

    #[test]
    fn compile_call() {
        assert_eq!(
            compile_and_run(
                "func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return add(3, 4); }"
            ),
            7
        );
    }

    #[test]
    fn compile_let_variable() {
        assert_eq!(
            compile_and_run("func main() -> Int32 { let x: Int32 = 10; return x + 1; }"),
            11
        );
    }

    // --- Phase 3 tests ---

    #[test]
    fn compile_if_else() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int32 = if true { yield 1; } else { yield 2; }; return x; }"
            ),
            1
        );
    }

    #[test]
    fn compile_while() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var s: Int32 = 0; var i: Int32 = 0; while i < 3 { s = s + i; i = i + 1; }; return s; }"
            ),
            3
        );
    }

    #[test]
    fn compile_comparison() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int32 = if 3 > 2 { yield 1; } else { yield 0; }; return x; }"
            ),
            1
        );
    }

    // --- Phase 4 tests ---

    #[test]
    fn compile_break() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var i: Int32 = 0; while true { if i == 3 { break; }; i = i + 1; }; return i; }"
            ),
            3
        );
    }

    #[test]
    fn compile_continue() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var i: Int32 = 0; var s: Int32 = 0; while i < 5 { i = i + 1; if i == 3 { continue; }; s = s + i; }; return s; }"
            ),
            12
        );
    }

    #[test]
    fn compile_nobreak_with_break() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var i: Int32 = 0; let x: Int32 = while i < 10 { if i == 5 { break 99; }; i = i + 1; } nobreak { yield 0; }; return x; }"
            ),
            99
        );
    }

    #[test]
    fn compile_nobreak_condition_false() {
        // while with no break in body → while_ty is Unit, so nobreak must also be Unit
        // Use nobreak to compute a value via a separate variable
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { var i: Int32 = 0; while i < 3 { i = i + 1; } nobreak { }; return i; }"
            ),
            3
        );
    }

    #[test]
    fn compile_cast_i64() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int64 = 100 as Int64; return x as Int32; }"
            ),
            100
        );
    }

    #[test]
    fn compile_i64_arithmetic() {
        assert_eq!(
            compile_and_run(
                "func main() -> Int32 { let x: Int64 = 10 as Int64; let y: Int64 = 20 as Int64; return (x + y) as Int32; }"
            ),
            30
        );
    }
}
