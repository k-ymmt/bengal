# Bengal Language Grammar — Phase 4

## Overview

Bengal is an expression-oriented language that compiles to native code via LLVM.
Phase 4 adds loop control (`break`/`continue`), multiple numeric types (`Int64`, `Float32`, `Float64`), local type inference, `as` casts, `while` expressions with `break` values and `nobreak` blocks, and constant folding optimization.

Key design principles:

- **Explicit returns**: Functions return values with `return`, block expressions with `yield`.
- **Optional type annotations**: Type annotations on `let`/`var` can be omitted when the type can be inferred from the initializer.
- **Immutable by default**: `let` bindings are immutable, `var` bindings are mutable.
- **Expression-oriented**: `if`/`else` and `while` are expressions that produce values.

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
        | "break" | "continue" | "as" | "nobreak" ;

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
program    = { function } ;

function   = "func" , identifier , param_list , [ "->" , type ] , block ;
           (* return type defaults to () if omitted *)

param_list = "(" , [ param , { "," , param } ] , ")" ;
param      = identifier , ":" , type ;
type       = "Int32" | "Int64" | "Float32" | "Float64" | "Bool" | "Void" | "(" , ")" ;

block      = "{" , { statement } , "}" ;

statement  = let_stmt | var_stmt | assign_stmt | return_stmt | yield_stmt
           | break_stmt | continue_stmt | expr_stmt ;
let_stmt      = "let" , identifier , [ ":" , type ] , "=" , expression , ";" ;
var_stmt      = "var" , identifier , [ ":" , type ] , "=" , expression , ";" ;
assign_stmt   = identifier , "=" , expression , ";" ;
return_stmt   = "return" , [ expression ] , ";" ;
yield_stmt    = "yield" , expression , ";" ;
break_stmt    = "break" , [ expression ] , ";" ;
continue_stmt = "continue" , ";" ;
expr_stmt     = expression , ";" ;

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
                 | identifier , "(" , [ expression , { "," , expression } ] , ")"  (* call *)
                 | identifier                                                       (* variable *)
                 | "(" , expression , ")"                                           (* grouping *)
                 | block                                                            (* block expr *)
                 | if_expr
                 | while_expr
                 ;

if_expr    = "if" , expression , block , [ "else" , block ] ;
while_expr = "while" , expression , block , [ "nobreak" , block ] ;
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
| 8 (highest) | `!` (prefix) | Right |

## Type System

| Type | Description | Phase |
|---|---|---|
| `Int32` | 32-bit signed integer | 1 |
| `Bool` | Boolean (`true` / `false`) | 3 |
| `()` / `Void` | Unit type (no value) | 3 |
| `Int64` | 64-bit signed integer | 4 |
| `Float32` | 32-bit floating point | 4 |
| `Float64` | 64-bit floating point | 4 |

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

## Features Not Yet Supported

- **Unary minus**: `-x` negation
- **String type**: string literals and operations
- **Arrays/structs**: composite data types
- **Closures/first-class functions**
- **Integer literal suffixes**: `42i64` (use `42 as Int64` instead)
