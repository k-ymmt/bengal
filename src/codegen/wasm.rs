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

fn collect_locals(func: &BirFunction) -> (HashMap<Value, u32>, u32) {
    let mut locals = HashMap::new();
    let param_count = func.params.len() as u32;

    // Parameters occupy local indices 0..param_count
    for (i, (val, _)) in func.params.iter().enumerate() {
        locals.insert(*val, i as u32);
    }

    // Instruction results get indices starting at param_count
    let mut next_local = param_count;
    for block in &func.blocks {
        // Block params (merge block args, loop header args)
        for (val, _) in &block.params {
            if !locals.contains_key(val) {
                locals.insert(*val, next_local);
                next_local += 1;
            }
        }
        for inst in &block.instructions {
            let (result, skip) = match inst {
                Instruction::Literal { result, .. }
                | Instruction::BinaryOp { result, .. }
                | Instruction::Compare { result, .. }
                | Instruction::Not { result, .. } => (result, false),
                Instruction::Call { result, ty, .. } => {
                    // Skip local allocation for void calls
                    (result, *ty == BirType::Unit)
                }
            };
            if !skip && !locals.contains_key(result) {
                locals.insert(*result, next_local);
                next_local += 1;
            }
        }
    }

    let extra_locals = next_local - param_count;
    (locals, extra_locals)
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
                return; // Unit literals don't produce WASM values
            }
            func.instruction(&wasm_encoder::Instruction::I32Const(*value as i32));
            func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
        }
        Instruction::BinaryOp {
            result,
            op,
            lhs,
            rhs,
            ..
        } => {
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[lhs]));
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[rhs]));
            let wasm_op = match op {
                BirBinOp::Add => wasm_encoder::Instruction::I32Add,
                BirBinOp::Sub => wasm_encoder::Instruction::I32Sub,
                BirBinOp::Mul => wasm_encoder::Instruction::I32Mul,
                BirBinOp::Div => wasm_encoder::Instruction::I32DivS,
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
        } => {
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[lhs]));
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[rhs]));
            let wasm_op = match op {
                BirCompareOp::Eq => wasm_encoder::Instruction::I32Eq,
                BirCompareOp::Ne => wasm_encoder::Instruction::I32Ne,
                BirCompareOp::Lt => wasm_encoder::Instruction::I32LtS,
                BirCompareOp::Gt => wasm_encoder::Instruction::I32GtS,
                BirCompareOp::Le => wasm_encoder::Instruction::I32LeS,
                BirCompareOp::Ge => wasm_encoder::Instruction::I32GeS,
            };
            func.instruction(&wasm_op);
            func.instruction(&wasm_encoder::Instruction::LocalSet(locals[result]));
        }
        Instruction::Not { result, operand } => {
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[operand]));
            func.instruction(&wasm_encoder::Instruction::I32Eqz);
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
fn emit_br_args(args: &[(Value, BirType)], target_block: &BasicBlock, locals: &HashMap<Value, u32>, func: &mut Function) {
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
) {
    for region in regions {
        emit_region(region, blocks, locals, func_index_map, func);
    }
}

/// Emit a single CfgRegion
fn emit_region(
    region: &CfgRegion,
    blocks: &[BasicBlock],
    locals: &HashMap<Value, u32>,
    func_index_map: &HashMap<String, u32>,
    func: &mut Function,
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
                    // Emit local.set for block args only (no WASM br)
                    let target_block = find_block(blocks, *target);
                    emit_br_args(args, target_block, locals, func);
                }
                Terminator::CondBr { .. } => {
                    // Should not appear in CfgRegion::Block — handled by parent IfElse/While
                    unreachable!("CondBr in CfgRegion::Block should not happen");
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
            // 1. Emit cond_region (short-circuit evaluation, may be empty)
            emit_regions(cond_region, blocks, locals, func_index_map, func);

            // 2. Emit cond_bb instructions only (no CondBr)
            let cond_block = find_block(blocks, *cond_bb);
            emit_block_instructions(cond_block, locals, func_index_map, func);

            // 3. local.get cond_value → if
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[cond_value]));
            func.instruction(&wasm_encoder::Instruction::If(BlockType::Empty));

            // 4. then_region
            emit_regions(then_region, blocks, locals, func_index_map, func);

            // 5. else
            func.instruction(&wasm_encoder::Instruction::Else);
            emit_regions(else_region, blocks, locals, func_index_map, func);

            // end
            func.instruction(&wasm_encoder::Instruction::End);

            // 6. merge_bb instructions + terminator
            let merge_block = find_block(blocks, *merge_bb);
            emit_block_instructions(merge_block, locals, func_index_map, func);
            // merge_bb's terminator is handled as part of the parent region's next Block
            // (if merge_bb has Return, it will be the next CfgRegion::Block)
        }

        CfgRegion::IfOnly {
            cond_region,
            cond_bb,
            cond_value,
            then_region,
            merge_bb,
        } => {
            emit_regions(cond_region, blocks, locals, func_index_map, func);

            let cond_block = find_block(blocks, *cond_bb);
            emit_block_instructions(cond_block, locals, func_index_map, func);

            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[cond_value]));
            func.instruction(&wasm_encoder::Instruction::If(BlockType::Empty));

            emit_regions(then_region, blocks, locals, func_index_map, func);

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
            exit_bb,
        } => {
            // 1. entry_bb instructions
            let entry_block = find_block(blocks, *entry_bb);
            emit_block_instructions(entry_block, locals, func_index_map, func);

            // 2. entry_bb Br args → local.set (initialize header block args)
            if let Terminator::Br { args, target } = &entry_block.terminator {
                let header_block = find_block(blocks, *target);
                emit_br_args(args, header_block, locals, func);
            }

            // block $exit { loop $loop {
            func.instruction(&wasm_encoder::Instruction::Block(BlockType::Empty));
            func.instruction(&wasm_encoder::Instruction::Loop(BlockType::Empty));

            // 3. header_region (short-circuit in condition, may be empty)
            emit_regions(header_region, blocks, locals, func_index_map, func);

            // 4. header_bb instructions only (no CondBr)
            let header_block = find_block(blocks, *header_bb);
            emit_block_instructions(header_block, locals, func_index_map, func);

            // 5. local.get cond → i32.eqz → br_if $exit (depth 1 = outer block)
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[cond_value]));
            func.instruction(&wasm_encoder::Instruction::I32Eqz);
            func.instruction(&wasm_encoder::Instruction::BrIf(1)); // break to outer block

            // 6. body_region
            emit_regions(body_region, blocks, locals, func_index_map, func);

            // 7. br $loop (depth 0 = inner loop)
            func.instruction(&wasm_encoder::Instruction::Br(0));

            // end loop, end block
            func.instruction(&wasm_encoder::Instruction::End);
            func.instruction(&wasm_encoder::Instruction::End);

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
            vec![] // No return value for Unit functions
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

        let (locals, extra_locals) = collect_locals(bir_func);

        let local_types: Vec<(u32, ValType)> = if extra_locals > 0 {
            vec![(extra_locals, ValType::I32)]
        } else {
            vec![]
        };
        let mut func = Function::new(local_types);

        // Use CfgRegion tree for structured control flow emission
        emit_regions(
            &bir_func.body,
            &bir_func.blocks,
            &locals,
            &func_index_map,
            &mut func,
        );

        // For non-Unit functions, the last Return in the body leaves the value on stack
        // via the WASM return instruction. But we still need End.
        func.instruction(&wasm_encoder::Instruction::End);
        code.function(&func);

        // Only export main
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
        assert_eq!(
            compile_and_run("func main() -> i32 { return 42; }"),
            42
        );
    }

    #[test]
    fn compile_call() {
        assert_eq!(
            compile_and_run(
                "func add(a: i32, b: i32) -> i32 { return a + b; } func main() -> i32 { return add(3, 4); }"
            ),
            7
        );
    }

    #[test]
    fn compile_let_variable() {
        assert_eq!(
            compile_and_run("func main() -> i32 { let x: i32 = 10; return x + 1; }"),
            11
        );
    }

    // --- Phase 3 tests ---

    #[test]
    fn compile_if_else() {
        assert_eq!(
            compile_and_run(
                "func main() -> i32 { let x: i32 = if true { yield 1; } else { yield 2; }; return x; }"
            ),
            1
        );
    }

    #[test]
    fn compile_while() {
        assert_eq!(
            compile_and_run(
                "func main() -> i32 { var s: i32 = 0; var i: i32 = 0; while i < 3 { s = s + i; i = i + 1; }; return s; }"
            ),
            3
        );
    }

    #[test]
    fn compile_comparison() {
        assert_eq!(
            compile_and_run(
                "func main() -> i32 { let x: i32 = if 3 > 2 { yield 1; } else { yield 0; }; return x; }"
            ),
            1
        );
    }
}
