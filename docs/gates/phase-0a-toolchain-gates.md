# Phase 0a toolchain gates G1, G2, G5: evidence

Criteria: [ADR-0001 evaluation, section 6](../../wiki/decisions/adr-0001-language-evaluation.md).
Evidence run: [CI run 40138301722](https://github.com/giamat90/DuckEngine/actions/runs/40138301722),
commit `481b91c0458a869292c32422a6a2602377f11879` on branch
`feature/0a-9-toolchain-gates` (GitHub-hosted `windows-latest` and
`ubuntu-latest`, Rust 1.94.1, 2026-10-24). Run logs and artifacts expire, so
the numbers are copied here.
Subjects are tiny on purpose (walking skeleton); the numbers prove the chain
works, not that it scales. Phase 0c recalibrates with larger code.

## G1 Mutation testing: PASSED

| Criterion | Result |
|---|---|
| `cargo-mutants` finishes within 15 min on hosted Windows and Linux | 9 s on Windows, 3 s on Linux (well-tested set); control set also seconds |
| Score >= 70% on a deliberately well-tested module | 100% (15 of 15 caught) on `skeleton` (`align_up`) and `skeleton-unsafe` (`read_u32_le`, containing an `unsafe` function), both OSes |
| Score < 50% on a deliberately weakly tested control | 7.7% (2 of 26 caught) on `g1-control`, both OSes |

The score discriminates: same tool, same settings, 100% against 7.7%. From now
on the `metrics` job asserts it: `tools/metrics/g1_assert.py` fails the job if
the well-tested set drops below 70%, the control reaches 50%, or the run takes
over 15 minutes (the job otherwise tolerates missed mutants). The
`unsafe` function (`read_u32_le_unchecked`) is mutated like the rest; the
bounds check in its safe wrapper is covered by boundary tests (last valid
offset, one past, overflow).

Limits of this evidence: 15 and 26 mutants is a small sample, and the 15-minute
criterion is far from tested. Miri (clean on changed `unsafe`) is not part of
G1 and was not run: it needs a nightly toolchain, which the repo does not pin.
That belongs to the first real `unsafe` work.

## G2 CI chain: PASSED

Jobs green on both OSes in the evidence run, wall time from first job start to
last job end: 58 s (criterion: <= 10 min).

| Part of the chain | Job | Windows | Linux |
|---|---|---|---|
| rustfmt, Clippy (`-D warnings`), build, test, tool tests, digest check | `g1` | 41 s | 20 s |
| coverage, mutation, benchmark, report | `metrics` | 58 s | 30 s |
| `cargo-deny` (licences, bans, sources, advisories) | `deny` | 35 s | 16 s |

`deny.toml` allows permissive licences only (no GPL/AGPL). With no third-party
crate in the tree, the licence check has nothing to reject yet, so only the
configuration and the pass are evidenced, not a rejection.

## G5 Build cost: PASSED

| Criterion | Result |
|---|---|
| PR pipeline without mutation <= 15 min wall | 58 s for the whole pipeline including the `metrics` job (mutation 3-9 s); all three jobs run in parallel |
| Projected monthly Actions minutes at 20 PRs/month fit the plan | about 540 of 2,000 included minutes (27%) |

Projection: GitHub rounds each job up to a whole minute and bills Windows at 2x.
One CI run is 3 Windows jobs x 1 min x 2 + 3 Linux jobs x 1 min = 9 billed
minutes. CI runs on every push to a non-`main` branch; assuming 3 pushes per PR,
that is 27 minutes per PR and 540 for 20 PRs. Not included: the release
workflow (not built yet), manual `workflow_dispatch` runs, and growth of the
pipeline as real code arrives. Sensitivity: 6 pushes per PR still fits (1,080).

## Not yet enforced

The gates above prove the chain runs. These rules from the digest are **not**
enforced by any tool yet, so a PR can break them while CI stays green:

| Rule | State |
|---|---|
| Coverage, mutation-on-diff and benchmark thresholds (digest section 7) | Reported only. The G1 assertion is the one enforced number. |
| Branch coverage >= 80% | Cannot be measured on the pinned stable toolchain (needs nightly). |
| PR benchmark gate on instruction counts | The bench harness measures wall-clock time; an instruction-count tool comes with the PR gate. |
| Clean-code limits (function length, cyclomatic, cognitive, nesting, parameters, duplication, magic numbers, dead code) | Numbers in `gates/gates.toml`; since card 0b-3 Clippy enforces function length, cognitive complexity, parameters (fail tier) and nesting, see [clean-code-lint-profile](clean-code-lint-profile.md) for what is still unenforced. |
| `unsafe` allowlist and per-module budget | No mechanism. `skeleton-unsafe` is a per-crate `deny` plus one module-level `allow`; `undocumented_unsafe_blocks` is set only in that crate, and nothing stops another crate from defining its own `[lints]`. |
| Miri, zero-allocation counting allocator, `disallowed-methods`, banned imports, 0 skipped tests | Not configured. |
| Gate tools run from a trusted checkout of `main` | Not yet. CI runs `check_digest.py` and `gates.toml` from the PR tree, so one PR can change digest, config and script together. |
| `governance-guard` check | Not built. |
| Test code may use `unwrap`/`expect` | Clippy denies them in tests too; add `allow-unwrap-in-tests` to `clippy.toml` when the first test needs it. |

## Accepted by the owner (not changed here)

- `run_claude.bat` starts Claude Code with `--dangerously-skip-permissions`.
  Deny rules still apply in that mode, but nothing prompts before other commands.
- `.claude/settings.json` allows every `git *` and `gh *` command and has no
  deny rule for push to `main`, force push, `--no-verify` or `gh pr merge`
  (owner decision, 2026-10-24, recorded in
  [tooling-and-ci](../../wiki/workflow/tooling-and-ci.md)). Those rules are
  followed by the agent, not enforced by the tool.
