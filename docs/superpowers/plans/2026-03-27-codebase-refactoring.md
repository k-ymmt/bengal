# Codebase Refactoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split all source files >500 lines into focused modules of ≤500 lines each, improving developer experience.

**Architecture:** Pure structural refactoring. Two patterns: Pattern A (submodule hierarchy) for `impl`-based code (`Lowering`, `Parser`), Pattern B (sibling modules with `pub(super)`) for free-function code (`semantic/`, `codegen/`). All extracted functions get `pub(super)` visibility. Parent modules re-export public API via `pub use`. Sibling functions are accessed via `use super::sibling_module::fn_name` or through parent re-exports.

**Tech Stack:** Rust, cargo (build/test/clippy/fmt)

**Spec:** `docs/superpowers/specs/2026-03-27-codebase-refactoring-design.md`

---

## Important Context

### Visibility pattern for semantic/ (Pattern B — free functions)

All functions in `semantic/mod.rs` are free functions. When split into submodule files:

1. **Private items in `mod.rs`** (e.g., `sem_err`, `SemanticInfo`) are accessible from all submodules (Rust: private = visible to defining module + descendants)
2. **Extracted functions** get `pub(super)` so sibling submodules can call them
3. **Public API functions** (`analyze_package`, `validate_main`, etc.) are re-exported from `mod.rs` via `pub use submodule::fn_name`
4. **Submodule imports**: Each submodule uses specific imports: `use super::{sem_err, sem_err_with_help, ...}` for parent items, `use super::sibling::fn_name` for sibling items

### Visibility pattern for bir/lowering/ and parser/ (Pattern A — impl blocks)

Struct with private fields stays in `mod.rs`. Submodules extend via `impl StructName`. Private fields are accessible from submodules (Rust's descendant visibility). Private helper types (`StmtResult`, etc.) in `mod.rs` are also accessible.

### Verification commands (used after every extraction)

```bash
cargo build 2>&1 | head -50     # Quick check
cargo test 2>&1 | tail -20      # All tests pass
```

### Commit convention

One commit per completed file extraction (or small group of related extractions). Message format: `refactor(<module>): extract <filename> from <source>`

---

## Task 1: Branch Setup

**Files:** None (git operations only)

- [ ] **Step 1: Ensure working tree is clean**

```bash
git stash  # if needed for Plan.md changes
```

- [ ] **Step 2: Merge feature/type-inference to main**

```bash
git checkout main
git merge feature/type-inference
```

- [ ] **Step 3: Create refactoring branch**

```bash
git checkout -b feature/file-splitting
```

- [ ] **Step 4: Verify baseline**

```bash
cargo test
```
Expected: All tests pass. This is our baseline — every subsequent task must preserve this.

---

## Task 2: Split `semantic/mod.rs` — Phase 1 (Independent Modules)

Extract modules that have minimal cross-dependencies first: `package_analysis`, `generic_validation`, `pre_mono`, `post_mono`.

**Files:**
- Modify: `src/semantic/mod.rs`
- Create: `src/semantic/package_analysis.rs`
- Create: `src/semantic/single_module_analysis.rs`
- Create: `src/semantic/generic_validation.rs`
- Create: `src/semantic/pre_mono.rs`
- Create: `src/semantic/post_mono.rs`

### Reference: Current line ranges in `semantic/mod.rs`

| Function group | Lines | Target file |
|---|---|---|
| Imports, structs, error helpers | 1-50 | `mod.rs` (keep) |
| `SymbolKind`, `GlobalSymbol`, `GlobalSymbolTable` | 57-73 | `package_analysis.rs` |
| `analyze_package`, `collect_global_symbols`, `resolve_imports_*`, `import_*` | 81-401 | `package_analysis.rs` |
| `analyze_single_module` | 407-702 | `single_module_analysis.rs` |
| `validate_main`, `validate_generics*`, `validate_constraints` | 709-993 | `generic_validation.rs` |
| `analyze_pre_mono*`, `validate_inferred_constraints` | 1003-1400 | `pre_mono.rs` |
| `is_builtin_type`, `type_annotation_display_name` | 1403-1428 | `mod.rs` (keep) |
| `analyze_post_mono` | 1430-1723 | `post_mono.rs` |
| Type utils: `resolve_type_checked` .. `substitute_type` | 1725-1822 | `mod.rs` (keep) |
| `resolve_struct_members` | 1824-1959 | `mod.rs` (keep, widely used) |

- [ ] **Step 1: Add submodule declarations to `mod.rs`**

At the top of `mod.rs`, after the existing `pub mod` lines, add:

```rust
mod package_analysis;
mod single_module_analysis;
mod generic_validation;
mod pre_mono;
mod post_mono;
```

- [ ] **Step 2: Create `package_analysis.rs`**

Move lines 57-401 (SymbolKind, GlobalSymbol, GlobalSymbolTable, analyze_package, collect_global_symbols, resolve_imports_for_module, import_single_symbol, import_symbol_to_resolver, resolve_import_module_path).

File header (adjust based on actual usage — verify with `cargo build`):
```rust
use std::collections::{HashMap, HashSet};
use crate::error::{BengalError, DiagCtxt, Result, Span};
use crate::package::{ModuleGraph, ModulePath};
use crate::parser::ast::*;
use crate::suggest::find_suggestion;
use crate::semantic::resolver::{FuncSig, ProtocolInfo, Resolver, StructInfo, VarInfo, is_accessible};
use crate::semantic::types::{Type, resolve_type};
use super::{
    sem_err, sem_err_with_help, pkg_err,
    SemanticInfo, PackageSemanticInfo,
    resolve_type_checked, resolve_struct_members,
};
```

Make `analyze_package` `pub(super)` (it's re-exported as `pub` from mod.rs). All other functions stay private or become `pub(super)` as needed.

Add to `mod.rs`: `pub use package_analysis::analyze_package;`

- [ ] **Step 3: Create `single_module_analysis.rs`**

Move lines 407-702 (`analyze_single_module`).

Header imports: same pattern — import from `super::` what this function calls (`sem_err`, `resolve_type_checked`, `resolve_struct_members`, `analyze_struct_members`, `analyze_function`, etc.).

Note: `analyze_struct_members` and `analyze_function` haven't been extracted yet — they're still in `mod.rs`. This is fine; they'll be moved in Phase 2, and imports will be updated then.

- [ ] **Step 4: Create `generic_validation.rs`**

Move lines 709-993 (`validate_main`, `validate_generics`, `validate_generics_block`, `validate_generics_stmt`, `validate_generics_expr`, `validate_constraints`).

Add to `mod.rs`:
```rust
pub use generic_validation::{validate_main, validate_generics};
```

- [ ] **Step 5: Create `pre_mono.rs`**

Move lines 1003-1400 (`analyze_pre_mono`, `analyze_pre_mono_lenient`, `analyze_pre_mono_inner`, `validate_inferred_constraints`).

Add to `mod.rs`:
```rust
pub use pre_mono::{analyze_pre_mono, analyze_pre_mono_lenient};
```

- [ ] **Step 6: Create `post_mono.rs`**

Move lines 1430-1723 (`analyze_post_mono`).

Add to `mod.rs`:
```rust
pub use post_mono::analyze_post_mono;
```

- [ ] **Step 7: Build and fix compilation errors**

```bash
cargo build 2>&1 | head -80
```

Fix any missing imports or visibility issues. Common fixes:
- Add `pub(super)` to functions called from sibling submodules
- Add missing `use` imports in submodule files
- Update paths for moved types (e.g., `SymbolKind` moved to `package_analysis`)

- [ ] **Step 8: Run tests**

```bash
cargo test 2>&1 | tail -20
```
Expected: All tests pass.

- [ ] **Step 9: Format and lint**

```bash
cargo fmt && cargo clippy 2>&1 | tail -20
```

- [ ] **Step 10: Commit**

```bash
git add src/semantic/
git commit -m "refactor(semantic): extract package_analysis, single_module, generic_validation, pre_mono, post_mono"
```

---

## Task 3: Split `semantic/mod.rs` — Phase 2 (Mutually-Recursive Modules)

Extract the mutually-recursive analysis functions and distribute tests.

**Files:**
- Modify: `src/semantic/mod.rs`
- Create: `src/semantic/function_analysis.rs`
- Create: `src/semantic/stmt_analysis.rs`
- Create: `src/semantic/expr_analysis.rs`
- Create: `src/semantic/expr_call_analysis.rs`
- Create: `src/semantic/expr_method_analysis.rs`
- Create: `src/semantic/struct_analysis.rs`

### Reference: Remaining line ranges in `mod.rs` (after Phase 1)

| Function group | Lines (original) | Target file |
|---|---|---|
| `stmt_always_returns`, `block_always_returns` | 1962-1983 | `function_analysis.rs` |
| `analyze_function` | 1985-2056 | `function_analysis.rs` |
| `analyze_block_expr`, `analyze_control_block`, `analyze_loop_block` | 2059-2190 | `function_analysis.rs` |
| `analyze_stmt` | 2192-2598 | `stmt_analysis.rs` |
| `analyze_expr` (lines 2600-3569, ~970 lines) | 2600-3569 | Split across 3 files (see below) |
| `analyze_struct_members`, `check_all_fields_initialized`, `analyze_getter_block`, `check_assignment_target_mutable` | 3571-3831 | `struct_analysis.rs` |
| `mod tests` | 3833-4425 | Distribute to relevant files |
| `mod module_tests` | 4427-4641 | `package_analysis.rs` or `single_module_analysis.rs` |

### Splitting `analyze_expr` (~970 lines)

The `analyze_expr` function is a large match on `ExprKind`. Split by extracting match arms into helper functions:

1. **`expr_analysis.rs`**: Contains the `analyze_expr` dispatcher function + simple arms (Number, Bool, Ident, Unary, Binary, LogicalAnd/Or, Cast, Self_, ArrayLiteral, IndexAccess, BlockExpr, If, While). ~400 lines.

2. **`expr_call_analysis.rs`**: Helper functions for complex arms — `analyze_call_expr` (Call + generic inference), `analyze_struct_init_expr` (StructInit), `analyze_field_access_expr` (FieldAccess), `analyze_computed_prop_expr`. ~400 lines.

3. **`expr_method_analysis.rs`**: Helper function `analyze_method_call_expr` (MethodCall arm, ~240 lines). ~250 lines.

The dispatcher in `expr_analysis.rs` calls these helpers:
```rust
pub(super) fn analyze_expr(...) -> Result<Type> {
    match &expr.kind {
        // Simple arms handled inline
        ExprKind::Number(_) => { ... }
        // Complex arms delegate to helpers
        ExprKind::Call { .. } => expr_call_analysis::analyze_call_expr(...),
        ExprKind::MethodCall { .. } => expr_method_analysis::analyze_method_call_expr(...),
        // etc.
    }
}
```

- [ ] **Step 1: Add submodule declarations**

Add to `mod.rs`:
```rust
mod function_analysis;
mod stmt_analysis;
mod expr_analysis;
mod expr_call_analysis;
mod expr_method_analysis;
mod struct_analysis;
```

- [ ] **Step 2: Create `function_analysis.rs`**

Move `stmt_always_returns`, `block_always_returns`, `analyze_function`, `analyze_block_expr`, `analyze_control_block`, `analyze_loop_block` (original lines 1962-2190).

All functions `pub(super)`. Imports from super: `sem_err`, `resolve_type_checked`, `check_type_match`, etc. Imports from siblings: `stmt_analysis::analyze_stmt`, `expr_analysis::analyze_expr`.

- [ ] **Step 3: Create `stmt_analysis.rs`**

Move `analyze_stmt` (original lines 2192-2598).

`pub(super) fn analyze_stmt(...)`. Imports: `analyze_expr` from `expr_analysis`, `resolve_type_checked`, `check_type_match`, `find_suggestion`, `check_assignment_target_mutable` from `struct_analysis`.

- [ ] **Step 4: Create `expr_analysis.rs` with dispatcher**

Move `analyze_expr` (original lines 2600-3569). Refactor: extract Call, StructInit, FieldAccess, MethodCall arms into calls to sibling modules. Keep simple arms inline.

- [ ] **Step 5: Create `expr_call_analysis.rs`**

Extract Call, StructInit, FieldAccess, computed property match arms from `analyze_expr` into separate `pub(super)` helper functions.

- [ ] **Step 6: Create `expr_method_analysis.rs`**

Extract MethodCall match arm into `pub(super) fn analyze_method_call_expr(...)`.

- [ ] **Step 7: Create `struct_analysis.rs`**

Move `analyze_struct_members`, `check_all_fields_initialized`, `analyze_getter_block`, `check_assignment_target_mutable` (original lines 3571-3831).

- [ ] **Step 8: Distribute tests**

Move `mod tests` (original lines 3833-4425) — distribute test functions to the file that contains the tested function. Move `mod module_tests` (original lines 4427-4641) to `package_analysis.rs` or `single_module_analysis.rs`.

If any file exceeds 500 lines after adding tests, create a dedicated `tests.rs` submodule for that file's tests.

- [ ] **Step 9: Build and fix**

```bash
cargo build 2>&1 | head -80
```

This step will likely require multiple iterations. Common fixes:
- Missing imports for types used in function signatures
- Circular dependency resolution (ensure no `mod` cycles)
- Visibility adjustments

- [ ] **Step 10: Run tests**

```bash
cargo test 2>&1 | tail -20
```

- [ ] **Step 11: Verify line counts**

```bash
wc -l src/semantic/*.rs | sort -rn
```
Expected: No file exceeds 500 lines.

- [ ] **Step 12: Format, lint, commit**

```bash
cargo fmt && cargo clippy 2>&1 | tail -20
git add src/semantic/
git commit -m "refactor(semantic): extract analysis functions and distribute tests"
```

---

## Task 4: Split `bir/lowering.rs` (Convert to Directory Module)

**Files:**
- Remove: `src/bir/lowering.rs`
- Create: `src/bir/lowering/mod.rs`
- Create: `src/bir/lowering/lower_expr.rs`
- Create: `src/bir/lowering/lower_short_circuit.rs`
- Create: `src/bir/lowering/lower_stmt.rs`
- Create: `src/bir/lowering/lower_control_flow.rs`
- Create: `src/bir/lowering/lower_struct.rs`
- Create: `src/bir/lowering/lower_program.rs`
- No change needed to `src/bir/mod.rs` — it re-exports `lowering::lower_module`, `lowering::lower_program`, `lowering::semantic_type_to_bir`, which will continue to work since `lowering/mod.rs` re-exports from `lower_program.rs` and keeps `semantic_type_to_bir`

### Reference: Line ranges in `bir/lowering.rs`

| Content | Lines | Target file |
|---|---|---|
| Imports | 1-6 | `mod.rs` |
| StmtResult, LoopContext, SemInfoRef, ReceiverInfo, Lowering struct | 8-73 | `mod.rs` |
| `impl Lowering` — core utilities (new..try_lookup_var) | 75-241 | `mod.rs` |
| `impl Lowering` — struct ops (emit_struct_init..collect_mutable_var_values) | 245-592 | `lower_struct.rs` |
| `impl Lowering` — lower_function, lower_block_stmts | 596-692 | `mod.rs` |
| `impl Lowering` — lower_explicit_init | 696-751 | `lower_stmt.rs` |
| `impl Lowering` — lower_stmt | 755-936 | `lower_stmt.rs` |
| `impl Lowering` — lower_expr | 940-1451 | `lower_expr.rs` (extract FieldAccess arm to lower_struct.rs) |
| `impl Lowering` — lower_short_circuit_and, lower_short_circuit_or | 1455-1568 | `lower_short_circuit.rs` |
| `impl Lowering` — lower_if | 1572-1789 | `lower_control_flow.rs` |
| `impl Lowering` — lower_while | 1793-2002 | `lower_control_flow.rs` |
| Free fns: convert_binop, convert_compare_op, semantic_type_to_bir, check_acyclic_structs, convert_type | 2005-2114 | `mod.rs` |
| Free fns: lower_program, lower_program_with_inferred | 2116-2318 | `lower_program.rs` |
| Free fns: lower_module, lower_module_with_inferred | 2328-2545 | `lower_program.rs` |
| Tests | 2547-2936 | Distribute to relevant files |

- [ ] **Step 1: Create directory structure**

```bash
mkdir -p src/bir/lowering
```

- [ ] **Step 2: Rename `lowering.rs` to `lowering/mod.rs`**

```bash
mv src/bir/lowering.rs src/bir/lowering/mod.rs
```

- [ ] **Step 3: Add submodule declarations to `mod.rs`**

Add at top of `mod.rs` (after `use` imports):
```rust
mod lower_expr;
mod lower_short_circuit;
mod lower_stmt;
mod lower_control_flow;
mod lower_struct;
mod lower_program;
```

- [ ] **Step 4: Create `lower_expr.rs`**

Move `lower_expr` method (lines 940-1451) into `impl Lowering` block. Import: `use super::*;` (gets parent types). The FieldAccess arm (~120 lines) should be extracted as a call to `super::lower_struct::lower_field_access(self, ...)`.

```rust
use crate::error::{BengalError, Span};
use crate::parser::ast::*;
use super::instruction::*;

impl super::Lowering {
    pub(super) fn lower_expr(&mut self, expr: &Expr) -> Value {
        // ... match arms, with FieldAccess delegating to lower_struct
    }
}
```

Wait — since this is a submodule of `lowering/`, `Lowering` is defined in `super` (the parent `lowering/mod.rs`). The `impl` needs `impl super::Lowering` or bring it into scope. Actually, since private items in the parent are visible to submodules, just `use super::Lowering;` (though it's not `pub`). Actually in Rust, you can `impl` a type from the parent module in a child module directly — `impl super::Lowering` works, or use `use super::*;` and then `impl Lowering`.

- [ ] **Step 5: Create `lower_short_circuit.rs`**

Move `lower_short_circuit_and` (lines 1455-1510) and `lower_short_circuit_or` (lines 1514-1568).

- [ ] **Step 6: Create `lower_stmt.rs`**

Move `lower_stmt` (lines 755-936) and `lower_explicit_init` (lines 696-751).

- [ ] **Step 7: Create `lower_control_flow.rs`**

Move `lower_if` (lines 1572-1789) and `lower_while` (lines 1793-2002).

- [ ] **Step 8: Create `lower_struct.rs`**

Move struct operation methods (lines 245-592): `emit_struct_init`, `lower_receiver`, `infer_struct_name_no_lower`, `inline_getter`, `try_lower_computed_setter`, `expr_refers_to_self`, `lower_field_assign_recursive`, `collect_mutable_var_values`. Also add FieldAccess handler extracted from `lower_expr`.

- [ ] **Step 9: Create `lower_program.rs`**

Move free functions (lines 2116-2545): `lower_program`, `lower_program_with_inferred`, `lower_module`, `lower_module_with_inferred`. Make them `pub` or `pub(crate)` as they were originally.

Re-export from `mod.rs`:
```rust
pub use lower_program::{lower_program, lower_program_with_inferred, lower_module, lower_module_with_inferred};
```

Keep in `mod.rs`: `convert_binop`, `convert_compare_op`, `semantic_type_to_bir`, `check_acyclic_structs`, `convert_type` (lines 2005-2114, ~110 lines).

Re-export `semantic_type_to_bir` from mod.rs since it's `pub`:
```rust
pub use self::semantic_type_to_bir;  // already in mod.rs, no re-export needed
```

- [ ] **Step 10: Distribute tests**

Move test functions to the file containing the tested functionality. Integration tests (lower_module_*) go to `lower_program.rs`. Struct tests go to `lower_struct.rs`. Control flow tests to `lower_control_flow.rs`. etc.

- [ ] **Step 11: Build, fix, test**

```bash
cargo build 2>&1 | head -80
cargo test 2>&1 | tail -20
```

- [ ] **Step 12: Verify line counts**

```bash
wc -l src/bir/lowering/*.rs | sort -rn
```

- [ ] **Step 13: Format, lint, commit**

```bash
cargo fmt && cargo clippy 2>&1 | tail -20
git add src/bir/
git commit -m "refactor(bir): convert lowering.rs to directory module with 7 files"
```

---

## Task 5: Split `parser/mod.rs`

**Files:**
- Modify: `src/parser/mod.rs`
- Create: `src/parser/parse_expr.rs`
- Create: `src/parser/parse_primary.rs`
- Create: `src/parser/parse_definition.rs`
- Create: `src/parser/parse_stmt.rs`
- Create: `src/parser/tests.rs` (or `src/parser/tests/` directory)

### Reference: Line ranges in `parser/mod.rs`

| Content | Lines | Target file |
|---|---|---|
| Parser struct, core methods (new..no_space_before_current) | 1-76 | `mod.rs` |
| Visibility/program parsing (is_visibility_token..parse_import_tail) | 80-274 | `parse_definition.rs` |
| Type params, function/struct/protocol defs | 276-543 | `parse_definition.rs` |
| Param list, param, type parsing | 545-614 | `mod.rs` (type parsing is core) |
| Block, stmt, expect_ident | 618-725 | `parse_stmt.rs` |
| Precedence climbing: parse_expr..parse_cast, parse_unary | 730-925 | `parse_expr.rs` |
| parse_factor, parse_postfix, parse_primary | 927-1078 | `parse_primary.rs` |
| parse_postfix_call, type_arg_list, call_with_type_args | 1080-1233 | `parse_primary.rs` |
| parse_if_expr, parse_while_expr | 1235-1277 | `parse_primary.rs` |
| pub fn parse | 1280-1334 | `mod.rs` |
| mod tests | 1336-2241 | `tests.rs` |
| mod module_tests | 2243-2472 | `tests.rs` |

- [ ] **Step 1: Add submodule declarations**

```rust
mod parse_expr;
mod parse_primary;
mod parse_definition;
mod parse_stmt;
#[cfg(test)]
mod tests;
```

- [ ] **Step 2: Create `parse_definition.rs`**

Move all definition parsing methods (lines 80-543) into `impl Parser` block. These methods need access to `self.peek()`, `self.advance()`, etc. Since `Parser` is in the parent module, use `impl super::Parser`.

```rust
use crate::error::{BengalError, Result, Span};
use crate::lexer::token::Token;
use crate::parser::ast::*;

impl super::Parser {
    // Methods moved here...
}
```

- [ ] **Step 3: Create `parse_expr.rs`**

Move precedence climbing methods (lines 730-925): `parse_expr`, `parse_and`, `parse_equality`, `parse_comparison`, `parse_additive`, `parse_term`, `parse_cast`, `parse_unary`.

- [ ] **Step 4: Create `parse_primary.rs`**

Move atom/postfix methods (lines 927-1277): `parse_factor`, `parse_postfix`, `parse_primary`, `parse_postfix_call`, `parse_type_arg_list`, `parse_postfix_call_with_type_args`, `parse_if_expr`, `parse_while_expr`.

- [ ] **Step 5: Create `parse_stmt.rs`**

Move block/statement methods (lines 618-725): `parse_block`, `parse_stmt`, `expect_ident`.

- [ ] **Step 6: Create `tests.rs`**

Move both `mod tests` (lines 1336-2241) and `mod module_tests` (lines 2243-2472) to `tests.rs`. Since the test module needs access to `Parser` and `parse`, make sure they're accessible:

```rust
use super::*;
use crate::lexer::token::SpannedToken;
// ... test helper functions and tests
```

If `tests.rs` exceeds 500 lines (likely at ~1136 lines), convert to `tests/` directory:
```
parser/tests/mod.rs         — helpers + expr tests
parser/tests/test_definition.rs — definition tests
parser/tests/test_module.rs     — module_tests
```

- [ ] **Step 7: Build, fix, test**

```bash
cargo build 2>&1 | head -80
cargo test 2>&1 | tail -20
```

- [ ] **Step 8: Verify line counts**

```bash
wc -l src/parser/*.rs src/parser/tests/*.rs 2>/dev/null | sort -rn
```

- [ ] **Step 9: Format, lint, commit**

```bash
cargo fmt && cargo clippy 2>&1 | tail -20
git add src/parser/
git commit -m "refactor(parser): extract parse_expr, parse_primary, parse_definition, parse_stmt, tests"
```

---

## Task 6: Split `codegen/llvm.rs`

**Files:**
- Modify: `src/codegen/llvm.rs`
- Modify: `src/codegen/mod.rs`
- Create: `src/codegen/emit_arithmetic.rs`
- Create: `src/codegen/emit_structural.rs`
- Create: `src/codegen/types.rs`
- Create: `src/codegen/generic_resolution.rs`
- Create: `src/codegen/mono_compile.rs`

### Reference: Line ranges in `codegen/llvm.rs`

| Content | Lines | Target file |
|---|---|---|
| Imports | 1-18 | `llvm.rs` |
| `codegen_err` | 20-24 | `llvm.rs` |
| `bir_type_to_llvm_type` | 27-59 | `types.rs` |
| `collect_value_types` | 66-168 | `types.rs` |
| `find_block`, `EmitCtx`, `load_value`, `emit_bounds_check` | 171-238 | Split between emit files |
| `emit_instruction` (241-769, ~530 lines) | 241-769 | Split: arithmetic arms → `emit_arithmetic.rs`, structural arms → `emit_structural.rs` |
| `store_br_args`, `emit_terminator` | 772-879 | `emit_structural.rs` |
| `compile_function` | 883-955 | `llvm.rs` |
| `contains_type_param`, `build_struct_types`, `build_generic_struct_types`, `collect_type_params` | 959-1093 | `types.rs` |
| `compile_to_module`, `compile_module` | 1096-1285 | `llvm.rs` |
| `compile_module_with_mono` | 1293-1476 | `mono_compile.rs` |
| `link_objects` | 1479-1490 | `llvm.rs` |
| `resolve_instruction`, `resolve_terminator`, `resolve_basic_block`, `resolve_function` | 1498-1775 | `generic_resolution.rs` |
| `compile_with_mono`, `compile_to_module_with_mono` | 1782-2045 | `mono_compile.rs` |
| `compile` | 2048-2076 | `llvm.rs` |
| Tests | 2078-2459 | Distribute |

### Splitting `emit_instruction`

The `emit_instruction` function (~530 lines) is a single large match. Split into two helper functions:

- `emit_arithmetic_instruction(...)` — handles: Literal, BinOp, Compare, Cast, Unary, Negate, Not. ~300 lines.
- `emit_structural_instruction(...)` — handles: Call, StructInit, FieldGet, FieldSet, ArrayInit, IndexGet, IndexSet, Copy. ~230 lines.

The main `emit_instruction` dispatches to these. Place the dispatcher and structural helper in `emit_structural.rs`, arithmetic helper in `emit_arithmetic.rs`.

`EmitCtx` struct must get `pub(super)` on both struct and fields, defined in `llvm.rs`:

```rust
pub(super) struct EmitCtx<'a, 'ctx> {
    pub(super) context: &'ctx Context,
    pub(super) module: &'a Module<'ctx>,
    pub(super) builder: &'a Builder<'ctx>,
    pub(super) current_fn: FunctionValue<'ctx>,
    pub(super) alloca_map: &'a HashMap<Value, PointerValue<'ctx>>,
    pub(super) value_types: &'a HashMap<Value, BirType>,
    pub(super) struct_types: &'a HashMap<String, inkwell::types::StructType<'ctx>>,
}
```

- [ ] **Step 1: Add module declarations to `codegen/mod.rs`**

```rust
mod emit_arithmetic;
mod emit_structural;
pub mod types;  // or mod types; with pub use
mod generic_resolution;
mod mono_compile;
```

Current `codegen/mod.rs` declares `pub mod llvm;` and re-exports:
```rust
pub use llvm::{
    compile, compile_module, compile_module_with_mono, compile_to_module,
    compile_to_module_with_mono, compile_with_mono, link_objects,
};
```

After the split, `compile_module_with_mono`, `compile_with_mono`, `compile_to_module_with_mono` move to `mono_compile.rs`. To avoid breaking `codegen/mod.rs`, have `llvm.rs` re-export them:
```rust
// In llvm.rs — re-export from mono_compile so codegen/mod.rs pub use llvm::{...} continues to work
pub use super::mono_compile::{compile_with_mono, compile_to_module_with_mono, compile_module_with_mono};
```

Add module declarations to `codegen/mod.rs`:
```rust
pub mod llvm;
mod emit_arithmetic;
mod emit_structural;
mod types;
mod generic_resolution;
mod mono_compile;
```

- [ ] **Step 2: Create `types.rs`**

Move: `bir_type_to_llvm_type` (27-59), `collect_value_types` (66-168), `contains_type_param` (959-966), `build_struct_types` (968-1001), `build_generic_struct_types` (1009-1069), `collect_type_params` (1072-1093). All `pub(super)`.

- [ ] **Step 3: Create `generic_resolution.rs`**

Move: `resolve_instruction` (1498-1679), `resolve_terminator` (1682-1726), `resolve_basic_block` (1730-1749), `resolve_function` (1753-1775). All `pub(super)`.

- [ ] **Step 4: Create `mono_compile.rs`**

Move: `compile_module_with_mono` (1293-1476), `compile_with_mono` (1782-1914), `compile_to_module_with_mono` (1920-2045). Re-export public functions from `codegen/mod.rs`:
```rust
pub use mono_compile::{compile_with_mono, compile_to_module_with_mono, compile_module_with_mono};
```

- [ ] **Step 5: Split `emit_instruction` and create emit files**

Create `emit_arithmetic.rs` with `pub(super) fn emit_arithmetic_instruction(...)` containing arithmetic match arms.

Create `emit_structural.rs` with:
- `pub(super) fn emit_structural_instruction(...)` for structural match arms
- `pub(super) fn emit_instruction(...)` as the dispatcher
- Move `emit_terminator`, `store_br_args`, `emit_bounds_check`, `load_value`, `find_block` here

- [ ] **Step 6: Update `llvm.rs`**

`llvm.rs` keeps: imports, `codegen_err`, `EmitCtx` (with `pub(super)`), `compile_function`, `compile_to_module`, `compile_module`, `link_objects`, `compile`.

Update internal calls to use the new module paths.

- [ ] **Step 7: Distribute tests**

Tests (lines 2078-2459) — arithmetic tests to `emit_arithmetic.rs`, integration tests to `llvm.rs`, struct tests to `emit_structural.rs`.

- [ ] **Step 8: Build, fix, test**

```bash
cargo build 2>&1 | head -80
cargo test 2>&1 | tail -20
```

- [ ] **Step 9: Verify line counts**

```bash
wc -l src/codegen/*.rs | sort -rn
```

- [ ] **Step 10: Format, lint, commit**

```bash
cargo fmt && cargo clippy 2>&1 | tail -20
git add src/codegen/
git commit -m "refactor(codegen): split llvm.rs into emit, types, generic_resolution, mono_compile"
```

---

## Task 7: Split Medium Files

**Files:**
- Modify: `src/semantic/infer.rs`
- Create: `src/semantic/unify.rs`
- Modify: `src/pipeline.rs`
- Create: `src/pipeline_helpers.rs`

### 7a: Split `semantic/infer.rs` (1134 lines)

| Content | Lines | Target |
|---|---|---|
| Types, public fns, InferredTypeArgs impl | 1-86 | `infer.rs` |
| VarKind, VarState, InferenceContext struct | 88-110 | `infer.rs` |
| InferenceContext impl (new..record_inferred_type_args) | 112-394 | `infer.rs` |
| `unify` method | 398-538 | `unify.rs` |
| `find` method | 541-554 | `unify.rs` |
| Default impl | 557-561 | `infer.rs` |
| Tests | 563-1134 | Distribute |

- [ ] **Step 1: Create `src/semantic/unify.rs`**

Extract `unify` (398-538) and `find` (541-554) into `impl InferenceContext` in a new file. Add `mod unify;` to `infer.rs`.

Wait — `infer.rs` is already a submodule of `semantic/mod.rs`. To add `unify.rs` as a submodule of `infer`, we'd need to convert `infer.rs` to `infer/mod.rs`. Alternatively, make `unify.rs` a sibling of `infer.rs` under `semantic/`.

**Recommended approach**: Create `semantic/unify.rs` as a sibling, declared in `semantic/mod.rs`. Move `unify` and `find` as free functions or as `impl InferenceContext` (since InferenceContext is defined in `infer.rs`, a sibling can still `impl` it if the struct is `pub(super)`).

Check if `InferenceContext` is currently `pub` — if so, sibling modules can impl it. If not, make it `pub(super)`.

- [ ] **Step 2: Distribute tests**

Unification tests go to `unify.rs`. Remaining tests stay in `infer.rs`. Verify neither file exceeds 500 lines. If `infer.rs` still exceeds 500 (code ~394 + remaining tests), extract provenance tests or further split.

- [ ] **Step 3: Build, test, commit**

```bash
cargo build && cargo test 2>&1 | tail -20
cargo fmt && cargo clippy 2>&1 | tail -20
git add src/semantic/
git commit -m "refactor(semantic): extract unify.rs from infer.rs"
```

### 7b: Split `pipeline.rs` (626 lines)

| Content | Lines | Target |
|---|---|---|
| Structs | 15-63 | `pipeline.rs` |
| `parse`, `parse_source`, `analyze` | 66-190 | `pipeline.rs` |
| `build_name_map` | 193-280 | `pipeline_helpers.rs` |
| `lower`, `optimize` | 283-357 | `pipeline.rs` |
| `collect_external_functions` | 360-437 | `pipeline_helpers.rs` |
| `monomorphize`, `codegen`, `link` | 440-544 | `pipeline.rs` |
| Tests | 546-626 | `pipeline.rs` |

- [ ] **Step 4: Create `src/pipeline_helpers.rs`**

Move `build_name_map` (193-280) and `collect_external_functions` (360-437). Both `pub(crate)` so `pipeline.rs` can call them.

Add to `src/lib.rs` (where `pub mod pipeline;` is declared on line 9):
```rust
mod pipeline_helpers;
```

- [ ] **Step 5: Build, test, commit**

```bash
cargo build && cargo test 2>&1 | tail -20
cargo fmt && cargo clippy 2>&1 | tail -20
git add src/pipeline.rs src/pipeline_helpers.rs src/lib.rs
git commit -m "refactor(pipeline): extract build_name_map and collect_external_functions"
```

### 7c: Inspect remaining medium files

- [ ] **Step 6: Check `bir/mono.rs`, `lexer/mod.rs`, `bir/printer.rs`**

```bash
wc -l src/bir/mono.rs src/lexer/mod.rs src/bir/printer.rs
```

If any exceeds 500 lines, split using the same patterns. If at 500 or below, leave as-is.

---

## Task 8: Split Test Files + Final Verification

**Files:**
- Modify: `tests/control_flow.rs`
- Create: `tests/control_flow_if.rs`
- Create: `tests/control_flow_loop.rs`
- Modify: `tests/type_inference.rs`
- Create: `tests/type_inference_basic.rs` (or similar split)

### 8a: Split `tests/control_flow.rs` (524 lines)

| Tests | Lines | Target |
|---|---|---|
| `mod common`, if/else, comparison, logical, early return, divergence | 1-221 | `control_flow_if.rs` |
| while, break/continue, break with value, nobreak, short-circuit, errors | 226-524 | `control_flow_loop.rs` |

- [ ] **Step 1: Create `tests/control_flow_if.rs`**

Move if/else tests (lines 8-43), comparison tests (48-85), logical tests (90-143), early return/divergence tests (148-221). Add `mod common;` at top.

- [ ] **Step 2: Create `tests/control_flow_loop.rs`**

Move while tests (226-253), break/continue tests (258-351), break with value (356-402), nobreak (407-434), short-circuit (441-460), error tests (465-524). Add `mod common;` at top.

- [ ] **Step 3: Remove `tests/control_flow.rs`**

- [ ] **Step 4: Verify**

```bash
cargo test 2>&1 | tail -20
wc -l tests/control_flow_*.rs
```

### 8b: Split `tests/type_inference.rs` (513 lines)

| Tests | Lines | Target |
|---|---|---|
| Basic inference (infer_i64_*, infer_default_*, infer_f32_*, etc.) | 5-130 | `type_inference_basic.rs` |
| Generic inference, loop inference, constraints, end-to-end | 135-408 | `type_inference_advanced.rs` |
| Error cases, coexistence | 413-513 | `type_inference_errors.rs` |

- [ ] **Step 5: Create split files**

Create `type_inference_basic.rs`, `type_inference_advanced.rs`, `type_inference_errors.rs` with `mod common;` headers. Move test functions according to the table.

- [ ] **Step 6: Remove `tests/type_inference.rs`**

- [ ] **Step 7: Verify**

```bash
cargo test 2>&1 | tail -20
wc -l tests/type_inference_*.rs
```

### 8c: Final verification

- [ ] **Step 8: Verify ALL file line counts**

```bash
find src tests -name '*.rs' -exec wc -l {} + | sort -rn | head -30
```
Expected: No file exceeds 500 lines.

- [ ] **Step 9: Full test suite**

```bash
cargo test
```
Expected: All tests pass, same count as baseline.

- [ ] **Step 10: Full lint**

```bash
cargo fmt -- --check
cargo clippy -- -D warnings
```
Expected: Clean.

- [ ] **Step 11: Commit test file splits**

```bash
cargo fmt
git add tests/
git commit -m "refactor(tests): split control_flow.rs and type_inference.rs"
```

- [ ] **Step 12: Final commit — verify no file >500 lines**

```bash
# Verify constraint
! find src tests -name '*.rs' -exec wc -l {} + | awk '$1 > 500 && !/total/ {print; found=1} END {exit found ? 1 : 0}'
```

If any file still exceeds 500 lines, fix it before proceeding.
