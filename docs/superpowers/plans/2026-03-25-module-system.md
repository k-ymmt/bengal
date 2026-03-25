# Module System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a multi-file module system with Rust-style `module`/`import` declarations and Swift-style five-level visibility to the Bengal compiler.

**Architecture:** Approach A — all module files are lexed/parsed independently, then analyzed together in a unified semantic pass. Per-module BIR lowering and LLVM codegen produce separate object files, linked by the system linker. Backward-compatible single-file mode when no `Bengal.toml` is present.

**Tech Stack:** Rust, logos (lexer), inkwell/LLVM 20.1, toml (new dependency for Bengal.toml parsing), clap (CLI)

**Spec:** `docs/superpowers/specs/2026-03-25-module-system-design.md`

---

## File Structure

### New Files
- `src/package.rs` — Bengal.toml discovery and parsing, module graph construction
- `src/mangle.rs` — Length-prefixed name mangling

### Modified Files
- `Cargo.toml` — Add `toml` and `serde` dependencies
- `src/lib.rs` — New public modules, new multi-file compile entry point
- `src/main.rs` — CLI changes for package-aware compilation
- `src/error.rs` — New error variant for package/module errors
- `src/lexer/token.rs` — New tokens (ColonColon, keywords)
- `src/lexer/mod.rs` — Tests for new tokens
- `src/parser/ast.rs` — Visibility enum, ModuleDecl, ImportDecl, updated Program
- `src/parser/mod.rs` — Parse module decls, import decls, visibility modifiers
- `src/semantic/resolver.rs` — Module-aware symbol tables, import tracking, visibility
- `src/semantic/mod.rs` — Multi-module analysis, import resolution, visibility checks
- `src/bir/lowering.rs` — Accept module path for name mangling
- `src/bir/instruction.rs` — No changes needed (functions already have string names)
- `src/codegen/llvm.rs` — Per-module codegen with external function declarations
- `tests/compile_test.rs` — Multi-file integration tests

---

## Task 1: Add New Tokens to Lexer

**Files:**
- Modify: `src/lexer/token.rs`
- Modify: `src/lexer/mod.rs` (tests)

- [ ] **Step 1: Write failing tests for new tokens**

In `src/lexer/mod.rs`, add tests:

```rust
#[test]
fn module_keyword() {
    assert_eq!(token_nodes("module"), vec![Token::Module, Token::Eof]);
}

#[test]
fn import_keyword() {
    assert_eq!(token_nodes("import"), vec![Token::Import, Token::Eof]);
}

#[test]
fn visibility_keywords() {
    assert_eq!(token_nodes("public"), vec![Token::Public, Token::Eof]);
    assert_eq!(token_nodes("package"), vec![Token::Package, Token::Eof]);
    assert_eq!(token_nodes("internal"), vec![Token::Internal, Token::Eof]);
    assert_eq!(token_nodes("fileprivate"), vec![Token::Fileprivate, Token::Eof]);
    assert_eq!(token_nodes("private"), vec![Token::Private, Token::Eof]);
}

#[test]
fn super_keyword() {
    assert_eq!(token_nodes("super"), vec![Token::Super, Token::Eof]);
}

#[test]
fn colon_colon_token() {
    assert_eq!(
        token_nodes("foo::bar"),
        vec![
            Token::Ident("foo".to_string()),
            Token::ColonColon,
            Token::Ident("bar".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
fn colon_colon_does_not_break_existing_colon() {
    assert_eq!(
        token_nodes("x: Int32"),
        vec![
            Token::Ident("x".to_string()),
            Token::Colon,
            Token::Ident("Int32".to_string()),
            Token::Eof,
        ]
    );
}

#[test]
fn keyword_prefix_not_captured() {
    // "modules" should be Ident, not Module + "s"
    assert_eq!(
        token_nodes("modules"),
        vec![Token::Ident("modules".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("imported"),
        vec![Token::Ident("imported".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("publicly"),
        vec![Token::Ident("publicly".to_string()), Token::Eof]
    );
    assert_eq!(
        token_nodes("superb"),
        vec![Token::Ident("superb".to_string()), Token::Eof]
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib lexer -- --nocapture`
Expected: compilation errors (Token variants don't exist)

- [ ] **Step 3: Add token variants**

In `src/lexer/token.rs`, add to the `Token` enum (after the Protocol keyword section):

```rust
// Module system: keywords
#[token("module")]
Module,
#[token("import")]
Import,
#[token("public")]
Public,
#[token("package")]
Package,
#[token("internal")]
Internal,
#[token("fileprivate")]
Fileprivate,
#[token("private")]
Private,
#[token("super")]
Super,

// Module system: symbols
#[token("::")]
ColonColon,
```

Note: `::` must be defined before `:` in logos — logos matches the longest token automatically, but the `#[token("::")]` attribute registers a literal match that takes priority. However, since both are `#[token(...)]` (not regex), logos handles them correctly with longest-match semantics. Verify in tests.

Also update the `Display` impl to include the new variants:

```rust
Token::Module => write!(f, "module"),
Token::Import => write!(f, "import"),
Token::Public => write!(f, "public"),
Token::Package => write!(f, "package"),
Token::Internal => write!(f, "internal"),
Token::Fileprivate => write!(f, "fileprivate"),
Token::Private => write!(f, "private"),
Token::Super => write!(f, "super"),
Token::ColonColon => write!(f, "::"),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib lexer`
Expected: all tests pass including existing ones

- [ ] **Step 5: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 6: Commit**

```bash
git add src/lexer/token.rs src/lexer/mod.rs
git commit -m "Add module system tokens: keywords and ColonColon"
```

---

## Task 2: Add AST Nodes for Module System

**Files:**
- Modify: `src/parser/ast.rs`

- [ ] **Step 1: Add Visibility enum and new AST nodes**

In `src/parser/ast.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Package,
    Internal,
    Fileprivate,
    Private,
}

impl Default for Visibility {
    fn default() -> Self {
        Visibility::Internal
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    pub visibility: Visibility,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportTail {
    /// `import foo::bar::Baz;`
    Single(String),
    /// `import foo::bar::{A, B};`
    Group(Vec<String>),
    /// `import foo::bar::*;`
    Glob,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub visibility: Visibility,
    pub path: Vec<String>,   // ["foo", "bar"] for `import foo::bar::Baz`
    pub tail: ImportTail,
}

/// Indicates the path prefix for imports
#[derive(Debug, Clone, PartialEq)]
pub enum PathPrefix {
    SelfKw,
    Super,
    Named(String),
}
```

Update the `ImportDecl` to carry the prefix:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub visibility: Visibility,
    pub prefix: PathPrefix,
    pub path: Vec<String>,   // remaining path segments after prefix
    pub tail: ImportTail,
}
```

Add visibility to existing nodes:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub module_decls: Vec<ModuleDecl>,
    pub import_decls: Vec<ImportDecl>,
    pub structs: Vec<StructDef>,
    pub protocols: Vec<ProtocolDef>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub visibility: Visibility,
    pub name: String,
    pub conformances: Vec<String>,
    pub members: Vec<StructMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDef {
    pub visibility: Visibility,
    pub name: String,
    pub members: Vec<ProtocolMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub visibility: Visibility,
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: TypeAnnotation,
    pub body: Block,
}
```

Add visibility to `StructMember` variants:

```rust
pub enum StructMember {
    StoredProperty {
        visibility: Visibility,
        name: String,
        ty: TypeAnnotation,
    },
    ComputedProperty {
        visibility: Visibility,
        name: String,
        ty: TypeAnnotation,
        getter: Block,
        setter: Option<Block>,
    },
    Initializer {
        visibility: Visibility,
        params: Vec<Param>,
        body: Block,
    },
    Method {
        visibility: Visibility,
        name: String,
        params: Vec<Param>,
        return_type: TypeAnnotation,
        body: Block,
    },
}
```

- [ ] **Step 2: Fix all compilation errors from AST changes**

This will cause compilation errors throughout the codebase. Fix each file by adding `Visibility::Internal` (the default) where visibility is now required and adding empty `module_decls`/`import_decls` to `Program` construction.

Key files to fix:
- `src/parser/mod.rs`: `parse_program()`, `parse_struct_def()`, `parse_function()`, struct member parsing — add `visibility: Visibility::Internal` to all constructions
- `src/semantic/mod.rs`: Pattern matches on `StructMember` — add `visibility: _` or `visibility` fields
- `src/bir/lowering.rs`: Pattern matches on `StructMember` — add `visibility: _`
- `tests/compile_test.rs`: Should not need changes (uses `compile_and_run` helper)

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: all existing tests pass (behavior unchanged)

- [ ] **Step 4: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add AST nodes for module system: Visibility, ModuleDecl, ImportDecl"
```

---

## Task 3: Parse Visibility, Module Declarations, and Import Declarations

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write failing parse tests**

Add to the test module at the bottom of `src/parser/mod.rs` (create it if it doesn't exist, or add as a new test file `tests/parse_module_test.rs`):

```rust
#[cfg(test)]
mod module_tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::ast::*;

    fn parse_source(source: &str) -> Program {
        let tokens = tokenize(source).unwrap();
        parse(tokens).unwrap()
    }

    #[test]
    fn parse_module_decl() {
        let prog = parse_source("module math; func main() -> Int32 { return 0; }");
        assert_eq!(prog.module_decls.len(), 1);
        assert_eq!(prog.module_decls[0].name, "math");
        assert_eq!(prog.module_decls[0].visibility, Visibility::Internal);
    }

    #[test]
    fn parse_public_module_decl() {
        let prog = parse_source("public module math; func main() -> Int32 { return 0; }");
        assert_eq!(prog.module_decls[0].visibility, Visibility::Public);
    }

    #[test]
    fn parse_import_single() {
        let prog = parse_source("import math::Vector; func main() -> Int32 { return 0; }");
        assert_eq!(prog.import_decls.len(), 1);
        assert_eq!(prog.import_decls[0].prefix, PathPrefix::Named("math".to_string()));
        assert_eq!(prog.import_decls[0].tail, ImportTail::Single("Vector".to_string()));
    }

    #[test]
    fn parse_import_group() {
        let prog = parse_source("import math::{Vector, Matrix}; func main() -> Int32 { return 0; }");
        assert_eq!(prog.import_decls[0].tail, ImportTail::Group(vec!["Vector".to_string(), "Matrix".to_string()]));
    }

    #[test]
    fn parse_import_glob() {
        let prog = parse_source("import math::*; func main() -> Int32 { return 0; }");
        assert_eq!(prog.import_decls[0].tail, ImportTail::Glob);
    }

    #[test]
    fn parse_import_self_path() {
        let prog = parse_source("import self::sub::helper; func main() -> Int32 { return 0; }");
        assert_eq!(prog.import_decls[0].prefix, PathPrefix::SelfKw);
        assert_eq!(prog.import_decls[0].path, vec!["sub".to_string()]);
        assert_eq!(prog.import_decls[0].tail, ImportTail::Single("helper".to_string()));
    }

    #[test]
    fn parse_import_super_path() {
        let prog = parse_source("import super::common::Util; func main() -> Int32 { return 0; }");
        assert_eq!(prog.import_decls[0].prefix, PathPrefix::Super);
        assert_eq!(prog.import_decls[0].path, vec!["common".to_string()]);
        assert_eq!(prog.import_decls[0].tail, ImportTail::Single("Util".to_string()));
    }

    #[test]
    fn parse_public_import_reexport() {
        let prog = parse_source("public import self::internal::Vector; func main() -> Int32 { return 0; }");
        assert_eq!(prog.import_decls[0].visibility, Visibility::Public);
    }

    #[test]
    fn parse_visibility_on_func() {
        let prog = parse_source("public func add(a: Int32, b: Int32) -> Int32 { return a + b; } func main() -> Int32 { return 0; }");
        assert_eq!(prog.functions[0].visibility, Visibility::Public);
        assert_eq!(prog.functions[1].visibility, Visibility::Internal);
    }

    #[test]
    fn parse_visibility_on_struct() {
        let prog = parse_source("public struct Foo { private var x: Int32; } func main() -> Int32 { return 0; }");
        assert_eq!(prog.structs[0].visibility, Visibility::Public);
        match &prog.structs[0].members[0] {
            StructMember::StoredProperty { visibility, .. } => {
                assert_eq!(*visibility, Visibility::Private);
            }
            _ => panic!("expected StoredProperty"),
        }
    }

    #[test]
    fn parse_nested_import_path() {
        let prog = parse_source("import graphics::renderer::Shader; func main() -> Int32 { return 0; }");
        assert_eq!(prog.import_decls[0].prefix, PathPrefix::Named("graphics".to_string()));
        assert_eq!(prog.import_decls[0].path, vec!["renderer".to_string()]);
        assert_eq!(prog.import_decls[0].tail, ImportTail::Single("Shader".to_string()));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test parse_module`
Expected: compilation errors (methods don't exist)

- [ ] **Step 3: Implement parser changes**

In `src/parser/mod.rs`, add these methods to the `Parser` impl:

```rust
fn try_parse_visibility(&mut self) -> Visibility {
    match &self.peek().node {
        Token::Public => { self.advance(); Visibility::Public }
        Token::Package => { self.advance(); Visibility::Package }
        Token::Internal => { self.advance(); Visibility::Internal }
        Token::Fileprivate => { self.advance(); Visibility::Fileprivate }
        Token::Private => { self.advance(); Visibility::Private }
        _ => Visibility::Internal,
    }
}

fn is_visibility_token(token: &Token) -> bool {
    matches!(token, Token::Public | Token::Package | Token::Internal | Token::Fileprivate | Token::Private)
}
```

Update `parse_program()`:

```rust
fn parse_program(&mut self) -> Result<Program> {
    let mut module_decls = Vec::new();
    let mut import_decls = Vec::new();
    let mut structs = Vec::new();
    let mut protocols = Vec::new();
    let mut functions = Vec::new();

    // Parse module declarations (must come first)
    while self.peek().node != Token::Eof {
        let vis = if Self::is_visibility_token(&self.peek().node)
            && self.tokens.get(self.pos + 1).map(|t| &t.node) == Some(&Token::Module)
        {
            self.try_parse_visibility()
        } else if self.peek().node == Token::Module {
            Visibility::Internal
        } else {
            break;
        };
        self.expect(Token::Module)?;
        let name = self.expect_ident()?;
        self.expect(Token::Semicolon)?;
        module_decls.push(ModuleDecl { visibility: vis, name });
    }

    // Parse import declarations
    while self.peek().node != Token::Eof {
        let vis = if Self::is_visibility_token(&self.peek().node)
            && self.tokens.get(self.pos + 1).map(|t| &t.node) == Some(&Token::Import)
        {
            self.try_parse_visibility()
        } else if self.peek().node == Token::Import {
            Visibility::Internal
        } else {
            break;
        };
        self.advance(); // consume `import`
        let import_decl = self.parse_import_path(vis)?;
        self.expect(Token::Semicolon)?;
        import_decls.push(import_decl);
    }

    // Parse top-level declarations
    while self.peek().node != Token::Eof {
        let vis = self.try_parse_visibility();
        match self.peek().node {
            Token::Struct => {
                let mut s = self.parse_struct_def()?;
                s.visibility = vis;
                structs.push(s);
            }
            Token::Protocol => {
                let mut p = self.parse_protocol_def()?;
                p.visibility = vis;
                protocols.push(p);
            }
            _ => {
                let mut f = self.parse_function()?;
                f.visibility = vis;
                functions.push(f);
            }
        }
    }

    Ok(Program {
        module_decls,
        import_decls,
        structs,
        protocols,
        functions,
    })
}
```

Add `parse_import_path()`:

```rust
fn parse_import_path(&mut self, visibility: Visibility) -> Result<ImportDecl> {
    // Parse prefix: self, super, or identifier
    let prefix = match &self.peek().node {
        Token::SelfKw => {
            self.advance();
            PathPrefix::SelfKw
        }
        Token::Super => {
            self.advance();
            PathPrefix::Super
        }
        Token::Ident(_) => {
            let name = self.expect_ident()?;
            PathPrefix::Named(name)
        }
        _ => {
            return Err(BengalError::ParseError {
                message: format!("expected module path, found `{}`", self.peek().node),
                span: self.peek().span,
            });
        }
    };

    self.expect(Token::ColonColon)?;

    // Parse remaining path segments and tail
    let mut path = Vec::new();
    let tail = self.parse_import_tail(&mut path)?;

    Ok(ImportDecl {
        visibility,
        prefix,
        path,
        tail,
    })
}

fn parse_import_tail(&mut self, path: &mut Vec<String>) -> Result<ImportTail> {
    // Check for glob
    if self.peek().node == Token::Star {
        self.advance();
        return Ok(ImportTail::Glob);
    }

    // Check for group: { A, B, C }
    if self.peek().node == Token::LBrace {
        self.advance(); // consume {
        let mut names = vec![self.expect_ident()?];
        while self.peek().node == Token::Comma {
            self.advance();
            names.push(self.expect_ident()?);
        }
        self.expect(Token::RBrace)?;
        return Ok(ImportTail::Group(names));
    }

    // Must be an identifier
    let name = self.expect_ident()?;

    // Check if path continues with ::
    if self.peek().node == Token::ColonColon {
        self.advance(); // consume ::
        path.push(name);
        return self.parse_import_tail(path);
    }

    // Terminal identifier
    Ok(ImportTail::Single(name))
}
```

Update struct member parsing to accept visibility:

In the existing struct member parsing methods, add visibility parameter or parse it before dispatching. For each member variant, parse visibility with `try_parse_visibility()` and assign it.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: all tests pass (new and existing)

- [ ] **Step 5: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 6: Commit**

```bash
git add src/parser/mod.rs
git commit -m "Parse module declarations, import declarations, and visibility modifiers"
```

---

## Task 4: Package Discovery and Bengal.toml

**Files:**
- Create: `src/package.rs`
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add dependencies**

In `Cargo.toml`, add:

```toml
toml = "0.8"
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 2: Write failing tests for package discovery**

Create `src/package.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn find_bengal_toml_in_same_dir() {
        let dir = TempDir::new().unwrap();
        let toml_path = dir.path().join("Bengal.toml");
        fs::write(&toml_path, "[package]\nname = \"test\"\nentry = \"main.bengal\"").unwrap();
        let result = find_package_root(dir.path()).unwrap();
        assert_eq!(result, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn find_bengal_toml_in_parent_dir() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("src");
        fs::create_dir(&sub).unwrap();
        let toml_path = dir.path().join("Bengal.toml");
        fs::write(&toml_path, "[package]\nname = \"test\"\nentry = \"src/main.bengal\"").unwrap();
        let result = find_package_root(&sub).unwrap();
        assert_eq!(result, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn no_bengal_toml_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = find_package_root(dir.path()).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn parse_bengal_toml() {
        let content = "[package]\nname = \"my_app\"\nentry = \"src/main.bengal\"";
        let config = parse_package_config(content).unwrap();
        assert_eq!(config.package.name, "my_app");
        assert_eq!(config.package.entry, "src/main.bengal");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib package`
Expected: compilation errors

- [ ] **Step 4: Implement package discovery**

In `src/package.rs`:

```rust
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{BengalError, Result};

#[derive(Debug, Deserialize)]
pub struct PackageConfig {
    pub package: PackageSection,
}

#[derive(Debug, Deserialize)]
pub struct PackageSection {
    pub name: String,
    pub entry: String,
}

pub fn find_package_root(start: &Path) -> Result<Option<PathBuf>> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("Bengal.toml").exists() {
            return Ok(Some(current));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn parse_package_config(content: &str) -> Result<PackageConfig> {
    toml::from_str(content).map_err(|e| BengalError::PackageError {
        message: format!("failed to parse Bengal.toml: {}", e),
    })
}

pub fn load_package(root: &Path) -> Result<PackageConfig> {
    let toml_path = root.join("Bengal.toml");
    let content = std::fs::read_to_string(&toml_path).map_err(|e| BengalError::PackageError {
        message: format!("failed to read {}: {}", toml_path.display(), e),
    })?;
    parse_package_config(&content)
}
```

- [ ] **Step 5: Add PackageError variant to error.rs**

In `src/error.rs`, add to `BengalError`:

```rust
#[error("Package error: {message}")]
PackageError { message: String },
```

And in `into_diagnostic`, add:

```rust
BengalError::PackageError { message } => BengalDiagnostic {
    message,
    src_code: source,
    span: None,
    label: String::new(),
},
```

- [ ] **Step 6: Register module in lib.rs**

In `src/lib.rs`, add:

```rust
pub mod package;
```

Also add `tempfile` as a dev-dependency in `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib package`
Expected: all tests pass

- [ ] **Step 8: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/package.rs src/lib.rs src/error.rs
git commit -m "Add Bengal.toml discovery and parsing"
```

---

## Task 5: Module Graph Construction

**Files:**
- Modify: `src/package.rs`

- [ ] **Step 1: Write failing tests for module graph**

Add to `src/package.rs` tests:

```rust
#[test]
fn build_module_graph_single_module() {
    let dir = TempDir::new().unwrap();
    let main_path = dir.path().join("main.bengal");
    fs::write(&main_path, "module math; func main() -> Int32 { return 0; }").unwrap();
    let math_path = dir.path().join("math.bengal");
    fs::write(&math_path, "func add(a: Int32, b: Int32) -> Int32 { return a + b; }").unwrap();

    let graph = build_module_graph(&main_path).unwrap();
    assert_eq!(graph.modules.len(), 2); // root + math
    assert!(graph.modules.contains_key(&ModulePath(vec![])));
    assert!(graph.modules.contains_key(&ModulePath(vec!["math".to_string()])));
}

#[test]
fn module_graph_cycle_detection() {
    let dir = TempDir::new().unwrap();
    let a_path = dir.path().join("a.bengal");
    fs::write(&a_path, "module b;").unwrap();
    let b_path = dir.path().join("b.bengal");
    fs::write(&b_path, "module a;").unwrap();

    let result = build_module_graph(&a_path);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("circular"));
}

#[test]
fn module_graph_missing_file() {
    let dir = TempDir::new().unwrap();
    let main_path = dir.path().join("main.bengal");
    fs::write(&main_path, "module nonexistent;").unwrap();

    let result = build_module_graph(&main_path);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found"));
}

#[test]
fn module_graph_directory_module() {
    let dir = TempDir::new().unwrap();
    let main_path = dir.path().join("main.bengal");
    fs::write(&main_path, "module graphics;").unwrap();
    let graphics_dir = dir.path().join("graphics");
    fs::create_dir(&graphics_dir).unwrap();
    fs::write(graphics_dir.join("module.bengal"), "func draw() -> Int32 { return 1; }").unwrap();

    let graph = build_module_graph(&main_path).unwrap();
    assert!(graph.modules.contains_key(&ModulePath(vec!["graphics".to_string()])));
}

#[test]
fn module_graph_duplicate_file() {
    let dir = TempDir::new().unwrap();
    let main_path = dir.path().join("main.bengal");
    fs::write(&main_path, "module a; module b;").unwrap();
    // Both a.bengal and b.bengal point to the same file via symlink or same content
    // But more practically: two modules try to claim the same file
    let a_dir = dir.path().join("a");
    fs::create_dir(&a_dir).unwrap();
    fs::write(a_dir.join("module.bengal"), "module shared;").unwrap();
    let b_path = dir.path().join("b.bengal");
    fs::write(&b_path, "module shared;").unwrap();
    // shared.bengal exists at both a/shared.bengal and ./shared.bengal
    fs::write(dir.path().join("shared.bengal"), "func x() -> Int32 { return 1; }").unwrap();
    fs::write(a_dir.join("shared.bengal"), "func y() -> Int32 { return 2; }").unwrap();
    // This should work — different files at different paths are fine.
    // The real duplicate check is for the SAME canonical file path.
    let graph = build_module_graph(&main_path).unwrap();
    assert!(graph.modules.len() >= 3);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib package`
Expected: compilation errors

- [ ] **Step 3: Implement module graph construction**

Add to `src/package.rs`:

```rust
use std::collections::HashMap;

use crate::lexer;
use crate::parser;
use crate::parser::ast::{Program, ModuleDecl};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath(pub Vec<String>);

impl ModulePath {
    pub fn root() -> Self {
        ModulePath(vec![])
    }

    pub fn child(&self, name: &str) -> Self {
        let mut path = self.0.clone();
        path.push(name.to_string());
        ModulePath(path)
    }
}

#[derive(Debug)]
pub struct ModuleInfo {
    pub path: ModulePath,
    pub file_path: PathBuf,
    pub source: String,
    pub ast: Program,
}

#[derive(Debug)]
pub struct ModuleGraph {
    pub modules: HashMap<ModulePath, ModuleInfo>,
}

pub fn build_module_graph(entry_path: &Path) -> Result<ModuleGraph> {
    let mut modules = HashMap::new();
    let mut visiting = Vec::new(); // for cycle detection

    fn visit(
        file_path: &Path,
        module_path: ModulePath,
        modules: &mut HashMap<ModulePath, ModuleInfo>,
        visiting: &mut Vec<String>,
    ) -> Result<()> {
        let path_str = module_path.0.join("::");
        let display_name = if path_str.is_empty() { "root".to_string() } else { path_str.clone() };

        if visiting.contains(&display_name) {
            visiting.push(display_name);
            return Err(BengalError::PackageError {
                message: format!(
                    "circular module dependency detected: {}",
                    visiting.join(" -> ")
                ),
            });
        }
        visiting.push(display_name);

        let source = std::fs::read_to_string(file_path).map_err(|e| BengalError::PackageError {
            message: format!("failed to read {}: {}", file_path.display(), e),
        })?;
        let tokens = lexer::tokenize(&source)?;
        let ast = parser::parse(tokens)?;

        let dir = file_path.parent().unwrap();

        // Process child module declarations
        for decl in &ast.module_decls {
            let child_path = module_path.child(&decl.name);
            let child_file = resolve_module_file(dir, &decl.name)?;
            visit(&child_file, child_path, modules, visiting)?;
        }

        modules.insert(
            module_path.clone(),
            ModuleInfo {
                path: module_path,
                file_path: file_path.to_path_buf(),
                source,
                ast,
            },
        );

        visiting.pop();
        Ok(())
    }

    visit(entry_path, ModulePath::root(), &mut modules, &mut visiting)?;

    Ok(ModuleGraph { modules })
}

fn resolve_module_file(parent_dir: &Path, name: &str) -> Result<PathBuf> {
    let file_path = parent_dir.join(format!("{}.bengal", name));
    if file_path.exists() {
        return Ok(file_path);
    }
    let dir_path = parent_dir.join(name).join("module.bengal");
    if dir_path.exists() {
        return Ok(dir_path);
    }
    Err(BengalError::PackageError {
        message: format!(
            "module '{}' not found: expected '{}.bengal' or '{}/module.bengal'",
            name, name, name
        ),
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib package`
Expected: all tests pass

- [ ] **Step 5: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 6: Commit**

```bash
git add src/package.rs
git commit -m "Add module graph construction with cycle detection"
```

---

## Task 6: Name Mangling

**Files:**
- Create: `src/mangle.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests**

Create `src/mangle.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangle_root_function() {
        let result = mangle_function("my_app", &[""], "add");
        // root module: package + name
        assert_eq!(result, "_BG6my_app3add");
    }

    #[test]
    fn mangle_nested_function() {
        let result = mangle_function("my_app", &["math"], "add");
        assert_eq!(result, "_BG6my_app4math3add");
    }

    #[test]
    fn mangle_deeply_nested() {
        let result = mangle_function("my_app", &["graphics", "renderer"], "draw");
        assert_eq!(result, "_BG6my_app8graphics8renderer4draw");
    }

    #[test]
    fn mangle_method() {
        let result = mangle_method("my_app", &["math"], "Vector", "length");
        assert_eq!(result, "_BG6my_app4math6Vector6length");
    }

    #[test]
    fn mangle_with_underscores_no_ambiguity() {
        let a = mangle_function("my_app", &["foo_bar"], "add");
        let b = mangle_function("my_app", &["foo"], "bar_add");
        assert_ne!(a, b);
    }

    #[test]
    fn mangle_main_in_entry_not_mangled() {
        assert_eq!(mangle_entry_main(), "main");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib mangle`
Expected: compilation errors

- [ ] **Step 3: Implement name mangling**

In `src/mangle.rs`:

```rust
use crate::package::ModulePath;

fn length_prefix(s: &str) -> String {
    format!("{}{}", s.len(), s)
}

pub fn mangle_function(package_name: &str, module_segments: &[&str], func_name: &str) -> String {
    let mut result = String::from("_BG");
    result.push_str(&length_prefix(package_name));
    for seg in module_segments {
        if !seg.is_empty() {
            result.push_str(&length_prefix(seg));
        }
    }
    result.push_str(&length_prefix(func_name));
    result
}

pub fn mangle_method(
    package_name: &str,
    module_segments: &[&str],
    struct_name: &str,
    method_name: &str,
) -> String {
    let mut result = String::from("_BG");
    result.push_str(&length_prefix(package_name));
    for seg in module_segments {
        if !seg.is_empty() {
            result.push_str(&length_prefix(seg));
        }
    }
    result.push_str(&length_prefix(struct_name));
    result.push_str(&length_prefix(method_name));
    result
}

pub fn mangle_entry_main() -> &'static str {
    "main"
}

pub fn mangle_from_module_path(
    package_name: &str,
    module_path: &ModulePath,
    func_name: &str,
) -> String {
    let segments: Vec<&str> = module_path.0.iter().map(|s| s.as_str()).collect();
    mangle_function(package_name, &segments, func_name)
}

pub fn mangle_method_from_module_path(
    package_name: &str,
    module_path: &ModulePath,
    struct_name: &str,
    method_name: &str,
) -> String {
    let segments: Vec<&str> = module_path.0.iter().map(|s| s.as_str()).collect();
    mangle_method(package_name, &segments, struct_name, method_name)
}
```

- [ ] **Step 4: Register in lib.rs**

```rust
pub mod mangle;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib mangle`
Expected: all tests pass

- [ ] **Step 6: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 7: Commit**

```bash
git add src/mangle.rs src/lib.rs
git commit -m "Add length-prefixed name mangling"
```

---

## Task 7: Multi-Module Semantic Analysis — Symbol Registration

**Files:**
- Modify: `src/semantic/resolver.rs`
- Modify: `src/semantic/mod.rs`

This is the most complex task. It extends the resolver to be module-aware and adds import resolution.

- [ ] **Step 1: Add module-aware structures to resolver**

In `src/semantic/resolver.rs`, add:

```rust
use crate::package::ModulePath;
use crate::parser::ast::Visibility;

#[derive(Debug, Clone)]
pub struct ModuleSymbol {
    pub visibility: Visibility,
    pub module_path: ModulePath,
}

#[derive(Debug, Clone)]
pub struct QualifiedName {
    pub module_path: ModulePath,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum SymbolKind {
    Function(FuncSig),
    Struct(StructInfo),
    Protocol(ProtocolInfo),
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub module_path: ModulePath,
    pub file_path: String,
}
```

Add a `ModuleResolver` that wraps the global symbol table:

```rust
pub struct ModuleResolver {
    /// All symbols indexed by (module_path, name)
    symbols: HashMap<(ModulePath, String), Symbol>,
    /// Module declarations: which modules exist and their visibility
    module_decls: HashMap<ModulePath, ModuleSymbol>,
    /// Per-module imported symbols: (importing_module, local_name) -> QualifiedName
    imports: HashMap<(ModulePath, String), QualifiedName>,
    /// Per-module glob imports: importing_module -> list of source module paths
    glob_imports: HashMap<ModulePath, Vec<ModulePath>>,
    /// Re-exports: (module_path, local_name) -> (source_module, source_name, visibility)
    reexports: HashMap<(ModulePath, String), (ModulePath, String, Visibility)>,
    /// Package name
    pub package_name: String,
}
```

This is a significant refactor. The existing `Resolver` continues to handle local scope (variables, loops, etc.) during Pass 3 body analysis. `ModuleResolver` handles cross-module name resolution.

- [ ] **Step 2: Implement ModuleResolver methods**

Key methods needed:

```rust
impl ModuleResolver {
    pub fn new(package_name: String) -> Self { ... }
    pub fn register_symbol(&mut self, module: &ModulePath, name: String, symbol: Symbol) -> Result<()> { ... }
    pub fn register_module(&mut self, path: ModulePath, symbol: ModuleSymbol) { ... }
    pub fn register_import(&mut self, importing_module: &ModulePath, local_name: String, target: QualifiedName) { ... }
    pub fn register_glob_import(&mut self, importing_module: &ModulePath, source_module: ModulePath) { ... }
    pub fn resolve_symbol(&self, from_module: &ModulePath, name: &str, file_path: &str) -> Result<&Symbol> { ... }
    pub fn lookup_in_module(&self, module: &ModulePath, name: &str) -> Option<&Symbol> { ... }
    pub fn check_visibility(&self, symbol: &Symbol, accessor_module: &ModulePath, accessor_file: &str) -> bool { ... }
}
```

Visibility checking logic:
```rust
fn check_visibility(symbol: &Symbol, accessor_module: &ModulePath, accessor_file: &str) -> bool {
    match symbol.visibility {
        Visibility::Public => true,
        Visibility::Package => true, // same package always
        Visibility::Internal => symbol.module_path == *accessor_module,
        Visibility::Fileprivate => symbol.file_path == accessor_file,
        Visibility::Private => false, // handled separately in struct context
    }
}
```

- [ ] **Step 3: Add multi-module analyze function**

In `src/semantic/mod.rs`, add:

```rust
pub fn analyze_package(
    graph: &ModuleGraph,
    package_name: &str,
) -> Result<PackageSemanticInfo> { ... }
```

This follows the same pass structure as the existing `analyze()` but iterates over all modules:

- Pass 1a: For each module, register structs/protocols/functions with module paths
- Pass 1b: For each module, resolve types and members
- Pass 1b (imports): Resolve all import declarations
- Pass 2: Verify `main() -> Int32` in root module
- Pass 3: Analyze bodies with visibility checking
- Pass 3b: Protocol conformance checking

The existing single-module `analyze()` remains for backward compatibility.

- [ ] **Step 4: Write tests for multi-module analysis**

Add tests that create `ModuleGraph` manually and verify:
- Functions from different modules can be registered without conflict
- Same name in different modules is allowed
- Import resolution works for single/group/glob
- Visibility violations are caught
- `self::` and `super::` paths resolve correctly

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 7: Commit**

```bash
git add src/semantic/resolver.rs src/semantic/mod.rs
git commit -m "Add multi-module semantic analysis with import resolution and visibility"
```

---

## Task 8: Per-Module BIR Lowering with Name Mangling

**Files:**
- Modify: `src/bir/lowering.rs`
- Modify: `src/bir/mod.rs`

- [ ] **Step 1: Add module context to lowering**

Extend the `Lowering` struct to carry module path and package name:

```rust
struct Lowering {
    // ... existing fields ...
    package_name: String,
    module_path: ModulePath,
}
```

- [ ] **Step 2: Update function name emission**

In the function lowering, apply name mangling:

- For the entry module's `main` function: keep as `"main"`
- For all other functions: `mangle_from_module_path(package_name, module_path, func_name)`
- For methods: `mangle_method_from_module_path(package_name, module_path, struct_name, method_name)`

Update `Call` instructions: when calling a function, look up its module path and mangle accordingly. Cross-module calls must use the mangled name of the target function.

- [ ] **Step 3: Add lower_module function**

In `src/bir/mod.rs` or `src/bir/lowering.rs`, add:

```rust
pub fn lower_module(
    ast: &Program,
    sem_info: &SemanticInfo,
    package_name: &str,
    module_path: &ModulePath,
    is_entry: bool,
) -> Result<BirModule> { ... }
```

This is a variant of `lower_program` that applies mangling.

- [ ] **Step 4: Write tests**

Test that lowered BIR has correctly mangled function names.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 7: Commit**

```bash
git add src/bir/lowering.rs src/bir/mod.rs
git commit -m "Add per-module BIR lowering with name mangling"
```

---

## Task 9: Per-Module LLVM Codegen and Linking

**Files:**
- Modify: `src/codegen/llvm.rs`
- Modify: `src/codegen/mod.rs`

- [ ] **Step 1: Add external function declarations**

When compiling a module to LLVM IR, functions from other modules that are called must be declared as external:

```rust
pub fn compile_module(
    bir: &BirModule,
    external_functions: &[(String, Vec<BirType>, BirType)], // (mangled_name, param_types, return_type)
) -> Result<Vec<u8>> { ... }
```

Before emitting the module's functions, iterate over `external_functions` and emit LLVM `declare` for each.

- [ ] **Step 2: Add multi-module compile and link**

In `src/codegen/mod.rs` or a new function in `llvm.rs`:

```rust
pub fn compile_and_link(
    modules: Vec<(&BirModule, &[(String, Vec<BirType>, BirType)])>,
    output_path: &Path,
) -> Result<()> {
    let mut obj_paths = Vec::new();
    for (i, (bir, externals)) in modules.iter().enumerate() {
        let obj_bytes = compile_module(bir, externals)?;
        let obj_path = output_path.with_extension(format!("{}.o", i));
        std::fs::write(&obj_path, &obj_bytes)?;
        obj_paths.push(obj_path);
    }
    // Link all .o files
    let status = std::process::Command::new("cc")
        .args(&obj_paths)
        .arg("-o")
        .arg(output_path)
        .status()?;
    if !status.success() {
        return Err(codegen_err("linker failed"));
    }
    // Clean up .o files
    for p in &obj_paths {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}
```

- [ ] **Step 3: Write tests**

Test compilation of two modules where one calls the other's function.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 5: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 6: Commit**

```bash
git add src/codegen/llvm.rs src/codegen/mod.rs
git commit -m "Add per-module LLVM codegen with external declarations and linking"
```

---

## Task 10: Multi-File Compile Entry Point

**Files:**
- Modify: `src/lib.rs`

- [ ] **Step 1: Add compile_package function**

```rust
pub fn compile_package(entry_path: &Path) -> Result<Vec<(bir::instruction::BirModule, Vec<(String, Vec<bir::instruction::BirType>, bir::instruction::BirType)>)>> {
    let graph = package::build_module_graph(entry_path)?;
    let entry_dir = entry_path.parent().unwrap();
    let package_root = package::find_package_root(entry_dir)?;
    let package_name = match &package_root {
        Some(root) => {
            let config = package::load_package(root)?;
            config.package.name
        }
        None => "main".to_string(), // single-file fallback
    };

    let sem_info = semantic::analyze_package(&graph, &package_name)?;

    let mut modules = Vec::new();
    for (mod_path, mod_info) in &graph.modules {
        let is_entry = mod_path == &package::ModulePath::root();
        let bir = bir::lower_module(&mod_info.ast, &sem_info, &package_name, mod_path, is_entry)?;
        // Collect external function signatures for this module
        let externals = sem_info.external_functions_for(mod_path);
        modules.push((bir, externals));
    }

    Ok(modules)
}
```

The exact API will depend on Task 7 and 8's `PackageSemanticInfo` structure.

- [ ] **Step 2: Ensure backward compatibility**

The existing `compile_source(&str) -> Result<Vec<u8>>` must still work for single-file mode. It should not be modified — single-file mode uses the old pipeline without modules.

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all existing tests still pass

- [ ] **Step 4: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs
git commit -m "Add compile_package entry point for multi-file compilation"
```

---

## Task 11: CLI Changes

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Update Compile command**

Modify the `Compile` command to:
1. Check if the file is part of a package (find Bengal.toml upward)
2. If yes: use `compile_package` pipeline
3. If no: use existing `compile_source` pipeline (backward compat)

```rust
Command::Compile { file, emit_bir } => {
    let source_path = file.canonicalize().map_err(|e| miette::miette!("{e}"))?;
    let start_dir = source_path.parent().unwrap();

    if let Some(package_root) = bengal::package::find_package_root(start_dir)
        .map_err(|e| Report::new(e.into_diagnostic("<package>", "")))?
    {
        // Package mode: multi-file compilation
        let config = bengal::package::load_package(&package_root)
            .map_err(|e| Report::new(e.into_diagnostic("<package>", "")))?;
        let entry_path = package_root.join(&config.package.entry);
        // ... use compile_package ...
    } else {
        // Single-file mode (existing behavior)
        let source = std::fs::read_to_string(&file).map_err(|e| miette::miette!("{e}"))?;
        // ... existing code ...
    }
}
```

- [ ] **Step 2: Run existing CLI test**

Run: `cargo run -- eval "func main() -> Int32 { return 42; }"`
Expected: prints `42` (backward compat)

- [ ] **Step 3: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "Update CLI for package-aware compilation"
```

---

## Task 12: Integration Tests — Multi-File Compilation

**Files:**
- Modify: `tests/compile_test.rs`

- [ ] **Step 1: Add multi-file test helper**

```rust
fn compile_and_run_package(files: &[(&str, &str)]) -> i32 {
    // files: [(relative_path, source_code)]
    // Creates a temp directory, writes Bengal.toml and all files,
    // runs the full package compilation pipeline, JIT-executes main
    let dir = tempfile::TempDir::new().unwrap();

    // Write Bengal.toml
    let toml_content = format!(
        "[package]\nname = \"test_pkg\"\nentry = \"{}\"",
        files[0].0
    );
    std::fs::write(dir.path().join("Bengal.toml"), toml_content).unwrap();

    // Write source files
    for (path, source) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full_path, source).unwrap();
    }

    // Compile to executable
    let entry_path = dir.path().join(files[0].0);
    let exe_path = dir.path().join("test_exe");
    bengal::compile_package_to_executable(&entry_path, &exe_path).unwrap();

    // Run the executable and capture exit code
    let output = std::process::Command::new(&exe_path)
        .output()
        .expect("failed to run compiled executable");
    output.status.code().unwrap_or(-1)
}

fn compile_package_result(files: &[(&str, &str)]) -> bengal::error::Result<()> {
    let dir = tempfile::TempDir::new().unwrap();
    let toml_content = format!(
        "[package]\nname = \"test_pkg\"\nentry = \"{}\"",
        files[0].0
    );
    std::fs::write(dir.path().join("Bengal.toml"), toml_content).unwrap();
    for (path, source) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full_path, source).unwrap();
    }
    let entry_path = dir.path().join(files[0].0);
    let exe_path = dir.path().join("test_exe");
    bengal::compile_package_to_executable(&entry_path, &exe_path)
}
```

- [ ] **Step 2: Write integration tests**

```rust
#[test]
fn multi_file_cross_module_call() {
    let result = compile_and_run_package(&[
        ("main.bengal", r#"
            module math;
            import math::add;
            func main() -> Int32 {
                return add(1, 2);
            }
        "#),
        ("math.bengal", r#"
            public func add(a: Int32, b: Int32) -> Int32 {
                return a + b;
            }
        "#),
    ]);
    assert_eq!(result, 3);
}

#[test]
fn multi_file_visibility_internal_denied() {
    // internal func in math should not be accessible from main
    let result = compile_package_result(&[
        ("main.bengal", r#"
            module math;
            import math::helper;
            func main() -> Int32 { return helper(); }
        "#),
        ("math.bengal", r#"
            func helper() -> Int32 { return 1; }
        "#),
    ]);
    assert!(result.is_err());
}

#[test]
fn multi_file_struct_across_modules() {
    let result = compile_and_run_package(&[
        ("main.bengal", r#"
            module shapes;
            import shapes::Point;
            func main() -> Int32 {
                let p = Point(x: 3, y: 4);
                return p.x + p.y;
            }
        "#),
        ("shapes.bengal", r#"
            public struct Point {
                public var x: Int32;
                public var y: Int32;
            }
        "#),
    ]);
    assert_eq!(result, 7);
}

#[test]
fn multi_file_glob_import() {
    let result = compile_and_run_package(&[
        ("main.bengal", r#"
            module math;
            import math::*;
            func main() -> Int32 {
                return add(10, mul(2, 3));
            }
        "#),
        ("math.bengal", r#"
            public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
            public func mul(a: Int32, b: Int32) -> Int32 { return a * b; }
        "#),
    ]);
    assert_eq!(result, 16);
}

#[test]
fn multi_file_reexport() {
    let result = compile_and_run_package(&[
        ("main.bengal", r#"
            module facade;
            import facade::add;
            func main() -> Int32 { return add(5, 6); }
        "#),
        ("facade.bengal", r#"
            module math;
            public import self::math::add;
        "#),
        ("facade/math.bengal", r#"
            public func add(a: Int32, b: Int32) -> Int32 { return a + b; }
        "#),
    ]);
    assert_eq!(result, 11);
}

#[test]
fn single_file_backward_compat() {
    // No Bengal.toml — should compile as before
    let result = compile_and_run("func main() -> Int32 { return 42; }");
    assert_eq!(result, 42);
}

#[test]
fn import_self_path() {
    let result = compile_and_run_package(&[
        ("main.bengal", r#"
            module util;
            import self::util::helper;
            func main() -> Int32 { return helper(); }
        "#),
        ("util.bengal", r#"
            public func helper() -> Int32 { return 99; }
        "#),
    ]);
    assert_eq!(result, 99);
}

#[test]
fn import_super_path() {
    let result = compile_and_run_package(&[
        ("main.bengal", r#"
            module sub;
            public func shared() -> Int32 { return 42; }
            import self::sub::call_shared;
            func main() -> Int32 {
                return call_shared();
            }
        "#),
        ("sub.bengal", r#"
            import super::shared;
            public func call_shared() -> Int32 { return shared(); }
        "#),
    ]);
    assert_eq!(result, 42);
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy`

- [ ] **Step 5: Commit**

```bash
git add tests/compile_test.rs
git commit -m "Add integration tests for multi-file module system"
```

---

## Task 13: Update Documentation

**Files:**
- Modify: `docs/grammar.md`

- [ ] **Step 1: Update grammar.md with module system syntax**

Add sections for:
- New keywords and tokens
- Module declarations
- Import declarations
- Visibility modifiers
- Updated Program grammar rule

Follow the existing style in grammar.md.

- [ ] **Step 2: Run cargo fmt and clippy (final check)**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: everything clean, all tests pass

- [ ] **Step 3: Commit**

```bash
git add docs/grammar.md
git commit -m "Document module system syntax in grammar.md"
```
