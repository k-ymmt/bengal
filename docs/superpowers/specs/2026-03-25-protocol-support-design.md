# Protocol Support Design

## Overview

Add protocol support to Bengal, inspired by Swift's protocols. This is split into two sub-phases:

- **Phase 6a**: Add methods to structs
- **Phase 6b**: Add protocol definitions and conformance checking

Phase 1 scope uses **static dispatch only** — protocol is a compile-time contract checked by semantic analysis. No runtime dispatch, no existential types.

## Approach

**Approach A: Staged introduction** — first add struct methods (6a), then protocols (6b). Each sub-phase is independently testable.

Methods are flattened to regular functions in BIR using name mangling (`StructName_methodName`). Protocols exist only in semantic analysis and produce no BIR/codegen artifacts.

---

## Section 1: Grammar

### Struct methods

```ebnf
struct_member = stored_property | computed_property | initializer | method ;

method = "func" , identifier , param_list , [ "->" , type ] , block ;
```

- `self` is implicitly available in method bodies (immutable)
- `self` is not written in the parameter list (like Swift)
- Call syntax: `obj.methodName(args...)`

**Example:**

```
struct Point {
    var x: Int32;
    var y: Int32;

    func sum() -> Int32 {
        return self.x + self.y;
    }

    func add(other: Point) -> Point {
        return Point(x: self.x + other.x, y: self.y + other.y);
    }
}
```

### Top-level grammar update

```ebnf
top_level = function | struct_def | protocol_def ;
```

### Protocol definition

```ebnf
protocol_def = "protocol" , identifier , "{" , { protocol_member } , "}" ;

protocol_member = method_sig | property_req ;

method_sig    = "func" , identifier , param_list , [ "->" , type ] , ";" ;

property_req  = "var" , identifier , ":" , type , "{" , "get" , [ "set" ] , "}" , ";" ;
```

**Example:**

```
protocol Summable {
    func sum() -> Int32;
    var total: Int32 { get };
}
```

### Conformance declaration

```ebnf
struct_def = "struct" , identifier , [ ":" , identifier_list ] , "{" , { struct_member } , "}" ;

identifier_list = identifier , { "," , identifier } ;
```

**Example:**

```
struct Point: Summable {
    var x: Int32;
    var y: Int32;

    func sum() -> Int32 {
        return self.x + self.y;
    }

    var total: Int32 {
        get { yield self.x + self.y; }
    };
}
```

---

## Section 2: Semantic Analysis

### Struct methods

- **Pass 1a**: Register struct names (unchanged)
- **Pass 1b**: Register method signatures in `StructInfo` as `MethodInfo { name, params: Vec<(String, Type)>, return_type: Type }`. `params` excludes `self` — `self` is added implicitly during BIR flattening.
- **Pass 3**: Analyze method bodies with `self_context` set. `self` is immutable.

**Method call resolution:**

- `obj.methodName(args...)` is parsed as a new `ExprKind::MethodCall { object, method, args }`
- Semantic analysis resolves the object's type to a struct, looks up the method signature, and type-checks arguments
- Return type is determined from the method signature

### Protocol

- **Pass 1a**: Register protocol names. Add `ProtocolInfo { name, methods: Vec<MethodSig>, properties: Vec<PropertyReq> }` to resolver.
- **Pass 1b**: Resolve types in protocol member signatures (validate parameter/return types)
- **Pass 1b (conformance)**: Verify that conformance target protocols exist
- **Pass 3**: Conformance check — verify struct implements all required methods and computed properties with correct signatures

**Conformance rules:**

- Protocol method signature must match struct method: parameter names, types, and return type
- Protocol `{ get }` property: struct must have a computed property with getter, OR a stored property (readable)
- Protocol `{ get set }` property: struct must have a computed property with getter + setter, OR a mutable stored property (`var`)

---

## Section 3: BIR Lowering & Codegen

### Method flattening

Methods become regular BIR functions with name mangling:

- `Point.sum()` becomes `Point_sum(self: Point) -> Int32`
- `Point.add(other: Point)` becomes `Point_add(self: Point, other: Point) -> Point`
- `self` is the first explicit argument

**Method calls:**

- `p.sum()` becomes `Call { func_name: "Point_sum", args: [p] }`
- `p.add(q)` becomes `Call { func_name: "Point_add", args: [p, q] }`

Uses existing `Call` instruction. No new BIR instructions needed.

### Protocol at BIR/codegen level

No representation. Protocol is purely a semantic-level construct:

- Protocol definitions produce no BIR output
- Conformance declarations are verified in semantic analysis and ignored in lowering
- All method calls are resolved to concrete types at compile time

### Codegen impact

- No changes to `struct_layouts` (methods are independent functions)
- Flattened method functions are processed by existing function codegen
- No new LLVM instructions or types needed

### Name mangling

`{StructName}_{methodName}` — simple and collision-resistant. Struct names conventionally start with uppercase, so conflicts with user-defined functions are unlikely. Semantic analysis rejects top-level functions whose names match a mangled method name. Can be revised later if needed.

---

## Section 4: Error Messages

| Error case | Message |
|------------|---------|
| Undefined method call | `type 'Point' has no method 'foo'` |
| Method argument type mismatch | `expected 'Int32' but got 'Bool' in argument 'x' of method 'add'` |
| Method argument count mismatch | `method 'add' expects 1 argument but got 2` |
| Unknown protocol in conformance | `unknown protocol 'Foo'` |
| Protocol method not implemented | `type 'Point' does not implement method 'sum' required by protocol 'Summable'` |
| Protocol property not implemented | `type 'Point' does not implement property 'total' required by protocol 'Summable'` |
| Method signature mismatch | `method 'sum' has return type 'Bool' but protocol 'Summable' requires 'Int32'` |
| Property missing setter | `property 'total' requires a setter to conform to protocol 'Summable'` |

---

## Section 5: Test Strategy

### Phase 6a (struct methods)

- Basic method definition and call
- `self` field access in method
- Method with arguments
- Method returning a struct
- Method chaining: `p.toPoint().sum()`
- Self method call: `self.sum()` within another method
- Method calls in if/while
- Nested struct method calls

### Phase 6b (protocol)

- Basic protocol definition + conformance
- Protocol with multiple methods
- Property requirement (`{ get }`, `{ get set }`)
- Stored property satisfying `{ get }` / `{ get set }`
- Conformance to multiple protocols
- Error cases (missing implementation, signature mismatch)

---

## Section 6: Future Work

These features are **out of scope** for Phase 1 but planned for future consideration:

| Feature | Description | Prerequisites |
|---------|-------------|---------------|
| **Existential types (Phase 2)** | Store protocol types in variables, pass as function arguments (`var x: Summable = Point(...)`) | Vtable/witness table for dynamic dispatch; `MethodCall` BIR instruction |
| **BIR MethodCall instruction** | Dedicated instruction for method dispatch in BIR, enabling dynamic dispatch | Required by existential types |
| **Extension conformance** | `extension Point: Drawable { ... }` for retroactive conformance | Extension feature itself |
| **Default implementations** | Method bodies in protocol definitions | Phase 2+ |
| **Associated types** | `associatedtype Element` | Generics support |
| **Protocol inheritance** | `protocol A: B { ... }` | Phase 2+ |
