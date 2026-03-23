---
name: design-phase
description: Read a specific phase from Plan.md, design detailed implementation tasks, and output to Task.md
argument-hint: <phase-number>
disable-model-invocation: true
---

# Instructions

Design a detailed implementation plan for Phase $ARGUMENTS from Plan.md, and output the result to Task.md.

## Steps

### 1. Read Plan.md

Read Plan.md and locate the section for Phase $ARGUMENTS.
If the phase is not found, report an error and stop.

### 2. Investigate the codebase

Read source code related to the phase content to understand the current implementation state.
Check the following:

- Current contents of files mentioned as modification targets in the phase
- Related type definitions, function signatures, and module structure
- Existing test structure (if the phase involves tests)
- Dependencies (Cargo.toml, etc.)

### 3. Design detailed tasks

Based on the Plan.md description, break down the work into **actionable units**.
Use the following granularity guidelines:

- 1 task = 1 logical unit of change (modifying 1 file, implementing 1 function, etc.)
- Explicitly state ordering when there are dependencies between tasks
- Include concrete implementation details for each task (files to change, functions to add, code locations to modify)

### 4. Output to Task.md

**Overwrite** Task.md with the following format.

```markdown
# Phase N: <phase title> — implementation plan

## overview

<Explain the purpose of the phase in 2-3 sentences>

---

## task list

### 1. <task name>

- [ ] <concrete subtask>
- [ ] <concrete subtask>

### 2. <task name>

- [ ] <concrete subtask>
- [ ] <concrete subtask>

...

### N. final checks

- [ ] `cargo fmt`
- [ ] `cargo clippy` — no warnings
- [ ] `cargo test` — all tests pass (existing + new)
- [ ] commit
```

## Important rules

- Reflect design decisions and caveats documented in Plan.md in each task
- Supplement implementation details not covered in Plan.md based on codebase investigation
- Always include a "final checks" section (fmt, clippy, test, commit) at the end
- Do not deviate from the design intent of Plan.md. If a design decision is questionable, ask the user for clarification
