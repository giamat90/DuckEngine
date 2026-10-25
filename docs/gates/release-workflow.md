# Release workflow and PR title check (card 0b-6)

Governance task (owner-declared). Files: `.github/workflows/release.yml`,
`.github/workflows/pr-title.yml`, `tools/release/*`.

## What a merge does

1. PRs are **squash-merged**: `gh pr merge N --squash --subject "<PR title> (#N)" --body "" --match-head-commit <sha>`.
   The PR title is the Conventional Commit message (`type(scope)!: summary`).
   Always pass `--body ""`: the version tool scans the whole message for a `BREAKING CHANGE:` footer,
   so a PR description or an intermediate commit message must not leak into it. A deliberate
   breaking change uses `!` in the title (or `--body "BREAKING CHANGE: ..."`).
   Earlier PRs (#14-#18) were merged with `--merge`; their commits are not conventional and count as patch.
2. `release.yml` runs on every push to `main`, one run at a time (`concurrency: release`,
   no cancelling). `release_plan.py` takes the highest `vX.Y.Z` tag, reads the commits since it
   (`git log -z --format=%B <tag>..HEAD`) and computes the version with `next_version.py`
   (before 1.0: `feat` patch, breaking change minor, everything else patch).
3. With no tag yet, the first release is `v0.1.0` (the crate version), without a bump.
4. `make_assets.py` builds the source archive, a **placeholder** dummy artifact, a **placeholder**
   metric report, `baselines.json` (outside git, a Release asset) and `SHA256SUMS`;
   the workflow adds `THIRD_PARTY.md` and a changelog.
5. A **draft** Release is created with the assets (the tag is created at the merge commit),
   then published. The repository is private, so the Release is private.
6. The workflow never commits or pushes to a branch. It uses only `GITHUB_TOKEN`
   with `contents: write`; no extra secrets.

## Why two merges cannot get the same version

- Runs are serialised; each plans after the previous run created its tag.
- A commit that already has a release tag is skipped (re-runs are safe).
- A latest tag that is not an ancestor of `HEAD` stops the run with an error.
- A leftover draft from a failed attempt is replaced; a published Release is never touched.

Limits: if a run fails, the next run still releases from the last tag, so its version covers both merges.
GitHub also keeps only one *pending* run per concurrency group: when several merges land while a run
is going, only the newest pending run happens and its version covers the skipped ones. So "every merge
bumps a version" (digest section 9) holds per release, not per merge; no version is duplicated and no commit is lost.
Drafts are found through the API (a draft has no tag, so `gh release view TAG` cannot see it).

## PR title check

`pr-title.yml` (event `pull_request`, read-only token) runs `check_pr_title.py` from the **base**
commit, with the title passed through an environment variable. On the PR that introduces the checker
it prints a notice and passes (no checker in the base yet); this is deliberate.
Merging is policy on the Free plan, so the check is not enforced by branch protection.

## Verification

Unit tests in `tools/release` (run in CI) cover the plan, title check, assets and static workflow
properties (trigger, serialisation, token scope, pinned actions, no expression injection).
The workflow itself can only run after the merge: the first run on `main` is the acceptance check.
