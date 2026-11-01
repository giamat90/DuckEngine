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
| Fine-grained token scope names | **Verified** (2026-10-25, card 0b-1) | probed: [agent-token-setup](../../docs/gates/agent-token-setup.md), `tools/token-probe`. Contents rw, Pull requests rw, Actions read, Metadata read; Administration and Secrets are refused; Workflows is allowed on purpose (card 0c-8) and not probed. `gh pr checks` does not work (check runs need a Checks permission a personal fine-grained token does not offer); `gh run watch --exit-status` does |
| arm64 hosted runner availability on Free private repos | **Verified** (2026-11-01, card 2-10): the label `ubuntu-24.04-arm` is served for this private repository on the Free plan; a probe job started in 2 seconds and `uname -m` printed `aarch64`. Rate in the docs: Linux arm64 $0.005/min; the run-timing API showed no arm64 line for a 2-second job, so the billing is unmeasured; assume at most Linux 1x (the docs rate is below x64) | probed: card 2-10, ci run 41335316800 |

Not checked: the exact plan name and billing page (the API needs the `user` scope; the owner stays on Free anyway).

## 2. What this changes in the design

- Merging is **policy, not enforcement**. Claude Code merges only after `gh run watch --exit-status` is green, and only after the owner's approval against the diff hash. Nothing in GitHub stops a direct push to `main`, so the audit trail (git history, PR list, card comments) is the control.
- The target flow in [delivery-pipeline](delivery-pipeline.md) (`gh pr merge --auto`, merge queue, rulesets, environment protection) is replaced by the fallback: the agent runs `gh pr merge` itself after the checks pass. The session stays open while the checks run.
- Release credentials live as repository-level Actions secrets or OIDC, not as environment secrets. Because there is no environment protection, the release workflow is the only consumer and is a Class 3 file.
- Gate tools should run from a trusted checkout of `main` ([rules-digest](../rules-digest.md) section 8): built for `pr-title`, `governance-guard` and the `ci` `gates` job (arch-check still runs cargo inside the push, so it honours the push's toolchain file and cargo config, both protected paths); only the tools' own unit tests run from the PR tree in `ci`. With no protection on `main`, the rest rests on the policy above.
- A PR can edit or delete a workflow that runs from its own tree, which switches that check off. The `trusted-pr-gates` workflow (card 0c-14) runs on `pull_request_target`, so GitHub always loads it from the base branch; it treats the PR tree as data only, and from the base it runs the governance guard, a check that every workflow in `gates/required-workflows.txt` is still present, the owner approval record against the diff hash (no record yet is reported as pending, a wrong record fails) and the acceptance validator. The list is read from the base, so removing or renaming an enforcement workflow takes two merges. The workflow serves only PRs into `main`, and the presence check also rejects a hollowed-out file (no trigger or no job). It cannot be made a required check on this plan, so a red `trusted-pr-gates` is a stop by policy. A green run means "no approval record contradicts the diff", not "the owner approved": the agent writes the PR description, so the explicit in-session approval stays the only approval.
- ARM64 queue stall (card 2-10): the job waits for a runner for up to about 24 hours before GitHub cancels it. If GitHub withdraws or throttles the ARM label for private repositories and `arm64` stays queued, cancel that job and report `not run: runner unavailable`, never green; the other jobs still decide. The job proves unit tests and clippy on AArch64 only: no Miri, no mutation run, no adapters, no Windows on ARM, no `--no-default-features` allocator path, and no cross-platform determinism claim (ARM64 is not gated for D2).
- Cross-platform jobs: a test or job that fails only because its device or runner is not set up yet (for example a job on a runner or device that is not set up; the ARM64 job of card 2-10 runs on a hosted runner that is set up, so it counts like any other job) does not count towards CI pass or fail (owner decision, 2026-10-25). It is reported as `not run: device not set up`, never as green. A failure on a device that is set up counts as always.

## 3. Budget (gate G5)

2,000 minutes per month, Linux 1x, Windows 2x (conservative). One PR run of 15 minutes wall on Linux plus 15 on Windows bills about 45 minutes. At 20 PRs per month that is about 900 minutes, inside the allowance with room for reruns. Heavy jobs (mutation on full runs, long benchmarks; the ARM64 job of card 2-10 is a full workspace compile on every `main` push and manual run, counted as Linux 1x) do not fit on top of that every PR: keep them nightly or on a self-hosted runner, as [tooling-and-ci](tooling-and-ci.md) already says. This is an estimate: billed minutes round up per job. Measured later (2026-10-27): one `ci` run bills about 15 minutes and it ran on every branch push, which used most of a month in a few days; card 1-38 moves `ci` to `main` and to manual runs, with work in phase branches ([delivery-pipeline](delivery-pipeline.md)).

## 4. Options (owner decides; current decision: stay on Free)

1. **Stay on Free** (current). Policy-only merging, no auto-merge, minutes budgeted as above.
2. **GitHub Pro** (paid, personal account): branch protection, 3,000 minutes, environments on private repos per the docs. Whether auto-merge and rulesets then work on a personal private repo, and whether merge queue is possible at all (it may need an organisation), is not verified here.
3. **Public repository**: unlocks everything, rejected by the owner (private, proprietary).
