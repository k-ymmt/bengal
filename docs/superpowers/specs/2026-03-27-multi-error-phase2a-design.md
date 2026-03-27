# Multiple Error Reporting — Phase 2a: Type::Error Foundation

## Overview

Add `Type::Error` and `BirType::Error` variants to represent failed type resolutions. `Type::Error` unifies with any type, preventing cascading errors when subsequent code encounters an already-failed expression. This is a prerequisite for Phase 2b-2d (converting semantic analysis to emit + continue).

## Scope

- Add `Type::Error` to `src/semantic/types.rs`
- Add `BirType::Error` to `src/bir/instruction.rs`
- Update `unify` in `src/semantic/infer.rs` to treat `Type::Error` as universally compatible
- Update `semantic_type_to_bir` in `src/bir/lowering.rs` to map `Type::Error` → `BirType::Error`
- Add panic guard in codegen for `BirType::Error` (should never reach codegen)
- No behavioral changes to existing code — `Type::Error` is not yet generated anywhere

## Context: Multi-Error Reporting Phases

| Phase | Status |
|-------|--------|
| 1: DiagCtxt foundation | Done |
| **2a: Type::Error foundation** | **This spec** |
| 2b: Top-level item continuation | Future |
| 2c: Expression-level emit + Error return | Future |
| 2d: Pipeline module-level continuation | Future |
| 3-6: Inference, lowering, codegen, CLI | Future |

## Design

### 1. `Type::Error` (`src/semantic/types.rs`)

Add `Error` variant to the `Type` enum:

```rust
pub enum Type {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Unit,
    Struct(String),
    TypeParam { name: String, bound: Option<String> },
    Generic { name: String, args: Vec<Type> },
    Array { element: Box<Type>, size: u64 },
    InferVar(u32),
    IntegerLiteral(u32),
    FloatLiteral(u32),
    Error,  // NEW: represents a failed type resolution
}
```

Update `resolve_type` if it has a match on `Type` — add `Error` arm that returns `Type::Error` (pass-through).

### 2. Unification (`src/semantic/infer.rs`)

Add early return at the top of `unify`:

```rust
pub fn unify(&mut self, ty1: Type, ty2: Type) -> Result<()> {
    // Error type unifies with anything — prevents cascading errors
    if matches!(&ty1, Type::Error) || matches!(&ty2, Type::Error) {
        return Ok(());
    }
    // ... existing unification logic ...
}
```

### 3. `BirType::Error` (`src/bir/instruction.rs`)

Add `Error` variant:

```rust
pub enum BirType {
    Unit,
    I32,
    I64,
    F32,
    F64,
    Bool,
    Struct { name: String, type_args: Vec<BirType> },
    Array { element: Box<BirType>, size: u64 },
    TypeParam(String),
    Error,  // NEW: placeholder for unresolved types
}
```

### 4. Type Conversion (`src/bir/lowering.rs`)

Update `semantic_type_to_bir`:

```rust
pub fn semantic_type_to_bir(ty: &Type) -> BirType {
    match ty {
        // ... existing arms ...
        Type::Error => BirType::Error,
    }
}
```

### 5. Codegen Guard (`src/codegen/llvm.rs`)

In `bir_type_to_llvm_type`, add:

```rust
BirType::Error => panic!("BirType::Error reached codegen — this is a compiler bug"),
```

### 6. Other Match Exhaustiveness

Adding a variant to `Type` and `BirType` will require updating all `match` expressions on these enums throughout the codebase. Most will add a simple arm:

- `Type::Error` in type-checking code → return `Type::Error` (propagate)
- `Type::Error` in display/format code → display as `<error>`
- `BirType::Error` in BIR printer → print as `<error>`
- `BirType::Error` in monomorphization → pass through or skip
- `BirType::Error` in serde — it will auto-derive since `Serialize`/`Deserialize` are on the enum

The implementer should use `cargo check` iteratively to find and fix all exhaustive match sites.

## Changed Files

| File | Change |
|------|--------|
| `src/semantic/types.rs` | Add `Type::Error`; update `resolve_type` if needed |
| `src/semantic/infer.rs` | Add Error early-return in `unify` |
| `src/semantic/mod.rs` | Update any exhaustive matches on `Type` |
| `src/bir/instruction.rs` | Add `BirType::Error` |
| `src/bir/lowering.rs` | Map `Type::Error` → `BirType::Error` in `semantic_type_to_bir` |
| `src/bir/printer.rs` | Print `BirType::Error` as `<error>` |
| `src/bir/mono.rs` | Handle `BirType::Error` in resolve/mangle functions |
| `src/codegen/llvm.rs` | Panic on `BirType::Error` |

## Test Strategy

- Add unit test: `unify(Type::Error, Type::I32)` succeeds
- Add unit test: `unify(Type::I32, Type::Error)` succeeds
- Add unit test: `unify(Type::Error, Type::Error)` succeeds
- All existing tests pass (Error is not generated in normal paths)
