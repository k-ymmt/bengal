# Bengal Project Instructions

## Project Overview

Bengal is a statically-typed, expression-oriented programming language that compiles to native code via LLVM, written in Rust.

### Compilation Pipeline

`parse` → `analyze` → `lower` → `optimize` → `monomorphize` → `codegen` → `link`

Defined in `src/pipeline.rs`, orchestrated from `src/lib.rs`.

### Source Layout

```
src/
  lexer/          # Tokenizer (logos)
  parser/         # AST parser
  semantic/       # Name resolution, type inference, type checking
  bir/            # Bengal IR: lowering, optimization, monomorphization, printer
  codegen/        # LLVM code generation (inkwell)
  pipeline.rs     # Stage types and stage functions
  lib.rs          # Public API (compile_to_executable, compile_source_to_bir)
  main.rs         # CLI entry point (clap)
  package.rs      # Module graph and package resolution
  mangle.rs       # Symbol name mangling
  error.rs        # Error types (miette, thiserror)
tests/            # Integration tests (per-feature test files)
examples/         # Example .bengal programs
docs/grammar.md   # Language grammar specification (EBNF)
```

### Language Features

Generics with monomorphization, protocols, structs (value-type), module system, fixed-size arrays, expression-oriented control flow, local type inference.

## Important

- **This project is under active development. Do NOT worry about backward compatibility.** Breaking changes to APIs, data formats, syntax, or internal structures are perfectly fine. Always choose the best design for the future over preserving existing behavior.
- Always commit when work is done (only tracked files — if `git diff` or `git status` shows changes to tracked files, commit them)
- When planning changes, prioritize future extensibility and performance over change scope or backward compatibility.
- NEVER use the `-C` option with git commands. The working directory is always correct — the `-C` flag is unnecessary and must not be used.