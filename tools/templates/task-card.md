# <task-id> <short imperative title>

Copy this into the Trello card description (or the task file). Sources: wiki/workflow/tdd-workflow.md section 10, wiki/workflow/delivery-pipeline.md stage 1.

## Module and layer

<module name>, layer L0 | L1 | L2 | L3 | L4. Parent card or phase: <id>.

## Class

1 routine | 2 sensitive | 3 governance (the classifier assigns the final class; highest wins, unsure rounds up).

## Governance task

yes | no. Only "yes" if the owner declared it; it unlocks the protected paths for this task alone.

## Behaviour

- Given <state>, When <action>, Then <observable result>.
- Add one line per example, including edge cases and error paths.

## Tests to write first

The test list, one item per behaviour, in the order they will go red then green.

- [ ] <test 1>

## Performance expectation

n/a | <budget and the benchmark that measures it>. Hot paths: zero heap allocations per frame.

## Files allowed to change

<paths or globs>. Anything outside this list needs a comment on the card first.

## Acceptance basis

Product view | Metrics | Both. Default is Both when unsure; always Both for phase-exit tasks.

- Product view: the "try it" command and what to look for.
- Metrics: the metric report delta the owner reads.

## Out of scope

<what this task deliberately does not do>

## Done

Done = gates green on the PR SHA, review report `reviews/<task-id>.md` with 0 open blocker/major, owner approval against the diff hash, PR merged.

## Links

<wiki pages, ADRs, related cards>
