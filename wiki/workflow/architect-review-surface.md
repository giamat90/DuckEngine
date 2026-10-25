# The Architect's Review Surface `[P]`

Decision (Q21, owner): the product owner / architect reads **only**:

1. **Design notes**
2. **Metric reports**
3. **Summaries of Class 2 changes** (design note, diff summary, benchmark delta) and the **governance diffs of Class 3** (gate configuration, thresholds and workflow files, which are configuration, not Rust). You never read Rust code (D-2).

Everything else (Class 1 routine code and tests) is never read line by line. That is only safe if those three artefacts are *good*, and if the Class 1 / 2 / 3 split cannot be gamed. This page specifies both. Class definitions: [tdd-workflow §10](tdd-workflow.md).

## 1. Classification is done by tools, not by the author

An AI deciding that its own change is "routine" is a conflict of interest. A script in CI (`tools/classify-change`, itself a Class 3 file) assigns the class from the diff, and the **highest** class wins:

| Trigger in the diff | Class |
|---|---|
| Only files under a module's `src/` and `tests/`, no public header changes, none of the triggers below | 1 |
| Any change to public API (`pub` items of a module or port; detected by an API-diff tool such as cargo-public-api / cargo-semver-checks `[K]`) | 2 |
| New or changed entry in `THIRD_PARTY.md`, new vendored directory, new external crate or `use` outside the allowlist, new `Cargo.toml` dependency or build script | 2 |
| Files in concurrency / memory / SIMD areas: `jobs/`, `base/alloc*`, `base/simd*`, `base/math*` (determinism), any `unsafe` block, atomics, intrinsics, lock-free structures (pattern scan) | 2 |
| Benchmark baseline change, or a hot-path file where the benchmark delta exceeds the noise band | 2 |
| New adapter, or any change in `adapters/` that alters behaviour exercised by a contract suite | 2 |
| Contract-suite test semantics (`contracts/`), architecture checker, `tools/classify-change`, quality thresholds/gate config, CI workflows, ADRs, `THIRD_PARTY.md` structure, the agent instruction file, this wiki's rules pages | 3 |

Merge rules (updated by the owner's delivery decision, see [delivery-pipeline](delivery-pipeline.md)): **every task, in every class, needs the owner's acceptance (manual product test and/or metrics) before the merge**; the agent opens the PR first and gives you its link and diff hash, then squash-merges after your approval and green checks (Free plan: no auto-merge). There is **one** approval, yours. For Class 2 you read the design note + diff summary + benchmark delta; for Class 3 you read the governance diff (configuration). **Class 2 additionally requires an independent review (R3 + R6 + R7) by a different session or model than the author (D-2).** No separate GitHub approval step exists. Class 3 changes happen only in tasks you declare as governance tasks.

If the classifier is unsure, it rounds **up**. Changing the classifier is Class 3.

Status (card 0c-1): `tools/classify-change/classify_change.py` is built (Python, stdlib; usage in its docstring) and its tests run in `ci`. It decides from paths (class 3 = `gates/protected-paths.txt` plus `contracts/`; class 2 = the rows above; class 1 = module `src/`, `tests/`, `benches/`, docs, wiki, reviews) and from the **added lines** of `.rs` files (`unsafe`, atomics, locks and threads, intrinsics, new `pub` items). An unrecognised path is class 2. Also class 3: toolchain and lint configuration files at any depth, and any path that can alias another file on Windows (backslash, `:`, trailing dot or space, `~1`); path matching ignores case. An empty diff, an unknown revision or a malformed protected list is an error (exit 1, no `class=` line): a consumer must treat a non-zero exit as class 3. Not yet decided by the tool: the API-diff row (cargo-public-api; the `pub` scan is a conservative stand-in, and trait methods and enum variants outside a module root `lib.rs`/`mod.rs` are not parsed), the benchmark-noise row (needs benchmark data, supplied by the metrics report in 0c-4), and which wiki pages are "rules pages" (open question for the owner: only `wiki/rules-digest.md` and `wiki/decisions/` are protected today). It runs in the `ci` `gates` job from a trusted checkout of `main` and feeds the approval report (a report, not a pass/fail gate).

## 2. Design note (one per new module, port, or significant redesign)

Required for every Class 2 change that introduces or reshapes a module or port; stored at `wiki/design/<module>.md` (short, 1-2 pages). Template:

```
# <Module / port> design note        Status: draft | accepted
## Purpose and non-goals            (3-5 lines)
## Where it sits                    layer, depends on, depended on by, ports owned
## Public API surface               the handful of types/functions; what is deliberately NOT exposed
## Key data structures and why      layout, ownership, handles, allocation behaviour
## Invariants                       what must always hold (and which test enforces each)
## Concurrency and determinism      threads touching it, locks/atomics, determinism level promised
## Performance expectation          budget, complexity, benchmark names and baseline
## Alternatives considered          2-3, with why not
## Risks and open questions         what could be wrong, what was verified vs assumed
## Test strategy                    which test levels, which oracles, mutation score
## Change summary since last note   (for updates)
```

Rules: written by the implementing agent, **checked by a second agent** against the code (every claimed invariant must name a test; every "verified" claim must cite a command output or official document), and an `assumed` tag marks anything not verified. You read it to decide: accept the design, or ask for changes.

## 3. Metric report (generated per CI run, summarised per phase)

Generated by `tools/metrics`, not written by an AI, so it cannot flatter itself. **Implemented so far (card 0c-4, `approval_report.py`):** the verdict, the change class with the files that set it, the architecture check and the wiki lint, plus two self-checks (the diff hash is the hash of the diff; the checked-out tree is the head); the verdict counts only these rows; the report lists the checks that run in other jobs and ends with a "Not in this report" list for everything below that no tool produces yet. Contents (see [quality-metrics](quality-metrics.md) for thresholds):

1. **Verdict table**: each gate pass/fail, with trend arrows versus the last milestone.
2. **Tests**: count by level, runtime, coverage (line/branch) per module, **mutation score per module**, flaky/skip count (must be 0).
3. **Architecture**: layer-violation count, banned-import count, third-party manifest diff, dependency graph image, instability/abstractness per module, cycles.
4. **Performance**: benchmark deltas versus baseline (with noise band), frame-time budget table, allocations per frame, scaling curve.
5. **Robustness**: sanitizer, static-analysis and graphics-validation findings.
6. **Governance**: list of Class 2 / 3 changes since last report, any protected-path change, any waiver (must be 0 without an ADR).
7. **Top-5 risks of the code itself**: highest-complexity functions, lowest-mutation-score modules, biggest files, most-changed files (churn).
8. **Product view** (new, Q21c): evidence of the *product*, not the code - see §7.
9. **Agent audit findings**: findings only (not code) from the automated audit of random Class 1 diffs (§5).

## 4. Class 2 / 3 diff summary (what accompanies the diff you read)

Generated + AI-written, template:

```
Class: 2|3   Why (which trigger fired): ...
Intent in 3 lines
Files changed (grouped), size
Public API delta (added / removed / changed signatures)
New dependencies / external includes: none | list
Tests added first (red evidence link), mutation score of the change
Benchmark delta (hot paths)
What I could NOT verify: ...
Rollback plan: revert commit X
```

The "could NOT verify" line is mandatory; an empty one is itself flagged by the independent reviewer.

## 5. Safety net for the part you do not read

Because Class 1 is never read by a human, we rely on layered automated defences. If any layer shows weakness, the policy tightens:

| Defence | What it catches |
|---|---|
| Red-first evidence + test independence + mutation gate | tautological tests, untested behaviour |
| Architecture and banned-import checks, headless builds | layering and dependency drift |
| Benchmarks with baselines | hidden performance regressions |
| Sanitizers, static analysis, validation layers | memory/UB/race/API misuse |
| Independent AI review on every diff | author blind spots |
| Protected governance paths, classifier | gate weakening, mis-classification |
| **Class 1 audit** - human sample **OFF** (owner reads no routine code, Q21c). Replaced by an **automated audit**: a separate agent reviews N randomly chosen Class 1 diffs per phase against the design notes and gates, and only its *findings* reach the metric report | systematic weaknesses the gates miss; escalation trigger |
| **Escalation rule** | if an audit or a bug shows a Class 1 change that should have been Class 2, add the missing trigger to the classifier (Class 3 change) and re-audit the affected module |

## 6. Open points

- Human audit sample: not adopted (owner does not read code); replaced by the automated agent audit in §5. Every spike result and ADR proposal is evaluated by a second agent before it reaches the owner (D-2).
- The classifier triggers above are a first proposal; they will be tuned with real diffs in phase 0-1 and kept under Class 3 control.
- Agent tooling: **Claude Code** (owner, Q21c). Run mode (**confirmed by owner**): per task, autonomous until the checks pass and the owner accepts; then squash-merge, tag, Release through GitHub CI ([delivery-pipeline](delivery-pipeline.md)). No unattended runs.

## 7. Metrics-only governance: the Goodhart problem, and the product view

Owner stance (Q21c): *"I don't care about the code generated, only its effect on metrics and the product."* That makes the metrics the **contract** between you and the agents. A contract that can be gamed will be gamed by any optimiser rewarded for green numbers: trivial tests that lift coverage, code specialised to the benchmark scene, a loosened tolerance, a quietly excluded file. And some things no metric sees: API quality, long-term maintainability, security, behaviour in untested cases. Defences:

1. **Several independent, hard-to-game metrics together**: mutation score (not only coverage), property-based and differential tests, benchmarks across many scenes, architecture checks.
2. **Holdout acceptance suite specified by you in plain language** (scenarios, expected outputs, extra benchmark scenes) and **written by a separate agent** that is not the implementing one (D-2), kept in protected paths (Class 3). Part of it lives only in CI and is **rotated each phase**, so agents cannot overfit to it.
3. **Metric definitions are protected** (Class 3): thresholds, exclusions, tolerances, benchmark scene lists. A diff that adds an exclusion or widens a tolerance is flagged automatically.
4. **Independent agents** write tests, review diffs and audit samples (separation of duties).
5. **Product evidence in every report** (below), so a green dashboard on a broken product is visible.
6. **Phase-end product review by you**: run the sample, look at and listen to the output.

**Product view contents** (generated, per milestone and in nightly reports):

- Headless **replay result** (state hash matches golden or not).
- **Rendered frames of reference scenes** from every real backend, side by side, plus the diff image versus reference.
- **Audio render summary** of reference sounds (waveform/spectrogram, loudness, clipping).
- **Frame-time and memory charts** against the budget, per reference scene.
- **Exit-criteria checklist** of the current phase with pass/fail per item.
- **"Try it" command**: one command that launches the current sample so you can judge it by playing it.
