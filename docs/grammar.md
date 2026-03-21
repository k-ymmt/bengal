# Bengal Language Grammar — Phase 2

## Overview

Bengal is an expression-oriented language that compiles to WebAssembly.
Phase 2 adds function definitions, variable bindings, block expressions, and function calls.

Key design principles:

- **Explicit returns**: Functions return values with `return`, block expressions with `yield`.
- **Mandatory type annotations**: All variables and return types must be explicitly annotated.
- **Immutable by default**: `let` bindings are immutable, `var` bindings are mutable.

## Lexical Grammar

```ebnf
(* Literals *)
integer_literal = digit , { digit } ;
digit           = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;

(* Identifiers *)
identifier = letter_or_underscore , { letter_or_underscore | digit } ;
letter_or_underscore = "a".."z" | "A".."Z" | "_" ;

(* Keywords *)
keyword = "func" | "let" | "var" | "return" | "yield" ;

(* Operators *)
additive_op       = "+" | "-" ;
multiplicative_op = "*" | "/" ;

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

function   = "func" , identifier , param_list , "->" , type , block ;

param_list = "(" , [ param , { "," , param } ] , ")" ;
param      = identifier , ":" , type ;
type       = "i32" ;

block      = "{" , { statement } , "}" ;

statement  = let_stmt | var_stmt | assign_stmt | return_stmt | yield_stmt | expr_stmt ;
let_stmt    = "let" , identifier , ":" , type , "=" , expression , ";" ;
var_stmt    = "var" , identifier , ":" , type , "=" , expression , ";" ;
assign_stmt = identifier , "=" , expression , ";" ;
return_stmt = "return" , expression , ";" ;
yield_stmt  = "yield" , expression , ";" ;
expr_stmt   = expression , ";" ;

expression = term , { additive_op , term } ;
term       = factor , { multiplicative_op , factor } ;
factor     = integer_literal
           | identifier , "(" , [ expression , { "," , expression } ] , ")"   (* call *)
           | identifier                                                        (* variable *)
           | "(" , expression , ")"                                            (* grouping *)
           | block                                                             (* block expr *)
           ;
```

## Operator Precedence and Associativity

| Precedence | Operators | Associativity |
|---|---|---|
| High | `*` `/` | Left |
| Low | `+` `-` | Left |

## Type System

In Phase 2, all expressions evaluate to `i32`.

| Type | Description | Phase |
|---|---|---|
| `i32` | 32-bit signed integer | 1 |
| `i64` | 64-bit signed integer | Future |
| `f32` | 32-bit floating point | Future |
| `f64` | 64-bit floating point | Future |
| `bool` | Boolean (`true` / `false`) | Future |
| `()` | Unit type (no value) | Future |

## Semantic Rules (Phase 2)

- `main` function must exist, take no parameters, and return `i32`.
- `let` variables are immutable; `var` variables are mutable.
- Shadowing is allowed (re-declaring a variable in the same or inner scope).
- `return` must be the last statement in a function body.
- `yield` must be the last statement in a block expression.
- `return` is not allowed inside block expressions (Phase 2).
- `yield` is not allowed in function bodies.
- All functions must end with a `return` statement.
- All block expressions must end with a `yield` statement.

## Features Not Yet Supported

- **Control flow**: `if`/`else` expressions, loops
- **Additional types**: `i64`, `f32`, `f64`, `bool`, `()`
- **Unit return type**: `-> ()` or omitted return type
- **Early return**: `return` in non-tail position
- **Unary operators**: Negation (`-`)
- **Comparison operators**: `==`, `!=`, `<`, `>`, `<=`, `>=`
