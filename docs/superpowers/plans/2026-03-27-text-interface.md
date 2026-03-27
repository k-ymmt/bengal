# Text-Based Interface File (.bengalinterface) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement text-based `.bengalinterface` files for human-readable, compiler-version-stable module interface serialization.

**Architecture:** Reuse the existing Bengal parser with an `interface_mode` flag for body-less declarations. Add an emitter (`ModuleInterface` → text) and a reader (text → AST → `ModuleInterface`). AST body fields become `Option<Block>` to support declarations without bodies.

**Tech Stack:** Rust, existing Bengal lexer/parser, existing `ModuleInterface` types in `src/interface.rs`

---

### Task 1: Add visibility field to Interface entry types

**Files:**
- Modify: `src/interface.rs:96-100` (InterfaceFuncEntry)
- Modify: `src/interface.rs:109-118` (InterfaceStructEntry)
- Modify: `src/interface.rs:134-139` (InterfaceProtocolEntry)
- Modify: `src/interface.rs:149-276` (from_semantic_info)
- Modify: `tests/interface.rs` (update all test assertions constructing these types)

The text format must distinguish `public` from `package` visibility. Currently the interface entry types have no visibility field — add one.

- [ ] **Step 1: Add `visibility` field to the three entry types**

In `src/interface.rs`, add `pub visibility: Visibility` to `InterfaceFuncEntry`, `InterfaceStructEntry`, and `InterfaceProtocolEntry`. Use the existing `Visibility` type from `crate::parser::ast`. Add `Serialize`/`Deserialize` derives to `Visibility` in `src/parser/ast.rs` (it currently only has `Debug, Clone, Copy, PartialEq, Eq, Default`).

```rust
// src/parser/ast.rs — add serde import and derives
use serde::{Serialize, Deserialize};  // ADD at top of file

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Visibility { ... }
```

```rust
// src/interface.rs
pub struct InterfaceFuncEntry {
    pub visibility: Visibility,  // NEW
    pub name: String,
    pub sig: InterfaceFuncSig,
}

pub struct InterfaceStructEntry {
    pub visibility: Visibility,  // NEW
    pub name: String,
    // ... rest unchanged
}

pub struct InterfaceProtocolEntry {
    pub visibility: Visibility,  // NEW
    pub name: String,
    // ... rest unchanged
}
```

- [ ] **Step 2: Update `from_semantic_info` to populate visibility**

In `src/interface.rs`, the three `.map()` closures that build entries need to look up the item's visibility from `sem.visibilities` and include it:

```rust
// Functions (around line 159)
.map(|(name, sig)| InterfaceFuncEntry {
    visibility: sem.visibilities.get(name).copied().unwrap_or_default(),
    name: name.clone(),
    sig: InterfaceFuncSig { ... },
})

// Structs (around line 186) — same pattern
// Protocols (around line 239) — same pattern
```

- [ ] **Step 3: Bump `FORMAT_VERSION` to 2**

The new `visibility` field changes the binary serialization layout. Bump `FORMAT_VERSION` in `src/interface.rs` from `1` to `2` so old `.bengalmod` files produce a clear version-mismatch error. Update the version check in `read_interface` tests accordingly.

- [ ] **Step 4: Update all tests constructing entry types**

In `tests/interface.rs`, every test that constructs `InterfaceFuncEntry`, `InterfaceStructEntry`, or `InterfaceProtocolEntry` must add `visibility: Visibility::Public` (or the appropriate value). Also update `src/interface.rs` inline tests if any. Update version assertions in error-handling tests.

- [ ] **Step 5: Run tests and clippy**

Run: `cargo test --test interface && cargo clippy`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/parser/ast.rs src/interface.rs tests/interface.rs
git commit -m "feat(interface): add visibility field to interface entry types"
```

---

### Task 2: Make AST body fields optional

**Files:**
- Modify: `src/parser/ast.rs:91-109` (StructMember variants)
- Modify: `src/parser/ast.rs:113-121` (Function)
- Modify: `src/parser/parse_definition.rs:245,253` (parse_function)
- Modify: `src/parser/parse_definition.rs:306,318,334,338,351,357` (parse_struct_member)
- Modify: `src/parser/mod.rs:207` (bare expression wrapping)

Change `body: Block` to `body: Option<Block>` and `getter: Block` to `getter: Option<Block>` in the AST. Normal-mode parsing wraps in `Some(...)`.

- [ ] **Step 1: Change AST type definitions**

In `src/parser/ast.rs`:

```rust
// StructMember::ComputedProperty
ComputedProperty {
    visibility: Visibility,
    name: String,
    ty: TypeAnnotation,
    getter: Option<Block>,     // was: Block
    setter: Option<Block>,
},
// StructMember::Initializer
Initializer {
    visibility: Visibility,
    params: Vec<Param>,
    body: Option<Block>,       // was: Block
},
// StructMember::Method
Method {
    visibility: Visibility,
    name: String,
    params: Vec<Param>,
    return_type: TypeAnnotation,
    body: Option<Block>,       // was: Block
},

// Function
pub struct Function {
    pub visibility: Visibility,
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: TypeAnnotation,
    pub body: Option<Block>,   // was: Block
    pub span: Span,
}
```

- [ ] **Step 2: Update normal-mode parser to wrap in Some**

In `src/parser/parse_definition.rs`:

`parse_function()` (line 245):
```rust
let body = Some(self.parse_block()?);  // was: self.parse_block()?
```

`parse_struct_member()` — computed property (line 306):
```rust
let getter = Some(self.parse_getter()?);  // was: self.parse_getter()?
```

`parse_struct_member()` — initializer (line 334):
```rust
let body = Some(self.parse_block()?);
```

`parse_struct_member()` — method (line 351):
```rust
let body = Some(self.parse_block()?);
```

In `src/parser/mod.rs` (line 207), bare expression mode:
```rust
body: Some(Block {
    stmts: vec![Stmt::Return(Some(expr))],
}),
```

- [ ] **Step 3: Fix compilation errors in semantic analysis**

`src/semantic/function_analysis.rs` (line 50, 53):
```rust
let stmts = &func.body.as_ref().unwrap().stmts;
if !block_always_returns(func.body.as_ref().unwrap()) {
```

`src/semantic/generic_validation.rs` (line 48):
```rust
validate_generics_block(func.body.as_ref().unwrap(), &func_map, &struct_map)?;
```

Lines 54-66 — update match arms:
```rust
StructMember::Initializer { body, .. } => {
    validate_generics_block(body.as_ref().unwrap(), &func_map, &struct_map)?;
}
StructMember::Method { body, .. } => {
    validate_generics_block(body.as_ref().unwrap(), &func_map, &struct_map)?;
}
StructMember::ComputedProperty { getter, setter, .. } => {
    validate_generics_block(getter.as_ref().unwrap(), &func_map, &struct_map)?;
    if let Some(setter_block) = setter {
        validate_generics_block(setter_block, &func_map, &struct_map)?;
    }
}
```

`src/semantic/struct_analysis.rs`:
- Line 26: destructured `body` — add `.as_ref().unwrap()` where `body.stmts` is accessed (line 53):
  ```rust
  for stmt in &body.as_ref().unwrap().stmts {
  ```
- Line 58: `check_all_fields_initialized` — pass `body.as_ref().unwrap()`:
  ```rust
  if let Err(e) = check_all_fields_initialized(&struct_def.name, body.as_ref().unwrap(), resolver) {
  ```
- Line 66: destructured `getter` — update `analyze_getter_block` call (line 87):
  ```rust
  analyze_getter_block(getter.as_ref().unwrap(), resolver, ctx, diag);
  ```
- Line 127: destructured `body` — update accesses (lines 162, 172):
  ```rust
  if !block_always_returns(body.as_ref().unwrap()) {
  let stmts = &body.as_ref().unwrap().stmts;
  ```

- [ ] **Step 4: Fix compilation errors in BIR lowering**

`src/bir/lowering/mod.rs` (line 283):
```rust
let (result, body_regions) = self.lower_block_stmts(func.body.as_ref().unwrap());
```

`src/bir/lowering/lower_program.rs` (line 236):
```rust
body: body.clone(),  // body is already Option<Block> from destructuring, but Function.body is now Option<Block>
```
Since `body` is destructured from `StructMember::Method { body, .. }` where `body` is now `Option<Block>`, the `Function` constructor at line 230-238 already receives the right type. No change needed here — `body` is `Option<Block>` and `Function.body` is `Option<Block>`.

Same for `lower_methods()` (line 365) — already matches.

- [ ] **Step 5: Fix compilation errors in parser tests**

Parser tests in `src/parser/tests/` will have `body: Block { ... }` patterns that need to become `body: Some(Block { ... })`. These are mechanical changes. Search for `body: Block {` and `getter: Block {` in test files and wrap with `Some(...)`.

- [ ] **Step 6: Run full test suite**

Run: `cargo test && cargo clippy`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/parser/ast.rs src/parser/mod.rs src/parser/parse_definition.rs \
  src/semantic/function_analysis.rs src/semantic/generic_validation.rs \
  src/semantic/struct_analysis.rs src/bir/lowering/mod.rs src/bir/lowering/lower_program.rs \
  src/parser/tests/
git commit -m "refactor(ast): make body/getter fields optional for interface mode support"
```

---

### Task 3: Add `parse_interface` entry point

**Files:**
- Modify: `src/parser/mod.rs:14-18` (Parser struct)
- Modify: `src/parser/mod.rs` (add `parse_interface` function)
- Modify: `src/parser/parse_definition.rs:225-256` (parse_function)
- Modify: `src/parser/parse_definition.rs:295-368` (parse_struct_member)
- Test: `src/parser/tests/test_definitions.rs`

- [ ] **Step 1: Write failing tests for interface mode parsing**

Add tests in `src/parser/tests/test_definitions.rs` (or a new test file `test_interface.rs` in the same module):

```rust
#[test]
fn parse_interface_function() {
    let tokens = tokenize("public func add(a: Int32, b: Int32) -> Int32;");
    let program = parse_interface(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
    let f = &program.functions[0];
    assert_eq!(f.name, "add");
    assert_eq!(f.visibility, Visibility::Public);
    assert!(f.body.is_none());
}

#[test]
fn parse_interface_generic_function() {
    let tokens = tokenize("public func identity<T>(x: T) -> T;");
    let program = parse_interface(tokens).unwrap();
    let f = &program.functions[0];
    assert_eq!(f.type_params.len(), 1);
    assert_eq!(f.type_params[0].name, "T");
    assert!(f.body.is_none());
}

#[test]
fn parse_interface_unit_return_function() {
    let tokens = tokenize("public func doSomething();");
    let program = parse_interface(tokens).unwrap();
    let f = &program.functions[0];
    assert_eq!(f.return_type, TypeAnnotation::Unit);
    assert!(f.body.is_none());
}

#[test]
fn parse_interface_struct() {
    let tokens = tokenize("public struct Point { var x: Int32; var y: Int32; init(x: Int32, y: Int32); func sum() -> Int32; }");
    let program = parse_interface(tokens).unwrap();
    assert_eq!(program.structs.len(), 1);
    let s = &program.structs[0];
    assert_eq!(s.name, "Point");
    assert_eq!(s.members.len(), 4);
    // init has no body
    if let StructMember::Initializer { body, .. } = &s.members[2] {
        assert!(body.is_none());
    } else { panic!("expected Initializer"); }
    // method has no body
    if let StructMember::Method { body, .. } = &s.members[3] {
        assert!(body.is_none());
    } else { panic!("expected Method"); }
}

#[test]
fn parse_interface_computed_property() {
    let tokens = tokenize("public struct S { var x: Int32 { get }; var y: Int32 { get set }; }");
    let program = parse_interface(tokens).unwrap();
    let s = &program.structs[0];
    if let StructMember::ComputedProperty { getter, setter, .. } = &s.members[0] {
        assert!(getter.is_none());
        assert!(setter.is_none());
    } else { panic!("expected ComputedProperty"); }
    if let StructMember::ComputedProperty { getter, setter, .. } = &s.members[1] {
        assert!(getter.is_none());
        assert!(setter.is_some()); // has_setter = true, but setter block is None...
        // Actually: setter should indicate has_setter via the presence marker
    } else { panic!("expected ComputedProperty"); }
}
```

Note on computed property: In interface mode, `{ get set }` needs to produce `ComputedProperty { getter: None, setter: None }` with `has_setter` derivable. But the current `StructMember::ComputedProperty` uses `setter: Option<Block>` where `Some` means has setter. In interface mode, we need a way to indicate "has setter but no block". Options:
- Use a sentinel: `setter: Some(Block { stmts: vec![] })` — ugly but works without AST change
- Add a `has_setter: bool` field to `StructMember::ComputedProperty`

The simpler approach: add `has_setter: bool` to `ComputedProperty`. In normal mode, derive from `setter.is_some()`. In interface mode, parse directly.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- parse_interface`
Expected: Compilation error — `parse_interface` function doesn't exist.

- [ ] **Step 3: Add `interface_mode` to Parser and `parse_interface` entry point**

In `src/parser/mod.rs`:

```rust
struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    next_id: u32,
    interface_mode: bool,
}

impl Parser {
    fn new(tokens: Vec<SpannedToken>) -> Self {
        Self {
            tokens,
            pos: 0,
            next_id: 0,
            interface_mode: false,
        }
    }
    // ...
}

pub fn parse_interface(tokens: Vec<SpannedToken>) -> Result<Program> {
    let mut parser = Parser::new(tokens);
    parser.interface_mode = true;
    let program = parser.parse_program()?;
    let next = parser.peek();
    if next.node != Token::Eof {
        return Err(BengalError::ParseError {
            message: format!("unexpected token `{}`", next.node),
            span: next.span,
        });
    }
    Ok(program)
}
```

- [ ] **Step 4: Modify `parse_function` for interface mode**

In `src/parser/parse_definition.rs`, `parse_function()`:

Replace body parsing (line 245):
```rust
let body = if self.interface_mode {
    // TODO: Future @inlinable support — parse function body here when present
    self.expect(Token::Semicolon)?;
    None
} else {
    Some(self.parse_block()?)
};
```

- [ ] **Step 5: Modify `parse_struct_member` for interface mode**

In `parse_struct_member()`:

**Initializer** (around line 334):
```rust
Token::Init => {
    self.advance();
    let params = self.parse_param_list()?;
    let body = if self.interface_mode {
        self.expect(Token::Semicolon)?;
        None
    } else {
        Some(self.parse_block()?)
    };
    Ok(StructMember::Initializer { visibility, params, body })
}
```

**Method** (around line 351):
```rust
let body = if self.interface_mode {
    self.expect(Token::Semicolon)?;
    None
} else {
    Some(self.parse_block()?)
};
```

**Computed property** (around line 303-320): In interface mode, use protocol-style parsing (`{ get }` / `{ get set }`):
```rust
if self.peek().node == Token::LBrace {
    if self.interface_mode {
        // Protocol-style: { get [set] }
        self.advance(); // consume `{`
        let tok = self.expect(Token::Ident(String::new()))?;
        match &tok.node {
            Token::Ident(s) if s == "get" => {}
            _ => return Err(BengalError::ParseError {
                message: format!("expected `get`, found `{}`", tok.node),
                span: tok.span,
            }),
        }
        let has_setter = matches!(&self.peek().node, Token::Ident(s) if s == "set");
        if has_setter {
            self.advance();
        }
        self.expect(Token::RBrace)?;
        self.expect(Token::Semicolon)?;
        Ok(StructMember::ComputedProperty {
            visibility,
            name,
            ty,
            getter: None,
            setter: if has_setter { Some(Block { stmts: vec![] }) } else { None },
        })
    } else {
        // Normal mode: get { block } [set { block }]
        self.advance(); // consume `{`
        let getter = Some(self.parse_getter()?);
        let setter = if self.peek().node == Token::RBrace {
            None
        } else {
            Some(self.parse_setter()?)
        };
        self.expect(Token::RBrace)?;
        self.expect(Token::Semicolon)?;
        Ok(StructMember::ComputedProperty {
            visibility, name, ty, getter, setter,
        })
    }
}
```

Note: For `has_setter` in interface mode, we use the sentinel `Some(Block { stmts: vec![] })` for setter. This is consistent with the existing convention where `setter: Some(...)` means has setter. The `InterfaceComputedProp.has_setter` is derived from `setter.is_some()` in the existing `from_semantic_info`, and `from_ast` will do the same.

- [ ] **Step 6: Run tests**

Run: `cargo test && cargo clippy`
Expected: All pass including new interface parsing tests.

- [ ] **Step 7: Commit**

```bash
git add src/parser/mod.rs src/parser/parse_definition.rs src/parser/tests/
git commit -m "feat(parser): add interface_mode and parse_interface entry point"
```

---

### Task 4: Implement emitter (ModuleInterface → text)

**Files:**
- Modify: `src/interface.rs` (add `emit_text_interface`, `write_text_interface`)
- Test: `tests/interface.rs` (add emitter tests)

- [ ] **Step 1: Write failing emitter tests**

In `tests/interface.rs`:

```rust
use bengal::interface::{emit_text_interface, /* ... existing imports */};

#[test]
fn emit_empty_interface() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert_eq!(text, "// bengal-interface-format-version: 1\n");
}

#[test]
fn emit_simple_function() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![
                    ("a".to_string(), InterfaceType::I32),
                    ("b".to_string(), InterfaceType::I32),
                ],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public func add(a: Int32, b: Int32) -> Int32;"));
}

#[test]
fn emit_generic_function() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "identity".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![InterfaceTypeParam { name: "T".to_string(), bound: Some("Summable".to_string()) }],
                params: vec![("x".to_string(), InterfaceType::TypeParam { name: "T".to_string(), bound: Some("Summable".to_string()) })],
                return_type: InterfaceType::TypeParam { name: "T".to_string(), bound: Some("Summable".to_string()) },
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public func identity<T: Summable>(x: T) -> T;"));
}

#[test]
fn emit_unit_return_omitted() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "doSomething".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![],
                return_type: InterfaceType::Unit,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public func doSomething();"));
    assert!(!text.contains("Void"));
}

#[test]
fn emit_struct_with_members() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec![],
            fields: vec![("x".to_string(), InterfaceType::I32), ("y".to_string(), InterfaceType::I32)],
            methods: vec![InterfaceMethodSig {
                name: "sum".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
            computed: vec![InterfaceComputedProp {
                name: "total".to_string(),
                ty: InterfaceType::I32,
                has_setter: false,
            }],
            init_params: vec![("x".to_string(), InterfaceType::I32), ("y".to_string(), InterfaceType::I32)],
        }],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public struct Point {"));
    assert!(text.contains("  var x: Int32;"));
    assert!(text.contains("  var total: Int32 { get };"));
    assert!(text.contains("  init(x: Int32, y: Int32);"));
    assert!(text.contains("  func sum() -> Int32;"));
    assert!(text.contains("}"));
}

#[test]
fn emit_protocol() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Summable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "sum".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
            properties: vec![InterfacePropertyReq {
                name: "value".to_string(),
                ty: InterfaceType::I32,
                has_setter: true,
            }],
        }],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public protocol Summable {"));
    assert!(text.contains("  func sum() -> Int32;"));
    assert!(text.contains("  var value: Int32 { get set };"));
}

#[test]
fn emit_array_types() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "first".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("arr".to_string(), InterfaceType::Array {
                    element: Box::new(InterfaceType::I32),
                    size: 4,
                })],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("public func first(arr: [Int32; 4]) -> Int32;"));
}

#[test]
fn emit_package_visibility() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Package,
            name: "internal_helper".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("package func internal_helper() -> Int32;"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test interface -- emit_`
Expected: Compilation error — `emit_text_interface` doesn't exist.

- [ ] **Step 3: Implement `emit_text_interface`**

In `src/interface.rs`, add:

```rust
pub const TEXT_FORMAT_VERSION: u32 = 1;

fn emit_type(ty: &InterfaceType) -> String {
    match ty {
        InterfaceType::I32 => "Int32".to_string(),
        InterfaceType::I64 => "Int64".to_string(),
        InterfaceType::F32 => "Float32".to_string(),
        InterfaceType::F64 => "Float64".to_string(),
        InterfaceType::Bool => "Bool".to_string(),
        InterfaceType::Unit => "Void".to_string(),
        InterfaceType::Struct(name) => name.clone(),
        InterfaceType::TypeParam { name, .. } => name.clone(),
        InterfaceType::Generic { name, args } => {
            let args_str: Vec<String> = args.iter().map(emit_type).collect();
            format!("{}<{}>", name, args_str.join(", "))
        }
        InterfaceType::Array { element, size } => {
            format!("[{}; {}]", emit_type(element), size)
        }
    }
}

fn emit_visibility(vis: Visibility) -> &'static str {
    match vis {
        Visibility::Public => "public",
        Visibility::Package => "package",
        _ => "public", // should not appear in interface
    }
}

fn emit_type_params(tps: &[InterfaceTypeParam]) -> String {
    if tps.is_empty() {
        return String::new();
    }
    let params: Vec<String> = tps.iter().map(|tp| {
        if let Some(bound) = &tp.bound {
            format!("{}: {}", tp.name, bound)
        } else {
            tp.name.clone()
        }
    }).collect();
    format!("<{}>", params.join(", "))
}

fn emit_params(params: &[(String, InterfaceType)]) -> String {
    let parts: Vec<String> = params.iter().map(|(n, t)| format!("{}: {}", n, emit_type(t))).collect();
    format!("({})", parts.join(", "))
}

fn emit_return_type(ty: &InterfaceType) -> String {
    if matches!(ty, InterfaceType::Unit) {
        String::new()
    } else {
        format!(" -> {}", emit_type(ty))
    }
}

pub fn emit_text_interface(iface: &ModuleInterface) -> String {
    let mut out = String::new();
    out.push_str(&format!("// bengal-interface-format-version: {}\n", TEXT_FORMAT_VERSION));

    // Functions
    if !iface.functions.is_empty() {
        out.push('\n');
        for func in &iface.functions {
            out.push_str(&format!(
                "{} func {}{}{}{};\n",
                emit_visibility(func.visibility),
                func.name,
                emit_type_params(&func.sig.type_params),
                emit_params(&func.sig.params),
                emit_return_type(&func.sig.return_type),
            ));
        }
    }

    // Structs
    for s in &iface.structs {
        out.push('\n');
        let conformances = if s.conformances.is_empty() {
            String::new()
        } else {
            format!(": {}", s.conformances.join(", "))
        };
        out.push_str(&format!(
            "{} struct {}{}{} {{\n",
            emit_visibility(s.visibility),
            s.name,
            emit_type_params(&s.type_params),
            conformances,
        ));

        // Fields
        for (name, ty) in &s.fields {
            out.push_str(&format!("  var {}: {};\n", name, emit_type(ty)));
        }
        // Computed properties
        for cp in &s.computed {
            let accessor = if cp.has_setter { "get set" } else { "get" };
            out.push_str(&format!("  var {}: {} {{ {} }};\n", cp.name, emit_type(&cp.ty), accessor));
        }
        // Init
        out.push_str(&format!("  init{};\n", emit_params(&s.init_params)));
        // Methods
        for m in &s.methods {
            out.push_str(&format!(
                "  func {}{}{};\n",
                m.name,
                emit_params(&m.params),
                emit_return_type(&m.return_type),
            ));
        }
        out.push_str("}\n");
    }

    // Protocols
    for p in &iface.protocols {
        out.push('\n');
        out.push_str(&format!("{} protocol {} {{\n", emit_visibility(p.visibility), p.name));
        for m in &p.methods {
            out.push_str(&format!(
                "  func {}{}{};\n",
                m.name,
                emit_params(&m.params),
                emit_return_type(&m.return_type),
            ));
        }
        for prop in &p.properties {
            let accessor = if prop.has_setter { "get set" } else { "get" };
            out.push_str(&format!("  var {}: {} {{ {} }};\n", prop.name, emit_type(&prop.ty), accessor));
        }
        out.push_str("}\n");
    }

    out
}

pub fn write_text_interface(iface: &ModuleInterface, path: &Path) -> Result<()> {
    let text = emit_text_interface(iface);
    std::fs::write(path, text).map_err(|e| BengalError::InterfaceError { message: e.to_string() })?;
    Ok(())
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test interface -- emit_ && cargo clippy`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/interface.rs tests/interface.rs
git commit -m "feat(interface): implement text interface emitter"
```

---

### Task 5: Implement reader (text → ModuleInterface)

**Files:**
- Modify: `src/interface.rs` (add `InterfaceType::from_annotation`, `ModuleInterface::from_ast`, `read_text_interface`, `read_text_interface_file`)
- Test: `tests/interface.rs` (round-trip tests, error tests)

- [ ] **Step 1: Write failing round-trip tests**

In `tests/interface.rs`:

```rust
use bengal::interface::{read_text_interface, /* ... */};

#[test]
fn text_round_trip_simple_function() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![
                    ("a".to_string(), InterfaceType::I32),
                    ("b".to_string(), InterfaceType::I32),
                ],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_generic_function() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "identity".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![InterfaceTypeParam { name: "T".to_string(), bound: None }],
                params: vec![("x".to_string(), InterfaceType::TypeParam { name: "T".to_string(), bound: None })],
                return_type: InterfaceType::TypeParam { name: "T".to_string(), bound: None },
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_struct_full() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec!["Summable".to_string()],
            fields: vec![("x".to_string(), InterfaceType::I32), ("y".to_string(), InterfaceType::I32)],
            methods: vec![InterfaceMethodSig {
                name: "sum".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
            computed: vec![InterfaceComputedProp {
                name: "total".to_string(),
                ty: InterfaceType::I32,
                has_setter: false,
            }],
            init_params: vec![("x".to_string(), InterfaceType::I32), ("y".to_string(), InterfaceType::I32)],
        }],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_protocol() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Summable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "sum".to_string(),
                params: vec![],
                return_type: InterfaceType::I32,
            }],
            properties: vec![InterfacePropertyReq {
                name: "value".to_string(),
                ty: InterfaceType::I32,
                has_setter: true,
            }],
        }],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_array_types() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "first".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("arr".to_string(), InterfaceType::Array {
                    element: Box::new(InterfaceType::I32),
                    size: 4,
                })],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_mixed() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("a".to_string(), InterfaceType::I32), ("b".to_string(), InterfaceType::I32)],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Point".to_string(),
            type_params: vec![],
            conformances: vec![],
            fields: vec![("x".to_string(), InterfaceType::I32)],
            methods: vec![],
            computed: vec![],
            init_params: vec![("x".to_string(), InterfaceType::I32)],
        }],
        protocols: vec![InterfaceProtocolEntry {
            visibility: Visibility::Public,
            name: "Runnable".to_string(),
            methods: vec![InterfaceMethodSig {
                name: "run".to_string(),
                params: vec![],
                return_type: InterfaceType::Unit,
            }],
            properties: vec![],
        }],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}

#[test]
fn text_round_trip_generic_struct_with_conformance() {
    let iface = ModuleInterface {
        functions: vec![],
        structs: vec![InterfaceStructEntry {
            visibility: Visibility::Public,
            name: "Pair".to_string(),
            type_params: vec![
                InterfaceTypeParam { name: "T".to_string(), bound: None },
                InterfaceTypeParam { name: "U".to_string(), bound: None },
            ],
            conformances: vec!["Printable".to_string()],
            fields: vec![
                ("first".to_string(), InterfaceType::TypeParam { name: "T".to_string(), bound: None }),
                ("second".to_string(), InterfaceType::TypeParam { name: "U".to_string(), bound: None }),
            ],
            methods: vec![InterfaceMethodSig {
                name: "swap".to_string(),
                params: vec![],
                return_type: InterfaceType::Generic {
                    name: "Pair".to_string(),
                    args: vec![
                        InterfaceType::TypeParam { name: "U".to_string(), bound: None },
                        InterfaceType::TypeParam { name: "T".to_string(), bound: None },
                    ],
                },
            }],
            computed: vec![],
            init_params: vec![
                ("first".to_string(), InterfaceType::TypeParam { name: "T".to_string(), bound: None }),
                ("second".to_string(), InterfaceType::TypeParam { name: "U".to_string(), bound: None }),
            ],
        }],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}
```

Also add a `package` visibility round-trip test:

```rust
#[test]
fn text_round_trip_package_visibility() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Package,
            name: "helper".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("x".to_string(), InterfaceType::I32)],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("package func helper"));
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}
```

And a generic struct with type-param bounds:

```rust
#[test]
fn text_round_trip_generic_with_bound() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "sum".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![InterfaceTypeParam { name: "T".to_string(), bound: Some("Summable".to_string()) }],
                params: vec![("x".to_string(), InterfaceType::TypeParam { name: "T".to_string(), bound: Some("Summable".to_string()) })],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let text = emit_text_interface(&iface);
    assert!(text.contains("<T: Summable>"));
    let restored = read_text_interface(&text).unwrap();
    assert_eq!(iface, restored);
}
```

- [ ] **Step 2: Write failing error tests**

```rust
#[test]
fn read_text_missing_header() {
    let text = "public func add(a: Int32) -> Int32;";
    let result = read_text_interface(text);
    assert!(result.is_err());
}

#[test]
fn read_text_wrong_version() {
    let text = "// bengal-interface-format-version: 999\npublic func add(a: Int32) -> Int32;";
    let result = read_text_interface(text);
    assert!(result.is_err());
}

#[test]
fn read_text_invalid_syntax() {
    let text = "// bengal-interface-format-version: 1\npublic func ???;";
    let result = read_text_interface(text);
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test interface -- text_round_trip read_text_`
Expected: Compilation error — `read_text_interface` doesn't exist.

- [ ] **Step 4: Implement `InterfaceType::from_annotation`**

In `src/interface.rs`:

```rust
use crate::parser::ast::TypeAnnotation;

impl InterfaceType {
    pub fn from_annotation(ann: &TypeAnnotation, type_params: &[TypeParam]) -> Self {
        match ann {
            TypeAnnotation::I32 => InterfaceType::I32,
            TypeAnnotation::I64 => InterfaceType::I64,
            TypeAnnotation::F32 => InterfaceType::F32,
            TypeAnnotation::F64 => InterfaceType::F64,
            TypeAnnotation::Bool => InterfaceType::Bool,
            TypeAnnotation::Unit => InterfaceType::Unit,
            TypeAnnotation::Named(name) => {
                // Check if this is a type parameter
                if let Some(tp) = type_params.iter().find(|tp| tp.name == *name) {
                    InterfaceType::TypeParam {
                        name: tp.name.clone(),
                        bound: tp.bound.clone(),
                    }
                } else {
                    InterfaceType::Struct(name.clone())
                }
            }
            TypeAnnotation::Generic { name, args } => InterfaceType::Generic {
                name: name.clone(),
                args: args.iter().map(|a| InterfaceType::from_annotation(a, type_params)).collect(),
            },
            TypeAnnotation::Array { element, size } => InterfaceType::Array {
                element: Box::new(InterfaceType::from_annotation(element, type_params)),
                size: *size,
            },
        }
    }
}
```

- [ ] **Step 5: Implement `ModuleInterface::from_ast`**

In `src/interface.rs`:

```rust
use crate::parser::ast::Program;

impl ModuleInterface {
    pub fn from_ast(program: &Program) -> Self {
        let mut functions: Vec<InterfaceFuncEntry> = program.functions.iter().map(|f| {
            let type_params: Vec<InterfaceTypeParam> = f.type_params.iter()
                .map(InterfaceTypeParam::from_type_param).collect();
            InterfaceFuncEntry {
                visibility: f.visibility,
                name: f.name.clone(),
                sig: InterfaceFuncSig {
                    type_params: type_params.clone(),
                    params: f.params.iter()
                        .map(|p| (p.name.clone(), InterfaceType::from_annotation(&p.ty, &f.type_params)))
                        .collect(),
                    return_type: InterfaceType::from_annotation(&f.return_type, &f.type_params),
                },
            }
        }).collect();

        let mut structs: Vec<InterfaceStructEntry> = program.structs.iter().map(|s| {
            let struct_tps = &s.type_params;  // thread struct's type params
            let mut fields = Vec::new();
            let mut methods = Vec::new();
            let mut computed = Vec::new();
            let mut init_params = Vec::new();

            for member in &s.members {
                match member {
                    StructMember::StoredProperty { name, ty, .. } => {
                        fields.push((name.clone(), InterfaceType::from_annotation(ty, struct_tps)));
                    }
                    StructMember::ComputedProperty { name, ty, setter, .. } => {
                        computed.push(InterfaceComputedProp {
                            name: name.clone(),
                            ty: InterfaceType::from_annotation(ty, struct_tps),
                            has_setter: setter.is_some(),
                        });
                    }
                    StructMember::Initializer { params, .. } => {
                        init_params = params.iter()
                            .map(|p| (p.name.clone(), InterfaceType::from_annotation(&p.ty, struct_tps)))
                            .collect();
                    }
                    StructMember::Method { name, params, return_type, .. } => {
                        methods.push(InterfaceMethodSig {
                            name: name.clone(),
                            params: params.iter()
                                .map(|p| (p.name.clone(), InterfaceType::from_annotation(&p.ty, struct_tps)))
                                .collect(),
                            return_type: InterfaceType::from_annotation(return_type, struct_tps),
                        });
                    }
                }
            }

            InterfaceStructEntry {
                visibility: s.visibility,
                name: s.name.clone(),
                type_params: struct_tps.iter().map(InterfaceTypeParam::from_type_param).collect(),
                conformances: s.conformances.clone(),
                fields,
                methods,
                computed,
                init_params,
            }
        }).collect();

        let mut protocols: Vec<InterfaceProtocolEntry> = program.protocols.iter().map(|p| {
            InterfaceProtocolEntry {
                visibility: p.visibility,
                name: p.name.clone(),
                methods: p.members.iter().filter_map(|m| match m {
                    ProtocolMember::MethodSig { name, params, return_type } => {
                        Some(InterfaceMethodSig {
                            name: name.clone(),
                            params: params.iter()
                                .map(|p| (p.name.clone(), InterfaceType::from_annotation(&p.ty, &[])))
                                .collect(),
                            return_type: InterfaceType::from_annotation(return_type, &[]),
                        })
                    }
                    _ => None,
                }).collect(),
                properties: p.members.iter().filter_map(|m| match m {
                    ProtocolMember::PropertyReq { name, ty, has_setter } => {
                        Some(InterfacePropertyReq {
                            name: name.clone(),
                            ty: InterfaceType::from_annotation(ty, &[]),
                            has_setter: *has_setter,
                        })
                    }
                    _ => None,
                }).collect(),
            }
        }).collect();

        // Sort for deterministic output (input may be in declaration order)
        functions.sort_by(|a, b| a.name.cmp(&b.name));
        structs.sort_by(|a, b| a.name.cmp(&b.name));
        protocols.sort_by(|a, b| a.name.cmp(&b.name));

        ModuleInterface { functions, structs, protocols }
    }
}
```

- [ ] **Step 6: Implement `read_text_interface` and `read_text_interface_file`**

In `src/interface.rs`:

```rust
use crate::lexer;
use crate::parser;

pub fn read_text_interface(text: &str) -> Result<ModuleInterface> {
    // Step 1: Extract and validate header
    let first_line = text.lines().next().ok_or_else(|| {
        BengalError::InterfaceError { message: "empty interface file".to_string() }
    })?;

    let expected_prefix = "// bengal-interface-format-version: ";
    if !first_line.starts_with(expected_prefix) {
        return Err(BengalError::InterfaceError {
            message: "missing interface format header".to_string(),
        });
    }
    let version_str = &first_line[expected_prefix.len()..];
    let version: u32 = version_str.trim().parse().map_err(|_| {
        BengalError::InterfaceError {
            message: format!("invalid interface version: {}", version_str),
        }
    })?;
    if version != TEXT_FORMAT_VERSION {
        return Err(BengalError::InterfaceError {
            message: format!(
                "unsupported interface format version {} (expected {})",
                version, TEXT_FORMAT_VERSION
            ),
        });
    }

    // Step 2: Strip header, tokenize remaining text
    let body = text[first_line.len()..].trim_start_matches('\n');
    if body.trim().is_empty() {
        return Ok(ModuleInterface {
            functions: vec![],
            structs: vec![],
            protocols: vec![],
        });
    }
    let tokens = lexer::tokenize(body)?;

    // Step 3: Parse in interface mode
    let program = parser::parse_interface(tokens)?;

    // Step 4: Convert AST to ModuleInterface
    Ok(ModuleInterface::from_ast(&program))
}

pub fn read_text_interface_file(path: &Path) -> Result<ModuleInterface> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| BengalError::InterfaceError { message: e.to_string() })?;
    read_text_interface(&text)
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test --test interface && cargo clippy`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add src/interface.rs tests/interface.rs
git commit -m "feat(interface): implement text interface reader with round-trip support"
```

---

### Task 6: File I/O integration test

**Files:**
- Test: `tests/interface.rs`

- [ ] **Step 1: Write file I/O test**

```rust
#[test]
fn write_and_read_text_interface_file() {
    let iface = ModuleInterface {
        functions: vec![InterfaceFuncEntry {
            visibility: Visibility::Public,
            name: "add".to_string(),
            sig: InterfaceFuncSig {
                type_params: vec![],
                params: vec![("a".to_string(), InterfaceType::I32), ("b".to_string(), InterfaceType::I32)],
                return_type: InterfaceType::I32,
            },
        }],
        structs: vec![],
        protocols: vec![],
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.bengalinterface");
    write_text_interface(&iface, &path).unwrap();
    assert!(path.exists());
    let restored = read_text_interface_file(&path).unwrap();
    assert_eq!(iface, restored);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test interface -- write_and_read_text && cargo clippy`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/interface.rs
git commit -m "test(interface): add text interface file I/O integration test"
```
