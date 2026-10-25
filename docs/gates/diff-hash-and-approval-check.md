# Diff hash and approval check (card 0b-5)

What the owner approves is a change, not a commit id. The diff hash identifies
the change by its content ([delivery-pipeline section 2](../../wiki/workflow/delivery-pipeline.md)).
The tool is `tools/diff-hash/diff_hash.py` (Python stdlib).

## The hash

The first 16 hex characters of the sha256 of `git diff base...head` run with
pinned options (binary, no renames, no textconv or external diff, Myers
algorithm, no indent heuristic, 3 context lines, fixed `a/` `b/` prefixes, no
user or system git config), after two normalizations: `index` lines (blob ids)
are dropped and each hunk header `@@ -a,b +c,d @@ ...` becomes `@@` (line
numbers and the context label). Lines are split on `\n` only, so a carriage
return or form feed inside the content changes the hash. A revision starting
with `-` is refused, so it cannot be read by git as an option.

Consequences, each covered by a test:

- Commit ids, messages, dates and authors do not matter.
- A rebase onto a newer `main` without conflicts keeps the hash when the files
  and the context lines around the change are untouched. If `main` moved a line
  inside the three context lines of a hunk, the hash changes: the approver saw
  other surrounding code, so the approval is asked again. This is deliberate.
- A mode change, a binary change, or a change in a different file changes it.
- `base...head` is the merge-base form, so new commits on `main` after the branch
  point do not change it.

## The approval record

The agent writes this section in the PR description after the owner's approval
in the session (task id, hash, basis, the owner's words as an audit trail):

```
## Owner approval
Task: 0b-5
Diff hash: 0123456789abcdef
Basis: Both
Approved: "I approve"
```

`Basis` is `Product view`, `Metrics` or `Both`. The section ends at the next
heading. A hash written outside the section does not count, nor one inside a
code fence or an HTML comment, and a description with two such sections is
rejected.

## Use

```
python tools/diff-hash/diff_hash.py compute --base origin/main --head origin/<branch>
python tools/diff-hash/diff_hash.py check   --base origin/main --head origin/<branch> \
       --body-file pr.md --task 0b-5          # pr.md from: gh pr view N --json body -q .body
```

`check` exits 1 and prints one `FAIL` line per problem when the record is
missing, malformed, names another task, records a hash that is not the PR's
current hash, or the diff is empty. It does not print the PR body (only the
problem descriptions, which may quote a short `Basis` or `Task` value).

The check proves that the record matches the change. It cannot prove that the
owner really approved: the `Approved` line is only checked to be present, and
the owner's approval itself stays a session decision (a record is written only
after it).

Run it from a trusted checkout of `main`, never from the PR tree, as the digest
requires (gate tools are executed from `main`).

## Status and limits

- CI runs the tool's own tests (a seeded mismatch fails, a match passes, a
  rebase keeps the hash). Nothing runs `check` automatically on a PR yet: on the
  GitHub Free plan a required check cannot be enforced, and a PR-triggered
  workflow that reads the description needs the trusted-checkout design of
  0b-6/0b-7. Until then the agent runs `check` before every merge, as policy.
- The `tools/templates/task-card.md` and the PR flow are unchanged; the record
  format above is the one to use.
