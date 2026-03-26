# BIR Serialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serialize and deserialize generic BIR to/from `.bengalmod` interface files using MessagePack, enabling future separate compilation.

**Architecture:** Add `Serialize`/`Deserialize` derives to existing BIR types, create a new `src/interface.rs` module with `write_interface` and `read_interface` functions, and wrap the payload with a magic + version header. A custom serde helper handles the `conformance_map`'s tuple key.

**Tech Stack:** Rust, `serde` (already present), `rmp-serde` (new dependency), `tempfile` (dev, already present)

**Spec:** `docs/superpowers/specs/2026-03-26-bir-serialization-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Add `rmp-serde` dependency |
| `src/bir/instruction.rs` | Add `Serialize, Deserialize` derives to all BIR types; add `conformance_map_serde` helper module |
| `src/package.rs` | Add `Serialize, Deserialize` to `ModulePath` |
| `src/error.rs` | Add `InterfaceError` variant to `BengalError` |
| `src/interface.rs` | **New** — `BengalModFile`, `MAGIC`, `FORMAT_VERSION`, `write_interface`, `read_interface` |
| `src/lib.rs` | Add `pub mod interface;` |
| `tests/interface.rs` | **New** — round-trip and validation tests |

---

### Task 1: Add `rmp-serde` dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add rmp-serde to Cargo.toml**

Add to `[dependencies]`:

```toml
rmp-serde = "1"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "Add rmp-serde dependency for BIR serialization"
```

---

### Task 2: Add `Serialize, Deserialize` derives to BIR types and `ModulePath`

**Files:**
- Modify: `src/bir/instruction.rs`
- Modify: `src/package.rs`

- [ ] **Step 1: Add serde import and derives to `src/bir/instruction.rs`**

Add `use serde::{Serialize, Deserialize};` at the top of the file.

Add `Serialize, Deserialize` to the derive list of every type. The 10 types to modify (preserving existing derives):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Value(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BirType { ... }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BirBinOp { ... }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BirCompareOp { ... }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Instruction { ... }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Terminator { ... }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasicBlock { ... }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CfgRegion { ... }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BirFunction { ... }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BirModule { ... }
```

On `BirModule.conformance_map`, add a serde attribute:

```rust
#[serde(with = "conformance_map_serde")]
pub conformance_map: HashMap<(String, BirType), String>,
```

- [ ] **Step 2: Add `conformance_map_serde` helper module at the bottom of `src/bir/instruction.rs`**

This module converts the `HashMap<(String, BirType), String>` to/from `Vec<((String, BirType), String)>` for serialization:

```rust
mod conformance_map_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        map: &HashMap<(String, BirType), String>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries: Vec<_> = map.iter().map(|(k, v)| (k, v)).collect();
        entries.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> std::result::Result<HashMap<(String, BirType), String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries: Vec<((String, BirType), String)> = Vec::deserialize(deserializer)?;
        Ok(entries.into_iter().collect())
    }
}
```

- [ ] **Step 3: Add `Serialize, Deserialize` to `ModulePath` in `src/package.rs`**

Change the existing import:
```rust
use serde::Deserialize;
```
to:
```rust
use serde::{Deserialize, Serialize};
```

Add `Serialize, Deserialize` to the derive list:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModulePath(pub Vec<String>);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/bir/instruction.rs src/package.rs
git commit -m "Add Serialize, Deserialize derives to BIR types and ModulePath"
```

---

### Task 3: Add `InterfaceError` variant to `BengalError`

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Add `InterfaceError` variant**

Add after the `PackageError` variant in `BengalError`:

```rust
#[error("Interface error: {message}")]
InterfaceError { message: String },
```

- [ ] **Step 2: Add match arm in `into_diagnostic`**

Add after the `PackageError` arm in `BengalError::into_diagnostic`:

```rust
BengalError::InterfaceError { message } => BengalDiagnostic {
    message,
    src_code: source,
    span: None,
    label: String::new(),
},
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 4: Run existing tests**

Run: `cargo test`
Expected: all existing tests pass (no regressions)

- [ ] **Step 5: Commit**

```bash
git add src/error.rs
git commit -m "Add InterfaceError variant to BengalError"
```

---

### Task 4: Implement `src/interface.rs` — write_interface

**Files:**
- Create: `src/interface.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing test for `write_interface`**

Create `tests/interface.rs`:

```rust
mod common;

use bengal::interface::{read_interface, write_interface};
use bengal::pipeline::{self, LoweredPackage};
use tempfile::NamedTempFile;

/// Helper: compile source to LoweredPackage (through optimize stage).
fn source_to_lowered(source: &str) -> LoweredPackage {
    let parsed = pipeline::parse_source("test", source).unwrap();
    let analyzed = pipeline::analyze(parsed).unwrap();
    let lowered = pipeline::lower(analyzed).unwrap();
    pipeline::optimize(lowered)
}

#[test]
fn write_interface_creates_file() {
    let lowered = source_to_lowered("func main() -> Int32 { return 42; }");
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let metadata = std::fs::metadata(file.path()).unwrap();
    assert!(metadata.len() > 8, "file must contain header + payload");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test interface write_interface_creates_file`
Expected: FAIL — `bengal::interface` module does not exist

- [ ] **Step 3: Create `src/interface.rs` with `write_interface`**

```rust
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bir::instruction::BirModule;
use crate::error::{BengalError, Result};
use crate::package::ModulePath;
use crate::pipeline::LoweredPackage;

pub const MAGIC: &[u8; 4] = b"BGMD";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct BengalModFile {
    pub package_name: String,
    pub modules: HashMap<ModulePath, BirModule>,
}

/// Write a LoweredPackage to a .bengalmod interface file.
pub fn write_interface(package: &LoweredPackage, path: &Path) -> Result<()> {
    let modules: HashMap<ModulePath, BirModule> = package
        .modules
        .iter()
        .map(|(k, v)| (k.clone(), v.bir.clone()))
        .collect();

    let file = BengalModFile {
        package_name: package.package_name.clone(),
        modules,
    };

    let payload = rmp_serde::to_vec(&file).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to serialize interface: {}", e),
    })?;

    let mut out =
        std::fs::File::create(path).map_err(|e| BengalError::InterfaceError {
            message: format!("failed to create file '{}': {}", path.display(), e),
        })?;

    out.write_all(MAGIC)
        .and_then(|()| out.write_all(&FORMAT_VERSION.to_le_bytes()))
        .and_then(|()| out.write_all(&payload))
        .map_err(|e| BengalError::InterfaceError {
            message: format!("failed to write interface file: {}", e),
        })?;

    Ok(())
}
```

Add the `read_interface` function as a stub (to satisfy the test import):

```rust
/// Read a .bengalmod interface file.
pub fn read_interface(_path: &Path) -> Result<BengalModFile> {
    todo!("read_interface not yet implemented")
}
```

- [ ] **Step 4: Add `pub mod interface;` to `src/lib.rs`**

Add after the existing module declarations:

```rust
pub mod interface;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --test interface write_interface_creates_file`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/interface.rs src/lib.rs tests/interface.rs
git commit -m "Implement write_interface for .bengalmod files"
```

---

### Task 5: Implement `read_interface`

**Files:**
- Modify: `src/interface.rs`
- Modify: `tests/interface.rs`

- [ ] **Step 1: Write failing test for round-trip (simple function)**

Add to `tests/interface.rs`:

```rust
use bengal::package::ModulePath;

#[test]
fn round_trip_simple_function() {
    let lowered = source_to_lowered(
        "func add(a: Int32, b: Int32) -> Int32 { return a + b; }
         func main() -> Int32 { return add(1, 2); }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    assert_eq!(loaded.package_name, "test");
    let original_bir = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original_bir, loaded_bir);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test interface round_trip_simple_function`
Expected: FAIL — `read_interface` panics with `todo!()`

- [ ] **Step 3: Implement `read_interface`**

Replace the stub in `src/interface.rs`:

```rust
/// Read a .bengalmod interface file.
pub fn read_interface(path: &Path) -> Result<BengalModFile> {
    let data = std::fs::read(path).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to read '{}': {}", path.display(), e),
    })?;

    if data.len() < 8 {
        return Err(BengalError::InterfaceError {
            message: "file too short to be a valid .bengalmod file".to_string(),
        });
    }

    if &data[..4] != MAGIC {
        return Err(BengalError::InterfaceError {
            message: "invalid magic bytes: not a .bengalmod file".to_string(),
        });
    }

    let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
    if version != FORMAT_VERSION {
        return Err(BengalError::InterfaceError {
            message: format!(
                "incompatible format version {} (expected {}), please rebuild",
                version, FORMAT_VERSION
            ),
        });
    }

    rmp_serde::from_slice(&data[8..]).map_err(|e| BengalError::InterfaceError {
        message: format!("failed to deserialize interface: {}", e),
    })
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test interface round_trip_simple_function`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/interface.rs tests/interface.rs
git commit -m "Implement read_interface with header validation"
```

---

### Task 6: Round-trip tests for all BIR features

**Files:**
- Modify: `tests/interface.rs`

- [ ] **Step 1: Add round-trip test for generic function**

```rust
#[test]
fn round_trip_generic_function() {
    let lowered = source_to_lowered(
        "func identity<T>(x: T) -> T { return x; }
         func main() -> Int32 { return identity<Int32>(42); }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}
```

- [ ] **Step 2: Add round-trip test for struct with methods**

```rust
#[test]
fn round_trip_struct_with_methods() {
    let lowered = source_to_lowered(
        "struct Point {
            var x: Int32;
            var y: Int32;
            func sum() -> Int32 { return self.x + self.y; }
         }
         func main() -> Int32 {
            let p = Point(x: 3, y: 4);
            return p.sum();
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}
```

- [ ] **Step 3: Add round-trip test for generic struct**

```rust
#[test]
fn round_trip_generic_struct() {
    let lowered = source_to_lowered(
        "struct Box<T> {
            var value: T;
         }
         func main() -> Int32 {
            let b = Box<Int32>(value: 42);
            return b.value;
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}
```

- [ ] **Step 4: Add round-trip test for protocol conformance**

```rust
#[test]
fn round_trip_protocol_conformance() {
    let lowered = source_to_lowered(
        "protocol Summable {
            func sum() -> Int32;
         }
         struct Pair: Summable {
            var a: Int32;
            var b: Int32;
            func sum() -> Int32 { return self.a + self.b; }
         }
         func total<T: Summable>(item: T) -> Int32 { return item.sum(); }
         func main() -> Int32 {
            return total<Pair>(Pair(a: 10, b: 20));
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}
```

- [ ] **Step 5: Add round-trip test for arrays**

```rust
#[test]
fn round_trip_array() {
    let lowered = source_to_lowered(
        "func main() -> Int32 {
            let arr: [Int32; 3] = [10, 20, 30];
            return arr[1];
         }",
    );
    let file = NamedTempFile::new().unwrap();
    write_interface(&lowered, file.path()).unwrap();
    let loaded = read_interface(file.path()).unwrap();

    let original = &lowered.modules.get(&ModulePath::root()).unwrap().bir;
    let loaded_bir = loaded.modules.get(&ModulePath::root()).unwrap();
    assert_eq!(original, loaded_bir);
}
```

- [ ] **Step 6: Run all round-trip tests**

Run: `cargo test --test interface`
Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add tests/interface.rs
git commit -m "Add round-trip tests for generics, structs, protocols, and arrays"
```

---

### Task 7: Validation tests for error cases

**Files:**
- Modify: `tests/interface.rs`

- [ ] **Step 1: Add validation tests**

```rust
use bengal::interface::{MAGIC, FORMAT_VERSION};
use std::io::Write;

#[test]
fn read_invalid_magic() {
    let file = NamedTempFile::new().unwrap();
    let mut f = std::fs::File::create(file.path()).unwrap();
    f.write_all(b"XXXX").unwrap();
    f.write_all(&FORMAT_VERSION.to_le_bytes()).unwrap();
    f.write_all(b"dummy").unwrap();
    drop(f);

    let err = read_interface(file.path()).unwrap_err();
    assert!(err.to_string().contains("not a .bengalmod file"), "{}", err);
}

#[test]
fn read_wrong_version() {
    let file = NamedTempFile::new().unwrap();
    let mut f = std::fs::File::create(file.path()).unwrap();
    f.write_all(MAGIC).unwrap();
    f.write_all(&(FORMAT_VERSION + 1).to_le_bytes()).unwrap();
    f.write_all(b"dummy").unwrap();
    drop(f);

    let err = read_interface(file.path()).unwrap_err();
    assert!(err.to_string().contains("incompatible format version"), "{}", err);
}

#[test]
fn read_empty_file() {
    let file = NamedTempFile::new().unwrap();
    // file is empty (0 bytes)

    let err = read_interface(file.path()).unwrap_err();
    assert!(err.to_string().contains("too short"), "{}", err);
}

#[test]
fn read_truncated_payload() {
    let file = NamedTempFile::new().unwrap();
    let mut f = std::fs::File::create(file.path()).unwrap();
    f.write_all(MAGIC).unwrap();
    f.write_all(&FORMAT_VERSION.to_le_bytes()).unwrap();
    f.write_all(&[0xff, 0xff]).unwrap(); // invalid msgpack
    drop(f);

    let err = read_interface(file.path()).unwrap_err();
    assert!(err.to_string().contains("failed to deserialize"), "{}", err);
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test --test interface`
Expected: all tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/interface.rs
git commit -m "Add validation tests for .bengalmod error cases"
```

---

### Task 8: Multi-module package round-trip test

**Files:**
- Modify: `tests/interface.rs`

- [ ] **Step 1: Add multi-module round-trip test**

This test creates a package with Bengal.toml and multiple modules on disk, then runs the full pipeline through `pipeline::parse` (file-based):

```rust
use bengal::pipeline;
use tempfile::TempDir;

#[test]
fn round_trip_multi_module_package() {
    // Create package on disk
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("Bengal.toml"),
        "[package]\nname = \"mypkg\"\nentry = \"main.bengal\"",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.bengal"),
        "module math;\nimport math::add;\nfunc main() -> Int32 { return add(1, 2); }",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("math.bengal"),
        "public func add(a: Int32, b: Int32) -> Int32 { return a + b; }",
    )
    .unwrap();

    // Run pipeline through optimize
    let parsed = pipeline::parse(&dir.path().join("main.bengal")).unwrap();
    let analyzed = pipeline::analyze(parsed).unwrap();
    let lowered = pipeline::lower(analyzed).unwrap();
    let optimized = pipeline::optimize(lowered);

    // Round-trip
    let interface_file = dir.path().join("mypkg.bengalmod");
    write_interface(&optimized, &interface_file).unwrap();
    let loaded = read_interface(&interface_file).unwrap();

    assert_eq!(loaded.package_name, "mypkg");
    assert_eq!(loaded.modules.len(), optimized.modules.len());
    for (path, module) in &optimized.modules {
        let loaded_bir = loaded.modules.get(path).unwrap_or_else(|| panic!("missing module {}", path));
        assert_eq!(&module.bir, loaded_bir);
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --test interface round_trip_multi_module`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/interface.rs
git commit -m "Add multi-module package round-trip test"
```

---

### Task 9: Final verification

**Files:** (none — verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass, no regressions

- [ ] **Step 2: Run clippy**

Run: `cargo clippy`
Expected: no warnings

- [ ] **Step 3: Run fmt**

Run: `cargo fmt`
Expected: no changes (or apply formatting)

- [ ] **Step 4: Final commit if fmt changed anything**

```bash
git add -A
git commit -m "Apply cargo fmt"
```
