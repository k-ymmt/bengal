# Test Reorganization Design

## Goal

Reorganize the monolithic `tests/compile_test.rs` (~1586 lines) into feature-based test files with shared helpers, and add missing test coverage for language features.

## Current State

- Single file `tests/compile_test.rs` containing all integration tests
- Mixed concerns: JIT tests, native emit tests, error cases, module system tests
- 5 helper functions duplicated/co-located in one file
- Phase-based section comments that don't align well with feature boundaries
- Notable test coverage gaps (Float32, recursive functions, complex struct scenarios, etc.)

## File Structure

```
tests/
  common/
    mod.rs            # Shared helper functions
  expressions.rs      # Literals, arithmetic, precedence, parentheses, casts
  functions.rs        # Function definition, calls, recursion, parameters
  variables.rs        # let/var, shadowing, scoping, type inference
  control_flow.rs     # if/else, while, break/continue, nobreak, divergence
  structs.rs          # Struct definition, fields, init, computed properties
  methods.rs          # Method definition, calls, chaining, self
  protocols.rs        # Protocol definition, conformance, property requirements
  modules.rs          # Multi-file compilation, visibility, imports, re-exports
  native_emit.rs      # Native compilation smoke tests (representative subset)
```

## Shared Helpers (`tests/common/mod.rs`)

Each test file imports helpers via `mod common;` at the top.

```rust
// --- Single-file helpers ---
compile_and_run(source: &str) -> i32              // JIT execution via inkwell
compile_to_native_and_run(source: &str) -> i32    // Compile -> link -> execute
compile_should_fail(source: &str) -> String       // Semantic-level error (tokenize -> parse -> analyze)
compile_source_should_fail(source: &str) -> String // Full-pipeline error (bengal::compile_source)
TEST_COUNTER: AtomicU64                           // Unique ID for native test temp dirs

// --- Multi-file (package) helpers --- (depends on `tempfile` dev-dependency)
compile_and_run_package(files: &[(&str, &str)]) -> i32       // Package compile -> link -> run
compile_package_should_fail(files: &[(&str, &str)]) -> String // Package compile error
```

Notes:
- `compile_should_fail` operates at the semantic analysis level only (tokenize -> parse -> `semantic::analyze`). Use for tests that specifically target semantic errors.
- `compile_source_should_fail` uses `bengal::compile_source` which covers all phases (parse, semantic, BIR, codegen). Use when the error phase is unimportant or when testing non-semantic errors.
- Existing error tests using `bengal::compile_source(...).is_err()` are migrated to use `compile_source_should_fail` for consistency.

## Migration Mapping

### expressions.rs

**Moved from compile_test.rs:**
- `literal`, `addition`, `subtraction`, `multiplication`, `division`
- `precedence`, `parentheses`, `nested_parentheses`
- `left_assoc_division`, `left_assoc_subtraction`
- `i64_arithmetic`, `i64_comparison`, `f64_arithmetic`, `mixed_cast_chain`
- `cast_i32_to_i64`, `cast_noop`
- `err_cast_bool`, `err_mixed_arithmetic`, `err_infer_mismatch`, `err_integer_overflow`

**New tests:**
- `float32_arithmetic` — Float32 basic arithmetic
- `cast_i32_to_f32` — Int32 to Float32 cast
- `cast_f32_to_f64` — Float32 to Float64 cast
- `cast_chain_all_types` — Int32 -> Int64 -> Float64 -> Float32 -> Int32
- `as_binds_tighter_than_addition` — `1 + 2 as Int64` is `1 + (2 as Int64)`, type error
- `complex_precedence` — All operators in one expression
- `err_comparison_type_mismatch` — Comparing different numeric types

### functions.rs

**Moved from compile_test.rs:**
- `fn_simple` (renamed: `simple`), `fn_call` -> `call`, `fn_call_chain` -> `call_chain`
- `fn_multiple_funcs` -> `multiple_functions`
- `unit_func` -> `unit_return`, `fibonacci` (originally a regression test, retained here)
- `err_no_main`, `err_main_with_params`, `err_no_return`
- `err_no_yield`, `err_yield_in_func`, `err_return_in_block`

**New tests:**
- `recursive_countdown` — Basic recursive function
- `multi_param_function` — Function with 3+ parameters
- `function_returns_bool` — Function returning Bool
- `err_return_type_mismatch` — Return type doesn't match declaration
- `err_duplicate_function` — Duplicate function name
- `err_wrong_arg_count` — Wrong number of arguments at call site

### variables.rs

**Moved from compile_test.rs:**
- `fn_with_let` -> `let_binding`, `fn_with_var` -> `var_binding`
- `fn_let_arithmetic` -> `let_arithmetic`
- `fn_shadowing` -> `shadowing`, `fn_var_update` -> `var_update`
- `fn_block_expr` -> `block_expression`, `fn_block_shadow` -> `block_shadow`
- `fn_block_var_assign` -> `block_var_assign`
- `infer_i32`, `infer_i32_expr`, `infer_bool`, `infer_var`
- `err_undefined_var`, `err_immutable_assign`

**New tests:**
- `shadow_function_param` — Shadow a function parameter with let
- `shadow_nested_scopes` — 3+ levels of nested shadowing
- `infer_from_block_expr` — `let x = { yield 1 + 2; };`
- `infer_from_if_else` — `let x = if true { yield 1; } else { yield 2; };`
- `infer_float64` — `let x = 3.14;` infers Float64
- `err_type_annotation_mismatch` — `let x: Int64 = 10;` (Int32 literal vs Int64 annotation)

### control_flow.rs

**Moved from compile_test.rs:**
- `if_else_true`, `if_else_false`, `if_else_comparison`, `if_no_else`
- `while_sum`, `while_factorial`
- `comparison_eq`, `comparison_ne`, `comparison_le`, `comparison_ge`
- `logical_and`, `logical_and_short`, `logical_or`, `logical_not`
- `early_return`, `diverging_then`, `diverging_else`, `nested_if`
- `while_break`, `while_continue`, `nested_break`
- `break_with_var_update`, `continue_skip_even`
- `break_diverge_in_if_else`, `continue_diverge_in_if_else`
- `break_with_value`, `break_with_value_computed`, `break_with_value_nested_if`
- `nobreak_basic`, `nobreak_condition_false`, `nobreak_no_break_in_body`
- `err_if_non_bool_cond`, `err_if_branch_mismatch`, `err_while_non_bool_cond`
- `err_yield_in_while`, `err_break_outside_loop`, `err_continue_outside_loop`
- `err_break_value_no_nobreak`, `err_break_value_type_mismatch`
- `err_nobreak_in_while_true`, `err_nobreak_type_mismatch`

**New tests:**
- `both_branches_diverge` — Both if/else branches return
- `break_with_complex_expr` — `break (i + 1) * (j - 1);`
- `continue_nested_loops` — Inner continue doesn't affect outer loop
- `while_false_body_not_executed` — `while false { }` body is never entered
- `logical_not_with_comparison` — `!(x > 5)`
- `short_circuit_and` — `false && f()` doesn't call f
- `short_circuit_or` — `true || f()` doesn't call f

### structs.rs

**New JIT versions of existing native-only tests:**
- `basic` — Struct creation and field access (JIT version of `native_struct_basic`)
- `function_arg_return` — Struct as function arg/return (JIT version of `native_struct_function_arg_return`)

**New tests:**
- `nested_struct` — Struct containing another struct as a field
- `explicit_init` — Struct with explicit `init` block
- `zero_arg_init` — `init() { self.x = 0; }` called as `Foo()`
- `computed_property_get` — Getter-only computed property
- `computed_property_get_set` — Getter + setter computed property
- `computed_property_multi` — Multiple computed properties in one struct
- `field_assign_complex_expr` — `s.x = if cond { yield 1; } else { yield 2; };`
- `err_recursive_struct` — Self-referencing struct field
- `err_duplicate_member` — Duplicate member name in struct
- `err_init_missing_field` — Init body doesn't initialize all stored fields
- `err_let_struct_field_assign` — Field assignment on let-bound struct
- `err_memberwise_with_explicit_init` — Memberwise init unavailable when explicit init exists

### methods.rs

**Moved from compile_test.rs:**
- `method_basic`, `method_with_args`, `method_chaining`
- `method_calls_other_method`, `method_in_control_flow`, `method_unit_return`

**New tests:**
- `method_returns_struct` — `obj.make().field` chain
- `method_param_same_type` — Method taking same struct type as parameter
- `method_calls_computed_property` — Method reading a computed property
- `method_in_while_loop` — Method call in loop condition or body
- `err_self_outside_struct` — Using `self` outside struct context

### protocols.rs

**Moved from compile_test.rs:**
- `protocol_basic_conformance`, `protocol_multiple_methods`
- `protocol_property_get`, `protocol_stored_property_satisfies_get`
- `protocol_multiple_conformance`, `protocol_property_get_set`
- `protocol_error_missing_method`, `protocol_error_return_type_mismatch`
- `protocol_error_unknown_protocol`, `protocol_error_missing_property`
- `protocol_error_missing_setter`

**New tests:**
- `protocol_method_with_params` — Method with parameters in protocol conformance
- `err_param_count_mismatch` — Protocol method vs implementation param count differs
- `err_param_type_mismatch` — Protocol method vs implementation param type differs
- `err_duplicate_protocol` — Duplicate protocol name
- `err_property_type_mismatch` — Property type doesn't match protocol requirement

### modules.rs

**Moved from compile_test.rs:**
- `multi_file_cross_module_function_call`
- `multi_file_visibility_internal_denied`
- `multi_file_struct_across_modules`
- `multi_file_glob_import`
- `single_file_backward_compat` (note: uses `compile_and_run`, not the package helpers)
- `multi_file_package_visibility`
- `multi_file_method_call_across_modules`
- `multi_file_three_modules`

**New tests:**
- `re_export_public_import` — `public import self::internal::Symbol;`
- `self_relative_import` — `import self::sub::helper;`
- `super_import` — `import super::common::Symbol;`
- `group_import` — `import math::{add, mul};` explicit test
- `hierarchical_modules` — 3-level module hierarchy
- `err_super_at_root` — `super` used at package root
- `err_import_nonexistent_symbol` — Importing non-existent symbol
- `err_circular_module` — Circular module declarations

### native_emit.rs

**Retained from compile_test.rs (smoke tests):**
- `native_bare_expression` — Bare expression without explicit main
- `native_arithmetic` — Basic expression `2 + 3 * 4`
- `native_function_call` — Function call across functions
- `native_control_flow` — While loop
- `native_i64_cast` (renamed: `native_type_cast`) — Cast between numeric types

**New tests (filling native coverage gaps):**
- `native_struct_basic` — Struct creation and field access (kept as native alongside JIT version in structs.rs)
- `native_method_call` — Method invocation on struct

**Removed (covered by JIT tests in feature files):**
- `native_simple_return` — covered by `functions::simple`
- `native_if_else` — covered by `control_flow::if_else_true`
- `native_break_continue` — covered by `control_flow::while_continue`
- `native_unit_call` — covered by `functions::unit_return`
- `native_i64_arithmetic` — covered by `expressions::i64_arithmetic`
- `native_float` — covered by `expressions::f64_arithmetic`
- `native_break_with_value` — covered by `control_flow::break_with_value`
- `native_diverging_if` — covered by `control_flow::diverging_then`
- `native_struct_function_arg_return` — covered by `structs::function_arg_return`

## Design Principles

1. **One feature, one file** — Each test file covers a single language feature domain.
2. **Test names are self-documenting** — Remove redundant prefixes (e.g., `fn_` in functions.rs).
3. **Error tests co-located with success tests** — Within each file, error tests follow success tests.
4. **Shared helpers, not duplicated code** — All helpers in `tests/common/mod.rs`.
5. **JIT over native for correctness tests** — Use `compile_and_run` for feature correctness; native_emit.rs is a smoke test for the object emission pipeline.

## Out of Scope

- Comments support (lexer does not currently support comments)
- String/Array types (not yet supported per grammar)
- Existential types, extension conformance, default protocol implementations (future features)
- Unit tests within `src/` modules (this spec covers integration tests only)
