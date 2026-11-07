# Review report: <task-id> (<short title>)

Copy this to `reviews/<task-id>.md`. Source: wiki/workflow/clean-code-and-review.md section 4.3. The owner reads the verdict and "Not verified"; the rest is evidence.

Task: <task-id>
SHA reviewed: <sha>
Class: 1 | 2 | 3
Classifier verdict: <class and reason, or "classifier not available yet" and who judged>
Loops: <n> (maximum 3; after 3 stop and report)
Author: <role, for example implementer; no link or identifier of the run>
Reviewer: <role, a separate read-only reviewer from the author; no link or identifier of the run>

## Summary verdict

ready | not ready. One paragraph: the result and anything the owner must decide. "ready" requires 0 open blocker/major.

## Checklist

Each item: pass | fail | n/a, with evidence.

- Measurable rules: function size, complexity, nesting, parameters, duplication, errors, magic numbers, dead code, unsafe.
- TDD: test list, red evidence, test-first commits, tests clean, no weakened or skipped tests.
- Architecture: layers, ports, banned imports, ADRs, public API delta, design note.
- Security: input handling, memory safety, dependencies and licences, secrets.
- Performance: benchmark delta, allocations per frame, `PERF:` waivers (each with a benchmark).
- Product fit: acceptance criteria met, no scope creep.

## Findings

Severity: blocker (must fix), major (must fix), minor (fix, or list with a reason). Status: fixed | accepted with reason | open.

| id | severity | location | description | status |
|---|---|---|---|---|
| 1 | | | | |

## Not verified

What could not be checked, and why. Write "nothing" only if it is true.

## Metrics snapshot

Mutation score, coverage, complexity top-5, duplication. Write "n/a" with a reason where a tool does not apply.
