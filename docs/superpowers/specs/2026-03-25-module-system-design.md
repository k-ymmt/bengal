# Module System Design

## Overview

Bengal's module system provides multi-file compilation with a hierarchical namespace structure. It draws from Rust's module declaration and path resolution model, combined with Swift's five-level visibility system.

### Design Decisions

- **Approach A (unified semantic analysis)**: All modules are parsed independently, then analyzed together in a single semantic pass. Per-module LLVM codegen produces separate object files linked at the end.
- **Future migration to Approach B**: Interface-based separate compilation for parallel builds (tracked separately).

## Package and Module Structure

### Package

A package is the top-level project unit, defined by a `Bengal.toml` file at the project root.

```toml
[package]
name = "my_app"
entry = "src/main.bengal"
```

- `name`: Package identifier used in module paths and name mangling.
- `entry`: Path to the root module file (relative to `Bengal.toml`).

When no `Bengal.toml` exists, the compiler operates in single-file mode for backward compatibility.

### Module Tree

Modules form a tree rooted at the entry file. Each module is declared with the `module` keyword and maps to a file.

```
src/
  main.bengal          <- root module (entry)
  math.bengal          <- `module math;`
  graphics/
    module.bengal      <- `module graphics;`
    renderer.bengal    <- `module renderer;` inside graphics
```

**File mapping rules:**

- `module foo;` searches for `foo.bengal` in the current module's directory.
- If not found, searches for `foo/module.bengal`.
- If neither exists, emit a compile error.

**Constraints:**

- Circular module declarations are a compile error.
- A file may only belong to one module.
- `module` declarations must appear at the top of the file, before `import` declarations and other definitions.

## Visibility

Five levels of access control, inspired by Swift.

| Keyword | Scope | Use Case |
|---|---|---|
| `public` | Accessible from outside the package | Library public API |
| `package` | Accessible from any module in the same package | Shared internal API |
| `internal` (default) | Accessible within the same module | Normal definitions |
| `fileprivate` | Accessible within the same file | File-local helpers |
| `private` | Accessible within the same declaration scope | Hidden struct fields/methods |

### Rules

- Omitting a visibility modifier defaults to `internal`.
- Struct member default visibility is also `internal`.
- `private` on struct fields: only accessible from methods and `init` within the same struct definition.
- A child module's `internal` symbols are not visible to its parent module.
- Re-exporting (`public import`) requires the original symbol to have `public` or `package` visibility.

### Applicable Targets

- Top-level declarations: `func`, `struct`, `protocol`, `module`
- Struct members: stored properties, computed properties, methods, `init`

## Import Syntax and Path Resolution

### Basic Syntax

```
import math::Vector;                     // single symbol
import math::{Vector, Matrix};           // group
import math::*;                          // glob (all public symbols)
import self::sub::helper;                // relative from current module
import super::common::Util;              // relative from parent module
public import self::internal::Vector;    // re-export
```

### Path Resolution

1. If the path starts with a keyword:
   - `self::` — resolves from the current module
   - `super::` — resolves from the parent module (error at package root)
2. Otherwise — resolves as an absolute path from the package root

### Name Resolution Rules

- Imported symbols are added to the current file's scope.
- Conflicting names from separate imports are a compile error.
- Glob import conflicts: error only at usage site if ambiguous (unused conflicts are tolerated, same as Rust).
- `import` declarations are only allowed at the top level of a file.
- Symbols within the same module are accessible without `import`.

## Compilation Pipeline

```
[1] Package Discovery
    Read Bengal.toml, identify entry file
         |
[2] Module Graph Construction
    Recursively follow `module` declarations from entry
    Detect cycles (must be a DAG)
         |
[3] Parallel Lex + Parse (per module)
    Tokenize and parse each file independently
    Result: HashMap<ModulePath, ModuleAST>
         |
[4] Unified Semantic Analysis
    Pass 1a: Register all top-level names with module paths
    Pass 1b: Resolve types, members, import paths
    Pass 2:  Verify main() -> Int32 in the entry module
    Pass 3:  Type-check function bodies, check visibility
    Pass 3b: Protocol conformance checking
         |
[5] Per-Module BIR Lowering
    AST -> BIR (function names qualified with module path)
         |
[6] Per-Module LLVM Codegen
    BIR Module -> LLVM Module -> object file (.o)
         |
[7] Linking
    Combine all .o files via system linker (cc) -> executable
```

## Name Mangling

- Functions: `_bg_<package>_<module_path>_<name>` (e.g., `_bg_my_app_math_add`)
- Struct methods: `_bg_<package>_<module_path>_<StructName>_<method>`
- `main` is not mangled (linker entry point)

## Grammar Changes

### New Keywords

`module`, `import`, `public`, `package`, `internal`, `fileprivate`, `private`, `self`, `super`

### Grammar Rules (EBNF)

```ebnf
(* Top level *)
Program        = { ModuleDecl } { ImportDecl } { TopLevelDecl } ;
ModuleDecl     = "module" Identifier ";" ;
ImportDecl     = [ Visibility ] "import" ImportPath ";" ;

(* Import path *)
ImportPath     = PathPrefix "::" ImportTail ;
PathPrefix     = "self" | "super" | Identifier ;
ImportTail     = "*"
               | Identifier
               | "{" Identifier { "," Identifier } "}"
               | Identifier "::" ImportTail ;

(* Visibility *)
Visibility     = "public" | "package" | "internal" | "fileprivate" | "private" ;

(* Top-level declarations with visibility *)
TopLevelDecl   = [ Visibility ] ( FuncDecl | StructDecl | ProtocolDecl ) ;

(* Struct members with visibility *)
MemberDecl     = [ Visibility ] ( StoredProp | ComputedProp | Method | Init ) ;
```

### Impact on Existing Syntax

- Struct conformance lists (`: Protocol`) are unchanged.
- `func`, `var`, `let` syntax bodies are unchanged.
- Visibility modifiers are simply prepended to existing declarations.

## Error Messages

| Situation | Error Message |
|---|---|
| Module file not found | `module 'foo' not found: expected 'foo.bengal' or 'foo/module.bengal'` |
| Circular module dependency | `circular module dependency detected: main -> foo -> bar -> foo` |
| File declared from multiple modules | `file 'foo.bengal' is declared as a module from multiple locations` |
| Unresolved import | `unresolved import: 'math::Vector' — module 'math' has no item 'Vector'` |
| Visibility violation | `'helper' is private to module 'math' and cannot be accessed from 'main'` |
| Ambiguous glob import | `ambiguous name 'Foo': found in both 'math' and 'graphics' (imported via *)` |
| `super` at package root | `cannot use 'super' from the package root module` |
| `import` not at top level | `'import' declarations are only allowed at the top level of a file` |
| Re-export visibility mismatch | `cannot re-export 'Foo' as public: 'Foo' is internal to module 'math'` |
| Entry file not found | `entry file 'src/main.bengal' not found` |

## Edge Cases

- Empty modules (declarations only, no definitions) are valid.
- Ordering: `module` declarations first, then `import` declarations, then other definitions.
- Symbols within the same module are accessible without `import`.
- `public import` re-export requires the target symbol to have `public` or `package` visibility.

## Test Strategy

### Unit Tests

- Module graph construction: tree building, cycle detection, missing files
- Path resolution: absolute, `self::`, `super::`, nested paths
- Visibility checking: all five levels in various module relationships
- Import resolution: single, group, glob, re-export
- Name collision detection

### Integration Tests

- Multi-file compilation: call functions across modules
- Struct and protocol usage across module boundaries
- Visibility enforcement across modules
- Glob import with and without ambiguity
- Single-file backward compatibility (no `Bengal.toml`)
- Name mangling correctness (link-time symbol resolution)

## Not in Scope (Future Work)

- Import aliases (`import math::Vector as Vec`)
- Interface-based separate compilation (Approach B)
- Package dependency management (external packages)
- Conditional compilation / feature flags
