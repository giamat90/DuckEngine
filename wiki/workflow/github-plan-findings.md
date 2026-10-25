# GitHub plan findings (V1) `[F]`

Checked 2026-10-24 for card 0a-1. Account `giamat90`, GitHub Free, private repository `giamat90/DuckEngine`. Owner decision: stay on the free plan, no upgrade ([tooling-and-ci](tooling-and-ci.md) section on branch protection).

Method: `gh api` probes against the real repository, then GitHub docs for what a probe cannot show. "Probed" = observed on this repo; "docs" = from GitHub documentation, not observed.

## 1. Findings

| Feature | Free, private repo | Evidence |
|---|---|---|
| Branch protection (required checks, no direct push, admins included) | **Not available** | probed: HTTP 403 "Upgrade to GitHub Pro or make this repository public" |
| Rulesets | **Not available** | probed: same 403 on the rulesets endpoint |
| Required status checks | **Not enforceable** (they are a protection/ruleset setting) | follows from the two rows above |
| Auto-merge | **Not available** | probed: setting `allow_auto_merge=true` is accepted by the API but reads back `false`; restored to `false` afterwards |
| Merge queue | **Not available** (needs branch protection or a ruleset) | docs: enabled through protection or ruleset; not tested directly |
| Environments, environment secrets, deployment protection rules | **Not available** | docs: Free can configure environments for public repositories only; probed: environments list is empty |
| Repository-level Actions secrets | **Available** | probed: the secrets endpoint answers (0 secrets) |
| GitHub Actions | **Enabled**, all actions allowed | probed |
| Actions minutes | **2,000 per month**, artifact storage 500 MB, cache 10 GB per repository; without a payment method, runs are blocked when the quota is used | docs |
| Runner cost | Linux 2-core x64 $0.006/min, Windows 2-core $0.010/min (about 1.67x), Linux arm64 $0.005/min, macOS $0.062/min (about 10x). Treat Windows as 2x for budgeting | docs (rates; ratios computed) |
| Fine-grained token scope names | **Verified** (2026-10-25, card 0b-1) | probed: [agent-token-setup](../../docs/gates/agent-token-setup.md), `tools/token-probe`. Contents rw, Pull requests rw, Actions read, Metadata read; Administration, Secrets and Workflows are refused. `gh pr checks` does not work (check runs need a Checks permission a personal fine-grained token does not offer); `gh run watch --exit-status` does |
| arm64 hosted runner availability on Free private repos | **Not verified** | needs a test run (phase 2) |

Not checked: the exact plan name and billing page (the API needs the `user` scope; the owner stays on Free anyway).

## 2. What this changes in the design

- Merging is **policy, not enforcement**. Claude Code merges only after `gh run watch --exit-status` is green, and only after the owner's approval against the diff hash. Nothing in GitHub stops a direct push to `main`, so the audit trail (git history, PR list, card comments) is the control.
- The target flow in [delivery-pipeline](delivery-pipeline.md) (`gh pr merge --auto`, merge queue, rulesets, environment protection) is replaced by the fallback: the agent runs `gh pr merge` itself after the checks pass. The session stays open while the checks run.
- Release credentials live as repository-level Actions secrets or OIDC, not as environment secrets. Because there is no environment protection, the release workflow is the only consumer and is a Class 3 file.
- Gate tools should run from a trusted checkout of `main` ([rules-digest](../rules-digest.md) section 8): built for `pr-title`, `governance-guard` and the `ci` `gates` job (arch-check still runs cargo inside the push, so it honours the push's toolchain file and cargo config, both protected paths); only the tools' own unit tests run from the PR tree in `ci`. With no protection on `main`, the rest rests on the policy above.

## 3. Budget (gate G5)

2,000 minutes per month, Linux 1x, Windows 2x (conservative). One PR run of 15 minutes wall on Linux plus 15 on Windows bills about 45 minutes. At 20 PRs per month that is about 900 minutes, inside the allowance with room for reruns. Heavy jobs (mutation on full runs, ARM64, long benchmarks) do not fit on top of that every PR: keep them nightly or on a self-hosted runner, as [tooling-and-ci](tooling-and-ci.md) already says. This is an estimate: billed minutes round up per job.

## 4. Options (owner decides; current decision: stay on Free)

1. **Stay on Free** (current). Policy-only merging, no auto-merge, minutes budgeted as above.
2. **GitHub Pro** (paid, personal account): branch protection, 3,000 minutes, environments on private repos per the docs. Whether auto-merge and rulesets then work on a personal private repo, and whether merge queue is possible at all (it may need an organisation), is not verified here.
3. **Public repository**: unlocks everything, rejected by the owner (private, proprietary).
