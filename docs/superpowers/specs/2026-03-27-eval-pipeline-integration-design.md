# Eval Subcommand Full Pipeline Integration

## Overview

Replace the JIT-based `eval` subcommand with native compile + run, unifying it with the `compile` subcommand's pipeline path. This eliminates the last JIT dependency in `main.rs`.

## Scope

- Rewrite `eval` in `src/main.rs` to use native compile + run instead of JIT
- Remove inkwell JIT imports from `main.rs`
- Keep `--emit-bir` functionality via `compile_source_to_bir`

## Motivation

The `eval` subcommand currently uses `compile_source_to_bir` (pipeline through optimize) then JIT-executes without monomorphization. This means generic functions don't work in eval. After the previous cleanup (JIT test removal), eval is the last JIT consumer in the main binary. Unifying it with native compile + run completes the pipeline consolidation.

## Design

### Eval Flow Change

**Before:** `compile_source_to_bir` → `compile_to_module` (no mono) → JIT execute → print result

**After:**
- Without `--emit-bir`: `compile_source_to_objects` → write to temp file → `cc` link → execute → print exit code
- With `--emit-bir`: `compile_source_to_bir` (print BIR) then `compile_source_to_objects` → native execute → print exit code

### Implementation

In `src/main.rs`, replace the `Command::Eval` branch:

1. If `emit_bir`, call `compile_source_to_bir` and print BIR texts
2. Call `compile_source_to_objects` to get object bytes
3. Write object bytes to a temp file, link with `cc`, execute the binary
4. Print the exit code

Remove JIT imports from `main.rs`:
- `inkwell::context::Context` (no longer referenced after removing JIT block)
- `inkwell::OptimizationLevel` (no longer referenced)

Note: Check if any other code in `main.rs` uses inkwell. Currently, only the `eval` JIT block does.

### Error Handling

Use `miette` for errors, matching the existing `compile` subcommand pattern. Pipeline errors go through `into_diagnostic()`. Linker and execution errors use `miette::miette!()`.

## Changed Files

| File | Change |
|------|--------|
| `src/main.rs` | Rewrite `eval` branch to native compile + run; remove JIT inkwell imports |

## Not Changed

- `src/lib.rs` — `compile_source_to_bir` and `compile_source_to_objects` unchanged
- `src/pipeline.rs` — unchanged
- Test files — eval is a CLI subcommand, no unit test impact

## Testing

Manual verification:

```bash
# Basic eval
cargo run -- eval "func main() -> Int32 { return 42; }"
# Expected: 42

# Eval with BIR output
cargo run -- eval --emit-bir "func main() -> Int32 { return 42; }"
# Expected: BIR text followed by 42

# Generic function (previously broken without mono)
cargo run -- eval "func identity<T>(x: T) -> T { return x; } func main() -> Int32 { return identity(42); }"
# Expected: 42
```
