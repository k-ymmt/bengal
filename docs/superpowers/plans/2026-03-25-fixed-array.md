# Fixed-Size Array Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add fixed-size array support (`[T; N]`) to Bengal with stack allocation, array literals, index access/assign, compile-time and runtime bounds checking.

**Architecture:** Arrays are value types represented by LLVM's native `[N x T]` type. The implementation extends each layer: lexer (bracket tokens), parser (array type, literal, index), AST, semantic analysis (type checking, bounds checking), BIR (new instructions), and LLVM codegen (GEP-based element access with bounds checks).

**Tech Stack:** Rust, logos (lexer), inkwell (LLVM bindings)

**Spec:** `docs/superpowers/specs/2026-03-25-fixed-array-design.md`

---

### Task 1: Lexer + AST — Add bracket tokens and array AST nodes

**Files:**
- Modify: `src/lexer/token.rs`
- Modify: `src/parser/ast.rs`

- [ ] **Step 1: Add LBracket and RBracket tokens**

In `src/lexer/token.rs`, add after the `RBrace` token (line ~109):

```rust
// Array: symbols
#[token("[")]
LBracket,
#[token("]")]
RBracket,
```

Add Display arms:
```rust
Token::LBracket => write!(f, "["),
Token::RBracket => write!(f, "]"),
```

- [ ] **Step 2: Add Array variant to TypeAnnotation**

In `src/parser/ast.rs`, add to `TypeAnnotation`:

```rust
Array {
    element: Box<TypeAnnotation>,
    size: u64,
},
```

- [ ] **Step 3: Add ArrayLiteral and IndexAccess to ExprKind**

```rust
ArrayLiteral {
    elements: Vec<Expr>,
},
IndexAccess {
    object: Box<Expr>,
    index: Box<Expr>,
},
```

- [ ] **Step 4: Add IndexAssign to Stmt**

```rust
IndexAssign {
    object: Box<Expr>,
    index: Box<Expr>,
    value: Expr,
},
```

- [ ] **Step 5: Fix all compilation errors**

Use `cargo check` iteratively. Add match arms for the new variants in:
- `src/parser/mod.rs`: normalize functions and test assertions that match on ExprKind/Stmt
- `src/semantic/mod.rs`: match arms on ExprKind and Stmt
- `src/semantic/types.rs`: resolve_type for TypeAnnotation::Array
- `src/bir/lowering.rs`: match arms on ExprKind and Stmt
- `src/monomorphize.rs`: match arms for TypeAnnotation, ExprKind, Stmt

For now, add `todo!()` or `unreachable!()` for the new arms (except resolve_type which should work):

```rust
// In resolve_type:
TypeAnnotation::Array { element, size } => Type::Array {
    element: Box::new(resolve_type(element)),
    size: *size,
},
```

- [ ] **Step 6: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: all existing tests PASS

- [ ] **Step 7: Commit**

```bash
cargo fmt && cargo clippy
git add src/lexer/token.rs src/parser/ast.rs src/parser/mod.rs src/semantic/ src/bir/lowering.rs src/monomorphize.rs
git commit -m "Add bracket tokens and array AST nodes"
```

---

### Task 2: Parser — Array types, literals, index access, and index assign

**Files:**
- Modify: `src/parser/mod.rs`

- [ ] **Step 1: Write parser tests**

Add to the `#[cfg(test)]` section in `src/parser/mod.rs`:

```rust
#[test]
fn parse_array_type() {
    let tokens = tokenize("func main() -> Int32 { let a: [Int32; 3] = [1, 2, 3]; return 0; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn parse_array_literal() {
    let tokens = tokenize("func main() -> Int32 { let a = [1, 2, 3]; return 0; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn parse_index_access() {
    let tokens = tokenize("func main() -> Int32 { let a = [1, 2, 3]; return a[0]; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}

#[test]
fn parse_index_assign() {
    let tokens = tokenize("func main() -> Int32 { var a = [1, 2, 3]; a[0] = 10; return a[0]; }").unwrap();
    let program = parse(tokens).unwrap();
    assert_eq!(program.functions.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test parse_array -- --nocapture 2>&1 | tail -10`
Expected: FAIL

- [ ] **Step 3: Implement parse_type for array types**

In `parse_type`, add a case at the top for `Token::LBracket`:

```rust
fn parse_type(&mut self) -> Result<TypeAnnotation> {
    if self.peek().node == Token::LBracket {
        self.advance(); // consume [
        let element = self.parse_type()?;
        self.expect(Token::Semicolon)?;
        let size_tok = self.expect(Token::Number(0))?;
        let size = match &size_tok.node {
            Token::Number(n) => *n as u64,
            _ => unreachable!(),
        };
        self.expect(Token::RBracket)?;
        return Ok(TypeAnnotation::Array {
            element: Box::new(element),
            size,
        });
    }
    if self.peek().node == Token::LParen {
        // ... existing Unit type parsing ...
    }
    // ... rest of existing parse_type ...
}
```

- [ ] **Step 4: Implement parse_primary for array literals**

In `parse_primary`, add a case for `Token::LBracket`:

```rust
Token::LBracket => {
    self.advance(); // consume [
    let mut elements = Vec::new();
    if self.peek().node != Token::RBracket {
        elements.push(self.parse_expr()?);
        while self.peek().node == Token::Comma {
            self.advance();
            elements.push(self.parse_expr()?);
        }
    }
    self.expect(Token::RBracket)?;
    Ok(self.expr(ExprKind::ArrayLiteral { elements }))
}
```

- [ ] **Step 5: Implement parse_postfix for index access**

In `parse_postfix`, add `Token::LBracket` arm (alongside `Token::Dot` and `Token::LParen`):

```rust
Token::LBracket => {
    self.advance(); // consume [
    let index = self.parse_expr()?;
    self.expect(Token::RBracket)?;
    expr = self.expr(ExprKind::IndexAccess {
        object: Box::new(expr),
        index: Box::new(index),
    });
}
```

- [ ] **Step 6: Implement index assign in parse_stmt**

In `parse_stmt`, in the `_ =>` branch where assignments are handled (line ~643), add `IndexAccess` alongside `FieldAccess`:

```rust
ExprKind::IndexAccess { object, index } => Stmt::IndexAssign {
    object: Box::new((**object).clone()),
    index: Box::new((**index).clone()),
    value,
},
```

- [ ] **Step 7: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests PASS

- [ ] **Step 8: Commit**

```bash
cargo fmt && cargo clippy
git add src/parser/mod.rs
git commit -m "Parse array types, literals, index access, and index assign"
```

---

### Task 3: Type System — Array type checking and bounds checking

**Files:**
- Modify: `src/semantic/types.rs`
- Modify: `src/semantic/mod.rs`

- [ ] **Step 1: Create tests/arrays.rs with integration tests**

```rust
mod common;

use common::{compile_and_run, compile_should_fail};

#[test]
fn array_literal_and_access() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { let a = [10, 20, 30]; return a[1]; }"
    ), 20);
}

#[test]
fn array_index_assign() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { var a = [1, 2, 3]; a[0] = 42; return a[0]; }"
    ), 42);
}

#[test]
fn array_with_type_annotation() {
    assert_eq!(compile_and_run(
        "func main() -> Int32 { let a: [Int32; 3] = [1, 2, 3]; return a[2]; }"
    ), 3);
}

#[test]
fn array_as_function_param() {
    assert_eq!(compile_and_run(
        "func sum(arr: [Int32; 3]) -> Int32 { return arr[0] + arr[1] + arr[2]; }
         func main() -> Int32 { return sum([10, 20, 30]); }"
    ), 60);
}

#[test]
fn array_as_return_type() {
    assert_eq!(compile_and_run(
        "func make() -> [Int32; 2] { return [5, 6]; }
         func main() -> Int32 { let a = make(); return a[0] + a[1]; }"
    ), 11);
}

#[test]
fn error_mixed_element_types() {
    let err = compile_should_fail(
        "func main() -> Int32 { let a = [1, true]; return 0; }"
    );
    assert!(err.contains("same type"));
}

#[test]
fn error_size_mismatch() {
    let err = compile_should_fail(
        "func main() -> Int32 { let a: [Int32; 3] = [1, 2]; return 0; }"
    );
    assert!(err.contains("size"));
}

#[test]
fn error_constant_oob() {
    let err = compile_should_fail(
        "func main() -> Int32 { let a = [1, 2, 3]; return a[3]; }"
    );
    assert!(err.contains("out of bounds"));
}

#[test]
fn error_index_non_array() {
    let err = compile_should_fail(
        "func main() -> Int32 { let x = 5; return x[0]; }"
    );
    assert!(err.contains("cannot index"));
}

#[test]
fn error_immutable_index_assign() {
    let err = compile_should_fail(
        "func main() -> Int32 { let a = [1, 2]; a[0] = 5; return 0; }"
    );
    assert!(err.contains("immutable"));
}

#[test]
fn error_non_integer_index() {
    let err = compile_should_fail(
        "func main() -> Int32 { let a = [1, 2, 3]; return a[true]; }"
    );
    assert!(err.contains("integer"));
}
```

- [ ] **Step 2: Add Array to Type enum**

In `src/semantic/types.rs`:

```rust
pub enum Type {
    // ... existing ...
    Array {
        element: Box<Type>,
        size: u64,
    },
}
```

Update `fmt::Display`:
```rust
Type::Array { element, size } => write!(f, "[{}; {}]", element, size),
```

Update `resolve_type` (should already be done from Task 1):
```rust
TypeAnnotation::Array { element, size } => Type::Array {
    element: Box::new(resolve_type(element)),
    size: *size,
},
```

- [ ] **Step 3: Add type checking for ArrayLiteral in check_expr**

In the `check_expr` function in `src/semantic/mod.rs`, add handling for `ExprKind::ArrayLiteral`:

```rust
ExprKind::ArrayLiteral { elements } => {
    if elements.is_empty() {
        return Err(sem_err("empty array literals are not supported"));
    }
    let first_ty = check_expr(&elements[0], resolver)?;
    for (i, elem) in elements.iter().enumerate().skip(1) {
        let elem_ty = check_expr(elem, resolver)?;
        if elem_ty != first_ty {
            return Err(sem_err(format!(
                "array elements must all have the same type: expected '{}', found '{}'",
                first_ty, elem_ty
            )));
        }
    }
    Ok(Type::Array {
        element: Box::new(first_ty),
        size: elements.len() as u64,
    })
}
```

- [ ] **Step 4: Add type checking for IndexAccess in check_expr**

```rust
ExprKind::IndexAccess { object, index } => {
    let obj_ty = check_expr(object, resolver)?;
    let idx_ty = check_expr(index, resolver)?;

    let (element_ty, size) = match &obj_ty {
        Type::Array { element, size } => (*element.clone(), *size),
        _ => return Err(sem_err(format!("cannot index into type '{}'", obj_ty))),
    };

    if !idx_ty.is_integer() {
        return Err(sem_err(format!(
            "array index must be an integer type, found '{}'", idx_ty
        )));
    }

    // Compile-time bounds check for constant indices
    if let ExprKind::Number(n) = &index.kind {
        if *n < 0 || *n as u64 >= size {
            return Err(sem_err(format!(
                "array index {} is out of bounds for array of size {}", n, size
            )));
        }
    }

    Ok(element_ty)
}
```

- [ ] **Step 5: Add type checking for IndexAssign in check_stmt**

In the `check_stmt` function, add handling for `Stmt::IndexAssign`:

```rust
Stmt::IndexAssign { object, index, value } => {
    let obj_ty = check_expr(object, resolver)?;
    let idx_ty = check_expr(index, resolver)?;

    let (element_ty, size) = match &obj_ty {
        Type::Array { element, size } => (*element.clone(), *size),
        _ => return Err(sem_err(format!("cannot index into type '{}'", obj_ty))),
    };

    if !idx_ty.is_integer() {
        return Err(sem_err(format!(
            "array index must be an integer type, found '{}'", idx_ty
        )));
    }

    // Compile-time bounds check
    if let ExprKind::Number(n) = &index.kind {
        if *n < 0 || *n as u64 >= size {
            return Err(sem_err(format!(
                "array index {} is out of bounds for array of size {}", n, size
            )));
        }
    }

    // Check mutability — the object must be a mutable variable
    if let ExprKind::Ident(name) = &object.kind {
        match resolver.lookup_var(name) {
            Some(info) if !info.mutable => {
                return Err(sem_err("cannot assign to index of immutable variable"));
            }
            None => return Err(sem_err(format!("undefined variable `{}`", name))),
            _ => {}
        }
    }

    let val_ty = check_expr(value, resolver)?;
    if val_ty != element_ty {
        return Err(sem_err(format!(
            "expected '{}', found '{}'", element_ty, val_ty
        )));
    }
    Ok(())
}
```

- [ ] **Step 6: Handle Array type annotation matching**

In the Let/Var handling in check_stmt, when a type annotation is provided and the value is an array literal, verify the sizes match. The existing type-checking logic compares the annotated type with the inferred type — add handling so `Type::Array` comparison works (it should work with `PartialEq` derive if element types and sizes are compared).

Add a specific error for size mismatches:
```rust
// In Let/Var type annotation checking:
(Type::Array { size: expected_size, .. }, Type::Array { size: actual_size, .. })
    if expected_size != actual_size =>
{
    return Err(sem_err(format!(
        "expected array of size {}, found array of size {}",
        expected_size, actual_size
    )));
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: error case tests PASS; `array_literal_and_access` and other success tests still FAIL (BIR/codegen not done yet)

- [ ] **Step 8: Commit**

```bash
cargo fmt && cargo clippy
git add src/semantic/ tests/arrays.rs
git commit -m "Add array type checking with bounds checking"
```

---

### Task 4: BIR — Array instructions and lowering

**Files:**
- Modify: `src/bir/instruction.rs`
- Modify: `src/bir/lowering.rs`
- Modify: `src/bir/printer.rs` (if it exists for BIR debug output)

- [ ] **Step 1: Add Array to BirType**

In `src/bir/instruction.rs`:

```rust
pub enum BirType {
    // ... existing ...
    Array {
        element: Box<BirType>,
        size: u64,
    },
}
```

- [ ] **Step 2: Add ArrayInit, ArrayGet, ArraySet instructions**

```rust
/// %result = array_init [%v0, %v1, ...] : [N x T]
ArrayInit {
    result: Value,
    ty: BirType,
    elements: Vec<Value>,
},
/// %result = array_get %array, %index : ElementType
ArrayGet {
    result: Value,
    ty: BirType,
    array: Value,
    index: Value,
    array_size: u64,
},
/// %result = array_set %array, %index, %value : ArrayType
ArraySet {
    result: Value,
    ty: BirType,
    array: Value,
    index: Value,
    value: Value,
    array_size: u64,
},
```

Note: `array_size` is included for runtime bounds checking in codegen.

- [ ] **Step 3: Add Type-to-BirType conversion**

In `src/bir/lowering.rs`, update the `convert_type` function:

```rust
Type::Array { element, size } => BirType::Array {
    element: Box::new(convert_type(element)),
    size: *size,
},
```

- [ ] **Step 4: Lower ArrayLiteral expression**

In the expression lowering function, add handling for `ExprKind::ArrayLiteral`:

```rust
ExprKind::ArrayLiteral { elements } => {
    let lowered: Vec<Value> = elements.iter().map(|e| self.lower_expr(e)).collect();
    // Infer element type from first element
    let elem_ty = self.get_value_type(&lowered[0]);
    let arr_ty = BirType::Array {
        element: Box::new(elem_ty),
        size: lowered.len() as u64,
    };
    let result = self.fresh_value();
    self.value_types.insert(result, arr_ty.clone());
    self.emit(Instruction::ArrayInit {
        result,
        ty: arr_ty,
        elements: lowered,
    });
    result
}
```

- [ ] **Step 5: Lower IndexAccess expression**

```rust
ExprKind::IndexAccess { object, index } => {
    let arr_val = self.lower_expr(object);
    let idx_val = self.lower_expr(index);
    let arr_ty = self.get_value_type(&arr_val);
    let (elem_ty, size) = match &arr_ty {
        BirType::Array { element, size } => (*element.clone(), *size),
        _ => panic!("IndexAccess on non-array"),
    };
    let result = self.fresh_value();
    self.value_types.insert(result, elem_ty.clone());
    self.emit(Instruction::ArrayGet {
        result,
        ty: elem_ty,
        array: arr_val,
        index: idx_val,
        array_size: size,
    });
    result
}
```

- [ ] **Step 6: Lower IndexAssign statement**

In the statement lowering, add handling for `Stmt::IndexAssign`. This follows the `FieldAssign` pattern — get the variable's current value, produce an `ArraySet`, then update the variable:

```rust
Stmt::IndexAssign { object, index, value } => {
    let idx_val = self.lower_expr(index);
    let new_val = self.lower_expr(value);
    if let ExprKind::Ident(name) = &object.kind {
        let arr_val = self.lookup_var(name);
        let arr_ty = self.get_value_type(&arr_val);
        let size = match &arr_ty {
            BirType::Array { size, .. } => *size,
            _ => panic!("IndexAssign on non-array"),
        };
        let result = self.fresh_value();
        self.value_types.insert(result, arr_ty.clone());
        self.emit(Instruction::ArraySet {
            result,
            ty: arr_ty,
            array: arr_val,
            index: idx_val,
            value: new_val,
            array_size: size,
        });
        self.assign_var(name, result);
    }
    StmtResult::None
}
```

- [ ] **Step 7: Fix compilation errors and update BIR printer**

Update `src/bir/printer.rs` to handle the new instructions and BirType::Array. Update `src/bir/optimize.rs` if it matches on instructions. Update `collect_value_types` in `src/codegen/llvm.rs` to handle the new instructions.

- [ ] **Step 8: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: all unit tests PASS; integration tests still FAIL (LLVM codegen not done yet)

- [ ] **Step 9: Commit**

```bash
cargo fmt && cargo clippy
git add src/bir/ src/codegen/llvm.rs
git commit -m "Add BIR array instructions and lowering"
```

---

### Task 5: LLVM Codegen — Array type mapping and instruction compilation

**Files:**
- Modify: `src/codegen/llvm.rs`

- [ ] **Step 1: Add Array to bir_type_to_llvm_type**

```rust
BirType::Array { element, size } => {
    let elem_ty = bir_type_to_llvm_type(context, element, struct_types)?;
    Some(elem_ty.array_type(*size as u32).into())
}
```

- [ ] **Step 2: Implement ArrayInit codegen**

For `Instruction::ArrayInit`:

```rust
Instruction::ArrayInit { result, ty, elements } => {
    let llvm_ty = bir_type_to_llvm_type(ctx.context, ty, &ctx.struct_types)
        .ok_or_else(|| codegen_err("ArrayInit: unsupported element type"))?;
    let arr_ty = llvm_ty.into_array_type();
    let mut agg: inkwell::values::AggregateValueEnum = arr_ty.get_undef().into();
    for (i, elem_val) in elements.iter().enumerate() {
        let val = load_value(ctx, elem_val)
            .ok_or_else(|| codegen_err("ArrayInit: failed to load element"))?;
        agg = ctx.builder
            .build_insert_value(agg, val, i as u32, "arr_insert")
            .map_err(|e| codegen_err(e.to_string()))?;
    }
    ctx.builder
        .build_store(ctx.alloca_map[result], agg.into_array_value())
        .map_err(|e| codegen_err(e.to_string()))?;
}
```

- [ ] **Step 3: Implement ArrayGet codegen**

For `Instruction::ArrayGet`:

```rust
Instruction::ArrayGet { result, ty, array, index, array_size } => {
    // Store array to alloca for GEP
    let arr_val = load_value(ctx, array)
        .ok_or_else(|| codegen_err("ArrayGet: failed to load array"))?;
    let arr_alloca = ctx.builder
        .build_alloca(arr_val.get_type(), "arr_tmp")
        .map_err(|e| codegen_err(e.to_string()))?;
    ctx.builder.build_store(arr_alloca, arr_val)
        .map_err(|e| codegen_err(e.to_string()))?;

    let idx_val = load_value(ctx, index)
        .ok_or_else(|| codegen_err("ArrayGet: failed to load index"))?;

    // Runtime bounds check for variable indices
    // (constant indices are already checked at compile time)
    emit_bounds_check(ctx, idx_val, *array_size)?;

    let zero = ctx.context.i32_type().const_zero();
    let gep = unsafe {
        ctx.builder.build_in_bounds_gep(
            arr_alloca.as_basic_value_enum().into_pointer_value().get_type().get_element_type().into_array_type(),
            arr_alloca,
            &[zero.into(), idx_val.into_int_value().into()],
            "arr_gep",
        ).map_err(|e| codegen_err(e.to_string()))?
    };
    let elem = ctx.builder
        .build_load(
            bir_type_to_llvm_type(ctx.context, ty, &ctx.struct_types)
                .ok_or_else(|| codegen_err("ArrayGet: unsupported element type"))?,
            gep,
            "arr_elem",
        )
        .map_err(|e| codegen_err(e.to_string()))?;
    ctx.builder
        .build_store(ctx.alloca_map[result], elem)
        .map_err(|e| codegen_err(e.to_string()))?;
}
```

Note: The exact GEP API depends on inkwell version. The implementer should check inkwell's `build_gep` or `build_in_bounds_gep` signature and adapt. The key idea: alloca the array, GEP to the element, load/store.

- [ ] **Step 4: Implement ArraySet codegen**

Similar to ArrayGet but stores the new value and loads the updated array:

```rust
Instruction::ArraySet { result, ty, array, index, value, array_size } => {
    let arr_val = load_value(ctx, array)
        .ok_or_else(|| codegen_err("ArraySet: failed to load array"))?;
    let arr_alloca = ctx.builder
        .build_alloca(arr_val.get_type(), "arr_tmp")
        .map_err(|e| codegen_err(e.to_string()))?;
    ctx.builder.build_store(arr_alloca, arr_val)
        .map_err(|e| codegen_err(e.to_string()))?;

    let idx_val = load_value(ctx, index)
        .ok_or_else(|| codegen_err("ArraySet: failed to load index"))?;
    let new_val = load_value(ctx, value)
        .ok_or_else(|| codegen_err("ArraySet: failed to load value"))?;

    emit_bounds_check(ctx, idx_val, *array_size)?;

    let zero = ctx.context.i32_type().const_zero();
    let gep = unsafe {
        ctx.builder.build_in_bounds_gep(
            arr_alloca.as_basic_value_enum().into_pointer_value().get_type().get_element_type().into_array_type(),
            arr_alloca,
            &[zero.into(), idx_val.into_int_value().into()],
            "arr_gep",
        ).map_err(|e| codegen_err(e.to_string()))?
    };
    ctx.builder.build_store(gep, new_val)
        .map_err(|e| codegen_err(e.to_string()))?;

    // Load updated array
    let updated = ctx.builder
        .build_load(arr_val.get_type(), arr_alloca, "arr_updated")
        .map_err(|e| codegen_err(e.to_string()))?;
    ctx.builder
        .build_store(ctx.alloca_map[result], updated)
        .map_err(|e| codegen_err(e.to_string()))?;
}
```

- [ ] **Step 5: Implement emit_bounds_check helper**

```rust
fn emit_bounds_check(ctx: &mut CodegenContext, index: BasicValueEnum, size: u64) -> Result<()> {
    let idx_int = index.into_int_value();
    let size_val = idx_int.get_type().const_int(size, false);
    let in_bounds = ctx.builder
        .build_int_compare(inkwell::IntPredicate::ULT, idx_int, size_val, "bounds_check")
        .map_err(|e| codegen_err(e.to_string()))?;

    let current_fn = ctx.current_function;
    let ok_bb = ctx.context.append_basic_block(current_fn, "bounds_ok");
    let trap_bb = ctx.context.append_basic_block(current_fn, "bounds_trap");

    ctx.builder.build_conditional_branch(in_bounds, ok_bb, trap_bb)
        .map_err(|e| codegen_err(e.to_string()))?;

    // Trap block
    ctx.builder.position_at_end(trap_bb);
    let trap_fn = inkwell::intrinsics::Intrinsic::find("llvm.trap")
        .ok_or_else(|| codegen_err("llvm.trap intrinsic not found"))?
        .get_declaration(ctx.module, &[])
        .ok_or_else(|| codegen_err("failed to get llvm.trap declaration"))?;
    ctx.builder.build_call(trap_fn, &[], "trap")
        .map_err(|e| codegen_err(e.to_string()))?;
    ctx.builder.build_unreachable()
        .map_err(|e| codegen_err(e.to_string()))?;

    // Continue in ok block
    ctx.builder.position_at_end(ok_bb);
    Ok(())
}
```

Note: The exact intrinsic API depends on inkwell version. The implementer should check if `inkwell::intrinsics::Intrinsic` exists or use `module.add_function("llvm.trap", ...)` directly.

- [ ] **Step 6: Run integration tests**

Run: `cargo test --test arrays -- --nocapture 2>&1 | tail -20`
Expected: all array tests PASS

- [ ] **Step 7: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests PASS

- [ ] **Step 8: Commit**

```bash
cargo fmt && cargo clippy
git add src/codegen/llvm.rs
git commit -m "Add LLVM codegen for array instructions with bounds checking"
```

---

### Task 6: Monomorphize support and cleanup

**Files:**
- Modify: `src/monomorphize.rs`

- [ ] **Step 1: Add Array handling to monomorphize**

In `src/monomorphize.rs`, the type substitution functions need to handle the new AST nodes:

- `substitute_type_annotation`: handle `TypeAnnotation::Array` by recursively substituting the element type
- `walk_expr` / `collect_from_expr`: handle `ExprKind::ArrayLiteral` and `ExprKind::IndexAccess`
- `walk_stmt` / `rewrite_stmt`: handle `Stmt::IndexAssign`

These should be straightforward recursive walks with no special logic.

- [ ] **Step 2: Write a test combining generics with arrays**

Add to `tests/generics.rs`:

```rust
#[test]
fn generic_function_with_array() {
    assert_eq!(compile_and_run(
        "func first<T>(arr: [T; 3]) -> T { return arr[0]; }
         func main() -> Int32 { return first<Int32>([10, 20, 30]); }"
    ), 10);
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests PASS

- [ ] **Step 4: Commit**

```bash
cargo fmt && cargo clippy
git add src/monomorphize.rs tests/generics.rs
git commit -m "Add array support to monomorphization"
```

---

### Task 7: Documentation update

**Files:**
- Modify: `docs/grammar.md`
- Modify: `TODO.md`

- [ ] **Step 1: Update grammar.md**

Add array grammar rules:
- `array_type`, `array_literal`, `index_expr`, `index_assign` productions
- Updated `type` rule
- Note about fixed-size arrays as value types
- Update Phase number

- [ ] **Step 2: Update TODO.md**

Remove the fixed-size array items from TODO (they're now implemented). Keep the variable-length array and future enhancement items.

- [ ] **Step 3: Commit**

```bash
git add docs/grammar.md
git commit -m "Document fixed-size arrays in grammar"
```

---

### Task 8: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests PASS

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | tail -5`
Expected: no warnings

- [ ] **Step 3: Verify all spec examples work**

Ensure these examples from the spec compile and run:

```bengal
let a: [Int32; 3] = [1, 2, 3];
let b = [1.0, 2.0];
let x = a[0];
var arr = [1, 2, 3];
arr[1] = 10;
func sum(arr: [Int32; 3]) -> Int32 { return arr[0] + arr[1] + arr[2]; }
func makeArray() -> [Int32; 2] { return [10, 20]; }
```
