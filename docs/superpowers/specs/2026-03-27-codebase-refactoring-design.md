# Codebase Refactoring: File Splitting for Developer Experience

## Goal

Improve developer experience by splitting large files (>500 lines) into focused, single-responsibility modules. Every resulting file should be ≤500 lines including inline tests.

## Approach

Hybrid strategy: responsibility-based splitting for `semantic/`, syntax-element-based splitting for `parser/`, `bir/lowering`, and `codegen/`. Tests follow their associated code inline (`#[cfg(test)] mod tests`).

## Constraints

- Target: all files ≤500 lines (including inline tests)
- No behavioral changes — pure structural refactoring
- Branch strategy: merge current `feature/type-inference` to `main`, then create a new branch

## Module Hierarchy Strategy

Two patterns are used depending on the code structure:

### Pattern A: Submodule hierarchy (for `impl`-based code)

For files with a private struct and `impl` block (`Lowering`, `Parser`), convert to a directory module:

```
bir/lowering.rs → bir/lowering/mod.rs + bir/lowering/lower_expr.rs + ...
```

Submodules of the defining module can access private fields in Rust, so no visibility changes are needed on struct fields. Each submodule adds `impl Lowering` (or `impl Parser`) with its subset of methods. Private helper types (`StmtResult`, `LoopContext`, etc.) stay in `mod.rs`.

`parser/` already uses this pattern (`parser/mod.rs`), so new submodules are added directly.

### Pattern B: Sibling modules with `pub(super)` (for free functions)

For files using free functions (`semantic/mod.rs`, `codegen/llvm.rs`), extract functions to new sibling files. Functions called across file boundaries get `pub(super)` visibility. Shared helpers (`sem_err`, `sem_err_with_help`, `pkg_err`, `codegen_err`) stay in the parent module and are re-exported or accessed via `super::`.

---

## 1. `semantic/mod.rs` (4641 lines → 11 files, responsibility-based)

All functions in `semantic/mod.rs` are **free functions** (no `Analyzer` struct). Shared data structures (`SemanticInfo`, `PackageSemanticInfo`) and error helpers (`sem_err`, `sem_err_with_help`, `pkg_err`) stay in `mod.rs`. Extracted functions use `pub(super)` when called from other submodules. Type utility functions (`resolve_type_checked`, `check_type_match`, `types_compatible`, `substitute_type`, `is_builtin_type`, `type_annotation_display_name`, ~200 lines) stay in `mod.rs` since they are widely shared.

| File | Responsibility | Key functions | Est. lines |
|------|---------------|---------------|-----------|
| `mod.rs` | Data types, error helpers, type utilities, re-exports | `SemanticInfo`, `PackageSemanticInfo`, `SymbolKind`, `sem_err*`, `pkg_err`, type utility functions, submodule declarations | ~350 |
| `package_analysis.rs` | Multi-module orchestration (excluding single-module) | `analyze_package`, `collect_global_symbols`, `resolve_imports_for_module`, `import_single_symbol`, `import_symbol_to_resolver`, `resolve_import_module_path` | ~350 |
| `single_module_analysis.rs` | Per-module analysis entry | `analyze_single_module` | ~300 |
| `generic_validation.rs` | Generic type validation | `validate_main`, `validate_generics`, `validate_generics_block/stmt/expr`, `validate_constraints` | ~300 |
| `pre_mono.rs` | Pre-monomorphization analysis | `analyze_pre_mono`, `analyze_pre_mono_lenient`, `analyze_pre_mono_inner`, `validate_inferred_constraints` | ~400 |
| `post_mono.rs` | Post-monomorphization analysis | `analyze_post_mono` (pub fn, called from codegen/lowering tests) | ~300 |
| `expr_analysis.rs` | Expression analysis (simple) | `analyze_expr` — literals, binary/unary ops, cast, identifiers, block expr, if, while | ~400 |
| `expr_call_analysis.rs` | Expression analysis (calls) | Function calls (with generic inference), struct init, field access, computed properties | ~400 |
| `expr_method_analysis.rs` | Expression analysis (method calls) | Method calls (~240 lines), related helpers | ~250 |
| `stmt_analysis.rs` | Statement analysis | `analyze_stmt` | ~410 |
| `function_analysis.rs` | Function & control flow analysis | `analyze_function`, `analyze_block_expr`, `analyze_control_block`, `analyze_loop_block`, `stmt_always_returns`, `block_always_returns` | ~230 |
| `struct_analysis.rs` | Struct-specific analysis | `analyze_struct_members`, `check_all_fields_initialized`, `analyze_getter_block`, `check_assignment_target_mutable` | ~300 |

`resolve_struct_members` (~136 lines) stays in `mod.rs` — it is called from 4+ submodules (package_analysis, single_module_analysis, pre_mono, post_mono) and is most naturally a shared utility.

### Test distribution (~808 lines total)

Tests are distributed to the file containing the functions they exercise. If a test file section exceeds the budget, tests are split further or a dedicated `tests.rs` submodule is created for that section.

## 2. `bir/lowering.rs` (2936 lines → directory module, 7 files)

Convert `bir/lowering.rs` to `bir/lowering/mod.rs` with submodules. The `Lowering` struct (36 private fields) and helper types (`StmtResult`, `LoopContext`, `SemInfoRef`, `ReceiverInfo`) stay in `mod.rs`. Submodules access private fields through Rust's descendant module visibility.

| File | Responsibility | Key functions | Est. lines |
|------|---------------|---------------|-----------|
| `mod.rs` | Struct, utilities, scope/var management | `Lowering` struct, `new`, `fresh_value`, `fresh_block`, `seal_block`, `start_block`, `emit`, `push/pop_scope`, `define/lookup/assign_var`, `lower_function`, `lower_block_stmts`, helper types | ~460 (incl. conversion utilities) |
| `lower_expr.rs` | Expression lowering | `lower_expr` (excluding FieldAccess arm, which moves to `lower_struct.rs`) | ~410 |
| `lower_short_circuit.rs` | Short-circuit logic | `lower_short_circuit_and`, `lower_short_circuit_or` | ~200 |
| `lower_stmt.rs` | Statement lowering | `lower_stmt`, `lower_explicit_init` | ~300 |
| `lower_control_flow.rs` | Control flow lowering | `lower_if`, `lower_while` | ~440 |
| `lower_struct.rs` | Struct operation lowering | `emit_struct_init`, `lower_receiver`, `infer_struct_name_no_lower`, `inline_getter`, `try_lower_computed_setter`, `expr_refers_to_self`, `lower_field_assign_recursive`, `collect_mutable_var_values`, FieldAccess arm from `lower_expr` (~120 lines) | ~470 |
| `lower_program.rs` | Program/module entry points | `lower_program`, `lower_program_with_inferred`, `lower_module`, `lower_module_with_inferred` | ~400 |

Conversion utility functions (`convert_binop`, `convert_compare_op`, `semantic_type_to_bir`, `check_acyclic_structs`, `convert_type`, ~110 lines) stay in `mod.rs` which has room.

### Test distribution (~390 lines)

Tests follow their functions. `lower_program.rs` tests cover integration; unit tests go to the relevant submodule.

## 3. `parser/mod.rs` (2472 lines → 5 files, syntax-element-based)

`parser/` already uses `mod.rs`. The `Parser` struct (3 private fields) stays in `mod.rs`. Submodules extend via `impl Parser`.

| File | Responsibility | Key functions | Est. lines |
|------|---------------|---------------|-----------|
| `mod.rs` | Core parser, token ops, type parsing, entry point | `Parser` struct, `new`, `peek`, `advance`, `expect`, span helpers, `parse_type`, `parse_param_list`, `parse_type_params`, `parse()` | ~350 |
| `parse_expr.rs` | Precedence climbing (operators) | `parse_expr` → `parse_and` → ... → `parse_unary`, `parse_cast` | ~300 |
| `parse_primary.rs` | Atoms, postfix, calls | `parse_factor`, `parse_postfix`, `parse_primary`, `parse_postfix_call`, `parse_type_arg_list`, `parse_if_expr`, `parse_while_expr` | ~400 |
| `parse_definition.rs` | Top-level definition parsing | `parse_program`, `parse_function`, `parse_struct_def`, `parse_struct_member`, `parse_protocol_def`, `parse_protocol_member`, `parse_getter`, `parse_setter`, `parse_import_*`, visibility helpers | ~400 |
| `parse_stmt.rs` | Statement parsing | `parse_block`, `parse_stmt`, `expect_ident` | ~150 |

### Test distribution (~1132 lines)

Parser tests are large (~1132 lines). With ~1340 lines of code split across 5 files, inline tests will cause multiple files to exceed 500 lines. **Primary plan**: create `parser/tests.rs` as a dedicated test submodule (requires making `Parser` and its methods `pub(super)`). This keeps code files clean and tests centralized. If `tests.rs` itself exceeds 500 lines, split into `parser/tests/` directory with per-category files (`test_expr.rs`, `test_definition.rs`, etc.).

## 4. `codegen/llvm.rs` (2459 lines → 6 files, hybrid)

All functions are module-level free functions. Convert `codegen/llvm.rs` to sibling files under `codegen/`. Shared helpers use `pub(super)`.

The `emit_instruction` function is ~531 lines (a single large match). It is split into two files by instruction category.

| File | Responsibility | Key functions | Est. lines |
|------|---------------|---------------|-----------|
| `llvm.rs` | Entry points, function compilation, linking | `compile`, `compile_module`, `compile_to_module`, `compile_function`, `link_objects`, `codegen_err` | ~370 |
| `emit_arithmetic.rs` | Arithmetic/logic instruction emission | `emit_instruction` — literals, binary ops, comparisons, casts, unary | ~300 |
| `emit_structural.rs` | Struct/array/call instruction emission | `emit_instruction` — calls, struct init/get/set, array index/store, `emit_terminator`, `store_br_args`, `emit_bounds_check`, `load_value`, `find_block` | ~400 |
| `types.rs` | Type system bridge | `bir_type_to_llvm_type`, `collect_value_types`, `build_struct_types`, `build_generic_struct_types`, `collect_type_params`, `contains_type_param` | ~250 |
| `generic_resolution.rs` | Generic function resolution | `resolve_instruction`, `resolve_terminator`, `resolve_basic_block`, `resolve_function` | ~300 |
| `mono_compile.rs` | Monomorphic compilation | `compile_with_mono`, `compile_to_module_with_mono`, `compile_module_with_mono` | ~460 |

Note: Splitting `emit_instruction` requires extracting match arms into helper functions (e.g., `emit_binary_op`, `emit_call`, `emit_struct_init`) that the two emit files call. Shared context (`EmitCtx` or equivalent parameters) is defined in `llvm.rs` with `pub(super)` on both the struct and its fields.

### Test distribution (~382 lines)

Tests follow their functions. Arithmetic tests → `emit_arithmetic.rs`, struct/integration tests → remaining files.

## 5. Medium Files (500–1134 lines)

| File | Lines | Action |
|------|-------|--------|
| `semantic/infer.rs` | 1134 | Extract `unify()` + `find()` + unification tests → `semantic/unify.rs` (~200 lines). Remaining: ~490 lines code + ~440 lines tests → if over 500, extract provenance helpers or further split tests |
| `pipeline.rs` | 626 | Extract `build_name_map` + `collect_external_functions` → `pipeline_helpers.rs` |
| `bir/mono.rs` | 609 | Inspect and split if >500 lines |
| `lexer/mod.rs` | 523 | Inspect and split if >500 lines |
| `bir/printer.rs` | 524 | Inspect and split if >500 lines |

## 6. Test Files

| File | Lines | Action |
|------|-------|--------|
| `tests/control_flow.rs` | 524 | Split → `tests/control_flow_if.rs` + `tests/control_flow_loop.rs` |
| `tests/type_inference.rs` | 513 | Split by category (literals, generics, errors, etc.) |

Files ≤500 lines are left unchanged.

## Non-Goals

- No API changes, no new features
- No changes to compilation pipeline behavior
- No changes to files already ≤500 lines
- No external test file reorganization (beyond the two listed above)

## Verification

- `cargo build` succeeds
- `cargo test` passes with no regressions
- No file exceeds 500 lines
- `cargo clippy` produces no new warnings
- `cargo fmt` applied
