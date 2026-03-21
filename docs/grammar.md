# Bengal Language Grammar — Phase 3

## Overview

Bengal is an expression-oriented language that compiles to WebAssembly.
Phase 3 adds control flow (`if`/`else`, `while`), `bool` type, `()` (Unit) type, comparison and logical operators, and early `return`.

Key design principles:

- **Explicit returns**: Functions return values with `return`, block expressions with `yield`.
- **Mandatory type annotations**: All variables and return types must be explicitly annotated.
- **Immutable by default**: `let` bindings are immutable, `var` bindings are mutable.
- **Expression-oriented**: `if`/`else` is an expression that produces a value via `yield`.

## Lexical Grammar

```ebnf
(* Literals *)
integer_literal = digit , { digit } ;
digit           = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
bool_literal    = "true" | "false" ;

(* Identifiers *)
identifier = letter_or_underscore , { letter_or_underscore | digit } ;
letter_or_underscore = "a".."z" | "A".."Z" | "_" ;

(* Keywords *)
keyword = "func" | "let" | "var" | "return" | "yield"
        | "true" | "false" | "if" | "else" | "while" ;

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
type       = "i32" | "bool" | "(" , ")" ;

block      = "{" , { statement } , "}" ;

statement  = let_stmt | var_stmt | assign_stmt | return_stmt | yield_stmt | expr_stmt ;
let_stmt    = "let" , identifier , ":" , type , "=" , expression , ";" ;
var_stmt    = "var" , identifier , ":" , type , "=" , expression , ";" ;
assign_stmt = identifier , "=" , expression , ";" ;
return_stmt = "return" , [ expression ] , ";" ;
yield_stmt  = "yield" , expression , ";" ;
expr_stmt   = expression , ";" ;

expression       = or_expr ;
or_expr          = and_expr , { "||" , and_expr } ;
and_expr         = equality_expr , { "&&" , equality_expr } ;
equality_expr    = comparison_expr , { ( "==" | "!=" ) , comparison_expr } ;
comparison_expr  = additive_expr , { ( "<" | ">" | "<=" | ">=" ) , additive_expr } ;
additive_expr    = term , { ( "+" | "-" ) , term } ;
term             = unary , { ( "*" | "/" ) , unary } ;
unary            = "!" , unary | factor ;
factor           = integer_literal
                 | bool_literal
                 | identifier , "(" , [ expression , { "," , expression } ] , ")"  (* call *)
                 | identifier                                                       (* variable *)
                 | "(" , expression , ")"                                           (* grouping *)
                 | block                                                            (* block expr *)
                 | if_expr
                 | while_expr
                 ;

if_expr    = "if" , expression , block , [ "else" , block ] ;
while_expr = "while" , expression , block ;
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
| 7 (highest) | `!` (prefix) | Right |

## Type System

| Type | Description | Phase |
|---|---|---|
| `i32` | 32-bit signed integer | 1 |
| `bool` | Boolean (`true` / `false`) | 3 |
| `()` | Unit type (no value) | 3 |
| `i64` | 64-bit signed integer | Future |
| `f32` | 32-bit floating point | Future |
| `f64` | 64-bit floating point | Future |

## Semantic Rules

### Functions
- `main` function must exist, take no parameters, and return `i32`.
- Function return type defaults to `()` if `->` is omitted.
- All functions must end with a `return` statement.
- `return` may appear at any position (early return is allowed in Phase 3).
- `return;` (no value) is valid for `()` return type functions.

### Variables
- `let` variables are immutable; `var` variables are mutable.
- Shadowing is allowed (re-declaring a variable in the same or inner scope).
- Variable initializers and assignments must match the declared type.

### Block expressions
- `yield` must be the last statement in a block expression.
- `return` is not allowed inside block expressions (`{ ... }` used as values).
- All block expressions must end with a `yield` statement.

### if/else
- `if` condition must be `bool`.
- `if`/`else` with both branches: then and else types must match (or one may diverge via `return`).
- `if` without `else`: type is `()`.
- When one branch diverges (via `return`), the other branch's type is used.

### while
- `while` condition must be `bool`.
- `while` type is always `()`.
- `yield` is not allowed in while loop body.
- `return` is allowed in while loop body (early return).
- `break`/`continue` are not supported (Phase 4).

### Operators
- Arithmetic (`+`, `-`, `*`, `/`): both operands `i32`, result `i32`.
- Comparison (`==`, `!=`, `<`, `>`, `<=`, `>=`): both operands `i32`, result `bool`.
- Logical (`&&`, `||`): both operands `bool`, result `bool`. Short-circuit evaluation.
- Logical not (`!`): operand `bool`, result `bool`.

## Features Not Yet Supported

- **Loop control**: `break` / `continue` (Phase 4)
- **Additional types**: `i64`, `f32`, `f64`
- **Type inference**: `let x = 1 + 2;` (Phase 4)
- **Type conversion**: `as` casts (Phase 4)
- **Unary minus**: `-x` negation
