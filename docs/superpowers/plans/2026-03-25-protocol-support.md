# Protocol Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add struct methods (Phase 6a) and protocol definitions with conformance checking (Phase 6b) to the Bengal language.

**Architecture:** Methods are flattened to regular BIR functions via name mangling (`StructName_methodName`). Protocols are purely semantic-level constructs — no BIR or codegen representation. Method calls (`obj.method(args)`) are parsed as `MethodCall` AST nodes and lowered to `Call` instructions with the mangled name and `self` prepended to args.

**Tech Stack:** Rust, logos (lexer), inkwell (LLVM bindings)

**Spec:** `docs/superpowers/specs/2026-03-25-protocol-support-design.md`

---

## File Map

| File | Changes |
|------|---------|
| `src/lexer/token.rs` | Add `Protocol` keyword token |
| `src/parser/ast.rs` | Add `MethodCall` to `ExprKind`, `Method` to `StructMember`, `ProtocolDef` to `Program`, conformance list to `StructDef` |
| `src/parser/mod.rs` | Parse method members, `MethodCall` in postfix, protocol definitions, conformance syntax |
| `src/semantic/resolver.rs` | Add `MethodInfo`, `ProtocolInfo`, protocol storage/lookup methods, methods to `StructInfo` |
| `src/semantic/mod.rs` | Register methods in Pass 1b, analyze method bodies in Pass 3, register protocols in Pass 1a, conformance check in Pass 3 |
| `src/bir/lowering.rs` | Flatten methods to BIR functions, lower `MethodCall` to `Call` with mangled name |
| `tests/compile_test.rs` | Integration tests for methods and protocols |
| `docs/grammar.md` | Update grammar documentation |

---

## Phase 6a: Struct Methods

### Task 1: Add `MethodCall` AST node and `Method` struct member

**Files:**
- Modify: `src/parser/ast.rs`

- [ ] **Step 1: Add `Method` variant to `StructMember`**

```rust
// In StructMember enum, add after Initializer:
Method {
    name: String,
    params: Vec<Param>,
    return_type: TypeAnnotation,
    body: Block,
},
```

- [ ] **Step 2: Add `MethodCall` variant to `ExprKind`**

```rust
// In ExprKind enum, add after SelfRef:
MethodCall {
    object: Box<Expr>,
    method: String,
    args: Vec<Expr>,
},
```

- [ ] **Step 3: Add conformance list to `StructDef`**

```rust
// Modify StructDef to:
pub struct StructDef {
    pub name: String,
    pub conformances: Vec<String>,
    pub members: Vec<StructMember>,
}
```

- [ ] **Step 4: Add `ProtocolDef` and update `Program`**

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDef {
    pub name: String,
    pub members: Vec<ProtocolMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolMember {
    MethodSig {
        name: String,
        params: Vec<Param>,
        return_type: TypeAnnotation,
    },
    PropertyReq {
        name: String,
        ty: TypeAnnotation,
        has_setter: bool,
    },
}

// Update Program to:
pub struct Program {
    pub structs: Vec<StructDef>,
    pub protocols: Vec<ProtocolDef>,
    pub functions: Vec<Function>,
}
```

- [ ] **Step 5: Fix all compilation errors from `StructDef` and `Program` changes**

The `conformances` field and `protocols` field will cause compilation errors throughout the codebase. Fix each callsite:
- `src/parser/mod.rs` `parse_struct_def`: add `conformances: vec![]` (will be updated in Task 3)
- `src/parser/mod.rs` `parse_program`: add `protocols: vec![]` and pass through (will be updated in Task 7)
- `src/parser/mod.rs` bare-expression fallback in `parse`: add `protocols: vec![]`
- `src/semantic/mod.rs` `analyze`: iterate `program.protocols` (empty for now)
- `src/bir/lowering.rs` `lower_program`: handle `program.protocols` (skip for now)
- Any test files that construct `Program` or `StructDef` directly

- [ ] **Step 6: Run `cargo build` to verify compilation**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 7: Run `cargo clippy` and `cargo fmt`**

Run: `cargo fmt && cargo clippy`
Expected: no warnings

- [ ] **Step 8: Run all tests to verify no regressions**

Run: `cargo test`
Expected: all existing tests pass

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "Add Method, MethodCall, ProtocolDef AST nodes and conformance list"
```

---

### Task 2: Parse struct methods

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write a parser test for method parsing**

Add to `src/parser/mod.rs` tests module:

```rust
#[test]
fn parse_struct_method() {
    let source = r#"
        struct Point {
            var x: Int32;
            func sum() -> Int32 {
                return self.x;
            }
        }
        func main() -> Int32 { return 0; }
    "#;
    let tokens = crate::lexer::tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.structs.len(), 1);
    let s = &program.structs[0];
    assert!(s.members.iter().any(|m| matches!(m, StructMember::Method { name, .. } if name == "sum")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parse_struct_method`
Expected: FAIL — `Func` token inside struct body triggers parse error

- [ ] **Step 3: Add method parsing to `parse_struct_member`**

In `src/parser/mod.rs` `parse_struct_member`, add a `Token::Func` arm before the `_` catch-all:

```rust
Token::Func => {
    self.advance(); // consume `func`
    let name = self.expect_ident()?;
    let params = self.parse_param_list()?;
    let return_type = if self.peek().node == Token::Arrow {
        self.advance();
        self.parse_type()?
    } else {
        TypeAnnotation::Unit
    };
    let body = self.parse_block()?;
    Ok(StructMember::Method {
        name,
        params,
        return_type,
        body,
    })
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test parse_struct_method`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass, no warnings

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "Parse struct method definitions"
```

---

### Task 3: Parse conformance declaration syntax

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write a parser test for conformance syntax**

```rust
#[test]
fn parse_struct_conformance() {
    let source = r#"
        struct Point: Foo, Bar {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#;
    let tokens = crate::lexer::tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.structs[0].conformances, vec!["Foo", "Bar"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parse_struct_conformance`
Expected: FAIL — `:` after struct name is unexpected

- [ ] **Step 3: Update `parse_struct_def` to parse conformance list**

In `parse_struct_def`, after parsing the struct name and before `expect(Token::LBrace)`:

```rust
let conformances = if self.peek().node == Token::Colon {
    self.advance(); // consume `:`
    let mut list = vec![self.expect_ident()?];
    while self.peek().node == Token::Comma {
        self.advance();
        list.push(self.expect_ident()?);
    }
    list
} else {
    vec![]
};
```

Pass `conformances` to the `StructDef` constructor.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test parse_struct_conformance`
Expected: PASS

- [ ] **Step 5: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "Parse struct conformance declaration syntax"
```

---

### Task 4: Add `MethodInfo` to semantic resolver and register methods

**Files:**
- Modify: `src/semantic/resolver.rs`
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Add `MethodInfo` struct and methods field to `StructInfo`**

In `src/semantic/resolver.rs`:

```rust
#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
}
```

Add to `StructInfo`:

```rust
pub struct StructInfo {
    pub fields: Vec<(String, Type)>,
    pub field_index: HashMap<String, usize>,
    pub computed: Vec<ComputedPropInfo>,
    pub computed_index: HashMap<String, usize>,
    pub init: InitializerInfo,
    pub methods: Vec<MethodInfo>,
    pub method_index: HashMap<String, usize>,
}
```

- [ ] **Step 2: Fix compilation errors from new `StructInfo` fields**

Add `methods: vec![], method_index: HashMap::new()` to:
- `reserve_struct` in `resolver.rs`
- `define_struct` calls in `semantic/mod.rs` `resolve_struct_members`

- [ ] **Step 3: Register methods in `resolve_struct_members`**

In `src/semantic/mod.rs` `resolve_struct_members`, add a `StructMember::Method` match arm:

```rust
StructMember::Method {
    name: mname,
    params,
    return_type,
    ..
} => {
    if field_index.contains_key(mname)
        || computed_index.contains_key(mname)
        || method_index.contains_key(mname)
    {
        return Err(sem_err(format!(
            "duplicate member `{}` in struct `{}`",
            mname, name
        )));
    }
    let resolved_params: Vec<(String, Type)> = params
        .iter()
        .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, resolver)?)))
        .collect::<Result<Vec<_>>>()?;
    let resolved_return = resolve_type_checked(return_type, resolver)?;
    let idx = methods.len();
    methods.push(resolver::MethodInfo {
        name: mname.clone(),
        params: resolved_params,
        return_type: resolved_return,
    });
    method_index.insert(mname.clone(), idx);
}
```

Add `let mut methods: Vec<resolver::MethodInfo> = Vec::new();` and `let mut method_index: HashMap<String, usize> = HashMap::new();` at the start of the function. Pass `methods, method_index` to the `StructInfo` construction.

- [ ] **Step 4: Run `cargo build` and `cargo test`**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Add MethodInfo to resolver and register methods in Pass 1b"
```

---

### Task 5: Semantic analysis of method bodies (Pass 3)

**Files:**
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Write a semantic error test for `self` in method**

Add a test to `src/semantic/mod.rs` tests (or use integration test). We'll verify correct analysis via an integration test in Task 9. For now, add the analysis code.

- [ ] **Step 2: Analyze method bodies in `analyze_struct_members`**

In `analyze_struct_members`, add a `StructMember::Method` arm:

```rust
StructMember::Method {
    name: mname,
    params,
    return_type,
    body,
} => {
    let resolved_return = resolve_type_checked(return_type, resolver)?;
    let prev_self = resolver.self_context.clone();
    resolver.self_context = Some(SelfContext {
        struct_name: struct_def.name.clone(),
        mutable: false, // self is immutable in methods
    });
    let prev_return = resolver.current_return_type.clone();
    resolver.current_return_type = Some(resolved_return);

    resolver.push_scope();
    for param in params {
        resolver.define_var(
            param.name.clone(),
            VarInfo {
                ty: resolve_type_checked(&param.ty, resolver)?,
                mutable: false,
            },
        );
    }

    let stmts = &body.stmts;
    if stmts.is_empty() || !matches!(stmts.last(), Some(Stmt::Return(_))) {
        return Err(sem_err(format!(
            "method `{}` must end with a `return` statement",
            mname
        )));
    }
    for stmt in stmts {
        if matches!(stmt, Stmt::Yield(_)) {
            return Err(sem_err(
                "`yield` cannot be used in method body (use `return` instead)",
            ));
        }
        analyze_stmt(stmt, resolver)?;
    }

    resolver.pop_scope();
    resolver.current_return_type = prev_return;
    resolver.self_context = prev_self;
}
```

- [ ] **Step 3: Update `self` error message to include methods**

In `analyze_expr` for `ExprKind::SelfRef`, update the error:

```rust
"`self` can only be used inside struct initializers, computed properties, or methods"
```

- [ ] **Step 4: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Analyze method bodies in Pass 3 with immutable self"
```

---

### Task 6: Parse and analyze `MethodCall` expressions

**Files:**
- Modify: `src/parser/mod.rs`
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Update `parse_postfix` to detect method calls**

In `parse_postfix`, when we see `Token::Dot` followed by ident followed by `(`, it's a method call. Update the `Token::Dot` arm:

```rust
Token::Dot => {
    self.advance();
    let field = self.expect_ident()?;
    // Check if this is a method call: field followed by `(`
    if self.peek().node == Token::LParen {
        self.advance(); // consume `(`
        let mut args = Vec::new();
        if self.peek().node != Token::RParen {
            args.push(self.parse_expr()?);
            while self.peek().node == Token::Comma {
                self.advance();
                args.push(self.parse_expr()?);
            }
        }
        self.expect(Token::RParen)?;
        expr = self.expr(ExprKind::MethodCall {
            object: Box::new(expr),
            method: field,
            args,
        });
    } else {
        expr = self.expr(ExprKind::FieldAccess {
            object: Box::new(expr),
            field,
        });
    }
}
```

- [ ] **Step 2: Add `MethodCall` analysis to `analyze_expr`**

In `analyze_expr`, add the `ExprKind::MethodCall` arm:

```rust
ExprKind::MethodCall {
    object,
    method,
    args,
} => {
    let obj_ty = analyze_expr(object, resolver)?;
    match &obj_ty {
        Type::Struct(struct_name) => {
            let struct_info = resolver
                .lookup_struct(struct_name)
                .ok_or_else(|| sem_err(format!("undefined struct `{}`", struct_name)))?
                .clone();
            let method_info = match struct_info.method_index.get(method.as_str()) {
                Some(&idx) => struct_info.methods[idx].clone(),
                None => {
                    return Err(sem_err(format!(
                        "type `{}` has no method `{}`",
                        struct_name, method
                    )));
                }
            };
            if args.len() != method_info.params.len() {
                return Err(sem_err(format!(
                    "method `{}` expects {} argument(s) but {} were given",
                    method,
                    method_info.params.len(),
                    args.len()
                )));
            }
            for (arg, (param_name, param_ty)) in args.iter().zip(method_info.params.iter()) {
                let arg_ty = analyze_expr(arg, resolver)?;
                if arg_ty != *param_ty {
                    return Err(sem_err(format!(
                        "expected `{}` but got `{}` in argument `{}` of method `{}`",
                        param_ty, arg_ty, param_name, method
                    )));
                }
            }
            Ok(method_info.return_type)
        }
        _ => Err(sem_err(format!(
            "method call on non-struct type `{}`",
            obj_ty
        ))),
    }
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "Parse and analyze MethodCall expressions"
```

---

### Task 7: Name collision check for mangled method names

**Files:**
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Add collision check in `analyze` function**

After Pass 1b (after `resolve_struct_members` loop), add:

```rust
// Check for name collisions between mangled method names and top-level functions
for struct_def in &program.structs {
    if let Some(struct_info) = resolver.lookup_struct(&struct_def.name) {
        let struct_info = struct_info.clone();
        for method in &struct_info.methods {
            let mangled = format!("{}_{}", struct_def.name, method.name);
            if resolver.lookup_func(&mangled).is_some() {
                return Err(sem_err(format!(
                    "function `{}` conflicts with method `{}.{}`",
                    mangled, struct_def.name, method.name
                )));
            }
        }
    }
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "Reject top-level functions that collide with mangled method names"
```

---

### Task 8: BIR lowering — flatten methods to functions and lower MethodCall

**Files:**
- Modify: `src/bir/lowering.rs`

- [ ] **Step 1: Flatten methods to BIR functions in `lower_program`**

In `lower_program`, after building `func_sigs` for top-level functions but before lowering them, register and lower method functions:

```rust
// Register mangled method signatures
for (struct_name, info) in &sem_info.struct_defs {
    for method in &info.methods {
        let mangled = format!("{}_{}", struct_name, method.name);
        let bir_ret = semantic_type_to_bir(&method.return_type);
        lowering.func_sigs.insert(mangled, bir_ret);
    }
}
```

Then, create `Function` AST nodes for each method and lower them. Actually, since `lower_function` takes `&Function`, we need to construct synthetic `Function` nodes. Add method lowering after the function lowering:

```rust
// Lower methods as flattened functions
for struct_def in &program.structs {
    if let Some(info) = sem_info.struct_defs.get(&struct_def.name) {
        for member in &struct_def.members {
            if let StructMember::Method {
                name: mname,
                params,
                return_type,
                body,
            } = member
            {
                let mangled_name = format!("{}_{}", struct_def.name, mname);
                // Build synthetic function with `self` as first param
                let mut all_params = vec![Param {
                    name: "self".to_string(),
                    ty: TypeAnnotation::Named(struct_def.name.clone()),
                }];
                all_params.extend(params.clone());
                let func = Function {
                    name: mangled_name,
                    params: all_params,
                    return_type: return_type.clone(),
                    body: body.clone(),
                };

                // Set up self context for lowering
                lowering.self_var_name = Some("self".to_string());
                let bir_func = lowering.lower_function(&func);
                lowering.self_var_name = None;
                functions.push(bir_func);
            }
        }
    }
}
```

Note: need to change `functions` from the iterator collect to a mutable Vec.

- [ ] **Step 2: Lower `MethodCall` in `lower_expr`**

In `lower_expr`, add the `ExprKind::MethodCall` arm:

```rust
ExprKind::MethodCall {
    object,
    method,
    args,
} => {
    let obj_val = self.lower_expr(object);
    let struct_name = match self.value_types.get(&obj_val) {
        Some(BirType::Struct(n)) => n.clone(),
        _ => return self.record_error("method call on non-struct value"),
    };
    let mangled = format!("{}_{}", struct_name, method);
    let ret_ty = self
        .func_sigs
        .get(&mangled)
        .cloned()
        .unwrap_or(BirType::Unit);
    let mut call_args = vec![obj_val];
    for arg in args {
        call_args.push(self.lower_expr(arg));
    }
    let result = self.fresh_value();
    self.emit(Instruction::Call {
        result,
        func_name: mangled,
        args: call_args,
        ty: ret_ty.clone(),
    });
    self.value_types.insert(result, ret_ty);
    result
}
```

- [ ] **Step 3: Run `cargo build`**

Run: `cargo build`
Expected: compiles

- [ ] **Step 4: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Flatten methods to BIR functions and lower MethodCall to Call"
```

---

### Task 9: Integration tests for struct methods

**Files:**
- Modify: `tests/compile_test.rs`

- [ ] **Step 1: Write basic method test**

```rust
#[test]
fn method_basic() {
    let result = compile_and_run(r#"
        struct Point {
            var x: Int32;
            var y: Int32;
            func sum() -> Int32 {
                return self.x + self.y;
            }
        }
        func main() -> Int32 {
            let p = Point(x: 3, y: 4);
            return p.sum();
        }
    "#);
    assert_eq!(result, 7);
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test method_basic`
Expected: PASS

- [ ] **Step 3: Write method with arguments test**

```rust
#[test]
fn method_with_args() {
    let result = compile_and_run(r#"
        struct Point {
            var x: Int32;
            var y: Int32;
            func add(other: Point) -> Point {
                return Point(x: self.x + other.x, y: self.y + other.y);
            }
        }
        func main() -> Int32 {
            let a = Point(x: 1, y: 2);
            let b = Point(x: 10, y: 20);
            let c = a.add(b);
            return c.x + c.y;
        }
    "#);
    assert_eq!(result, 33);
}
```

- [ ] **Step 4: Write method chaining test**

```rust
#[test]
fn method_chaining() {
    let result = compile_and_run(r#"
        struct Wrapper {
            var value: Int32;
            func doubled() -> Wrapper {
                return Wrapper(value: self.value * 2);
            }
            func get() -> Int32 {
                return self.value;
            }
        }
        func main() -> Int32 {
            let w = Wrapper(value: 5);
            return w.doubled().doubled().get();
        }
    "#);
    assert_eq!(result, 20);
}
```

- [ ] **Step 5: Write self method call test**

```rust
#[test]
fn method_calls_other_method() {
    let result = compile_and_run(r#"
        struct Calc {
            var a: Int32;
            var b: Int32;
            func sum() -> Int32 {
                return self.a + self.b;
            }
            func doubled_sum() -> Int32 {
                return self.sum() * 2;
            }
        }
        func main() -> Int32 {
            let c = Calc(a: 3, b: 4);
            return c.doubled_sum();
        }
    "#);
    assert_eq!(result, 14);
}
```

- [ ] **Step 6: Write method in if/while test**

```rust
#[test]
fn method_in_control_flow() {
    let result = compile_and_run(r#"
        struct Counter {
            var n: Int32;
            func value() -> Int32 {
                return self.n;
            }
        }
        func main() -> Int32 {
            let c = Counter(n: 10);
            let result = if c.value() > 5 {
                yield c.value() + 1;
            } else {
                yield 0;
            };
            return result;
        }
    "#);
    assert_eq!(result, 11);
}
```

- [ ] **Step 7: Write method with unit return test**

```rust
#[test]
fn method_unit_return() {
    let result = compile_and_run(r#"
        struct Point {
            var x: Int32;
            func noop() {
                return;
            }
            func get() -> Int32 {
                return self.x;
            }
        }
        func main() -> Int32 {
            let p = Point(x: 42);
            p.noop();
            return p.get();
        }
    "#);
    assert_eq!(result, 42);
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test`
Expected: all pass

- [ ] **Step 9: Commit**

```bash
git add -A && git commit -m "Add integration tests for struct methods"
```

---

## Phase 6b: Protocol Support

### Task 10: Add `Protocol` keyword token

**Files:**
- Modify: `src/lexer/token.rs`

- [ ] **Step 1: Add `Protocol` token**

In `token.rs`, add in the keywords section:

```rust
#[token("protocol")]
Protocol,
```

Add the `Display` impl:

```rust
Token::Protocol => write!(f, "protocol"),
```

- [ ] **Step 2: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "Add protocol keyword token"
```

---

### Task 11: Parse protocol definitions

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write parser test for protocol**

```rust
#[test]
fn parse_protocol_def() {
    let source = r#"
        protocol Summable {
            func sum() -> Int32;
            var total: Int32 { get };
        }
        func main() -> Int32 { return 0; }
    "#;
    let tokens = crate::lexer::tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.protocols.len(), 1);
    assert_eq!(program.protocols[0].name, "Summable");
    assert_eq!(program.protocols[0].members.len(), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parse_protocol_def`
Expected: FAIL

- [ ] **Step 3: Add protocol parsing to `parse_program`**

Update the main parsing loop in `parse_program`:

```rust
fn parse_program(&mut self) -> Result<Program> {
    let mut structs = Vec::new();
    let mut protocols = Vec::new();
    let mut functions = Vec::new();
    while self.peek().node != Token::Eof {
        match &self.peek().node {
            Token::Struct => structs.push(self.parse_struct_def()?),
            Token::Protocol => protocols.push(self.parse_protocol_def()?),
            _ => functions.push(self.parse_function()?),
        }
    }
    Ok(Program {
        structs,
        protocols,
        functions,
    })
}
```

Update the check in the `pub fn parse` function — `Token::Protocol` should also trigger program parsing:

```rust
if matches!(parser.peek().node, Token::Func | Token::Struct | Token::Protocol) {
```

- [ ] **Step 4: Implement `parse_protocol_def`**

```rust
fn parse_protocol_def(&mut self) -> Result<ProtocolDef> {
    self.expect(Token::Protocol)?;
    let name = self.expect_ident()?;
    self.expect(Token::LBrace)?;
    let mut members = Vec::new();
    while self.peek().node != Token::RBrace {
        members.push(self.parse_protocol_member()?);
    }
    self.expect(Token::RBrace)?;
    Ok(ProtocolDef { name, members })
}

fn parse_protocol_member(&mut self) -> Result<ProtocolMember> {
    match &self.peek().node {
        Token::Func => {
            self.advance();
            let name = self.expect_ident()?;
            let params = self.parse_param_list()?;
            let return_type = if self.peek().node == Token::Arrow {
                self.advance();
                self.parse_type()?
            } else {
                TypeAnnotation::Unit
            };
            self.expect(Token::Semicolon)?;
            Ok(ProtocolMember::MethodSig {
                name,
                params,
                return_type,
            })
        }
        Token::Var => {
            self.advance();
            let name = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            self.expect(Token::LBrace)?;
            // expect `get`
            let tok = self.expect(Token::Ident(String::new()))?;
            match &tok.node {
                Token::Ident(s) if s == "get" => {}
                _ => {
                    return Err(BengalError::ParseError {
                        message: format!("expected `get`, found `{}`", tok.node),
                        span: tok.span,
                    });
                }
            }
            let has_setter = {
                let peek = self.peek();
                matches!(&peek.node, Token::Ident(s) if s == "set")
            };
            if has_setter {
                self.advance(); // consume `set`
            }
            self.expect(Token::RBrace)?;
            self.expect(Token::Semicolon)?;
            Ok(ProtocolMember::PropertyReq {
                name,
                ty,
                has_setter,
            })
        }
        _ => {
            let tok = self.peek();
            Err(BengalError::ParseError {
                message: format!("expected protocol member, found `{}`", tok.node),
                span: tok.span,
            })
        }
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test parse_protocol_def`
Expected: PASS

- [ ] **Step 6: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "Parse protocol definitions"
```

---

### Task 12: Add `ProtocolInfo` to resolver and register protocols in semantic analysis

**Files:**
- Modify: `src/semantic/resolver.rs`
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Add protocol types to resolver**

In `src/semantic/resolver.rs`:

```rust
#[derive(Debug, Clone)]
pub struct ProtocolMethodSig {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
}

#[derive(Debug, Clone)]
pub struct ProtocolPropertyReq {
    pub name: String,
    pub ty: Type,
    pub has_setter: bool,
}

#[derive(Debug, Clone)]
pub struct ProtocolInfo {
    pub name: String,
    pub methods: Vec<ProtocolMethodSig>,
    pub properties: Vec<ProtocolPropertyReq>,
}
```

Add to `Resolver`:

```rust
protocol_defs: HashMap<String, ProtocolInfo>,
```

Add methods:

```rust
pub fn define_protocol(&mut self, name: String, info: ProtocolInfo) {
    self.protocol_defs.insert(name, info);
}

pub fn lookup_protocol(&self, name: &str) -> Option<&ProtocolInfo> {
    self.protocol_defs.get(name)
}
```

Initialize in `Default` impl with `protocol_defs: HashMap::new()`.

- [ ] **Step 2: Register protocols in Pass 1a**

In `src/semantic/mod.rs` `analyze`, add after struct registration but before function registration:

```rust
// Pass 1a: register protocol names
for proto in &program.protocols {
    if resolver.lookup_struct(&proto.name).is_some()
        || resolver.lookup_func(&proto.name).is_some()
        || resolver.lookup_protocol(&proto.name).is_some()
    {
        return Err(sem_err(format!(
            "duplicate definition `{}`",
            proto.name
        )));
    }
    resolver.define_protocol(proto.name.clone(), resolver::ProtocolInfo {
        name: proto.name.clone(),
        methods: vec![],
        properties: vec![],
    });
}
```

- [ ] **Step 3: Resolve protocol member types in Pass 1b**

After the struct `resolve_struct_members` loop, add:

```rust
// Pass 1b: resolve protocol member types
for proto in &program.protocols {
    let mut methods = Vec::new();
    let mut properties = Vec::new();
    for member in &proto.members {
        match member {
            ProtocolMember::MethodSig {
                name,
                params,
                return_type,
            } => {
                let resolved_params: Vec<(String, Type)> = params
                    .iter()
                    .map(|p| Ok((p.name.clone(), resolve_type_checked(&p.ty, &resolver)?)))
                    .collect::<Result<Vec<_>>>()?;
                let resolved_return = resolve_type_checked(return_type, &resolver)?;
                methods.push(resolver::ProtocolMethodSig {
                    name: name.clone(),
                    params: resolved_params,
                    return_type: resolved_return,
                });
            }
            ProtocolMember::PropertyReq {
                name,
                ty,
                has_setter,
            } => {
                let resolved_ty = resolve_type_checked(ty, &resolver)?;
                properties.push(resolver::ProtocolPropertyReq {
                    name: name.clone(),
                    ty: resolved_ty,
                    has_setter: *has_setter,
                });
            }
        }
    }
    resolver.define_protocol(
        proto.name.clone(),
        resolver::ProtocolInfo {
            name: proto.name.clone(),
            methods,
            properties,
        },
    );
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Add ProtocolInfo to resolver and register protocols"
```

---

### Task 13: Conformance checking in Pass 3

**Files:**
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Add conformance checking after struct member analysis**

In `analyze`, after the Pass 3 `analyze_struct_members` loop, add conformance checking:

```rust
// Pass 3b: check protocol conformance
for struct_def in &program.structs {
    for proto_name in &struct_def.conformances {
        let proto_info = resolver
            .lookup_protocol(proto_name)
            .ok_or_else(|| sem_err(format!("unknown protocol `{}`", proto_name)))?
            .clone();
        let struct_info = resolver
            .lookup_struct(&struct_def.name)
            .ok_or_else(|| {
                sem_err(format!("undefined struct `{}`", struct_def.name))
            })?
            .clone();

        // Check methods
        for req_method in &proto_info.methods {
            match struct_info.method_index.get(&req_method.name) {
                Some(&idx) => {
                    let impl_method = &struct_info.methods[idx];
                    // Check param count
                    if impl_method.params.len() != req_method.params.len() {
                        return Err(sem_err(format!(
                            "method `{}` expects {} parameter(s) but protocol `{}` requires {}",
                            req_method.name,
                            impl_method.params.len(),
                            proto_name,
                            req_method.params.len()
                        )));
                    }
                    // Check param types
                    for ((impl_name, impl_ty), (req_name, req_ty)) in
                        impl_method.params.iter().zip(req_method.params.iter())
                    {
                        if impl_ty != req_ty {
                            return Err(sem_err(format!(
                                "method `{}` has parameter `{}` of type `{}` but protocol `{}` requires `{}`",
                                req_method.name, impl_name, impl_ty, proto_name, req_ty
                            )));
                        }
                        if impl_name != req_name {
                            return Err(sem_err(format!(
                                "method `{}` has parameter `{}` but protocol `{}` requires `{}`",
                                req_method.name, impl_name, proto_name, req_name
                            )));
                        }
                    }
                    // Check return type
                    if impl_method.return_type != req_method.return_type {
                        return Err(sem_err(format!(
                            "method `{}` has return type `{}` but protocol `{}` requires `{}`",
                            req_method.name,
                            impl_method.return_type,
                            proto_name,
                            req_method.return_type
                        )));
                    }
                }
                None => {
                    return Err(sem_err(format!(
                        "type `{}` does not implement method `{}` required by protocol `{}`",
                        struct_def.name, req_method.name, proto_name
                    )));
                }
            }
        }

        // Check properties
        for req_prop in &proto_info.properties {
            // Check stored properties first
            if let Some(&idx) = struct_info.field_index.get(&req_prop.name) {
                let (_, field_ty) = &struct_info.fields[idx];
                if *field_ty != req_prop.ty {
                    return Err(sem_err(format!(
                        "property `{}` has type `{}` but protocol `{}` requires `{}`",
                        req_prop.name, field_ty, proto_name, req_prop.ty
                    )));
                }
                // stored var always satisfies { get } and { get set }
                continue;
            }
            // Check computed properties
            if let Some(&idx) = struct_info.computed_index.get(&req_prop.name) {
                let computed = &struct_info.computed[idx];
                if computed.ty != req_prop.ty {
                    return Err(sem_err(format!(
                        "property `{}` has type `{}` but protocol `{}` requires `{}`",
                        req_prop.name, computed.ty, proto_name, req_prop.ty
                    )));
                }
                if req_prop.has_setter && !computed.has_setter {
                    return Err(sem_err(format!(
                        "property `{}` requires a setter to conform to protocol `{}`",
                        req_prop.name, proto_name
                    )));
                }
                continue;
            }
            return Err(sem_err(format!(
                "type `{}` does not implement property `{}` required by protocol `{}`",
                struct_def.name, req_prop.name, proto_name
            )));
        }
    }
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo fmt && cargo clippy && cargo test`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "Add protocol conformance checking in Pass 3"
```

---

### Task 14: Integration tests for protocols

**Files:**
- Modify: `tests/compile_test.rs`

- [ ] **Step 1: Write basic protocol conformance test**

```rust
#[test]
fn protocol_basic_conformance() {
    let result = compile_and_run(r#"
        protocol Summable {
            func sum() -> Int32;
        }
        struct Point: Summable {
            var x: Int32;
            var y: Int32;
            func sum() -> Int32 {
                return self.x + self.y;
            }
        }
        func main() -> Int32 {
            let p = Point(x: 3, y: 4);
            return p.sum();
        }
    "#);
    assert_eq!(result, 7);
}
```

- [ ] **Step 2: Write multiple methods protocol test**

```rust
#[test]
fn protocol_multiple_methods() {
    let result = compile_and_run(r#"
        protocol Describable {
            func first() -> Int32;
            func second() -> Int32;
        }
        struct Pair: Describable {
            var a: Int32;
            var b: Int32;
            func first() -> Int32 {
                return self.a;
            }
            func second() -> Int32 {
                return self.b;
            }
        }
        func main() -> Int32 {
            let p = Pair(a: 10, b: 20);
            return p.first() + p.second();
        }
    "#);
    assert_eq!(result, 30);
}
```

- [ ] **Step 3: Write property requirement (get) test**

```rust
#[test]
fn protocol_property_get() {
    let result = compile_and_run(r#"
        protocol HasTotal {
            var total: Int32 { get };
        }
        struct Numbers: HasTotal {
            var a: Int32;
            var b: Int32;
            var total: Int32 {
                get { yield self.a + self.b; }
            };
        }
        func main() -> Int32 {
            let n = Numbers(a: 5, b: 7);
            return n.total;
        }
    "#);
    assert_eq!(result, 12);
}
```

- [ ] **Step 4: Write stored property satisfies `{ get }` test**

```rust
#[test]
fn protocol_stored_property_satisfies_get() {
    let result = compile_and_run(r#"
        protocol HasValue {
            var value: Int32 { get };
        }
        struct Box: HasValue {
            var value: Int32;
        }
        func main() -> Int32 {
            let b = Box(value: 42);
            return b.value;
        }
    "#);
    assert_eq!(result, 42);
}
```

- [ ] **Step 5: Write multiple protocol conformance test**

```rust
#[test]
fn protocol_multiple_conformance() {
    let result = compile_and_run(r#"
        protocol Addable {
            func sum() -> Int32;
        }
        protocol Scalable {
            func scale(factor: Int32) -> Int32;
        }
        struct Value: Addable, Scalable {
            var n: Int32;
            func sum() -> Int32 {
                return self.n;
            }
            func scale(factor: Int32) -> Int32 {
                return self.n * factor;
            }
        }
        func main() -> Int32 {
            let v = Value(n: 5);
            return v.sum() + v.scale(factor: 3);
        }
    "#);
    assert_eq!(result, 20);
}
```

- [ ] **Step 6: Write property get+set requirement test**

```rust
#[test]
fn protocol_property_get_set() {
    let result = compile_and_run(r#"
        protocol Resettable {
            var current: Int32 { get set };
        }
        struct Counter: Resettable {
            var value: Int32;
            var current: Int32 {
                get { yield self.value; }
                set { self.value = newValue; }
            };
        }
        func main() -> Int32 {
            var c = Counter(value: 10);
            c.current = 99;
            return c.current;
        }
    "#);
    assert_eq!(result, 99);
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: all pass

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "Add integration tests for protocol conformance"
```

---

### Task 15: Protocol error case tests (semantic analysis)

**Files:**
- Modify: `tests/compile_test.rs`

- [ ] **Step 1: Add compile_should_fail helper**

```rust
fn compile_should_fail(source: &str) -> String {
    let tokens = tokenize(source).unwrap();
    let program = parse(tokens).unwrap();
    match semantic::analyze(&program) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected semantic error but analysis succeeded"),
    }
}
```

- [ ] **Step 2: Write missing method error test**

```rust
#[test]
fn protocol_error_missing_method() {
    let err = compile_should_fail(r#"
        protocol Summable {
            func sum() -> Int32;
        }
        struct Empty: Summable {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(err.contains("does not implement method `sum`"));
}
```

- [ ] **Step 3: Write return type mismatch error test**

```rust
#[test]
fn protocol_error_return_type_mismatch() {
    let err = compile_should_fail(r#"
        protocol Summable {
            func sum() -> Int32;
        }
        struct Bad: Summable {
            var x: Int32;
            func sum() -> Bool {
                return true;
            }
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(err.contains("return type"));
}
```

- [ ] **Step 4: Write unknown protocol error test**

```rust
#[test]
fn protocol_error_unknown_protocol() {
    let err = compile_should_fail(r#"
        struct Bad: NonExistent {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(err.contains("unknown protocol"));
}
```

- [ ] **Step 5: Write missing property error test**

```rust
#[test]
fn protocol_error_missing_property() {
    let err = compile_should_fail(r#"
        protocol HasTotal {
            var total: Int32 { get };
        }
        struct Bad: HasTotal {
            var x: Int32;
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(err.contains("does not implement property `total`"));
}
```

- [ ] **Step 6: Write missing setter error test**

```rust
#[test]
fn protocol_error_missing_setter() {
    let err = compile_should_fail(r#"
        protocol Writable {
            var value: Int32 { get set };
        }
        struct ReadOnly: Writable {
            var x: Int32;
            var value: Int32 {
                get { yield self.x; }
            };
        }
        func main() -> Int32 { return 0; }
    "#);
    assert!(err.contains("requires a setter"));
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: all pass

- [ ] **Step 8: Commit**

```bash
git add -A && git commit -m "Add protocol error case tests"
```

---

### Task 16: Update grammar documentation

**Files:**
- Modify: `docs/grammar.md`

- [ ] **Step 1: Read the current grammar.md**

Read `docs/grammar.md` to understand current structure and where to add new sections.

- [ ] **Step 2: Add method to struct_member rule**

Update `struct_member` production to include `method`:

```
struct_member = stored_property | computed_property | initializer | method ;

method = "func" , identifier , param_list , [ "->" , type ] , block ;
```

- [ ] **Step 3: Add conformance to struct_def rule**

Update `struct_def`:

```
struct_def = "struct" , identifier , [ ":" , identifier_list ] , "{" , { struct_member } , "}" ;

identifier_list = identifier , { "," , identifier } ;
```

- [ ] **Step 4: Add protocol_def to top_level and add protocol rules**

Update `top_level`:

```
top_level = function | struct_def | protocol_def ;
```

Add protocol rules:

```
protocol_def = "protocol" , identifier , "{" , { protocol_member } , "}" ;

protocol_member = method_sig | property_req ;

method_sig = "func" , identifier , param_list , [ "->" , type ] , ";" ;

property_req = "var" , identifier , ":" , type , "{" , "get" , [ "set" ] , "}" , ";" ;
```

- [ ] **Step 5: Add method call to expression grammar**

Add method_call to the postfix section:

```
postfix = primary , { "." , identifier [ "(" , [ arg_list ] , ")" ] } ;
```

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "Document method and protocol syntax in grammar.md"
```

---

### Task 17: Update TODO.md with future work notes

**Files:**
- Modify: `TODO.md`

- [ ] **Step 1: Read current TODO.md**

- [ ] **Step 2: Remove completed items and add future protocol work**

Remove "No methods" and "No protocols/traits" from known limitations. Add future work section:

```
## Future Protocol Enhancements

- Existential types (Phase 2): `var x: Summable = Point(...)` — protocol as variable/argument type with dynamic dispatch via vtable/witness table. Requires BIR `MethodCall` instruction.
- Extension conformance: `extension Point: Drawable { ... }` for retroactive conformance
- Default implementations in protocols
- Protocol inheritance: `protocol A: B { ... }`
```

- [ ] **Step 3: Commit**

```bash
git add -A && git commit -m "Update TODO.md with protocol future work"
```
