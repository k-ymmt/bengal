---
name: design-plan
description: Investigate the codebase and write a detailed, phased implementation plan to Plan.md for the given task. Use when the user wants to plan a new feature, refactoring, migration, or any non-trivial implementation work.
argument-hint: <what to implement>
disable-model-invocation: true
---

# Instructions

Write a detailed implementation plan for the following task and output it to Plan.md:

**Task:** $ARGUMENTS

## Steps

### 1. Understand the task

Parse the user's request and identify:

- What is being asked (new feature, refactoring, bug fix, migration, etc.)
- What the expected outcome is
- Any constraints or requirements mentioned

If the request is ambiguous or lacks critical details, ask the user for clarification before proceeding.

### 2. Investigate the codebase

Thoroughly explore the codebase to understand the current state. Focus on:

- **Architecture**: module structure, key abstractions, data flow
- **Relevant files**: files that will need to be changed or that are related to the task
- **Type definitions and interfaces**: public APIs, trait definitions, struct layouts
- **Existing tests**: test structure and coverage relevant to the task
- **Dependencies**: Cargo.toml, external crates involved
- **Recent git history**: recent commits related to the area being changed (`git log --oneline -20`)

Use the Explore agent or direct file reads as needed. Be thorough — a plan based on incorrect assumptions about the codebase is worse than no plan.

### 3. Analyze impact

Determine:

- **Files to change**: list every file that will be modified, created, or deleted
- **Files NOT changed**: explicitly note important files that remain untouched (helps prevent scope creep)
- **Breaking changes**: any public API changes, behavioral changes, or compatibility concerns
- **Risks**: what could go wrong, what edge cases exist, what assumptions are being made

### 4. Design phased implementation

Break the work into **phases** — each phase should be a logical, independently testable unit of work.

Guidelines for phasing:

- Each phase should leave the project in a compilable and testable state
- Order phases so that later phases build on earlier ones
- Within each phase, provide **specific, actionable sub-steps** with:
  - Exact file paths to modify
  - Code snippets showing before/after changes (when helpful for clarity)
  - Concrete implementation details (function signatures, type mappings, algorithms)
- Note dependencies between phases explicitly
- Include design decisions and their rationale (why this approach over alternatives)

### 5. Write Plan.md

```markdown
# Plan: <concise title>

## Overview

<2-4 sentences explaining what is being done and why>

## Current State

<Describe the relevant current architecture/state — diagrams using ASCII art are encouraged>

## Changed

<Describe the target state after implementation>

## Impact
- **Files to modify**: <list>
- **Files to create**: <list, if any>
- **Files to delete**: <list, if any>
- **Unchanged**: <important files explicitly excluded>

### Breaking Public API Changes

<Table or list of breaking changes, if any. "none" if none>

---

## Phase 1: <phase title>

### 1.1 <sub-step title>

<Detailed description with code snippets where helpful>

### 1.2 <sub-step title>

...

---

## Phase 2: <phase title>

...

---

## Phase N: Definition of Done

- `cargo fmt`
- `cargo clippy` — No warning
- `cargo test` — Pass all tests
``` 
Adjust the number of phases and sub-steps to match the complexity of the task. Simple tasks may need only 1-2 phases; complex migrations may need 6+.

### 6. Review the plan

After writing Plan.md, review it yourself:

- Are all referenced files and functions correct and currently existing in the codebase?
- Is the phasing order logical — does each phase build on the previous?
- Are there missing steps or unstated assumptions?
- Could someone follow this plan and implement it without further clarification?

Fix any issues found during review.

## Important rules

- Always investigate the codebase before writing the plan — never write a plan based on assumptions alone
- Include concrete code-level details (file paths, function signatures, type changes) — not vague descriptions
- Show before/after code snippets for non-trivial changes
- Each phase must leave the project in a compilable state
- Note design decisions and their rationale — explain *why*, not just *what*
- If the task is too vague to plan, ask for clarification instead of guessing
- If Plan.md already exists, overwrite it entirely — each plan is self-contained
