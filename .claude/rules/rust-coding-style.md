---
paths:
  - "**/*.rs"
---

- `cargo fmt` runs automatically via a PostToolUse hook when `.rs` files are modified.
- After changing `.rs` files, run `cargo clippy` and fix all warnings before considering the task complete.
- When fixing a clippy warning requires disabling it (e.g., `#[allow(...)]`), always ask the user for approval first — do not silently suppress warnings.
