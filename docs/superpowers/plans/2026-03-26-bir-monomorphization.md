# BIR-Level Monomorphization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate monomorphization from AST level to BIR level, keeping BIR generic and performing type substitution on-the-fly during codegen (Rust MIR style).

**Architecture:** Generic functions are lowered to BIR with `BirType::TypeParam`. A mono collector discovers required concrete instances by scanning BIR. Codegen substitutes type parameters per-instruction while generating LLVM IR — no BIR cloning. Per-module compilation is preserved using `linkonce_odr` linkage for cross-module generic instantiations.

**Tech Stack:** Rust, inkwell (LLVM bindings), existing Bengal compiler pipeline

**Spec:** `docs/superpowers/specs/2026-03-26-bir-monomorphization-design.md`

---

## File Structure

### New Files
- `src/bir/mono.rs` — Mono collector (`Instance`, `MonoCollectResult`, `mono_collect`, `resolve_bir_type`)

### Modified Files
- `src/bir/instruction.rs` — `BirType` (add `TypeParam`, change `Struct`), `BirFunction` (add `type_params`), `Call`/`StructInit` (add `type_args`), `BirModule` (add `conformance_map`)
- `src/bir/lowering.rs` — Generic-aware lowering (`TypeParam` handling, protocol method calls, conformance map population)
- `src/bir/printer.rs` — Print `TypeParam`, `type_args`, new `Struct` format, `type_params`
- `src/bir/optimize.rs` — Update `Struct` pattern matches
- `src/bir/mod.rs` — Re-export `mono` module
- `src/codegen/llvm.rs` — On-the-fly substitution, `linkonce_odr`, protocol method resolution, generic struct layout resolution
- `src/semantic/mod.rs` — Unified analysis (extend `analyze_pre_mono` to produce `SemanticInfo`)
- `src/lib.rs` — Pipeline rewiring (remove AST mono, integrate BIR mono)
- `src/monomorphize.rs` — Deleted in Phase 6

### Test Files
- `tests/generics.rs` — Existing tests (must all pass at every phase)
- `tests/common/mod.rs` — Test helpers (`compile_and_run`, `compile_should_fail`) — must be updated to new pipeline in Phase 6
- `tests/bir_mono.rs` — New: BIR mono-specific unit tests

---

## Phase 1: BIR Data Structure Changes

### Task 1: Add `BirType::TypeParam` and change `Struct` variant

**Files:**
- Modify: `src/bir/instruction.rs:6-16`

- [ ] **Step 1: Update `BirType` enum**

In `src/bir/instruction.rs`, change the derive and enum definition:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BirType {
    Unit,
    I32,
    I64,
    F32,
    F64,
    Bool,
    Struct {
        name: String,
        type_args: Vec<BirType>,
    },
    Array {
        element: Box<BirType>,
        size: u64,
    },
    TypeParam(String),
}
```

- [ ] **Step 2: Add helper constructor for non-generic struct**

Below the `BirType` enum, add:

```rust
impl BirType {
    /// Create a non-generic struct type (convenience for migration).
    pub fn struct_simple(name: String) -> Self {
        BirType::Struct {
            name,
            type_args: vec![],
        }
    }
}
```

- [ ] **Step 3: Fix all compilation errors from `Struct` variant change**

Every `BirType::Struct(name)` pattern in the codebase must change to `BirType::Struct { name, .. }` (for reads) or `BirType::struct_simple(name)` (for construction). Files affected:

- `src/bir/lowering.rs` — `semantic_type_to_bir` (line 1796), `convert_type_with_structs` (line 97), receiver init (line 215), field access (lines 232, 276, 284, 295), method call (line 1143), field set (line 1118), acyclic check (line 1835)
- `src/bir/printer.rs` — `format_type` (line 11), **plus inline tests**: `format_type_struct` (line ~401) and `print_struct_instructions` (line ~438) which construct `BirType::Struct("...".to_string())` directly — update to `BirType::struct_simple("...".to_string())`
- `src/bir/optimize.rs` — any `Struct` matches
- `src/codegen/llvm.rs` — `bir_type_to_llvm_type` (line 38), `build_struct_types` (line 827), StructInit emission (line 415), FieldGet/FieldSet emission (line ~453)

For pattern matches reading the name:
```rust
// Before:
BirType::Struct(name) => ...
// After:
BirType::Struct { name, .. } => ...
```

For construction:
```rust
// Before:
BirType::Struct(name.clone())
// After:
BirType::struct_simple(name.clone())
```

- [ ] **Step 4: Run `cargo test` to verify all existing tests pass**

Run: `cargo test`
Expected: All tests pass. No behavior change.

- [ ] **Step 5: Run `cargo fmt` and `cargo clippy`**

Run: `cargo fmt && cargo clippy`
Expected: No warnings.

- [ ] **Step 6: Commit**

```
git add src/bir/instruction.rs src/bir/lowering.rs src/bir/printer.rs src/bir/optimize.rs src/codegen/llvm.rs
git commit -m "Add BirType::TypeParam variant and change Struct to named fields"
```

### Task 2: Add `type_params` to `BirFunction` and `type_args` to `Call`/`StructInit`

**Files:**
- Modify: `src/bir/instruction.rs:53-58,77-82,197-203`

- [ ] **Step 1: Add `type_args` to `Call` instruction**

```rust
Call {
    result: Value,
    func_name: String,
    args: Vec<Value>,
    type_args: Vec<BirType>,
    ty: BirType,
},
```

- [ ] **Step 2: Add `type_args` to `StructInit` instruction**

```rust
StructInit {
    result: Value,
    struct_name: String,
    fields: Vec<(String, Value)>,
    type_args: Vec<BirType>,
    ty: BirType,
},
```

- [ ] **Step 3: Add `type_params` to `BirFunction`**

```rust
pub struct BirFunction {
    pub name: String,
    pub type_params: Vec<String>,
    pub params: Vec<(Value, BirType)>,
    pub return_type: BirType,
    pub blocks: Vec<BasicBlock>,
    pub body: Vec<CfgRegion>,
}
```

- [ ] **Step 4: Add `conformance_map` to `BirModule`**

```rust
pub struct BirModule {
    pub struct_layouts: HashMap<String, Vec<(String, BirType)>>,
    pub functions: Vec<BirFunction>,
    pub conformance_map: HashMap<(String, BirType), String>,
}
```

- [ ] **Step 5: Fix all compilation errors**

Every construction of `Call`, `StructInit`, `BirFunction`, and `BirModule` needs the new fields with default values:

- `Call` constructions in `lowering.rs`: add `type_args: vec![]`
- `StructInit` constructions in `lowering.rs`: add `type_args: vec![]`
- `BirFunction` constructions in `lowering.rs` (`lower_program`, `lower_module`): add `type_params: vec![]`
- `BirModule` constructions in `lowering.rs`: add `conformance_map: HashMap::new()`
- Pattern matches on `Call` in `codegen/llvm.rs`, `printer.rs`, `lib.rs`: add `type_args, ..` or `type_args: _`

- [ ] **Step 6: Run `cargo test`**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 7: Run `cargo fmt` and `cargo clippy`**

Run: `cargo fmt && cargo clippy`
Expected: No warnings.

- [ ] **Step 8: Commit**

```
git add src/bir/ src/codegen/ src/lib.rs
git commit -m "Add type_params, type_args, and conformance_map to BIR data structures"
```

### Task 3: Update BIR printer for new fields

**Files:**
- Modify: `src/bir/printer.rs`

- [ ] **Step 1: Update `format_type` for `TypeParam` and new `Struct`**

```rust
BirType::Struct { name, type_args } => {
    if type_args.is_empty() {
        name.clone()
    } else {
        let args: Vec<String> = type_args.iter().map(format_type).collect();
        format!("{}<{}>", name, args.join(", "))
    }
}
BirType::TypeParam(name) => name.clone(),
```

- [ ] **Step 2: Update `Call` printing to show `type_args`**

After the function name and args, if `type_args` is non-empty, append:
```
type_args=[I32, Bool]
```

- [ ] **Step 3: Update `StructInit` printing to show `type_args`**

Same pattern as Call.

- [ ] **Step 4: Update `BirFunction` printing to show `type_params`**

Print generic params in the function header:
```
@identity<T>(%0: T) -> T {
```

- [ ] **Step 5: Run `cargo test`**

Run: `cargo test`
Expected: All tests pass (BIR output format changed but tests compare execution results, not BIR text).

- [ ] **Step 6: Run `cargo fmt` and `cargo clippy`, then commit**

```
git add src/bir/printer.rs
git commit -m "Update BIR printer for TypeParam, type_args, and generic functions"
```

---

## Phase 2: Generic BIR Lowering

### Task 4: Add `semantic_type_to_bir` support for `TypeParam`

**Files:**
- Modify: `src/bir/lowering.rs:1788-1813`

- [ ] **Step 1: Write a test for TypeParam conversion**

In `tests/bir_mono.rs` (new file):

```rust
mod common;
use bengal::bir;

#[test]
fn bir_typeparam_in_identity() {
    let source = "func identity<T>(value: T) -> T { return value; }
                  func main() -> Int32 { return identity<Int32>(42); }";
    // Use the existing pipeline (which monomorphizes first) to verify compilation.
    // The BIR mono lowering test helper will be added in subsequent steps.
    let result = common::compile_and_run(source);
    assert_eq!(result, 42);
}
```

- [ ] **Step 2: Run test to verify it passes (baseline)**

Run: `cargo test bir_typeparam_in_identity`
Expected: PASS (using existing AST mono pipeline).

- [ ] **Step 3: Add `TypeParam` handling to `semantic_type_to_bir`**

In `src/bir/lowering.rs`, in the `semantic_type_to_bir` function, add:

```rust
crate::semantic::types::Type::TypeParam { name, .. } => BirType::TypeParam(name.clone()),
```

- [ ] **Step 4: Add `TypeParam` handling to `convert_type` and `convert_type_with_structs`**

For `TypeAnnotation` with a name matching a type parameter, return `BirType::TypeParam(name)`.

- [ ] **Step 5: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass.

```
git add src/bir/lowering.rs tests/bir_mono.rs
git commit -m "Add TypeParam support to semantic_type_to_bir and convert_type"
```

### Task 5: Lower generic functions to BIR with `type_params`

**Files:**
- Modify: `src/bir/lowering.rs` (lower_function, lower_program, lower_module)

- [ ] **Step 1: Create a test helper for generic BIR lowering**

In `tests/bir_mono.rs`, add a helper that parses + analyzes (pre-mono only) + lowers to BIR **without** monomorphizing:

```rust
fn lower_generic_bir(source: &str) -> bir::instruction::BirModule {
    // 1. Parse
    // 2. validate_generics
    // 3. analyze_pre_mono -> get SemanticInfo
    // 4. lower_program (generic-aware) -> BirModule
    // NOTE: This requires SemanticInfo from pre-mono, which is Phase 5.
    // For now, we test via the full pipeline and inspect BIR properties.
    todo!("Will be completed when unified analysis is available in Phase 5")
}
```

Since Phase 2 tests must work with the existing AST-mono pipeline, we validate generic lowering indirectly: add a `lower_generic_function` helper in `lowering.rs` that can lower a single generic function to BIR given semantic info, and unit-test it.

- [ ] **Step 2: Set `type_params` on BirFunction for generic functions**

In the lowering function builder (where `BirFunction` is constructed), populate `type_params` from the AST function's `type_params` field:

```rust
BirFunction {
    name: func_name,
    type_params: func.type_params.iter().map(|tp| tp.name.clone()).collect(),
    params: ...,
    // ...
}
```

Currently, in the AST-mono pipeline, all functions are non-generic after monomorphization so `type_params` is always `[]`. This change is safe.

- [ ] **Step 3: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass.

```
git add src/bir/lowering.rs
git commit -m "Populate BirFunction.type_params from AST function type parameters"
```

### Task 6: Set `type_args` on `Call` and `StructInit` during lowering

**Files:**
- Modify: `src/bir/lowering.rs`

- [ ] **Step 1: Pass `type_args` through for `Call` instructions**

In `ExprKind::FuncCall` lowering, extract type arguments from the AST node and convert them to `Vec<BirType>`. For the current AST-mono pipeline, these are always empty (generics already resolved), but the plumbing is needed for Phase 5+.

```rust
// In ExprKind::FuncCall handling:
let type_args: Vec<BirType> = ast_call.type_args
    .iter()
    .map(|ta| self.convert_type_annotation(ta))
    .collect();

self.emit(Instruction::Call {
    result,
    func_name: resolved,
    args: call_args,
    type_args,
    ty: ret_ty.clone(),
});
```

- [ ] **Step 2: Pass `type_args` through for `StructInit` instructions**

Same pattern for struct initializations with type arguments.

- [ ] **Step 3: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass (type_args always empty in current pipeline).

```
git add src/bir/lowering.rs
git commit -m "Pass type_args through Call and StructInit during BIR lowering"
```

### Task 7: Extend MethodCall lowering for `TypeParam` receivers (stub)

> **Note:** This task adds the code structure only. The protocol lookup helpers
> (`lookup_type_param_constraint`, `lookup_protocol_method_return_type`) require
> `SemanticInfo` to include protocol/constraint data, which is added in Phase 5
> Task 13a. The `todo!()` branches are completed in Phase 5 Task 13b.

**Files:**
- Modify: `src/bir/lowering.rs:1136-1166`

- [ ] **Step 1: Add TypeParam branch to MethodCall lowering**

In the `ExprKind::MethodCall` match, after the existing `BirType::Struct { name, .. }` branch, add a `BirType::TypeParam` branch with `todo!()`:

```rust
Some(BirType::TypeParam(_type_param_name)) => {
    // Protocol method call on constrained type parameter.
    // Completed in Phase 5 Task 13b when SemanticInfo has protocol data.
    todo!("TypeParam method call lowering requires protocol info in SemanticInfo")
}
```

This branch is unreachable in the current AST-mono pipeline (all TypeParams are resolved before lowering).

- [ ] **Step 2: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass (TypeParam branch not hit).

```
git add src/bir/lowering.rs
git commit -m "Add TypeParam branch to MethodCall lowering (stub for Phase 5)"
```

### Task 8: Populate conformance map during lowering (stub)

> **Note:** Full conformance map population requires `SemanticInfo` to include
> protocol definitions and struct conformance data. This is added in Phase 5
> Task 13a. For now, this task adds a stub that is completed in Phase 5 Task 13b.

**Files:**
- Modify: `src/bir/lowering.rs`

- [ ] **Step 1: Add empty conformance_map to BirModule construction**

In both `lower_program` and `lower_module`, pass an empty map:

```rust
BirModule {
    struct_layouts,
    functions,
    conformance_map: HashMap::new(),  // Populated in Phase 5
}
```

- [ ] **Step 2: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass.

```
git add src/bir/lowering.rs
git commit -m "Add empty conformance_map to BirModule (stub for Phase 5)"
```

---

## Phase 3: Codegen On-The-Fly Substitution

### Task 9: Implement `resolve_bir_type` utility

**Files:**
- Create: `src/bir/mono.rs`
- Modify: `src/bir/mod.rs`

- [ ] **Step 1: Write tests for `resolve_bir_type`**

In `tests/bir_mono.rs`:

```rust
use bengal::bir::mono::resolve_bir_type;
use bengal::bir::instruction::BirType;
use std::collections::HashMap;

#[test]
fn resolve_type_param() {
    let subst: HashMap<String, BirType> = [("T".into(), BirType::I32)].into();
    assert_eq!(
        resolve_bir_type(&BirType::TypeParam("T".into()), &subst),
        BirType::I32,
    );
}

#[test]
fn resolve_nested_array() {
    let subst: HashMap<String, BirType> = [("T".into(), BirType::Bool)].into();
    let input = BirType::Array {
        element: Box::new(BirType::TypeParam("T".into())),
        size: 3,
    };
    let expected = BirType::Array {
        element: Box::new(BirType::Bool),
        size: 3,
    };
    assert_eq!(resolve_bir_type(&input, &subst), expected);
}

#[test]
fn resolve_generic_struct() {
    let subst: HashMap<String, BirType> = [
        ("T".into(), BirType::I32),
        ("U".into(), BirType::Bool),
    ].into();
    let input = BirType::Struct {
        name: "Pair".into(),
        type_args: vec![BirType::TypeParam("T".into()), BirType::TypeParam("U".into())],
    };
    let expected = BirType::Struct {
        name: "Pair".into(),
        type_args: vec![BirType::I32, BirType::Bool],
    };
    assert_eq!(resolve_bir_type(&input, &subst), expected);
}

#[test]
fn resolve_concrete_type_passthrough() {
    let subst: HashMap<String, BirType> = HashMap::new();
    assert_eq!(resolve_bir_type(&BirType::I32, &subst), BirType::I32);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test resolve_type_param`
Expected: FAIL (module `mono` not found).

- [ ] **Step 3: Implement `resolve_bir_type` in `src/bir/mono.rs`**

```rust
use std::collections::HashMap;
use super::instruction::BirType;

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
            type_args: type_args.iter().map(|t| resolve_bir_type(t, subst)).collect(),
        },
        other => other.clone(),
    }
}
```

- [ ] **Step 4: Add `pub mod mono;` to `src/bir/mod.rs`**

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test resolve_`
Expected: All 4 tests PASS.

- [ ] **Step 6: Run `cargo fmt` and `cargo clippy`, then commit**

```
git add src/bir/mono.rs src/bir/mod.rs tests/bir_mono.rs
git commit -m "Implement resolve_bir_type utility for BIR type substitution"
```

### Task 10: Implement `Instance` type and name mangling

**Files:**
- Modify: `src/bir/mono.rs`

- [ ] **Step 1: Write test for name mangling**

In `tests/bir_mono.rs`:

```rust
use bengal::bir::mono::Instance;

#[test]
fn instance_mangle_name() {
    let inst = Instance {
        func_name: "identity".into(),
        type_args: vec![BirType::I32],
    };
    assert_eq!(inst.mangled_name(), "identity_Int32");
}

#[test]
fn instance_mangle_multi_args() {
    let inst = Instance {
        func_name: "swap".into(),
        type_args: vec![BirType::I32, BirType::Bool],
    };
    assert_eq!(inst.mangled_name(), "swap_Int32_Bool");
}

#[test]
fn instance_mangle_generic_struct() {
    let inst = Instance {
        func_name: "getFirst".into(),
        type_args: vec![
            BirType::Struct { name: "Point".into(), type_args: vec![] },
            BirType::I32,
        ],
    };
    assert_eq!(inst.mangled_name(), "getFirst_Point_Int32");
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement `Instance` and `mangled_name`**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Instance {
    pub func_name: String,
    pub type_args: Vec<BirType>,
}

impl Instance {
    pub fn mangled_name(&self) -> String {
        if self.type_args.is_empty() {
            self.func_name.clone()
        } else {
            let args: Vec<String> = self.type_args.iter().map(mangle_bir_type).collect();
            format!("{}_{}", self.func_name, args.join("_"))
        }
    }

    pub fn substitution_map(&self, type_params: &[String]) -> HashMap<String, BirType> {
        type_params
            .iter()
            .zip(&self.type_args)
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect()
    }
}

fn mangle_bir_type(ty: &BirType) -> String {
    match ty {
        BirType::I32 => "Int32".into(),
        BirType::I64 => "Int64".into(),
        BirType::F32 => "Float32".into(),
        BirType::F64 => "Float64".into(),
        BirType::Bool => "Bool".into(),
        BirType::Unit => "Unit".into(),
        BirType::Struct { name, type_args } => {
            if type_args.is_empty() {
                name.clone()
            } else {
                let args: Vec<String> = type_args.iter().map(mangle_bir_type).collect();
                format!("{}_{}", name, args.join("_"))
            }
        }
        BirType::Array { element, size } => {
            format!("Array_{}_{}", mangle_bir_type(element), size)
        }
        BirType::TypeParam(name) => panic!("cannot mangle unresolved TypeParam: {name}"),
    }
}
```

- [ ] **Step 4: Add `MonoCollectResult`**

```rust
use std::collections::HashSet;

pub struct MonoCollectResult {
    pub func_instances: Vec<Instance>,
    pub struct_instances: HashSet<(String, Vec<BirType>)>,
}
```

- [ ] **Step 5: Run tests, commit**

Run: `cargo test instance_mangle`
Expected: All PASS.

```
git add src/bir/mono.rs tests/bir_mono.rs
git commit -m "Implement Instance type with name mangling and substitution map"
```

### Task 11: Add `TypeParam` handling to `bir_type_to_llvm_type`

**Files:**
- Modify: `src/codegen/llvm.rs:26-44`

- [ ] **Step 1: Update `bir_type_to_llvm_type` to panic on unresolved TypeParam**

```rust
BirType::TypeParam(name) => panic!("unresolved TypeParam '{name}' in codegen — substitution missed"),
```

This ensures any missed substitution is caught immediately rather than producing silent incorrect code.

- [ ] **Step 2: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass (no TypeParam reaches codegen in current pipeline).

```
git add src/codegen/llvm.rs
git commit -m "Add TypeParam panic guard to bir_type_to_llvm_type"
```

---

## Phase 4: Mono Collector

### Task 12: Implement `mono_collect`

**Files:**
- Modify: `src/bir/mono.rs`

- [ ] **Step 1: Write tests for mono collector**

In `tests/bir_mono.rs`:

```rust
use bengal::bir::mono::{mono_collect, Instance, MonoCollectResult};
use bengal::bir::instruction::*;
use std::collections::HashMap;

fn make_identity_bir() -> BirModule {
    // Build a minimal BirModule with:
    // @identity<T>(%0: TypeParam("T")) -> TypeParam("T") { return %0 }
    // @main() -> I32 { %0 = literal 42; %1 = call @identity(%0) type_args=[I32]; return %1 }
    let identity = BirFunction {
        name: "identity".into(),
        type_params: vec!["T".into()],
        params: vec![(Value(0), BirType::TypeParam("T".into()))],
        return_type: BirType::TypeParam("T".into()),
        blocks: vec![BasicBlock {
            label: 0,
            params: vec![],
            instructions: vec![],
            terminator: Terminator::Return(Value(0)),
        }],
        body: vec![CfgRegion::Block(0)],
    };
    let main_fn = BirFunction {
        name: "main".into(),
        type_params: vec![],
        params: vec![],
        return_type: BirType::I32,
        blocks: vec![BasicBlock {
            label: 0,
            params: vec![],
            instructions: vec![
                Instruction::Literal { result: Value(0), value: 42, ty: BirType::I32 },
                Instruction::Call {
                    result: Value(1),
                    func_name: "identity".into(),
                    args: vec![Value(0)],
                    type_args: vec![BirType::I32],
                    ty: BirType::I32,
                },
            ],
            terminator: Terminator::Return(Value(1)),
        }],
        body: vec![CfgRegion::Block(0)],
    };
    BirModule {
        struct_layouts: HashMap::new(),
        functions: vec![identity, main_fn],
        conformance_map: HashMap::new(),
    }
}

#[test]
fn mono_collect_identity() {
    let bir = make_identity_bir();
    let result = mono_collect(&bir, "main");
    assert_eq!(result.func_instances.len(), 1);
    assert_eq!(result.func_instances[0].func_name, "identity");
    assert_eq!(result.func_instances[0].type_args, vec![BirType::I32]);
    assert!(result.struct_instances.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test mono_collect_identity`
Expected: FAIL (function not found).

- [ ] **Step 3: Implement `mono_collect`**

```rust
pub fn mono_collect(bir: &BirModule, entry: &str) -> MonoCollectResult {
    let func_map: HashMap<&str, &BirFunction> =
        bir.functions.iter().map(|f| (f.name.as_str(), f)).collect();

    let mut worklist: Vec<Instance> = Vec::new();
    let mut seen_funcs: HashSet<Instance> = HashSet::new();
    let mut struct_instances: HashSet<(String, Vec<BirType>)> = HashSet::new();

    // Seed with non-generic entry points
    for func in &bir.functions {
        if func.type_params.is_empty() {
            // Non-generic functions are implicitly instances with no type args
            // Scan them for generic calls
            scan_function(func, &HashMap::new(), &func_map, &mut worklist, &mut seen_funcs, &mut struct_instances);
        }
    }

    // Process worklist
    while let Some(instance) = worklist.pop() {
        if let Some(func) = func_map.get(instance.func_name.as_str()) {
            let subst = instance.substitution_map(&func.type_params);
            scan_function(func, &subst, &func_map, &mut worklist, &mut seen_funcs, &mut struct_instances);
        }
    }

    let func_instances: Vec<Instance> = seen_funcs.into_iter().collect();
    MonoCollectResult { func_instances, struct_instances }
}

fn scan_function(
    func: &BirFunction,
    subst: &HashMap<String, BirType>,
    func_map: &HashMap<&str, &BirFunction>,
    worklist: &mut Vec<Instance>,
    seen_funcs: &mut HashSet<Instance>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    // Scan function signature
    for (_, ty) in &func.params {
        collect_struct_instances(&resolve_bir_type(ty, subst), struct_instances);
    }
    collect_struct_instances(&resolve_bir_type(&func.return_type, subst), struct_instances);

    // Scan all blocks
    for block in &func.blocks {
        for (_, ty) in &block.params {
            collect_struct_instances(&resolve_bir_type(ty, subst), struct_instances);
        }
        for inst in &block.instructions {
            scan_instruction(inst, subst, worklist, seen_funcs, struct_instances);
        }
        scan_terminator(&block.terminator, subst, struct_instances);
    }
}

fn scan_instruction(
    inst: &Instruction,
    subst: &HashMap<String, BirType>,
    worklist: &mut Vec<Instance>,
    seen_funcs: &mut HashSet<Instance>,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    // Extract all BirType fields and collect struct instances
    for ty in instruction_types(inst) {
        collect_struct_instances(&resolve_bir_type(ty, subst), struct_instances);
    }
    // Check for generic Call instructions
    if let Instruction::Call { func_name, type_args, .. } = inst {
        if !type_args.is_empty() {
            let resolved_args: Vec<BirType> = type_args.iter()
                .map(|t| resolve_bir_type(t, subst))
                .collect();
            let instance = Instance {
                func_name: func_name.clone(),
                type_args: resolved_args,
            };
            if seen_funcs.insert(instance.clone()) {
                worklist.push(instance);
            }
        }
    }
}

/// Extract all BirType references from an instruction.
fn instruction_types(inst: &Instruction) -> Vec<&BirType> {
    match inst {
        Instruction::Literal { ty, .. } => vec![ty],
        Instruction::BinaryOp { ty, .. } => vec![ty],
        Instruction::Call { ty, type_args, .. } => {
            let mut types = vec![ty];
            types.extend(type_args.iter());
            types
        }
        Instruction::Compare { ty, .. } => vec![ty],
        Instruction::Not { .. } => vec![],
        Instruction::Cast { from_ty, to_ty, .. } => vec![from_ty, to_ty],
        Instruction::StructInit { ty, type_args, .. } => {
            let mut types = vec![ty];
            types.extend(type_args.iter());
            types
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
                collect_struct_instances(&resolve_bir_type(ty, subst), struct_instances);
            }
        }
        Terminator::BrBreak { args, value, .. } => {
            for (_, ty) in args {
                collect_struct_instances(&resolve_bir_type(ty, subst), struct_instances);
            }
            if let Some((_, ty)) = value {
                collect_struct_instances(&resolve_bir_type(ty, subst), struct_instances);
            }
        }
        Terminator::BrContinue { args, .. } => {
            for (_, ty) in args {
                collect_struct_instances(&resolve_bir_type(ty, subst), struct_instances);
            }
        }
        _ => {}
    }
}

fn collect_struct_instances(
    ty: &BirType,
    struct_instances: &mut HashSet<(String, Vec<BirType>)>,
) {
    match ty {
        BirType::Struct { name, type_args } if !type_args.is_empty() => {
            struct_instances.insert((name.clone(), type_args.clone()));
        }
        BirType::Array { element, .. } => collect_struct_instances(element, struct_instances),
        _ => {}
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test mono_collect_identity`
Expected: PASS.

- [ ] **Step 5: Add test for generic struct instance collection**

```rust
#[test]
fn mono_collect_generic_struct() {
    // Build BIR for: func idPair<T, U>(p: Pair<T, U>) -> Pair<T, U> { return p; }
    // main calls idPair<Int32, Bool>(...)
    let id_pair = BirFunction {
        name: "idPair".into(),
        type_params: vec!["T".into(), "U".into()],
        params: vec![(Value(0), BirType::Struct {
            name: "Pair".into(),
            type_args: vec![BirType::TypeParam("T".into()), BirType::TypeParam("U".into())],
        })],
        return_type: BirType::Struct {
            name: "Pair".into(),
            type_args: vec![BirType::TypeParam("T".into()), BirType::TypeParam("U".into())],
        },
        blocks: vec![BasicBlock {
            label: 0, params: vec![], instructions: vec![],
            terminator: Terminator::Return(Value(0)),
        }],
        body: vec![CfgRegion::Block(0)],
    };
    let main_fn = BirFunction {
        name: "main".into(),
        type_params: vec![],
        params: vec![],
        return_type: BirType::I32,
        blocks: vec![BasicBlock {
            label: 0, params: vec![],
            instructions: vec![
                Instruction::StructInit {
                    result: Value(0),
                    struct_name: "Pair".into(),
                    fields: vec![("first".into(), Value(10)), ("second".into(), Value(11))],
                    type_args: vec![BirType::I32, BirType::Bool],
                    ty: BirType::Struct { name: "Pair".into(), type_args: vec![BirType::I32, BirType::Bool] },
                },
                Instruction::Call {
                    result: Value(1),
                    func_name: "idPair".into(),
                    args: vec![Value(0)],
                    type_args: vec![BirType::I32, BirType::Bool],
                    ty: BirType::Struct { name: "Pair".into(), type_args: vec![BirType::I32, BirType::Bool] },
                },
            ],
            terminator: Terminator::Return(Value(1)),
        }],
        body: vec![CfgRegion::Block(0)],
    };
    let bir = BirModule {
        struct_layouts: [("Pair".into(), vec![
            ("first".into(), BirType::TypeParam("T".into())),
            ("second".into(), BirType::TypeParam("U".into())),
        ])].into(),
        functions: vec![id_pair, main_fn],
        conformance_map: HashMap::new(),
    };
    let result = mono_collect(&bir, "main");
    assert_eq!(result.func_instances.len(), 1);
    assert!(result.struct_instances.contains(&("Pair".into(), vec![BirType::I32, BirType::Bool])));
}
```

- [ ] **Step 6: Add test for transitive generic call discovery**

```rust
#[test]
fn mono_collect_transitive() {
    // foo<T> calls bar<T>; main calls foo<Int32>
    // Should discover both Instance("foo", [I32]) and Instance("bar", [I32])
    let bar = BirFunction {
        name: "bar".into(),
        type_params: vec!["T".into()],
        params: vec![(Value(0), BirType::TypeParam("T".into()))],
        return_type: BirType::TypeParam("T".into()),
        blocks: vec![BasicBlock {
            label: 0, params: vec![], instructions: vec![],
            terminator: Terminator::Return(Value(0)),
        }],
        body: vec![CfgRegion::Block(0)],
    };
    let foo = BirFunction {
        name: "foo".into(),
        type_params: vec!["T".into()],
        params: vec![(Value(0), BirType::TypeParam("T".into()))],
        return_type: BirType::TypeParam("T".into()),
        blocks: vec![BasicBlock {
            label: 0, params: vec![],
            instructions: vec![Instruction::Call {
                result: Value(1),
                func_name: "bar".into(),
                args: vec![Value(0)],
                type_args: vec![BirType::TypeParam("T".into())],
                ty: BirType::TypeParam("T".into()),
            }],
            terminator: Terminator::Return(Value(1)),
        }],
        body: vec![CfgRegion::Block(0)],
    };
    let main_fn = BirFunction {
        name: "main".into(),
        type_params: vec![],
        params: vec![],
        return_type: BirType::I32,
        blocks: vec![BasicBlock {
            label: 0, params: vec![],
            instructions: vec![
                Instruction::Literal { result: Value(0), value: 42, ty: BirType::I32 },
                Instruction::Call {
                    result: Value(1),
                    func_name: "foo".into(),
                    args: vec![Value(0)],
                    type_args: vec![BirType::I32],
                    ty: BirType::I32,
                },
            ],
            terminator: Terminator::Return(Value(1)),
        }],
        body: vec![CfgRegion::Block(0)],
    };
    let bir = BirModule {
        struct_layouts: HashMap::new(),
        functions: vec![bar, foo, main_fn],
        conformance_map: HashMap::new(),
    };
    let result = mono_collect(&bir, "main");
    assert_eq!(result.func_instances.len(), 2);
    let names: std::collections::HashSet<String> = result.func_instances.iter()
        .map(|i| i.func_name.clone()).collect();
    assert!(names.contains("foo"));
    assert!(names.contains("bar"));
}
```

- [ ] **Step 7: Run all tests, then commit**

Run: `cargo test mono_collect`
Expected: All PASS.

```
git add src/bir/mono.rs tests/bir_mono.rs
git commit -m "Implement mono_collect for BIR-level instance discovery"
```

---

## Phase 5: Unified Analysis

### Task 13: Extend `analyze_pre_mono` to produce `SemanticInfo`

**Files:**
- Modify: `src/semantic/mod.rs`
- Modify: `src/semantic/resolver.rs`

- [ ] **Step 1: Identify what `SemanticInfo` fields are built only in post-mono**

Read `analyze_post_mono` (and `analyze_package`) to catalog all `SemanticInfo` fields: `struct_defs`, `struct_init_calls`, etc. Note that `SemanticInfo` currently only has `struct_defs: HashMap<String, StructInfo>` and `struct_init_calls`.

- [ ] **Step 2: Add protocol info and conformance data to `SemanticInfo`**

Extend `SemanticInfo` with:

```rust
pub struct SemanticInfo {
    pub struct_defs: HashMap<String, StructInfo>,
    pub struct_init_calls: Vec<...>,
    pub protocols: HashMap<String, ProtocolInfo>,       // NEW
    pub type_param_constraints: HashMap<String, String>, // NEW: type_param_name -> protocol_name
}
```

Add `conformances: Vec<String>` to `StructInfo` in `resolver.rs`:

```rust
pub struct StructInfo {
    // ... existing fields ...
    pub conformances: Vec<String>,  // NEW: protocol names this struct conforms to
}
```

- [ ] **Step 3: Build `SemanticInfo` in `analyze_pre_mono`**

Build `struct_defs`, `protocols`, and `conformances` from the pre-mono AST. For generic functions, the signatures contain `Type::TypeParam`. Return both `InferredTypeArgs` and `SemanticInfo` from `analyze_pre_mono`.

```rust
pub fn analyze_pre_mono(program: &Program) -> Result<(InferredTypeArgs, SemanticInfo)> {
    // ... existing type inference logic ...
    // ... add SemanticInfo construction ...
}
```

- [ ] **Step 4: Validate that pre-mono SemanticInfo matches post-mono**

For non-generic functions, the pre-mono `SemanticInfo` should match what post-mono would produce. Add a debug assertion that compares them when both pipelines are active.

- [ ] **Step 5: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass.

```
git add src/semantic/mod.rs src/semantic/resolver.rs
git commit -m "Extend analyze_pre_mono to produce SemanticInfo with protocol data"
```

### Task 13b: Complete Phase 2 stubs (protocol method lowering + conformance map)

> **This task completes the `todo!()` stubs from Task 7 and Task 8.**

**Files:**
- Modify: `src/bir/lowering.rs`

- [ ] **Step 1: Implement `lookup_type_param_constraint` and `lookup_protocol_method_return_type`**

These methods query the newly available `SemanticInfo.protocols` and
`SemanticInfo.type_param_constraints`:

```rust
fn lookup_type_param_constraint(&self, type_param_name: &str) -> Option<String> {
    self.sem_info.type_param_constraints.get(type_param_name).cloned()
}

fn lookup_protocol_method_return_type(&self, proto_name: &str, method: &str) -> BirType {
    let proto = self.sem_info.protocols.get(proto_name).expect("protocol not found");
    let method_sig = proto.methods.iter().find(|m| m.name == method).expect("method not found");
    semantic_type_to_bir(&method_sig.return_type)
}
```

- [ ] **Step 2: Replace `todo!()` in MethodCall TypeParam branch**

Replace the `todo!()` from Task 7 with the full implementation:

```rust
Some(BirType::TypeParam(type_param_name)) => {
    let proto_name = self.lookup_type_param_constraint(type_param_name)
        .expect("method call on unconstrained TypeParam");
    let proto_method_name = format!("{}_{}", proto_name, method);
    let ret_ty = self.lookup_protocol_method_return_type(&proto_name, method);
    let mut call_args = vec![obj_val];
    for arg in args {
        call_args.push(self.lower_expr(arg));
    }
    let result = self.fresh_value();
    self.emit(Instruction::Call {
        result,
        func_name: proto_method_name,
        args: call_args,
        type_args: vec![BirType::TypeParam(type_param_name.clone())],
        ty: ret_ty.clone(),
    });
    self.value_types.insert(result, ret_ty);
    result
}
```

- [ ] **Step 3: Populate conformance_map from SemanticInfo**

Replace the empty `HashMap::new()` from Task 8 with actual population:

```rust
let mut conformance_map: HashMap<(String, BirType), String> = HashMap::new();
for (struct_name, struct_info) in &sem_info.struct_defs {
    for proto_name in &struct_info.conformances {
        if let Some(proto_info) = sem_info.protocols.get(proto_name) {
            for method in &proto_info.methods {
                let key = (
                    format!("{}_{}", proto_name, method.name),
                    BirType::struct_simple(struct_name.clone()),
                );
                let impl_name = format!("{}_{}", struct_name, method.name);
                conformance_map.insert(key, impl_name);
            }
        }
    }
}
```

- [ ] **Step 4: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass.

```
git add src/bir/lowering.rs
git commit -m "Complete protocol method lowering and conformance map population"
```

### Task 14: Wire unified analysis into single-file pipeline

**Files:**
- Modify: `src/lib.rs:19-30`

- [ ] **Step 1: Update `compile_source` to use new `analyze_pre_mono` return**

```rust
pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let program = parser::parse(tokenizer::tokenize(source)?)?;
    semantic::validate_generics(&program)?;
    let (inferred, sem_info) = semantic::analyze_pre_mono(&program)?;
    let mono_program = monomorphize::monomorphize(&program, &inferred);
    // Still use AST mono for now — Phase 6 removes this
    let _ = semantic::analyze_post_mono(&mono_program)?;
    // Use pre-mono sem_info for lowering (test that it works)
    let mut bir_module = bir::lower_program(&mono_program, &sem_info)?;
    bir::optimize_module(&mut bir_module);
    codegen::compile(&bir_module)
}
```

- [ ] **Step 2: Run `cargo test`**

Run: `cargo test`
Expected: All tests pass (pre-mono `sem_info` produces same results for concrete code).

- [ ] **Step 3: Remove `analyze_post_mono` call**

Once validated, remove the `analyze_post_mono` call from `compile_source`.

- [ ] **Step 4: Run `cargo test`, commit**

Run: `cargo test`
Expected: All tests pass.

```
git add src/lib.rs
git commit -m "Use unified analysis (pre-mono SemanticInfo) in single-file pipeline"
```

### Task 15: Wire unified analysis into package pipeline

**Files:**
- Modify: `src/lib.rs:54-252`

- [ ] **Step 1: Update `compile_package_to_executable`**

Update the package pipeline similarly — use `analyze_pre_mono` to get `SemanticInfo`, keep AST mono for now.

- [ ] **Step 2: Run `cargo test`**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```
git add src/lib.rs
git commit -m "Use unified analysis in package compilation pipeline"
```

---

## Phase 6: Cutover and Cleanup

### Task 16a: Implement `compile_with_mono` shell (non-generic functions only)

**Files:**
- Modify: `src/codegen/llvm.rs`

- [ ] **Step 1: Create `compile_with_mono` function**

Add a new public function that accepts `BirModule` + `MonoCollectResult`. Initially, it only handles non-generic functions (same as existing `compile`):

```rust
pub fn compile_with_mono(
    bir_module: &BirModule,
    mono_result: &MonoCollectResult,
) -> Result<Vec<u8>> {
    // For now, compile only non-generic functions.
    // Generic instances handled in Task 16b.
    compile(bir_module) // Delegate to existing compile
}
```

- [ ] **Step 2: Run `cargo test`, commit**

```
git add src/codegen/llvm.rs
git commit -m "Add compile_with_mono shell (delegates to existing compile)"
```

### Task 16b: Add per-instruction type substitution to codegen

**Files:**
- Modify: `src/codegen/llvm.rs`

- [ ] **Step 1: Add substitution map parameter to `emit_instruction`**

Thread a `subst: &HashMap<String, BirType>` through the instruction emission path. For non-generic functions, `subst` is empty (no-op). For generic instances, it maps type params to concrete types.

- [ ] **Step 2: Apply `resolve_bir_type` to every `BirType` in `emit_instruction`**

Before using any `BirType` (for LLVM type conversion, struct layout lookup, etc.), resolve it through the substitution map.

- [ ] **Step 3: Run `cargo test`, commit**

```
git add src/codegen/llvm.rs
git commit -m "Add per-instruction type substitution to codegen"
```

### Task 16c: Add generic struct layout resolution in codegen

**Files:**
- Modify: `src/codegen/llvm.rs`

- [ ] **Step 1: Extend `build_struct_types` to handle generic struct instances**

For each `(struct_name, concrete_type_args)` in `MonoCollectResult.struct_instances`:
1. Look up the generic layout from `bir_module.struct_layouts`
2. Build substitution map from struct's type params to concrete args
3. Apply `resolve_bir_type` to each field type
4. Create an LLVM struct type under the mangled name (e.g., `Pair_Int32_Bool`)

- [ ] **Step 2: Update FieldGet/FieldSet codegen for generic struct types**

When the struct type has `type_args`, use the mangled name for LLVM struct lookup and apply substitution to field types.

- [ ] **Step 3: Run `cargo test`, commit**

```
git add src/codegen/llvm.rs
git commit -m "Add generic struct layout resolution to codegen"
```

### Task 16d: Add protocol method resolution in codegen

**Files:**
- Modify: `src/codegen/llvm.rs`

- [ ] **Step 1: Implement conformance map lookup during Call emission**

When emitting a `Call` instruction, if the `func_name` matches a protocol method pattern (present as key prefix in `conformance_map`), resolve the concrete implementation:

```rust
let resolved_func_name = if let Some(concrete_name) = resolve_protocol_call(
    &func_name, &resolved_type_args, &bir_module.conformance_map
) {
    concrete_name
} else {
    func_name.clone()
};
```

- [ ] **Step 2: Run `cargo test`, commit**

```
git add src/codegen/llvm.rs
git commit -m "Add protocol method resolution via conformance map in codegen"
```

### Task 16e: Wire BIR mono into single-file pipeline

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Update `compile_source` to use BIR mono**

```rust
pub fn compile_source(source: &str) -> Result<Vec<u8>> {
    let program = parser::parse(tokenizer::tokenize(source)?)?;
    semantic::validate_generics(&program)?;
    let (inferred, sem_info) = semantic::analyze_pre_mono(&program)?;
    // No AST monomorphize — lower generics directly to BIR
    let mut bir_module = bir::lower_program(&program, &sem_info)?;
    bir::optimize_module(&mut bir_module);
    let mono_result = bir::mono::mono_collect(&bir_module, "main");
    codegen::compile_with_mono(&bir_module, &mono_result)
}
```

- [ ] **Step 2: Update `compile_to_bir`**

Same pattern — remove AST mono, use unified analysis:

```rust
pub fn compile_to_bir(source: &str) -> Result<(BirModule, String)> {
    let program = parser::parse(tokenizer::tokenize(source)?)?;
    semantic::validate_generics(&program)?;
    let (_inferred, sem_info) = semantic::analyze_pre_mono(&program)?;
    let mut bir_module = bir::lower_program(&program, &sem_info)?;
    bir::optimize_module(&mut bir_module);
    let text = bir::print_module(&bir_module);
    Ok((bir_module, text))
}
```

- [ ] **Step 3: Run `cargo test`**

Run: `cargo test`
Expected: Some tests may fail. Debug and fix substitution gaps.

- [ ] **Step 4: Iterate until all tests pass, then commit**

```
git add src/lib.rs
git commit -m "Wire BIR mono into single-file compilation pipeline"
```

### Task 17: Integrate BIR mono into package pipeline with `linkonce_odr`

**Files:**
- Modify: `src/lib.rs`
- Modify: `src/codegen/llvm.rs`

- [ ] **Step 1: Build package-wide BIR registry**

After all modules are lowered, collect all generic functions into a read-only registry:

```rust
let mut bir_registry: HashMap<String, &BirFunction> = HashMap::new();
for bir_module in &all_bir_modules {
    for func in &bir_module.functions {
        if !func.type_params.is_empty() {
            bir_registry.insert(func.name.clone(), func);
        }
    }
}
```

- [ ] **Step 2: Run per-module mono collector with registry access**

Extend `mono_collect` to accept an optional registry for cross-module lookups:

```rust
pub fn mono_collect_with_registry(
    bir: &BirModule,
    entry: &str,
    registry: &HashMap<String, &BirFunction>,
) -> MonoCollectResult
```

- [ ] **Step 3: Emit `linkonce_odr` linkage for generic instantiations**

In codegen, set `Linkage::LinkOnceODR` for functions that are generic instantiations:

```rust
if !instance.type_args.is_empty() {
    llvm_func.set_linkage(Linkage::LinkOnceODR);
}
```

- [ ] **Step 4: Adjust extern declarations**

Generic instantiations emitted locally don't need extern declarations. Update the extern collection logic to skip them.

- [ ] **Step 5: Run `cargo test` (including package tests)**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```
git add src/lib.rs src/codegen/llvm.rs src/bir/mono.rs
git commit -m "Integrate BIR mono into package pipeline with linkonce_odr"
```

### Task 18: Update test helpers and remove AST monomorphization

**Files:**
- Modify: `tests/common/mod.rs`
- Delete: `src/monomorphize.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs` (if it references monomorphize)

- [ ] **Step 1: Update test helpers in `tests/common/mod.rs`**

The `compile_and_run` and `compile_should_fail` functions call
`monomorphize::monomorphize` and `analyze_post_mono` directly. Update them to
use the new BIR mono pipeline (call `compile_source` or replicate its logic).

- [ ] **Step 2: Verify all integration tests pass with updated helpers**

Run: `cargo test`
Expected: All tests pass using the new pipeline.

- [ ] **Step 3: Remove `monomorphize` calls from `compile_package_to_executable`**

Remove the per-module monomorphize loop (already replaced by BIR mono in Task 17).

- [ ] **Step 4: Remove `mod monomorphize` and delete the file**

- [ ] **Step 5: Clean up `InferredTypeArgs` usage**

`InferredTypeArgs` was primarily used by the monomorphizer. Inventory which
fields are still needed by lowering (e.g., inferred type arguments for
`Call.type_args` during lowering). Keep those, remove mono-specific parts.

- [ ] **Step 6: Run `cargo test`**

Run: `cargo test`
Expected: All tests pass on the BIR mono pipeline.

- [ ] **Step 7: Run `cargo fmt` and `cargo clippy`**

Run: `cargo fmt && cargo clippy`
Expected: No warnings.

- [ ] **Step 8: Commit**

```
git add -A
git commit -m "Remove AST-level monomorphization (replaced by BIR mono)"
```

### Task 19: Final verification and cleanup

**Files:**
- Various

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run clippy with strict settings**

Run: `cargo clippy -- -W clippy::all`
Expected: No warnings.

- [ ] **Step 3: Verify BIR output for key test cases**

Manually inspect BIR output for `generic_identity_i32`, `generic_struct_with_constraint`, and `generic_struct_with_method` to verify generic BIR is being produced correctly.

- [ ] **Step 4: Commit any remaining cleanup**

```
git add -A
git commit -m "Final cleanup after BIR monomorphization migration"
```
