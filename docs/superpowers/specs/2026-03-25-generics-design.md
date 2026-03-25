# Generics Support Design

## Overview

Add generics (type parameterization) to the Bengal language with Swift-like syntax. This covers generic functions and generic structs with single protocol constraints and monomorphization-based code generation.

## Scope

| Feature | Included | Notes |
|---------|----------|-------|
| Generic functions | Yes | `func foo<T>(bar: T) -> T` |
| Generic structs | Yes | `struct Box<T> { var value: T; }` |
| Single protocol constraint | Yes | `<T: Summable>` |
| Multiple protocol constraints | No | Future: `<T: A & B>` |
| Type inference at call sites | No | Future: infer type args from arguments |
| Generic methods (own type params) | No | Methods use the struct's type params only |
| Associated types | No | Future |

## Syntax

### Generic function definition

```bengal
func identity<T>(value: T) -> T {
    return value;
}

func constrain<T: Summable>(item: T) -> Int32 {
    return item.sum();
}
```

### Generic struct definition

```bengal
struct Pair<A, B> {
    var first: A;
    var second: B;
}

struct Wrapper<T: Summable> {
    var value: T;

    func getSum() -> Int32 {
        return self.value.sum();
    }

    func getValue() -> T {
        return self.value;
    }
}
```

### Generic invocation (always explicit)

```bengal
let x = identity<Int32>(value: 42);
let p = Pair<Int32, Bool>(first: 10, second: true);
let w = Wrapper<Point>(value: Point(x: 1, y: 2));
let s = w.getSum();
```

### Grammar additions

```
type_params       = "<" type_param ("," type_param)* ">"
type_param        = IDENT (":" IDENT)?
type_args         = "<" type ("," type)* ">"

func_def          = "func" IDENT type_params? "(" params ")" ("->" type)? block
struct_def        = "struct" IDENT type_params? (":" protocols)? "{" members "}"
type              = ... | IDENT type_args?
call_expr         = IDENT type_args? "(" args ")"
```

Note: `call_expr` covers both function calls and struct constructors at the parser level. The parser produces a `Call` node in all cases. The semantic analyzer distinguishes function calls from struct constructors (via `struct_init_calls`) as it does today -- no change to this logic is needed.

## `<>` Disambiguation Strategy

### New language rule: infix operator spacing

This spec introduces a new language-wide rule: **all infix operators must be surrounded by spaces on both sides.** This is not generics-specific -- it applies to all infix operators (`+`, `-`, `*`, `/`, `<`, `>`, `<=`, `>=`, `==`, `!=`, `&&`, `||`). Violation produces a compile error. Existing tests may need updating to comply with this rule.

This rule eliminates the ambiguity between `<`/`>` as comparison operators and as type parameter delimiters:

- `foo<Int32>(bar: x)` -- no space before `<` -- type arguments
- `a < b` -- spaces around `<` -- comparison operator

### Implementation

Each token already carries span information via `Spanned<Token>`. The parser compares `prev_token.span.end` with `current_token.span.start`:

- If `prev_token.span.end == current_token.span.start` (no space), `<` is a type parameter opener
- If there is a gap, `<` is a comparison operator

No lookahead or backtracking is needed.

## AST Changes

### TypeAnnotation

```rust
pub enum TypeAnnotation {
    I32, I64, F32, F64, Bool, Unit,
    Named(String),
    // New
    Generic {
        name: String,
        args: Vec<TypeAnnotation>,
    },
}
```

`Named("T")` is used for type parameters at the AST level. The semantic analyzer distinguishes type parameters from concrete types using scope information.

### TypeParam (new)

```rust
pub struct TypeParam {
    pub name: String,
    pub bound: Option<String>,
}
```

### Function

```rust
pub struct Function {
    pub visibility: Visibility,
    pub name: String,
    pub type_params: Vec<TypeParam>,  // new
    pub params: Vec<Param>,
    pub return_type: TypeAnnotation,
    pub body: Block,
}
```

### StructDef

```rust
pub struct StructDef {
    pub visibility: Visibility,
    pub name: String,
    pub type_params: Vec<TypeParam>,  // new
    pub conformances: Vec<String>,
    pub members: Vec<StructMember>,
}
```

### ExprKind::Call

```rust
Call {
    name: String,
    type_args: Vec<TypeAnnotation>,  // new
    args: Vec<Expr>,
}
```

### ExprKind::StructInit

```rust
StructInit {
    name: String,
    type_args: Vec<TypeAnnotation>,  // new
    args: Vec<(String, Expr)>,
}
```

### ExprKind::MethodCall (no AST change needed)

`MethodCall` does not need `type_args`. Method calls on generic structs (e.g., `w.getSum()`) resolve the struct's type arguments through the receiver's type, not through explicit type arguments on the method call itself. The semantic analyzer knows the concrete type of `w` (e.g., `Wrapper<Point>`) and uses that to look up the monomorphized method during monomorphization. The `MethodCall` AST node remains unchanged:

```rust
MethodCall {
    object: Box<Expr>,  // receiver whose type carries the type args
    method: String,
    args: Vec<Expr>,
}
```

## Type System Changes

### Type enum

```rust
pub enum Type {
    I32, I64, F32, F64, Bool, Unit,
    Struct(String),
    // New
    TypeParam {
        name: String,
        bound: Option<String>,
    },
    Generic {
        name: String,
        args: Vec<Type>,
    },
}
```

### Resolver extensions

```rust
pub struct FuncSig {
    pub type_params: Vec<TypeParam>,  // new
    pub params: Vec<Type>,
    pub return_type: Type,
}

pub struct StructInfo {
    pub type_params: Vec<TypeParam>,  // new
    pub fields: Vec<(String, Type)>,
    pub field_index: HashMap<String, usize>,
    pub methods: Vec<MethodInfo>,
    pub method_index: HashMap<String, usize>,
    // existing fields (computed, computed_index, init) omitted for brevity
}
```

### Type resolution flow

1. **Definition**: `func foo<T: Summable>(bar: T)` registers `T` as `Type::TypeParam { name: "T", bound: Some("Summable") }` in the function scope
2. **Body type-checking**: Verifies that operations on `T` are permitted by its constraints. Unconstrained `T` allows only assignment, passing as argument, and returning.
3. **Call site**: `foo<Int32>(bar: 42)` verifies that `Int32` conforms to `Summable`

## Monomorphization

### Pipeline position

```
AST -> Semantic analysis (type checking) -> Monomorphization -> BIR lowering -> LLVM codegen
```

### How it works

1. Walk the entire program to collect all concrete instantiations of generic functions and structs
2. For each instantiation, substitute type parameters with concrete types and generate a specialized version
3. Replace all generic call sites with calls to the specialized versions

### Example

```bengal
// Source
func identity<T>(value: T) -> T { return value; }
let a = identity<Int32>(value: 42);
let b = identity<Bool>(value: true);
```

```
// After monomorphization (conceptual)
func identity_Int32(value: Int32) -> Int32 { return value; }
func identity_Bool(value: Bool) -> Bool { return value; }
let a = identity_Int32(value: 42);
let b = identity_Bool(value: true);
```

### Struct monomorphization

```bengal
struct Pair<A, B> { var first: A; var second: B; }
let p = Pair<Int32, Bool>(first: 10, second: true);
```

```
// After monomorphization
struct Pair_Int32_Bool { var first: Int32; var second: Bool; }
// Methods are also specialized: Pair_Int32_Bool_methodName
```

### Name mangling

Extend the existing module mangling scheme `_BG<len><segment>...` by appending an `_I` (Instantiation) suffix after the existing segments, followed by length-prefixed type argument names (`<len><name>` for each type arg):

```
identity<Int32>            -> _BG8identity_I5Int32
Pair<Int32, Bool>          -> _BG4Pair_I5Int324Bool
Wrapper<Point>.getSum      -> _BG7Wrapper_I5Point6getSum
```

For example, `_BG4Pair_I5Int324Bool` is read as: `4:Pair` (base name) + `_I` (instantiation marker) + `5:Int32` (first type arg) + `4:Bool` (second type arg). Module path segments precede the base name as usual.

### Impact on BIR / LLVM

None. After monomorphization, all types are concrete. `BirType` and the LLVM codegen work unchanged -- a monomorphized struct like `Pair_Int32_Bool` is just another `BirType::Struct("Pair_Int32_Bool")`.

## Error Messages

| Error | Example | Message |
|-------|---------|---------|
| Wrong number of type args | `Pair<Int32>(...)` | `expected 2 type arguments for 'Pair', found 1` |
| Constraint not satisfied | `foo<Int32>(...)` where `T: Summable` | `type 'Int32' does not conform to protocol 'Summable'` |
| Missing type args | `identity(value: 42)` | `generic function 'identity' requires explicit type arguments` |
| Type args on non-generic | `bar<Int32>(...)` | `function 'bar' does not take type arguments` |
| Method on unconstrained T | `t.foo()` where T has no bound | `cannot call method 'foo' on unconstrained type parameter 'T'` |
| Missing infix spaces | `a<b` | `infix operator '<' requires spaces on both sides` |

## Testing Strategy

New file: `tests/generics.rs`

1. **Generic functions**: definition, invocation, multiple type params
2. **Generic structs**: definition, constructor, field access, methods using T
3. **Type constraints**: protocol-constrained functions/structs, constraint violation errors
4. **Monomorphization**: same generic with different type args used multiple times
5. **Error cases**: each error from the table above
6. **Integration**: generics within modules, generic structs conforming to protocols

## Future Work (tracked in TODO.md)

- Multiple protocol constraints: `<T: A & B>`
- Type inference at call sites (omit type args when unambiguous)
