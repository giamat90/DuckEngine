# Phase 0a toolchain gates G1, G2, G5: evidence

Criteria: [ADR-0001 evaluation, section 6](../../wiki/decisions/adr-0001-language-evaluation.md).
Evidence run: CI run 40138018377 on branch `feature/0a-9-toolchain-gates`
(GitHub-hosted `windows-latest` and `ubuntu-latest`, Rust 1.94.1, 2026-10-24).
Subjects are tiny on purpose (walking skeleton); the numbers prove the chain
works, not that it scales. Phase 0c recalibrates with larger code.

## G1 Mutation testing: PASSED

| Criterion | Result |
|---|---|
| `cargo-mutants` finishes within 15 min on hosted Windows and Linux | 7 s on Windows, 3 s on Linux (well-tested set); control set also seconds |
| Score >= 70% on a deliberately well-tested module | 100% (15 of 15 caught) on `skeleton` (`align_up`) and `skeleton-unsafe` (`read_u32_le`, containing an `unsafe` function), both OSes |
| Score < 50% on a deliberately weakly tested control | 7.7% (2 of 26 caught) on `g1-control`, both OSes |

The score discriminates: same tool, same settings, 100% against 7.7%. The
`unsafe` function (`read_u32_le_unchecked`) is mutated like the rest; the
bounds check in its safe wrapper is covered by boundary tests (last valid
offset, one past, overflow).

Limits of this evidence: 15 and 26 mutants is a small sample, and the 15-minute
criterion is far from tested. Miri (clean on changed `unsafe`) is not part of
G1 and was not run: it needs a nightly toolchain, which the repo does not pin.
That belongs to the first real `unsafe` work.

## G2 CI chain: PASSED

Jobs green on both OSes in the evidence run, wall time from first job start to
last job end: 62 s (criterion: <= 10 min).

| Part of the chain | Job | Windows | Linux |
|---|---|---|---|
| rustfmt, Clippy (`-D warnings`), build, test, tool tests, digest check | `g1` | 43 s | 20 s |
| coverage, mutation, benchmark, report | `metrics` | 51 s | 29 s |
| `cargo-deny` (licences, bans, sources, advisories) | `deny` | 28 s | 20 s |

`deny.toml` allows permissive licences only (no GPL/AGPL). With no third-party
crate in the tree, the licence check has nothing to reject yet, so only the
configuration and the pass are evidenced, not a rejection.

## G5 Build cost: PASSED

| Criterion | Result |
|---|---|
| PR pipeline without mutation <= 15 min wall | about 45 s (`g1` and `deny` run in parallel; `metrics` adds mutation of a few seconds) |
| Projected monthly Actions minutes at 20 PRs/month fit the plan | about 540 of 2,000 included minutes (27%) |

Projection: GitHub rounds each job up to a whole minute and bills Windows at 2x.
One CI run is 3 Windows jobs x 1 min x 2 + 3 Linux jobs x 1 min = 9 billed
minutes. CI runs on every push to a non-`main` branch; assuming 3 pushes per PR,
that is 27 minutes per PR and 540 for 20 PRs. Not included: the release
workflow (not built yet), manual `workflow_dispatch` runs, and growth of the
pipeline as real code arrives. Sensitivity: 6 pushes per PR still fits (1,080).
