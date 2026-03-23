---
name: review-task-md
description: Review Task.md implementation plans, engineering task breakdowns, and milestone checklists for correctness, sequencing, feasibility, missing prerequisites, and validation gaps. Use when Codex is asked to review a Task.md, phased implementation plan, execution checklist, or similar Markdown task specification before coding starts or before approving the plan.
---

# Review Task.md

## Overview

Review `Task.md` as an execution plan, not as prose. Find concrete risks that would cause incorrect implementation, wasted effort, hidden regressions, or weak validation if the plan were followed literally.

Read the full `Task.md` first, then inspect the referenced code, modules, tests, and surrounding architecture before judging the plan. Use [`references/review-checklist.md`](references/review-checklist.md) as the detailed rubric.

## Workflow

### 1. Build execution context

Read the entire `Task.md` from top to bottom.

Identify:

- the intended user-visible behavior
- the files, modules, tests, and interfaces expected to change
- the phases or explicit deferrals
- the assumptions the plan makes about current code structure

Open the referenced files and verify that the plan matches the codebase as it exists now. If the document is written in Japanese or another language, preserve the original technical meaning and review the plan in English without forcing a translation rewrite.

### 2. Stress-test the plan

Evaluate the plan against the checklist in [`references/review-checklist.md`](references/review-checklist.md).

Prioritize issues that would materially affect implementation:

- impossible or incorrect assumptions about existing APIs or data flow
- missing prerequisite tasks
- invalid sequencing between steps
- missing behavior branches or edge cases
- incomplete test coverage for the new behavior
- rollout, migration, or verification gaps

Treat explicit phase boundaries as intentional unless the document contradicts itself. Distinguish "deferred to a later phase" from "forgotten."

### 3. Produce findings

Report findings first, ordered by severity.

For each finding:

- cite the relevant `Task.md` section or checkbox item
- explain the implementation risk or likely failure mode
- reference supporting code or tests when available
- suggest the narrowest correction that makes the plan executable

Focus on real engineering issues, not stylistic preferences. Avoid rewriting the entire plan unless the structure itself is the problem.

### 4. Close with residual risk

If no concrete findings remain, say so explicitly.

Then list any residual risks, open assumptions, or validation work that still depends on implementation details not settled by the plan.

## Output Shape

Use a code-review style response.

Start with `Findings` and list only concrete issues. After findings, optionally add `Open Questions` or `Residual Risks` if they help the author tighten the plan.

Keep the output specific:

- prefer "Step 6 stores Unit args into missing allocas and will panic" over "Unit handling may need more detail"
- prefer "the plan adds tests for happy paths but not divergent control-flow cases" over "tests could be improved"

## Guardrails

- Do not assume the plan is correct just because it is detailed.
- Do not flag intentionally deferred work as missing unless the current phase depends on it.
- Do not review only the Markdown text; inspect the codebase that the plan targets.
- Do not praise the document before checking for behavioral gaps.
- Do not spend time on wording polish when correctness or sequencing problems remain.
