# Bengal Language Grammar — Phase 1

## Overview

Bengal is an expression-oriented language that compiles to WebAssembly.
In Phase 1, the language supports integer arithmetic with the four basic operators and parenthesized grouping.

Key design principles:

- **Expression-oriented**: Every construct produces a value. In Phase 2+, block expressions, `if` expressions, and implicit return will be introduced.
- **Statement separation**: Semicolons will distinguish expressions from statements (Phase 2+).
- **Unit type**: `()` (unit) exists as the type of expressions evaluated for side effects only.

## Lexical Grammar

```ebnf
(* Literals *)
integer_literal = digit , { digit } ;
digit           = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;

(* Operators *)
additive_op       = "+" | "-" ;
multiplicative_op = "*" | "/" ;

(* Delimiters *)
lparen = "(" ;
rparen = ")" ;

(* Whitespace — skipped *)
whitespace = " " | "\t" | "\n" | "\r" ;
```

## Syntactic Grammar

```ebnf
program    = expression ;

expression = term , { additive_op , term } ;
term       = factor , { multiplicative_op , factor } ;
factor     = integer_literal
           | lparen , expression , rparen ;
```

## Operator Precedence and Associativity

| Precedence | Operators | Associativity |
|---|---|---|
| High | `*` `/` | Left |
| Low | `+` `-` | Left |

## Type System

In Phase 1, all expressions evaluate to `i32`.

The full type system (introduced incrementally across phases):

| Type | Description | Phase |
|---|---|---|
| `i32` | 32-bit signed integer | 1 |
| `i64` | 64-bit signed integer | Future |
| `f32` | 32-bit floating point | Future |
| `f64` | 64-bit floating point | Future |
| `bool` | Boolean (`true` / `false`) | Future |
| `()` | Unit type (no value) | Future |

## Features Not Yet Supported (Phase 1)

The following features are planned for future phases:

- **Variables**: `let` (immutable) and `var` (mutable) bindings with mandatory type annotations (Phase 2–3)
- **Functions**: User-defined functions with parameters and return types
- **Control flow**: `if`/`else` expressions, loops
- **Block expressions**: `{ ... }` that evaluate to the last expression
- **Semicolons**: Statement separation and expression/statement distinction
- **Additional types**: `i64`, `f32`, `f64`, `bool`, `()`
- **Unary operators**: Negation (`-`)
- **Comparison operators**: `==`, `!=`, `<`, `>`, `<=`, `>=`
