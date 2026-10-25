# Pipeline runbook (card 0b-7)

The steps Claude Code follows for every task, on the GitHub Free plan. This page is the dummy task of
card 0b-7: its merge is the end-to-end proof that the pipeline of phase 0b works. Canonical rules
stay in `wiki/workflow/delivery-pipeline.md` and `board-management.md`.

1. **Branch** `agent/<task-id>-<slug>` from `main`; card to In Progress.
2. **Work** test-first, then the review phase (R1-R8); report in `reviews/<task-id>.md`.
3. **PR** with a Conventional Commit title (`type(scope)!: summary`); the `pr-title` workflow checks it.
   Push runs `ci`. Card to Review, then Waiting for owner approval.
4. **Hash**: `python tools/diff-hash/diff_hash.py compute --base origin/main --head origin/<branch>`.
   Give the owner the full PR URL and the hash.
5. **Approval**: only the owner's explicit words in the session. After it, append the
   `## Owner approval` section to the PR description and run `diff_hash.py check`.
6. **CI**: `gh run list --commit <sha>`, then `gh run watch <id> --exit-status`. No green run, no merge.
7. **Merge** (policy, not enforcement): `gh pr merge N --squash --subject "<PR title> (#N)" --body "" --match-head-commit <sha>`; the sha is the head that was
   approved, checked and watched in CI, so a later push cannot slip in.
8. **Release**: `release.yml` computes the version from the squash commit and publishes a private
   Release. Check it with `gh release list` and move the card PR / CI, then Done.

## Controls: enforced and policy only (state after phase 0b, review in `reviews/0b-7.md`)

| Control | Status |
|---|---|
| `ci` on every push (fmt, clippy, build, test, digest check, all tool tests; the report-only metrics job skips `main`) | enforced by Actions, but not required for merging; on `main` it is a signal of a broken merge result and does not gate the release |
| `pr-title` check | enforced by Actions, not required |
| Squash subject equals the PR title | enforced in `release.yml` before anything is built (`check_squash_subject.py`); a mismatch blocks that release, the next merge releases both |
| Never push to `main`; merge only after approval | policy only (the token could push; no branch protection on Free) |
| Owner approval and its record in the PR description | policy only; `diff_hash.py check` is run by the agent |
| Merged head equals the approved, checked head | policy, helped by `--match-head-commit` |
| Gate tools run from a trusted `main` checkout | built for `pr-title` and `governance-guard`; `ci` still runs its tool tests from the PR tree (phase 0c) |
| `governance-guard` (protected paths in `gates/protected-paths.txt` need `Governance task: yes (card <id>)` in the PR description) | enforced by Actions from the base commit, not required; traceable, not unforgeable |
| `release.yml` (serialised, skips tagged commits, draft then publish, `contents: write` only) | enforced by Actions |
| Stray tags pushed with the agent token | a tag that is not an ancestor of `main` is ignored; one that takes the next version's name fails the release with a clear error until deleted |
| Card to Done only after the Release exists | policy: check `gh release list` first |
