# Task.md Review Checklist

Use this checklist to inspect the plan systematically. Apply only the sections that matter for the document in front of you.

## 1. Scope And Contract

- Does the plan state the intended behavior clearly enough to implement without guessing?
- Does it define what changes now versus what is explicitly deferred?
- Does each task item map to a real artifact such as a file, module, API, test, or CLI behavior?
- Does the plan accidentally change public behavior, exports, file layout, or integration points without calling that out?

## 2. Codebase Alignment

- Do the referenced files, types, modules, and helper functions actually exist?
- Does the proposed implementation strategy fit the current architecture instead of an imagined one?
- Does the plan rely on helpers, data structures, or invariants that are not present yet?
- Does it miss existing abstractions that should be reused instead of bypassed?

## 3. Sequencing And Dependencies

- Are prerequisites scheduled before dependent work?
- Does the plan create new files, exports, or declarations before later steps depend on them?
- Are two-pass or phased operations ordered correctly?
- Does the plan postpone integration work that the current phase secretly requires in order to compile, test, or run?

## 4. Data, State, And Control Flow

- For each step, what values move where, and is that path mechanically valid?
- Does the plan cover all relevant branches, including error paths, divergent control flow, and no-op cases?
- Does it describe how state is initialized, updated, and observed across boundaries?
- If values are skipped or omitted for special cases such as `Unit` or `None`, is every producer and consumer side aligned?

## 5. Failure Modes And Edge Cases

- What breaks if the plan is implemented literally?
- Does the plan handle mismatched types, empty inputs, missing results, or unreachable states?
- Are special cases documented for loops, early returns, conditional branches, or cross-block state transfer?
- Does the plan rely on cached or stale values where reloading is required for correctness?

## 6. Testing Strategy

- Do the tests cover both happy paths and the risky branches introduced by the plan?
- Are regression tests included for previously fragile behavior or known corner cases?
- Does the test plan validate observable outcomes instead of only internal mechanics?
- Are there missing tests for mixed behavior, phase boundaries, or interactions between features?

## 7. Verification And Delivery

- Does the plan include formatting, linting, and test execution steps that match the repository?
- If the change affects integration, does it include runtime verification or smoke coverage?
- Does it mention migration, rollout, or compatibility steps when behavior crosses module or package boundaries?
- Is the final validation strong enough to catch a partially implemented but compiling result?

## 8. Review Questions

Use these prompts when the plan looks plausible but may still be wrong:

- What assumption would fail first if I tried to implement this now?
- Which task item is underspecified enough to produce two different implementations?
- Which missing test would most likely hide a regression?
- Which step depends on a file or API that the plan does not actually create?
- Which deferred item is truly safe to defer, and which one is only labeled as deferred?
