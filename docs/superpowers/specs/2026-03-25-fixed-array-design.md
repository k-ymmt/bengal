# Fixed-Size Array Support Design

## Overview

Add fixed-size array support to Bengal. Arrays are stack-allocated value types with a size known at compile time. The size is part of the type: `[Int32; 3]` and `[Int32; 5]` are distinct types.

## Scope

| Feature | Included | Notes |
|---------|----------|-------|
| Array type `[T; N]` | Yes | Size is part of the type |
| Array literal `[1, 2, 3]` | Yes | Type inferred from elements |
| Index access `a[0]` | Yes | Returns element type |
| Index assign `a[0] = 5` | Yes | Requires `var` binding |
| Compile-time bounds check | Yes | Constant index checked at compile time |
| Runtime bounds check | Yes | Variable index checked at runtime, traps on OOB |
| Type inference | Yes | `let a = [1, 2, 3]` infers `[Int32; 3]` |
| Empty array `[]` | No | Deferred to variable-length arrays |
| `[Int32]()` syntax | No | Deferred to variable-length arrays |
| `.count` property | No | Future enhancement |
| Array comparison | No | Future enhancement |
| `Array<T>` alias | No | Deferred to variable-length arrays |

## Syntax

### Array type annotation

```bengal
let a: [Int32; 3] = [1, 2, 3];
let b: [Bool; 2] = [true, false];
```

### Array literal (with type inference)

```bengal
let a = [1, 2, 3];          // inferred as [Int32; 3]
let b = [1.0, 2.0];         // inferred as [Float64; 2]
let c = [true, false, true]; // inferred as [Bool; 3]
```

### Index access and assignment

```bengal
let x = a[0];       // element access
var arr = [1, 2, 3];
arr[1] = 10;        // element assignment (var only)
```

### Functions with array parameters and return types

```bengal
func sum(arr: [Int32; 3]) -> Int32 {
    return arr[0] + arr[1] + arr[2];
}

func makeArray() -> [Int32; 2] {
    return [10, 20];
}
```

### Grammar additions

```
array_type    = "[" , type , ";" , integer_literal , "]" ;
array_literal = "[" , expr , { "," , expr } , "]" ;
index_expr    = postfix_expr , "[" , expr , "]" ;
index_assign  = postfix_expr , "[" , expr , "]" , "=" , expr , ";" ;
type          = ... | array_type ;
```

## AST Changes

### TypeAnnotation

```rust
pub enum TypeAnnotation {
    // ... existing variants ...
    Array {
        element: Box<TypeAnnotation>,
        size: u64,
    },
}
```

### ExprKind

```rust
pub enum ExprKind {
    // ... existing variants ...
    ArrayLiteral {
        elements: Vec<Expr>,
    },
    IndexAccess {
        object: Box<Expr>,
        index: Box<Expr>,
    },
}
```

### Stmt

```rust
pub enum Stmt {
    // ... existing variants ...
    IndexAssign {
        object: Box<Expr>,
        index: Box<Expr>,
        value: Expr,
    },
}
```

`IndexAssign` follows the same pattern as the existing `FieldAssign`.

## Lexer Changes

Add two new tokens:

```rust
#[token("[")]
LBracket,
#[token("]")]
RBracket,
```

## Parser

### `[` disambiguation

`[` appears in three contexts, naturally distinguished by parser position:

| Context | Parser function | Meaning |
|---------|----------------|---------|
| Type position | `parse_type` | Array type `[Int32; 3]` |
| Primary expression | `parse_primary` | Array literal `[1, 2, 3]` |
| Postfix position | `parse_postfix` | Index access `a[0]` |

No ambiguity arises because each context is a different parser function.

### parse_type: `[Type; N]`

When `parse_type` sees `[`:
1. Consume `[`
2. Parse element type recursively (supports nested arrays like `[[Int32; 2]; 3]`)
3. Expect `;`
4. Parse integer literal for size
5. Expect `]`

### parse_primary: `[expr, ...]`

When `parse_primary` sees `[`:
1. Consume `[`
2. Parse first expression
3. Parse remaining expressions separated by `,`
4. Expect `]`

### parse_postfix: `expr[index]`

Add `Token::LBracket` arm in `parse_postfix` (alongside `Token::Dot` and `Token::LParen`):
1. Consume `[`
2. Parse index expression
3. Expect `]`
4. Produce `ExprKind::IndexAccess`

This chains naturally: `a[0].x`, `matrix[i][j]` etc.

### Index assignment parsing

In `parse_stmt`, when an expression statement results in an `IndexAccess` followed by `=`, produce `Stmt::IndexAssign` instead. This mirrors the existing `FieldAssign` handling.

## Type System Changes

### Type enum

```rust
pub enum Type {
    // ... existing variants ...
    Array {
        element: Box<Type>,
        size: u64,
    },
}
```

### Type checking rules

- **Array literal**: all elements must have the same type. The resulting type is `Array { element, size: elements.len() }`.
- **Index access** `a[i]`: `a` must be `Array { element, .. }`, result type is `*element`. Index must be an integer type (`I32` or `I64`).
- **Index assign** `a[i] = v`: `a` must be `var` (mutable), `v` must match the array's element type.
- **Type annotation match**: when `let a: [Int32; 3] = [1, 2, 3]`, verify the literal's inferred type matches the annotation (same element type and same size).

### Bounds checking

- **Constant index**: if the index is an integer literal, check `0 <= index < size` at compile time. Error: `array index 3 is out of bounds for array of size 3`
- **Variable index**: defer to runtime (see LLVM codegen section)

### resolve_type

```rust
TypeAnnotation::Array { element, size } => Type::Array {
    element: Box::new(resolve_type(element)),
    size: *size,
},
```

## BIR Changes

### BirType

```rust
pub enum BirType {
    // ... existing variants ...
    Array {
        element: Box<BirType>,
        size: u64,
    },
}
```

### New instructions

```rust
pub enum Instruction {
    // ... existing instructions ...
    ArrayInit {
        result: Value,
        ty: BirType,           // Array { element, size }
        elements: Vec<Value>,
    },
    ArrayGet {
        result: Value,
        ty: BirType,           // element type
        array: Value,
        index: Value,
    },
    ArraySet {
        result: Value,
        ty: BirType,           // Array type
        array: Value,
        index: Value,
        value: Value,
    },
}
```

## LLVM Code Generation

### Array type mapping

`BirType::Array { element: I32, size: 3 }` maps to LLVM `[3 x i32]`.

### ArrayInit

1. `alloca [N x T]`
2. For each element: GEP to element pointer, `store` value
3. `load` the entire array value

### ArrayGet

1. `alloca [N x T]` for the array value
2. `store` the array into the alloca
3. GEP with index to get element pointer
4. `load` the element

For variable indices, insert a bounds check before the GEP.

### ArraySet

1. `alloca [N x T]` for the array value
2. `store` the array into the alloca
3. GEP with index to get element pointer
4. `store` the new value
5. `load` the updated array

For variable indices, insert a bounds check before the GEP.

### Runtime bounds checking (variable index)

For variable indices, emit:

```llvm
%in_bounds = icmp ult i64 %index, <size>
br i1 %in_bounds, label %ok, label %trap
trap:
  call void @llvm.trap()
  unreachable
ok:
  ; GEP + load/store
```

`@llvm.trap()` is an LLVM intrinsic that immediately terminates the process. No external runtime library is needed.

### Value semantics

Fixed-size arrays are value types, like structs. Assignment copies the entire array. LLVM's `[N x T]` is a value type, so this works naturally.

## Error Messages

| Error | Example | Message |
|-------|---------|---------|
| Mixed element types | `[1, true]` | `array elements must all have the same type: expected 'Int32', found 'Bool'` |
| Size mismatch | `let a: [Int32; 3] = [1, 2]` | `expected array of size 3, found array of size 2` |
| Constant OOB | `a[3]` (size 3) | `array index 3 is out of bounds for array of size 3` |
| Index on non-array | `let x = 5; x[0]` | `cannot index into type 'Int32'` |
| Assign to immutable | `let a = [1,2]; a[0] = 5` | `cannot assign to index of immutable variable` |
| Non-integer index | `a[true]` | `array index must be an integer type, found 'Bool'` |

## Testing Strategy

New file: `tests/arrays.rs`

1. **Array literal and access**: create arrays of each type (Int32, Int64, Float32, Float64, Bool), access elements by constant index
2. **Index assignment**: `var` array element modification
3. **Type annotation**: explicit `[Int32; 3]` annotations
4. **Function params/returns**: pass arrays to functions, return arrays from functions
5. **Type inference**: `let a = [1, 2, 3]` infers `[Int32; 3]`
6. **Error cases**: each error from the table above
7. **Nested arrays**: `[[Int32; 2]; 3]` if naturally supported

## Future Work (tracked in TODO.md)

- Variable-length arrays (`[Int32]` = `Array<Int32>`, heap-allocated)
- `.count` property
- `==` / `!=` comparison
- Slice operations
- `[Int32]()` empty array constructor syntax
