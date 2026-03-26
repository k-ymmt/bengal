# BIR Serialization to Interface Files

## Overview

Serialize generic BIR to `.bengalmod` interface files so that consumers can perform codegen directly without re-parsing, re-analyzing, or re-lowering. This is a prerequisite for separate compilation (TODO 2.).

## Scope

- Serialize and deserialize `BirModule` per module within a single `.bengalmod` per package
- All symbols included regardless of visibility (Rust-style)
- Pipeline integration (e.g., `emit_interface` stage) is out of scope — deferred to separate compilation work

## File Format

### Binary Layout

```
[Magic bytes: 4 bytes] "BGMD"
[Format version: 4 bytes] u32 little-endian (initial: 1)
[MessagePack payload: remaining bytes]
```

- Magic bytes: reject files that are not `.bengalmod`
- Version number: reject incompatible versions with a "rebuild required" error (no backward compatibility guarantees, following Swift/Rust precedent)
- Payload: MessagePack via `rmp-serde`

### Payload Structure

```rust
#[derive(Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
}
```

Each module's `BirModule` contains the full BIR: struct layouts, struct type parameters, all functions (generic and concrete), and the conformance map.

### `conformance_map` Serialization

`BirModule.conformance_map` has type `HashMap<(String, BirType), String>`. MessagePack (via `rmp-serde`) does not support non-string map keys. To handle this, add a serde attribute on the field to serialize it as a `Vec<((String, BirType), String)>`:

```rust
#[serde(with = "conformance_map_serde")]
pub conformance_map: HashMap<(String, BirType), String>,
```

The `conformance_map_serde` helper module converts to/from `Vec` for serialization.

## Serialization Approach

Derive `Serialize` and `Deserialize` directly on existing BIR types. No intermediate DTO layer — format changes are handled by version number rejection.

### Types Requiring `Serialize, Deserialize`

In `src/bir/instruction.rs`:
- `BirType`
- `Value`
- `BirBinOp`
- `BirCompareOp`
- `Instruction`
- `Terminator`
- `BasicBlock`
- `CfgRegion`
- `BirFunction`
- `BirModule`

In `src/package.rs`:
- `ModulePath`

### `PartialEq`

All BIR types already derive `PartialEq`. No changes needed.

### Types NOT Serialized

- `MonoCollectResult`, `Instance` — consumers recompute monomorphization
- `SemanticInfo`, `PackageSemanticInfo` — BIR already contains all needed information
- AST types — BIR replaces the need for re-parsing

## API Design

### New Module: `src/interface.rs`

```rust
pub const MAGIC: &[u8; 4] = b"BGMD";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
}

/// Write a LoweredPackage to a .bengalmod file.
pub fn write_interface(package: &LoweredPackage, path: &Path) -> Result<()>

/// Read a .bengalmod file.
pub fn read_interface(path: &Path) -> Result<BengalModFile>
```

### Intended Call Point

`write_interface` is designed to be called after `optimize` and before `monomorphize` — at the point where BIR is fully lowered and optimized but still generic. Pipeline integration (adding an explicit stage) is deferred to separate compilation work.

### `write_interface` Flow

1. Build `BengalModFile` from `LoweredPackage.modules` (collect each `LoweredModule.bir`)
2. Write `MAGIC` (4 bytes)
3. Write `FORMAT_VERSION` as u32 little-endian (4 bytes)
4. Serialize payload via `rmp_serde::to_vec` and write

### `read_interface` Flow

1. Read entire file
2. Validate first 4 bytes match `MAGIC`
3. Read next 4 bytes as u32 little-endian, validate against `FORMAT_VERSION`
4. Deserialize remaining bytes via `rmp_serde::from_slice::<BengalModFile>`

### Error Handling

Add `InterfaceError { message: String }` variant to `BengalError`. Covers: file I/O errors, magic mismatch, version mismatch, deserialization failures. Also add a corresponding match arm in `into_diagnostic` (and any other exhaustive matches on `BengalError`).

### `lib.rs` Integration

Add `pub mod interface;` to `src/lib.rs`.

## Test Strategy

### Test File: `tests/interface.rs`

#### Round-Trip Tests

Compile Bengal source to `LoweredPackage` via pipeline, write to `.bengalmod`, read back, and assert equality with original `BirModule`.

Source patterns to cover:
- Simple function (`func add(a: Int32, b: Int32) -> Int32`)
- Generic function (`func identity<T>(x: T) -> T`)
- Struct with stored properties and methods
- Generic struct (`struct Box<T>`)
- Protocol conformance
- Array types
- Multi-module package

#### Validation Tests

- Invalid magic bytes -> `InterfaceError`
- Wrong version number -> `InterfaceError`
- Empty file -> `InterfaceError`
- Truncated payload -> `InterfaceError`

## Dependencies

### Added to `Cargo.toml`

- `rmp-serde` — MessagePack serialization/deserialization

(`serde` with `derive` feature is already present.)

## Changed Files

| File | Change |
|------|--------|
| `Cargo.toml` | Add `rmp-serde` |
| `src/bir/instruction.rs` | Add `Serialize, Deserialize` derives; add `conformance_map_serde` helper |
| `src/package.rs` | Add `Serialize, Deserialize` to `ModulePath` |
| `src/error.rs` | Add `InterfaceError` variant and corresponding `into_diagnostic` arm |
| `src/interface.rs` | **New** — `BengalModFile`, `write_interface`, `read_interface` |
| `src/lib.rs` | Add `pub mod interface;` |
| `tests/interface.rs` | **New** — round-trip and validation tests |

## Future Work

- Pipeline integration: `emit_interface` stage in `pipeline.rs`
- Separate compilation (TODO 2.): consume `.bengalmod` during compilation
- Text-based interface file (TODO 1.5): stable format across compiler versions
- BIR-level optimization (TODO 1.6.3): optimize before serialization
