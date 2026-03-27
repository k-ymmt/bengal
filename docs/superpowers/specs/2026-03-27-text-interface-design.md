# Text-Based Interface File (.bengalinterface) Design

## Overview

Implement a text-based module interface format (`.bengalinterface`) equivalent to Swift's `.swiftinterface`. This provides a human-readable, compiler-version-stable representation of a module's public API, enabling pre-built library distribution across compiler versions.

## Scope

- **Emit**: `ModuleInterface` → `.bengalinterface` text file
- **Parse**: `.bengalinterface` text file → `ModuleInterface`
- **Format**: Bengal source syntax subset (declaration signatures only, no bodies)
- **Generics**: Full support for type parameters and protocol bounds in signatures
- **Future**: `@inlinable` support (function bodies in interface) deferred

## Relationship to Binary Format

| File | Content | Stability |
|------|---------|-----------|
| `.bengalinterface` (text) | API signatures only | Stable across compiler versions |
| `.bengalmod` (binary) | BIR bodies + interface | Same compiler version only |

For generic monomorphization, consumers need BIR from `.bengalmod`. The text interface is sufficient for type-checking.

## Text Format Specification

### Header

```
// bengal-interface-format-version: 1
```

First line, required. Validated before parsing.

### Functions

```
public func add(a: Int32, b: Int32) -> Int32;
public func identity<T>(x: T) -> T;
public func constrained<T: Summable>(x: T) -> Int32;
```

No body. Terminated by `;`.

### Structs

```
public struct Pair<T, U>: Printable {
  var first: T;
  var second: U;
  var description: String { get };
  init(first: T, second: U);
  func swap() -> Pair<U, T>;
}
```

Members are signature-only:
- Stored property: `var name: Type;`
- Computed property: `var name: Type { get };` or `var name: Type { get set };`
- Initializer: `init(params);`
- Method: `func name(params) -> ReturnType;`

### Protocols

```
public protocol Summable {
  func sum() -> Int32;
  var value: Int32 { get set };
}
```

Same as existing Bengal protocol syntax.

### Type Representation

| InterfaceType | Text |
|---------------|------|
| `I32` | `Int32` |
| `I64` | `Int64` |
| `F32` | `Float32` |
| `F64` | `Float64` |
| `Bool` | `Bool` |
| `Unit` | `Void` |
| `Struct(name)` | `name` |
| `TypeParam { name, .. }` | `name` |
| `Generic { name, args }` | `name<arg1, arg2>` |
| `Array { element, size }` | `[element; size]` |

### Output Ordering

1. Header comment
2. Functions (alphabetical)
3. Structs (alphabetical)
4. Protocols (alphabetical)

Sections separated by blank lines. Empty sections omitted.

### Visibility

All entries emitted with `public` prefix. Only `Public`/`Package` visibility items appear in the interface (already filtered by `ModuleInterface::from_semantic_info`).

## Emitter Design

### API

```rust
// src/interface.rs
pub fn emit_text_interface(iface: &ModuleInterface) -> String
pub fn write_text_interface(iface: &ModuleInterface, path: &Path) -> Result<()>
```

`write_text_interface` calls `emit_text_interface()` and writes result to file.

## Parser Modifications

### Approach

Reuse the existing Bengal parser with an `interface_mode` flag (Swift-style). This enables future `@inlinable` support where function bodies are included in the interface.

### Parser Struct Change

```rust
struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    next_id: u32,
    interface_mode: bool,  // NEW
}
```

### New Entry Point

```rust
pub fn parse_interface(tokens: Vec<SpannedToken>) -> Result<Program>
```

Initializes parser with `interface_mode = true`.

### AST Changes

Body fields changed from required to optional:

| Type | Field | Before | After |
|------|-------|--------|-------|
| `Function` | `body` | `Block` | `Option<Block>` |
| `StructMember::Method` | `body` | `Block` | `Option<Block>` |
| `StructMember::Initializer` | `body` | `Block` | `Option<Block>` |
| `StructMember::ComputedProperty` | `getter` | `Block` | `Option<Block>` |

Normal mode always produces `Some(block)`. Existing code (semantic analysis, BIR lowering) uses `.unwrap()` — these are never called on interface-mode ASTs.

### Parsing Behavior in Interface Mode

- **Function**: After signature, expect `;` instead of `{` → `body: None`
- **Struct method**: Same — `;` instead of body → `body: None`
- **Struct init**: Same — `;` instead of body → `body: None`
- **Struct computed property**: `{ get set }` / `{ get }` only, no getter/setter blocks → `getter: None`
- **Struct stored property**: No change (already body-less)
- **Protocol**: No change (already body-less)

## Reader Design

### API

```rust
// src/interface.rs
pub fn read_text_interface(text: &str) -> Result<ModuleInterface>
pub fn read_text_interface_file(path: &Path) -> Result<ModuleInterface>
```

### Processing Flow

1. Extract and validate header comment from first line
2. Tokenize full text with existing lexer (header comment is skipped by lexer)
3. Call `parse_interface(tokens)` to get `Program` (AST)
4. Call `ModuleInterface::from_ast(&program)` to convert

### AST → ModuleInterface Conversion

New function: `ModuleInterface::from_ast(program: &Program) -> ModuleInterface`

Conversion mapping:

| AST | ModuleInterface |
|-----|-----------------|
| `Function` | `InterfaceFuncEntry` |
| `StructDef` | `InterfaceStructEntry` |
| `StructMember::StoredProperty` | `fields` |
| `StructMember::ComputedProperty` | `computed` |
| `StructMember::Method` | `methods` |
| `StructMember::Initializer` | `init_params` |
| `ProtocolDef` | `InterfaceProtocolEntry` |
| `ProtocolMember::MethodSig` | `methods` |
| `ProtocolMember::PropertyReq` | `properties` |

### TypeAnnotation → InterfaceType Conversion

New function: `InterfaceType::from_annotation(ann: &TypeAnnotation, type_params: &[TypeParam]) -> InterfaceType`

The `type_params` context resolves the `Named` ambiguity: if a `Named(s)` matches a type parameter name, produce `TypeParam { name, bound }`; otherwise produce `Struct(s)`.

## File I/O Integration

### Relationship to Existing Functions

| Function | Format | Extension |
|----------|--------|-----------|
| `write_interface` (existing) | Binary (MessagePack) | `.bengalmod` |
| `write_text_interface` (new) | Text (Bengal syntax) | `.bengalinterface` |
| `read_interface` (existing) | Binary | `.bengalmod` |
| `read_text_interface_file` (new) | Text | `.bengalinterface` |

### Pipeline Integration

No pipeline stage added at this point. Functions are standalone utilities. Pipeline integration happens when separate compilation (TODO 2.) is implemented.

## Testing Strategy

All tests in `tests/interface.rs` alongside existing binary round-trip tests.

### Emitter Tests

- `emit_simple_function` — basic function signature output
- `emit_generic_function` — type parameters and bounds
- `emit_struct_with_members` — fields, methods, computed props, init
- `emit_generic_struct_with_conformance` — generic struct + protocol conformance
- `emit_protocol` — method and property requirements
- `emit_array_types` — array type output
- `emit_empty_interface` — empty ModuleInterface (header only)
- `emit_ordering` — function→struct→protocol order, alphabetical within

### Parser Tests (Interface Mode)

- `parse_interface_function` — body-less function with `;`
- `parse_interface_struct` — signature-only members
- `parse_interface_protocol` — same behavior as normal mode
- `parse_interface_computed_property` — `{ get }` / `{ get set }`

### Round-Trip Tests (ModuleInterface → text → ModuleInterface)

- `text_round_trip_simple_function`
- `text_round_trip_generic_function`
- `text_round_trip_struct_full` — all member types
- `text_round_trip_generic_struct_with_conformance`
- `text_round_trip_protocol`
- `text_round_trip_array_types`
- `text_round_trip_mixed` — functions + structs + protocols

### Error Case Tests

- `read_text_missing_header` — no header
- `read_text_wrong_version` — version mismatch
- `read_text_invalid_syntax` — malformed syntax
