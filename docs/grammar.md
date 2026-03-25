# Bengal Language Grammar — Phase 6

## Overview

Bengal is an expression-oriented language that compiles to native code via LLVM.
Features include loop control (`break`/`continue`), multiple numeric types (`Int64`, `Float32`, `Float64`), local type inference, `as` casts, `while` expressions with `break` values and `nobreak` blocks, constant folding optimization, structs with stored/computed properties, initializers, and methods, and protocols for shared interfaces.

Key design principles:

- **Explicit returns**: Functions return values with `return`, block expressions with `yield`.
- **Optional type annotations**: Type annotations on `let`/`var` can be omitted when the type can be inferred from the initializer.
- **Immutable by default**: `let` bindings are immutable, `var` bindings are mutable.
- **Expression-oriented**: `if`/`else` and `while` are expressions that produce values.
- **Value-type structs**: Structs are stack-allocated value types with stored properties, computed properties, custom initializers, and methods.
- **Protocols**: Shared interfaces that structs can conform to, enabling compile-time checked structural contracts.

## Lexical Grammar

```ebnf
(* Literals *)
integer_literal = digit , { digit } ;
float_literal   = digit , { digit } , "." , digit , { digit } ;
digit           = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
bool_literal    = "true" | "false" ;

(* Identifiers *)
identifier = letter_or_underscore , { letter_or_underscore | digit } ;
letter_or_underscore = "a".."z" | "A".."Z" | "_" ;

(* Keywords *)
keyword = "func" | "let" | "var" | "return" | "yield"
        | "true" | "false" | "if" | "else" | "while"
        | "break" | "continue" | "as" | "nobreak"
        | "struct" | "init" | "self" | "get" | "set"
        | "protocol" ;

(* Arithmetic operators *)
additive_op       = "+" | "-" ;
multiplicative_op = "*" | "/" ;

(* Comparison operators *)
equality_op   = "==" | "!=" ;
relational_op = "<" | ">" | "<=" | ">=" ;

(* Logical operators *)
logical_or  = "||" ;
logical_and = "&&" ;
logical_not = "!" ;

(* Symbols *)
arrow     = "->" ;
colon     = ":" ;
semicolon = ";" ;
comma     = "," ;
eq        = "=" ;
dot       = "." ;

(* Delimiters *)
lparen = "(" ;
rparen = ")" ;
lbrace = "{" ;
rbrace = "}" ;

(* Whitespace — skipped *)
whitespace = " " | "\t" | "\n" | "\r" ;
```

## Syntactic Grammar

```ebnf
program    = { top_level } ;
top_level  = function | struct_def | protocol_def ;

function   = "func" , identifier , param_list , [ "->" , type ] , block ;
           (* return type defaults to () if omitted *)

param_list = "(" , [ param , { "," , param } ] , ")" ;
param      = identifier , ":" , type ;
type       = "Int32" | "Int64" | "Float32" | "Float64" | "Bool" | "Void" | "(" , ")"
           | identifier ;   (* named struct type *)

block      = "{" , { statement } , "}" ;

statement  = let_stmt | var_stmt | assign_stmt | field_assign_stmt
           | return_stmt | yield_stmt | break_stmt | continue_stmt | expr_stmt ;
let_stmt          = "let" , identifier , [ ":" , type ] , "=" , expression , ";" ;
var_stmt          = "var" , identifier , [ ":" , type ] , "=" , expression , ";" ;
assign_stmt       = identifier , "=" , expression , ";" ;
field_assign_stmt = field_access , "=" , expression , ";" ;
return_stmt       = "return" , [ expression ] , ";" ;
yield_stmt        = "yield" , expression , ";" ;
break_stmt        = "break" , [ expression ] , ";" ;
continue_stmt     = "continue" , ";" ;
expr_stmt         = expression , ";" ;

expression       = or_expr ;
or_expr          = and_expr , { "||" , and_expr } ;
and_expr         = equality_expr , { "&&" , equality_expr } ;
equality_expr    = comparison_expr , { ( "==" | "!=" ) , comparison_expr } ;
comparison_expr  = additive_expr , { ( "<" | ">" | "<=" | ">=" ) , additive_expr } ;
additive_expr    = term , { ( "+" | "-" ) , term } ;
term             = cast_expr , { ( "*" | "/" ) , cast_expr } ;
cast_expr        = unary , { "as" , type } ;
unary            = "!" , unary | factor ;
factor           = integer_literal
                 | float_literal
                 | bool_literal
                 | identifier , "(" , labeled_args , ")"                            (* struct init *)
                 | identifier , "(" , [ expression , { "," , expression } ] , ")"   (* call *)
                 | identifier                                                        (* variable *)
                 | "self"                                                             (* self ref *)
                 | "(" , expression , ")"                                            (* grouping *)
                 | block                                                             (* block expr *)
                 | if_expr
                 | while_expr
                 ;

(* Postfix operators: field/method access binds tighter than all binary operators *)
postfix    = factor , { "." , identifier , [ "(" , [ arg_list ] , ")" ] } ;
           (* field access: a.b.c → (a.b).c — left-associative *)
           (* method call: obj.method(args) — identifier followed by parenthesized arguments *)
           (* postfix replaces factor in the precedence chain: unary uses postfix *)

arg_list   = expression , { "," , expression } ;

labeled_args = labeled_arg , { "," , labeled_arg } ;
labeled_arg  = identifier , ":" , expression ;

if_expr    = "if" , expression , block , [ "else" , block ] ;
while_expr = "while" , expression , block , [ "nobreak" , block ] ;

(* Struct definitions *)
struct_def = "struct" , identifier , [ ":" , identifier_list ] , "{" , { struct_member } , "}" ;

identifier_list = identifier , { "," , identifier } ;

struct_member = stored_property | computed_property | initializer | method ;

stored_property   = "var" , identifier , ":" , type , ";" ;

computed_property = "var" , identifier , ":" , type ,
                    "{" , getter , [ setter ] , "}" , ";" ;
getter            = "get" , block ;
setter            = "set" , block ;

initializer       = "init" , param_list , block ;

method            = "func" , identifier , param_list , [ "->" , type ] , block ;

(* Protocol definitions *)
protocol_def = "protocol" , identifier , "{" , { protocol_member } , "}" ;

protocol_member = method_sig | property_req ;

method_sig    = "func" , identifier , param_list , [ "->" , type ] , ";" ;

property_req  = "var" , identifier , ":" , type , "{" , "get" , [ "set" ] , "}" , ";" ;
```

## Operator Precedence and Associativity

| Precedence | Operators | Associativity |
|---|---|---|
| 1 (lowest) | `\|\|` | Left |
| 2 | `&&` | Left |
| 3 | `==` `!=` | Left |
| 4 | `<` `>` `<=` `>=` | Left |
| 5 | `+` `-` | Left |
| 6 | `*` `/` | Left |
| 7 | `as` (postfix cast) | Left |
| 8 | `!` (prefix) | Right |
| 9 (highest) | `.` (field access / method call) | Left |

## Type System

| Type | Description | Phase |
|---|---|---|
| `Int32` | 32-bit signed integer | 1 |
| `Bool` | Boolean (`true` / `false`) | 3 |
| `()` / `Void` | Unit type (no value) | 3 |
| `Int64` | 64-bit signed integer | 4 |
| `Float32` | 32-bit floating point | 4 |
| `Float64` | 64-bit floating point | 4 |
| *StructName* | User-defined struct type (value type) | 5 |

### Default literal types
- Integer literals (`42`) default to `Int32`.
- Float literals (`3.14`) default to `Float64`.

## Semantic Rules

### Functions
- `main` function must exist, take no parameters, and return `Int32`.
- Function return type defaults to `()` if `->` is omitted.
- All functions must end with a `return` statement.
- `return` may appear at any position (early return is allowed).
- `return;` (no value) is valid for `()` return type functions.

### Variables and type inference
- `let` variables are immutable; `var` variables are mutable.
- Shadowing is allowed (re-declaring a variable in the same or inner scope).
- Type annotations on `let`/`var` are optional. When omitted, the type is inferred from the initializer expression.
- When a type annotation is present, the initializer type must match.

### Block expressions
- `yield` must be the last statement in a block expression.
- `return` is not allowed inside block expressions (`{ ... }` used as values).
- All block expressions must end with a `yield` statement.

### if/else
- `if` condition must be `Bool`.
- `if`/`else` with both branches: then and else types must match (or one may diverge via `return`, `break`, or `continue`).
- `if` without `else`: type is `()`.
- When one branch diverges, the other branch's type is used.

### while, break, continue
- `while` condition must be `Bool`.
- `break` and `continue` are only allowed inside a `while` loop body.
- `break;` exits the innermost loop. `break expr;` exits and provides a value.
- `continue;` jumps to the loop header (re-evaluates condition).
- `break` and `continue` are treated as diverging (like `return`) in control flow analysis.

#### while expression type and nobreak

The type of a `while` expression is determined by its `break` statements:

| Condition | break type | nobreak | while type |
|---|---|---|---|
| `while true` | Unit (`break;` or none) | Forbidden | `()` |
| `while true` | Non-Unit (`break expr`) | Forbidden | break value type |
| `while cond` | Unit (`break;` or none) | Optional | `()` |
| `while cond` | Non-Unit (`break expr`) | **Required** | break/nobreak common type |

- `while true` + `nobreak` → compile error (nobreak is unreachable).
- `while cond` + non-Unit break + no `nobreak` → compile error (condition-false path has no value).
- `nobreak` block type must match the break type. It uses `yield` to provide a value.

### Structs

#### Definition
- Struct names must be unique across all top-level definitions (no duplicate with functions or other structs).
- A struct body contains stored properties, computed properties, and at most one initializer.
- Member names must be unique within a struct.
- Recursive structs (a struct containing a stored field of its own type) are rejected.

#### Stored properties
- Declared with `var name: Type;`.
- All stored properties must be initialized in the initializer body via `self.field = value;`.

#### Computed properties
- Declared with `var name: Type { get { ... } set { ... } };`.
- Getter is required and must end with a `return` statement.
- Setter is optional. If absent, the property is read-only (assignment is a compile error).
- Inside getter, `self` is immutable. Inside setter, `self` is mutable.
- Setter receives an implicit parameter `newValue` of the property's type.

#### Initializer
- Declared with `init(params) { ... }`.
- At most one explicit `init` per struct.
- Parameters are immutable.
- All stored properties must be assigned via `self.field = value;` before the initializer returns.
- If no explicit `init` is provided, a memberwise initializer is auto-generated with parameters matching stored properties in declaration order.
- When an explicit `init` is defined, the memberwise initializer is unavailable.

#### Struct initialization
- With memberwise init: `Point(x: 1, y: 2)` — labeled arguments in field declaration order.
- With explicit init: `Counter(start: 5)` — labeled arguments matching init parameters.
- Zero-argument init (empty struct or no-param init): `Empty()` — parsed as a call expression, resolved to struct init by semantic analysis.
- Struct init expressions return a value of the struct's type.

#### Field access
- `object.field` reads a stored property or invokes a computed property getter.
- Chains are left-associative: `a.b.c` is `(a.b).c`.
- The object must be a struct type and the field must exist.

#### Field assignment
- `object.field = value;` writes a stored property or invokes a computed property setter.
- The object must be mutable (`var` binding, not `let`).
- For computed properties, a setter must be defined.
- Nested field assignment (e.g., `o.inner.x = 10;`) is supported.
- Nested field assignment on `self` during init (e.g., `self.inner.x = 10;`) is rejected because `self` is not fully materialized during initialization.

#### Methods
- Declared with `func name(params) -> ReturnType { ... }` inside a struct body.
- Methods receive an implicit `self` parameter referring to the struct instance.
- `self` is immutable inside method bodies (like getter context).
- Method calls use dot syntax: `obj.method(args)`.
- Methods are mangled to `StructName_methodName` to avoid name collisions with top-level functions.
- A struct may have any number of methods.

#### `self`
- Can only be used inside an initializer, getter, setter, or method body.
- `self` is mutable in initializer and setter contexts, immutable in getter and method contexts.
- During init body, `self` is not materialized as a struct value; fields are accessed as individual variables. Bare `self` usage (e.g., `let s = self;`) and computed property access on `self` are not allowed in init bodies.

### Protocols

#### Definition
- Protocols are declared at the top level with `protocol Name { ... }`.
- A protocol body contains method signatures and property requirements.
- Method signatures have no body: `func name(params) -> ReturnType;`.
- Property requirements specify getter/setter availability: `var name: Type { get set };`.
- Protocol names must be unique across all top-level definitions.

#### Conformance
- A struct declares conformance with a colon after its name: `struct Point: Summable { ... }`.
- A struct may conform to multiple protocols: `struct Point: Summable, Printable { ... }`.
- The compiler verifies that the struct provides all required methods and properties.
- Method signatures must match exactly (name, parameter types, return type).
- Conformance is checked at compile time (static dispatch, no vtable).

### Operators
- Arithmetic (`+`, `-`, `*`, `/`): both operands must be the same numeric type. Result is that type.
- Comparison (`==`, `!=`, `<`, `>`, `<=`, `>=`): both operands must be the same numeric type. Result is `Bool`.
- Logical (`&&`, `||`): both operands `Bool`, result `Bool`. Short-circuit evaluation.
- Logical not (`!`): operand `Bool`, result `Bool`.

### Type conversion (as)
- `expr as type` converts between numeric types (`Int32`, `Int64`, `Float32`, `Float64`).
- Casting to/from `Bool` is not allowed.
- `as` binds tighter than all binary operators but lower than `!` (prefix).

### Integer literal range
- Integer literals default to `Int32`. Values outside `Int32` range (`-2147483648` to `2147483647`) are a compile error.

## Optimizations

- **Constant folding**: Compile-time evaluation of arithmetic and comparison on literal operands (e.g., `2 + 3` → `5`).

## Examples

### Struct with methods

```bengal
struct Counter {
    var value: Int32;

    func get() -> Int32 {
        return self.value;
    }

    func addedTo(other: Int32) -> Int32 {
        return self.value + other;
    }
}

func main() -> Int32 {
    let c = Counter(value: 10);
    return c.addedTo(c.get());
}
```

### Protocol definition and conformance

```bengal
protocol Summable {
    func sum() -> Int32;
}

struct Point: Summable {
    var x: Int32;
    var y: Int32;

    func sum() -> Int32 {
        return self.x + self.y;
    }
}

func main() -> Int32 {
    let p = Point(x: 3, y: 4);
    return p.sum();
}
```

### Multiple protocol conformance

```bengal
protocol HasArea {
    func area() -> Int32;
}

protocol HasPerimeter {
    func perimeter() -> Int32;
}

struct Square: HasArea, HasPerimeter {
    var side: Int32;

    func area() -> Int32 {
        return self.side * self.side;
    }

    func perimeter() -> Int32 {
        return self.side * 4;
    }
}
```

## Features Not Yet Supported

- **Unary minus**: `-x` negation
- **String type**: string literals and operations
- **Arrays**: array data type
- **Closures/first-class functions**
- **Integer literal suffixes**: `42i64` (use `42 as Int64` instead)
- **Existential types**: using a protocol as a variable/argument type (`var x: Summable = ...`)
- **Extension conformance**: `extension Point: Drawable { ... }` for retroactive conformance
- **Default implementations in protocols**
- **Protocol inheritance**: `protocol A: B { ... }`
