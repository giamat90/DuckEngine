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
   then published. The repository is public, so the Release is public: anyone can read the assets listed below, and GitHub adds the source archives of the tag.
6. Before anything is built, `check_squash_subject.py` compares the commit subject with the PR title
   (number from the trailing `(#N)`, title read with `gh api`, so the token also has `pull-requests: read`).
   A mismatch, a missing `(#N)` or a direct push fails the run and publishes nothing; the next merge
   releases both, as for any failed run. This is how a wrong squash subject is caught without branch protection.
7. The workflow never commits or pushes to a branch. It uses only `GITHUB_TOKEN`
   with `contents: write`; no extra secrets.

## What a release contains (public since the repository became public, card 4-26)

Seven assets, pinned by `test_make_assets.py` (`test_writes_exactly_the_expected_files`, with the two extras):
`duckengine-vX.Y.Z-source.tar.gz` (`git archive` of the tree at the merge commit: source, wiki, reviews,
tools), `THIRD_PARTY.md`, `CHANGELOG.md` (commit subjects since the previous tag), `metric-report.md`
and `dummy-artifact.txt` (both placeholders), `baselines.json` (version, commit, no metrics) and
`SHA256SUMS`. Nothing else is published: no binaries, no credentials, nothing that is not already in the
repository. The workflow was not changed by card 4-26; only its wording.

## Why two merges cannot get the same version

- Runs are serialised; each plans after the previous run created its tag.
- A commit that already has a release tag is skipped (re-runs are safe).
- Only tags reachable from `HEAD` count (`git tag --merged HEAD`), so a stray tag on another branch is
  ignored (card 0b-8; before it, such a tag stopped every release). If the next version's name already
  exists as a tag that is not an ancestor, the run fails with a clear error until the tag is deleted.
  A stray tag that *is* an ancestor and has a higher number makes the next version jump from it.
- A leftover draft from a failed attempt is replaced; a published Release is never touched.

Limits: if a run fails, the next run still releases from the last tag, so its version covers both merges.
GitHub also keeps only one *pending* run per concurrency group: when several merges land while a run
is going, only the newest pending run happens and its version covers the skipped ones. So "every merge
bumps a version" (digest section 9) holds per release, not per merge; no version is duplicated and no commit is lost.
Accepted limit (0b-8 review F4): the squash check compares only the newest commit with its PR title; a
`BREAKING CHANGE:` footer in an older, unchecked commit body (after a failed run) can still change the next version.
Always merge with `--body ""`.
Remedy when the squash check fails: it cannot be re-run into success unless the PR title is restored to the
subject; otherwise the next merge releases both. Edit the title back and re-run the failed workflow if the
version must be published now.
Drafts are found through the API (a draft has no tag, so `gh release view TAG` cannot see it).

## PR title check

`pr-title.yml` (event `pull_request`, read-only token) runs `check_pr_title.py` from the **base**
commit, with the title passed through an environment variable. On the PR that introduces the checker
it prints a notice and passes (no checker in the base yet); this is deliberate.
Merging is policy (no branch protection is configured, by the owner's choice), so the check is not enforced by branch protection.

## Governance guard and CI on `main` (card 0b-8)

- `governance-guard.yml` (event `pull_request`, read-only token) checks out the base commit (tool and
  `gates/protected-paths.txt`) and the PR head separately as data, and fails when a protected path changed
  and the description lacks `Governance task: yes (card <id>)`. Like `pr-title` it skips with a notice on
  the PR that introduces it, and it is a check, not a required check.
- Limit: the tool and list come from the base, but GitHub runs the workflow *definition* from the PR, so a PR
  that edits `governance-guard.yml` or `pr-title.yml` can disable its own check. The owner reads any
  `.github/` diff; a real fix needs a paid plan (required workflows). Renames and deletes of protected
  files are caught (`--no-renames`); the declaration needs a card id and is ignored inside code or comments.
- `ci` also runs on pushes to `main`: a broken merge result is visible within minutes. It does not gate the
  release; the report-only `metrics` job is skipped there to save Actions minutes.

## Verification

Unit tests in `tools/release` (run in CI) cover the plan, title check, assets and static workflow
properties (trigger, serialisation, token scope, pinned actions, no expression injection).
The workflow itself can only run after the merge: the first run on `main` is the acceptance check.
