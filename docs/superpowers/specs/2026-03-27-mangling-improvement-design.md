# Mangling Scheme Improvement

## Overview

Redesign the name mangling scheme with Itanium ABI-inspired structured encoding. Unify the two existing mangling systems (`mangle.rs` for module namespacing and `Instance::mangled_name()` for generic instantiation) into a single coherent scheme with entity kind markers and generic type encoding.

## Scope

- Redesign `mangle.rs` with new format: `_BG <entity-tag> N <segments> E [I <type-args> E]`
- Add entity kind markers: `F` (function), `M` (method), `I` (initializer)
- Add generic type argument encoding
- Remove `Instance::mangled_name()` from `mono.rs`
- Update all call sites (pipeline, lowering, codegen)

## Current Problems

1. `mangle_function` and `mangle_method` produce structurally indistinguishable output (both are length-prefix concatenations)
2. `Instance::mangled_name()` in `mono.rs` uses a separate `name_Type1_Type2` scheme without `_BG` prefix
3. No initializer mangling
4. Generic type encoding is inconsistent between the two systems

## Design

### 1. New Mangling Format

```
_BG <entity-tag> N <len><seg> ... <len><name> E [I <type> ... E]

entity-tag:
  F = free function
  M = method (segments include struct name + method name)
  I = initializer (segments include struct name)

N...E = nested name envelope (length-prefixed segments)
I...E = generic type arguments (optional, only for generic instantiations)

type encoding:
  i = Int32
  l = Int64
  f = Float32
  d = Float64
  b = Bool
  v = Unit
  S<len><name> = struct type (e.g., S5Point)
  A<type><decimal-size> = array type (e.g., Ai3 = [Int32; 3])
```

### 2. Examples

| Code | New Mangled Name |
|------|-----------------|
| `func add()` in `pkg::math` | `_BGFN3pkg4math3addE` |
| `Point.sum()` in `pkg::math` | `_BGMN3pkg4math5Point3sumE` |
| `Point.init()` in `pkg` | `_BGIN3pkg5PointE` |
| `identity<Int32>()` in `pkg` | `_BGFN3pkg8identityEIiE` |
| `swap<Int32, Bool>()` in `pkg` | `_BGFN3pkg4swapEIibE` |
| `Box<Point>.value()` in `pkg` | `_BGMN3pkg3Box5valueEIS5PointEE` |
| `main` (entry point) | `main` (no mangling) |

### 3. Public API (`src/mangle.rs`)

```rust
pub fn mangle_function(pkg: &str, segments: &[&str], name: &str, type_args: &[BirType]) -> String
pub fn mangle_method(pkg: &str, segments: &[&str], struct_name: &str, method: &str, type_args: &[BirType]) -> String
pub fn mangle_initializer(pkg: &str, segments: &[&str], struct_name: &str) -> String
pub fn mangle_entry_main() -> &'static str  // unchanged

// Internal helpers
fn encode_type(ty: &BirType) -> String
fn encode_nested_name(pkg: &str, segments: &[&str], names: &[&str]) -> String
```

`mangle_function` and `mangle_method` now accept `type_args: &[BirType]`. When `type_args` is empty, no `I...E` suffix is appended. When non-empty, the type arguments are encoded and appended.

### 4. `Instance::mangled_name()` Removal

`Instance::mangled_name()` in `src/bir/mono.rs` is removed. Callers that need a mangled name for a generic instantiation use `mangle::mangle_function` (or `mangle_method`) with `type_args`.

`mangle_bir_type` in `mono.rs` is replaced by `mangle::encode_type`.

The `Instance` struct may need additional context (package name, module segments) or callers provide this context when calling the mangle functions.

### 5. `BirType` Dependency

`mangle.rs` now depends on `BirType` for the `type_args` parameter and `encode_type`. Add `use crate::bir::instruction::BirType;` to `mangle.rs`.

`BirType::Error` in `encode_type`: encode as `e` (should never appear in practice).

## Changed Files

| File | Change |
|------|--------|
| `src/mangle.rs` | New scheme: entity tags, `N...E` envelope, `I...E` type args, `encode_type`, `encode_nested_name` |
| `src/bir/mono.rs` | Remove `Instance::mangled_name()` and `mangle_bir_type`; callers use `mangle.rs` |
| `src/bir/lowering.rs` | Update conformance_map key generation for new scheme |
| `src/pipeline.rs` | Update `build_name_map` to use new `mangle_function`/`mangle_method` signatures (pass empty `type_args` for non-generic) |
| `src/codegen/llvm.rs` | Update monomorphization name generation to use `mangle.rs` |
| `src/bir/instruction.rs` | No change (BirType already has all needed variants) |

## Test Strategy

### Unit tests in `src/mangle.rs`

- Free function: `_BGFN3pkg4math3addE`
- Method: `_BGMN3pkg4math5Point3sumE`
- Initializer: `_BGIN3pkg5PointE`
- Generic function: `_BGFN3pkg8identityEIiE`
- Multi-type-arg generic: `_BGFN3pkg4swapEIibE`
- Generic struct method: `_BGMN3pkg3Box5valueEIS5PointEE`
- Array type arg: `_BGFN3pkg3fooEIAi3E` (for `[Int32; 3]`)
- `main` stays `main`
- No collisions between function/method/initializer with same name segments

### Regression

All existing integration tests pass (mangled names change but internal consistency is maintained since both definition and call sites use the same scheme).
