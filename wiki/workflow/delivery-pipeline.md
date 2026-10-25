# Delivery Pipeline: from task to deployed version `[P]` (v0.4, simplified per owner)

## The flow, in four steps (owner definition)

1. **PO defines the task.** (Product owner = you.)
2. **CC does the work of a software team.** (Claude Code acting as PM, architect, developer, QA, security reviewer, DevOps and reviewer.) It implements in **clean-code, test-first (TDD) style**, and when the implementation is done a mandatory **review phase** runs before anything reaches the PO ([clean-code-and-review](clean-code-and-review.md)).
3. **PO approves the work**, based on the **metric report and the product view**. This is the **only** human approval.
4. **CC deploys through a PR on GitHub**: opens the PR, waits for green CI, merges, and the release workflow tags a new version and publishes the versioned SDK artifacts. All automatic; no further clicks, approvals or reviews.

Everything below only makes those four steps safe and repeatable. Feature names and plan-dependent limits marked **[K]** come from background knowledge; verify during phase 0.

**Plan constraint (verified 2026-10-24, [github-plan-findings](github-plan-findings.md)):** the repository is private on the GitHub **Free** plan, which has no branch protection, rulesets, auto-merge, merge queue or environments. Wherever this page says auto-merge, ruleset, required check enforcement or protected environment, read the fallback in section 5: Claude Code runs `gh pr merge` itself after the owner's approval and a green `gh run watch <run-id> --exit-status`, and merging is policy, not enforcement.

**Built in phase 0b (verified end to end in card 0b-7):** PR title check (`pr-title`), diff hash and approval record (`tools/diff-hash`, run by the agent, not by a workflow), squash merge by the agent after a green `gh run watch`, release workflow (`release.yml`: draft Release, assets, publish). **Not built:** auto-merge, rulesets, required checks, `governance-guard`, a CI run on `main`, the package smoke test and per-platform tool/sample archives. Control by control, see [pipeline-runbook](../../docs/gates/pipeline-runbook.md).

## 1. Task lifecycle

```mermaid
flowchart TD
  T[1. PO writes task card: behaviour, acceptance basis, constraints] --> W[2. CC works as a software team: clean-code TDD, local gates]
  W --> L{local gates green?}
  L -- no --> W
  L -- yes --> RV[REVIEW PHASE: self, automated, independent, QA, architecture, security, performance, product fit]
  RV --> F{open blocker or major findings?}
  F -- yes --> W
  F -- no --> B[push branch agent/task-id; CI G1 runs on the branch]
  B --> C{CI green?}
  C -- no --> W
  C -- yes --> O[CC opens the PR, computes the diff hash]
  O --> R[Acceptance package: PR link, hash, metric report + product view]
  R --> A{3. PO approves?}
  A -- changes requested --> W
  A -- approved --> P[4. CC records the approval in the PR description, waits for green CI]
  P --> G[CC squash-merges the PR; Free plan: policy, not enforcement]
  G --> V[Release workflow: version, tag, GitHub Release with SDK artifacts]
  V --> N[CC reports result to PO; baselines updated on main]
```

| # | Stage | Who | Exit condition |
|---|---|---|---|
| 1 | **Task card** (module, layer, given/when/then, **acceptance basis**, perf expectation, allowed files; `governance task: yes/no`) | PO (CC can draft it with `/task`) | card given to the session |
| 2 | **Software-team work**: plan, design, **clean-code test-first implementation** (TDD style), design note | Claude Code (roles below) | implementation complete, local gates green |
| 3 | **REVIEW PHASE** (R1-R8: self, automated clean-up, independent, QA, architecture, security, performance, product fit) with a fix loop | Claude Code reviewer roles, separate sessions ([clean-code-and-review §4](clean-code-and-review.md)) | review report `reviews/<task-id>.md` with **0 open blocker/major** (max 3 loops, else stop and report to PO) |
| 4 | **Push** branch `agent/<task-id>`; **CI G1 on the branch** | CC / GitHub Actions | G1 green on this SHA |
| 5 | **Acceptance package** (includes the review report) | CC | delivered to PO |
| 6 | **PO approval** (in the session, a plain "approved"); the PR is already open and its full link and hash were given with the package | PO | approval recorded against this **diff hash** (content hash of the proposed change against `main`) |
| 7 | **Record and merge**: approval section in the PR description, `diff_hash.py check`, `gh run watch --exit-status`, `gh pr merge N --squash --subject "<PR title> (#N)" --body "" --match-head-commit <sha>` | CC | on `main` (Free plan: no auto-merge) |
| 8 | **Verify the merge** the head that was approved and checked is the one merged | CC | merge commit contains exactly the approved head |
| 9 | **Release**: version, tag, GitHub Release, SDK artifacts | GitHub Actions | Release exists |
| 10 | **Report** | CC | PO told the version, the Release link and the metric delta |

### Claude Code as a software team (stage 2)

Claude Code's role set follows the `software-team` skill (PM, architect, developer, QA, security, DevOps, reviewer). Mapping to this project (details in [tooling-and-ci §7](tooling-and-ci.md)):

| Role | Responsibility in this project |
|---|---|
| PM | turns the task card into a plan and the acceptance package; keeps scope to the card |
| Architect | enforces layers, ports and ADRs; writes/updates the design note |
| Developer | implements test-first inside the rules in [tdd-workflow](tdd-workflow.md) |
| QA | independent tests (test independence), property/golden/contract tests, mutation score |
| Security | dependency/licence check, memory-safety and sanitizer findings, secrets hygiene |
| DevOps | CI, release workflow, build matrix |
| Reviewer | independent review of the diff, classifier verdict, "what I could NOT verify" |

### Acceptance basis (set in the task card)

| Basis | When | What the PO looks at |
|---|---|---|
| **Product view** | anything product-visible (rendering, audio, physics feel, samples) | run the "try it" command, look/listen/play, check the product view |
| **Metrics** | internal refactors, performance, tooling | metric report delta (gates, mutation score, benchmarks, architecture) |
| **Both** | default when unsure; always for phase-exit tasks | both |

### Acceptance package (stage 5)

Formats are in [architect-review-surface](architect-review-surface.md):

1. Task card and the **commit SHA** being proposed.
2. Gate summary: local run + CI G1 on that SHA.
3. **Metric report delta** versus `main` (generated by `tools/metrics`).
4. **Product view** with the one-command "try it".
5. **Design note** if a module/port changed; for Class 2/3 changes also the diff summary with the "what I could NOT verify" line, since those diffs are part of what the PO reads before approving.
6. **Review report** (`reviews/<task-id>.md`): summary verdict, "Not verified" section, loop count, and the classifier's verdict. You read the verdict and the "Not verified" list, not the line-level findings.

## 2. Approval and what happens after it

- **Approval is the PO's explicit "approved" in the session**, tied to the **diff hash** of the package (N-7). Mechanical changes that leave the diff hash unchanged (rebase onto a newer `main` without conflicts, formatting that the formatter already applied) keep the approval. There is no second approval anywhere: no click on a permission prompt, no GitHub review, no CODEOWNERS approval.
- Claude Code writes the approval into the PR description (task id, diff hash, basis, the PO's words) as an audit trail. `diff_hash.py check` verifies that the recorded diff hash equals the PR's diff hash; the agent runs it before merging (on the Free plan it is not a required check, see [diff-hash-and-approval-check](../../docs/gates/diff-hash-and-approval-check.md)).
- **After approval everything is automatic**: PR, wait for CI, merge, tag, publish, report.
- **The one case where the flow can pause:** if, after approval, a failing check forces a change that alters product-visible behaviour or moves metrics outside tolerance (decided by the classifier, not by the author), Claude Code stops and asks again, because the PO approved one result and the change would ship another. Mechanical fixes (formatting, flaky-test retry, conflict-free rebase) continue automatically. This is the only exception.

## 3. Governance changes

Gate configuration, thresholds, the classifier, contract-suite semantics, CI/release workflows, ADRs and `THIRD_PARTY.md` structure are protected paths. They change only in a task the PO **declares as a governance task** in the card; for that task the protection is unlocked, and the governance diff is part of the acceptance package the PO reads. A `governance-guard` check (planned, **not built yet**; to run from the trusted base, N-2: gate tools and configuration are read from `main`, never from the PR tree) would fail any PR that touches protected paths without that declaration (traceable, not unforgeable - an accepted residual risk). Until it exists the declaration is policy only, and the CI steps that run from the PR tree are not a trusted-base control.

## 4. Credentials and blast radius (recommended, one-time, no per-task steps)

No approvals are involved, so this is purely about limiting damage if a session goes wrong (a bug, or a hostile instruction hidden in a file it reads):

| Setup | Effect |
|---|---|
| **Fine-grained token for Claude Code, scoped to this repository only**: contents (read/write on branches), pull requests (read/write), actions (read). **No** administration, no workflow-file editing, no secrets/environments access (scope names verified 2026-10-25, see [agent-token-setup](../../docs/gates/agent-token-setup.md)) | agent can push `agent/*`, open PRs, arm auto-merge, read CI logs; cannot change rulesets, edit CI workflows, or read secrets |
| Rulesets on `main` that also bind admins: PR required, required status checks, no direct push | nobody, including the PO's own token, can push straight to `main` by accident |
| Release/publish credentials exist only as GitHub Actions secrets or OIDC, in a protected environment | the agent never holds them |

A separate GitHub account or a GitHub App is optional (better attribution and isolation), but not needed for this flow: a scoped token under the PO's own account gives most of the benefit.

## 5. GitHub features doing the work

| Need | GitHub feature **[K]** |
|---|---|
| Nobody pushes to `main` | repository **ruleset** or branch protection: PR required, required status checks (G1), admins included |
| "Wait for green CI, then merge" | **auto-merge** (repo setting + `gh pr merge --auto --squash`); optional **merge queue** so the tested commit is what merges. The session does not have to stay open polling |
| Governance guard | a required check (`governance-guard`) running the classifier |
| Reuse CI results | checks attach to the head SHA; matching names let PR checks reuse the branch-push run, saving private-repo minutes **[K - verify]** |
| Version, tag, Release | workflow on `push` to `main` |
| Publish with guardrails | **Environment** `release` with protection and environment-scoped secrets, **OIDC** for short-lived credentials |
| Serialised releases | `concurrency` group so two merges cannot compute the same version |
| Notify the PO | Claude Code's report (stage 10) plus GitHub notifications |

Rulesets, merge queue, auto-merge and environment protection are **not available** on the Free plan (verified 2026-10-24). Fallbacks (risk R-20), as decided by the owner (stay on Free):

| Need | Fallback on Free |
|---|---|
| Nobody pushes to `main` | policy only: the agent opens a PR from a branch and never pushes to `main` after the bootstrap commit; git history and PR list are the audit trail |
| Wait for green CI, then merge | the session stays open: `gh run watch <run-id> --exit-status` (run id from `gh run list --commit <sha>`; `gh pr checks` does not work with the scoped token), then `gh pr merge N --squash --subject "<PR title> (#N)" --body ""` (no `--auto`; the PR title is the Conventional Commit message that decides the version, checked by the `pr-title` workflow); a failing or missing check means no merge |
| Governance guard as a required check | still runs as a check; it cannot be made required, so the agent treats a red `governance-guard` as a stop |
| Protected environment `release` | repository-level Actions secrets or OIDC; the release workflow is the only consumer and is a Class 3 file |
| Rulesets that bind admins | none; the fine-grained token (section 4) limits what the agent can do |

## 6. Release workflow (every merge to `main` -> new version -> SDK artifacts)

1. **Compute the version** with SemVer from the Conventional Commit types already in use: `feat:` minor, `fix:`/`perf:` patch, `!` or `BREAKING CHANGE:` major; all other types (`refactor:`, `docs:`, `test:`, `chore:`) bump patch, because every merge produces a version (owner). Pre-1.0 (`0.y.z`, owner decision of 2026-10-25, implemented in `tools/release/next_version.py`): `feat` bumps patch and breaking changes bump `y`. Queued merges can share one release (see [release-workflow](../../docs/gates/release-workflow.md)). A port or ABI change counts as breaking. Tooling (release-please, semantic-release, or a small in-repo script) is a dev-time T2 choice for phase 0.
2. **Build the release artifacts (D-1; phase 0b builds only a source archive plus placeholder artifact, report and baselines):** the **tagged, versioned source workspace** (consumed by Cargo through a git tag or a private registry; Rust has no stable ABI, so prebuilt engine libraries are not the delivery unit), prebuilt **tools** (shader/asset tools, arch-check, metrics) and **samples** on the CI matrix (Windows and Linux first), and a **C ABI + headers only for plug-ins or non-Rust consumers**.
3. **Smoke-test the package (not built yet):** build a minimal sample from the tagged workspace as an external Cargo consumer, run it headless with the replay hash check, and run the prebuilt tools.
4. **Tag** `vX.Y.Z` and create the **GitHub Release** (private repo): generated changelog, tool and sample archives per platform, source archive, checksums, the metric report, and `THIRD_PARTY.md`.
5. **Deploy** = these Release assets are the deployment (Q22, owner: deploy means versioned SDK artifacts). No server involved.
6. **Update baselines** (benchmarks, metrics) **outside git** (N-7): stored as workflow artifacts/cache or in a dedicated data branch written only by the release job (ruleset bypass for that job); the workflow never commits to `main`, so rulesets stay strict and no extra release is triggered. **Every merge still produces a version, docs-only merges included (owner decision); a docs-only merge bumps patch.**

Known GitHub behaviour **[K]**: a tag pushed with the default `GITHUB_TOKEN` does not trigger other workflows, so tag, Release and publish run as jobs in the **same** workflow.

## 7. Failure handling

| Situation | Behaviour |
|---|---|
| CI red on the branch | CC fixes and re-runs; the PO is asked only once CI is green |
| CI red after approval | mechanical fix: continue automatically; behaviour/metric-changing fix: pause and ask again (section 2) |
| Merge blocked or checks not green | PR stays open; CC investigates; it never bypasses rules (on Free there is no auto-merge: CC merges by hand after the checks pass) |
| Release job fails | stops, no retry loops, CC reports; fix is a new task |
| Bad version shipped | revert through the same pipeline -> new patch version; mark the bad Release as yanked/pre-release |
| Flaky test | quarantine within one day (quality gate); "re-run until green" is never the policy |

## 8. Open points

- Release cadence: **every merge produces a version** (confirmed by the PO).
- Review phase and coding standard: [clean-code-and-review](clean-code-and-review.md); open point Q24.
- Credentials: confirm the scoped-token setup in section 4 (a one-time setup, not a workflow step).
