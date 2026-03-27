use std::collections::{HashMap, HashSet};

use super::instruction::{BasicBlock, BirFunction, BirModule, BirType, Instruction, Terminator};

pub fn resolve_bir_type(ty: &BirType, subst: &HashMap<String, BirType>) -> BirType {
    match ty {
        BirType::TypeParam(name) => subst
            .get(name)
            .unwrap_or_else(|| panic!("unresolved TypeParam: {name}"))
            .clone(),
        BirType::Array { element, size } => BirType::Array {
            element: Box::new(resolve_bir_type(element, subst)),
            size: *size,
        },
        BirType::Struct { name, type_args } => BirType::Struct {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|t| resolve_bir_type(t, subst))
                .collect(),
        },
        other => other.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Instance {
    pub func_name: String,
    pub type_args: Vec<BirType>,
}

impl Instance {
    pub fn mangled_name(&self) -> String {
        crate::mangle::mangle_generic_suffix(&self.func_name, &self.type_args)
    }

    pub fn substitution_map(&self, type_params: &[String]) -> HashMap<String, BirType> {
        type_params
            .iter()
            .zip(&self.type_args)
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect()
    }
}

pub struct MonoCollectResult {
    pub func_instances: Vec<Instance>,
    pub struct_instances: HashSet<(String, Vec<BirType>)>,
}

// ---------------------------------------------------------------------------
// mono_collect implementation
// ---------------------------------------------------------------------------

pub fn mono_collect(bir: &BirModule, _entry: &str) -> MonoCollectResult {
    // Build function lookup map.
    let func_map: HashMap<&str, &BirFunction> =
        bir.functions.iter().map(|f| (f.name.as_str(), f)).collect();

    let mut func_instances: Vec<Instance> = Vec::new();
    let mut seen_func: HashSet<Instance> = HashSet::new();
    let mut struct_instances: HashSet<(String, Vec<BirType>)> = HashSet::new();
    let mut worklist: Vec<Instance> = Vec::new();

    // Seed: scan all non-generic functions for calls with type_args.
    let empty_subst: HashMap<String, BirType> = HashMap::new();
    for func in &bir.functions {
        if !func.type_params.is_empty() {
            continue;
        }
        scan_function(
            func,
            &empty_subst,
            &mut worklist,
            &mut seen_func,
            &mut struct_instances,
        );
    }

    // Process worklist.
    while let Some(inst) = worklist.pop() {
        if let Some(func) = func_map.get(inst.func_name.as_str()) {
            let subst = inst.substitution_map(&func.type_params);
            scan_function(
                func,
                &subst,
                &mut worklist,
                &mut seen_func,
                &mut struct_instances,
            );
        }
        func_instances.push(inst);
    }

    MonoCollectResult {
        func_instances,
        struct_instances,
    }
}

fn scan_function(
    func: &BirFunction,
    subst: &HashMap<String, BirType>,
    worklist: &mut Vec<Instance>,
    seen_func: &mut HashSet<Instance>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    // Scan parameter types and return type.
    for (_, ty) in &func.params {
        let resolved = resolve_bir_type_lenient(ty, subst);
        collect_struct_instances(&resolved, struct_instances);
    }
    let resolved_ret = resolve_bir_type_lenient(&func.return_type, subst);
    collect_struct_instances(&resolved_ret, struct_instances);

    // Scan all basic blocks.
    for block in &func.blocks {
        scan_block(block, subst, worklist, seen_func, struct_instances);
    }
}

fn scan_block(
    block: &BasicBlock,
    subst: &HashMap<String, BirType>,
    worklist: &mut Vec<Instance>,
    seen_func: &mut HashSet<Instance>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    // Block params.
    for (_, ty) in &block.params {
        let resolved = resolve_bir_type_lenient(ty, subst);
        collect_struct_instances(&resolved, struct_instances);
    }

    for inst in &block.instructions {
        scan_instruction(inst, subst, worklist, seen_func, struct_instances);
    }

    scan_terminator(&block.terminator, subst, struct_instances);
}

fn scan_instruction(
    inst: &Instruction,
    subst: &HashMap<String, BirType>,
    worklist: &mut Vec<Instance>,
    seen_func: &mut HashSet<Instance>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    // Collect struct instances from all BirType fields.
    for ty in instruction_types(inst) {
        let resolved = resolve_bir_type_lenient(ty, subst);
        collect_struct_instances(&resolved, struct_instances);
    }

    // Handle generic calls.
    if let Instruction::Call {
        func_name,
        type_args,
        ..
    } = inst
        && !type_args.is_empty()
    {
        let resolved_args: Vec<BirType> = type_args
            .iter()
            .map(|t| resolve_bir_type_lenient(t, subst))
            .collect();
        let new_inst = Instance {
            func_name: func_name.clone(),
            type_args: resolved_args,
        };
        if !seen_func.contains(&new_inst) {
            seen_func.insert(new_inst.clone());
            worklist.push(new_inst);
        }
    }
}

fn instruction_types(inst: &Instruction) -> Vec<&BirType> {
    match inst {
        Instruction::Literal { ty, .. } => vec![ty],
        Instruction::BinaryOp { ty, .. } => vec![ty],
        Instruction::Call { ty, type_args, .. } => {
            let mut v: Vec<&BirType> = type_args.iter().collect();
            v.push(ty);
            v
        }
        Instruction::Compare { ty, .. } => vec![ty],
        Instruction::Not { .. } => vec![],
        Instruction::Cast { from_ty, to_ty, .. } => vec![from_ty, to_ty],
        Instruction::StructInit { ty, type_args, .. } => {
            let mut v: Vec<&BirType> = type_args.iter().collect();
            v.push(ty);
            v
        }
        Instruction::FieldGet { object_ty, ty, .. } => vec![object_ty, ty],
        Instruction::FieldSet { ty, .. } => vec![ty],
        Instruction::ArrayInit { ty, .. } => vec![ty],
        Instruction::ArrayGet { ty, .. } => vec![ty],
        Instruction::ArraySet { ty, .. } => vec![ty],
    }
}

fn scan_terminator(
    term: &Terminator,
    subst: &HashMap<String, BirType>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    match term {
        Terminator::Br { args, .. } => {
            for (_, ty) in args {
                let resolved = resolve_bir_type_lenient(ty, subst);
                collect_struct_instances(&resolved, struct_instances);
            }
        }
        Terminator::BrBreak { args, value, .. } => {
            for (_, ty) in args {
                let resolved = resolve_bir_type_lenient(ty, subst);
                collect_struct_instances(&resolved, struct_instances);
            }
            if let Some((_, ty)) = value {
                let resolved = resolve_bir_type_lenient(ty, subst);
                collect_struct_instances(&resolved, struct_instances);
            }
        }
        Terminator::BrContinue { args, .. } => {
            for (_, ty) in args {
                let resolved = resolve_bir_type_lenient(ty, subst);
                collect_struct_instances(&resolved, struct_instances);
            }
        }
        Terminator::Return(_) | Terminator::ReturnVoid | Terminator::CondBr { .. } => {}
    }
}

fn collect_struct_instances(ty: &BirType, set: &mut HashSet<(String, Vec<BirType>)>) {
    match ty {
        BirType::Struct { name, type_args } if !type_args.is_empty() => {
            set.insert((name.clone(), type_args.clone()));
            // Recurse into type_args.
            for arg in type_args {
                collect_struct_instances(arg, set);
            }
        }
        BirType::Array { element, .. } => {
            collect_struct_instances(element, set);
        }
        _ => {}
    }
}

/// Like `resolve_bir_type` but passes through TypeParams that are not in subst
/// (used when scanning non-generic callers where subst is empty and types are concrete).
pub fn resolve_bir_type_lenient(ty: &BirType, subst: &HashMap<String, BirType>) -> BirType {
    match ty {
        BirType::TypeParam(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        BirType::Array { element, size } => BirType::Array {
            element: Box::new(resolve_bir_type_lenient(element, subst)),
            size: *size,
        },
        BirType::Struct { name, type_args } => BirType::Struct {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|t| resolve_bir_type_lenient(t, subst))
                .collect(),
        },
        other => other.clone(),
    }
}

#[cfg(test)]
#[path = "mono_tests.rs"]
mod tests;
