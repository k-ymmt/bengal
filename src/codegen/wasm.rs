use std::collections::HashMap;

use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, Module, TypeSection, ValType,
};

use crate::bir::instruction::*;
use crate::error::{BengalError, Result};

fn bir_type_to_val_type(ty: &BirType) -> Result<ValType> {
    match ty {
        BirType::I32 => Ok(ValType::I32),
        BirType::I64 => Ok(ValType::I64),
        BirType::F32 => Ok(ValType::F32),
        BirType::F64 => Ok(ValType::F64),
        _ => Err(BengalError::CodegenError {
            message: format!("unsupported type: {:?}", ty),
        }),
    }
}

fn collect_locals(func: &BirFunction) -> HashMap<Value, u32> {
    let mut locals = HashMap::new();
    for block in &func.blocks {
        for inst in &block.instructions {
            match inst {
                Instruction::Literal { result, .. } | Instruction::BinaryOp { result, .. } => {
                    locals.insert(*result, result.0);
                }
            }
        }
    }
    locals
}

fn emit_instruction(inst: &Instruction, locals: &HashMap<Value, u32>, func: &mut Function) {
    match inst {
        Instruction::Literal { result, value, .. } => {
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
    }
}

fn emit_terminator(term: &Terminator, locals: &HashMap<Value, u32>, func: &mut Function) {
    match term {
        Terminator::Return(value) => {
            func.instruction(&wasm_encoder::Instruction::LocalGet(locals[value]));
        }
    }
}

pub fn compile(bir_module: &BirModule) -> Result<Vec<u8>> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    let mut functions = FunctionSection::new();
    let mut exports = ExportSection::new();
    let mut code = CodeSection::new();

    for (i, bir_func) in bir_module.functions.iter().enumerate() {
        let return_type = bir_type_to_val_type(&bir_func.return_type)?;
        let params: Vec<ValType> = bir_func
            .params
            .iter()
            .map(|(_, ty)| bir_type_to_val_type(ty))
            .collect::<Result<_>>()?;

        types.ty().function(params, vec![return_type]);
        functions.function(i as u32);

        let locals = collect_locals(bir_func);
        let num_locals = locals.len() as u32;

        let local_types: Vec<(u32, ValType)> = if num_locals > 0 {
            vec![(num_locals, ValType::I32)]
        } else {
            vec![]
        };
        let mut func = Function::new(local_types);

        for block in &bir_func.blocks {
            for inst in &block.instructions {
                emit_instruction(inst, &locals, &mut func);
            }
            emit_terminator(&block.terminator, &locals, &mut func);
        }

        func.instruction(&wasm_encoder::Instruction::End);
        code.function(&func);

        exports.export(&bir_func.name, ExportKind::Func, i as u32);
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
    use crate::bir::lowering::lower;
    use crate::parser::ast::{BinOp, Expr};

    fn compile_and_run(expr: &Expr) -> i32 {
        let bir_module = lower(expr).unwrap();
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

    #[test]
    fn compile_literal() {
        let result = compile_and_run(&Expr::Number(42));
        assert_eq!(result, 42);
    }

    #[test]
    fn compile_binary_expr() {
        // 2 + 3 * 4 = 14
        let expr = Expr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(Expr::Number(2)),
            right: Box::new(Expr::BinaryOp {
                op: BinOp::Mul,
                left: Box::new(Expr::Number(3)),
                right: Box::new(Expr::Number(4)),
            }),
        };
        let result = compile_and_run(&expr);
        assert_eq!(result, 14);
    }
}
