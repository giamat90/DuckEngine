# TDD Workflow Proposal `[P]`

## 1. The loop

```
 pick next item from backlog (small, testable)
   -> RED:    write the smallest failing test that states the behaviour
   -> GREEN:  write the simplest code that passes
   -> REFACTOR: clean up with tests green (design, duplication, complexity)
   -> BENCH (hot-path code only): add/extend a benchmark, record baseline
   -> commit at each green
 task implementation complete
   -> REVIEW PHASE (mandatory): self, automated clean-up, independent, QA, architecture, security, performance, product fit
   -> fix loop until 0 open blocker/major -> push; CI gates decide -> acceptance package
```

Style rules (clean code, TDD micro-practices, tests as clean code) and the full review phase are specified in [clean-code-and-review](clean-code-and-review.md).

Rules:

1. No production code without a failing test that demands it (exception: spikes, below).
2. Tests are fast (unit suite target: < 10 s locally, < 60 s in CI) so the loop stays tight.
3. A bug fix starts with a failing regression test.
4. Refactoring never changes tests and behaviour at the same time.
5. Test code is held to the same measurable gates as production code (coverage, mutation, lint, no skipped tests).

## 2. Two loops (double-loop TDD)

- **Outer loop (acceptance):** a headless scenario test using Null adapters, e.g. "spawn 1,000 bodies, step 600 frames, state hash equals golden" or "render scene X produces command stream Y". It stays red while the inner loop builds the parts.
- **Inner loop (unit):** the classic red-green-refactor on individual functions/classes.

## 3. Test taxonomy (test pyramid adapted to an engine)

| Level | Scope | Needs hardware? | Runs |
|---|---|---|---|
| Unit | functions/classes in L0-L2 | no | every commit, local + CI |
| Property-based / fuzz | invariants (no NaN, SIMD == scalar, allocator never overlaps) | no | every commit (short), nightly (long) |
| Component / acceptance | module with Null adapters, deterministic scheduler | no | every commit |
| **Contract** | one suite per port, run against every adapter | adapters need device (GPU/audio) | CI where available; Null always |
| Golden / replay | command streams, physics state hashes, audio buffers | no | every commit |
| Image / audio-fidelity | offscreen render vs reference; spectral checks | GPU | nightly / per backend |
| Performance | micro + scene benchmarks | stable machine | nightly + on-demand PR bot |
| Sanitizer / UB builds | full suite under ASan (nightly toolchain), Miri on `unsafe` modules, TSan | no | every PR (ASan; Miri on changed `unsafe`), nightly (TSan) |

## 4. Test doubles: Null adapters first

Because every port has a Null adapter, we avoid mocking frameworks in most places. Null adapters are real implementations that record calls and can inject failures (device lost, underrun). Use hand-written fakes over mocks to avoid brittle, implementation-coupled tests.

## 5. Making non-deterministic things testable

| Hard thing | Technique |
|---|---|
| Time | injected `IClock`; tests drive time manually |
| Threads | injected `IScheduler`; a single-thread deterministic scheduler for unit tests; real scheduler for concurrency tests under TSan |
| GPU output | test the command stream (deterministic); image tests with tolerance and per-backend references |
| Audio | Offline adapter renders deterministically |
| Floating point | simulation code is compared **bit-exactly** on one platform (D0/D1; D2 is not pursued); tolerance helpers (ULP + relative/absolute bounds, documented per test) only for rendering, audio DSP and other non-simulation code |
| Performance | never asserted inside unit tests; benchmark pipeline compares with baseline using statistical thresholds |

## 6. Spikes (the honest exception)

Some decisions need experiments (ECS storage, job system, port shape). A **spike** is:

- bounded by an iteration budget stated with the question (not by calendar time, Q19), on a throw-away branch, with a written question and an expected output (a benchmark chart or prototype);
- **deleted afterwards**: findings go into an ADR; real implementation is re-done test-first. This keeps TDD honest without blocking exploration.

## 7. Branching, commits, review `[P]`

- Trunk-based: short-lived branches, small PRs (< ~400 changed lines).
- Commit convention: `test:` (red), `feat:`/`fix:` (green), `refactor:`, `perf:`, `docs:`, `adr:`. A PR may squash, but the PR description must show the failing test (link/CI run of the red commit).
- A PR is mergeable when: all gates in [quality-metrics](quality-metrics.md) pass, and a review confirms (project is **solo and AI-implemented**, so the reviewers are the automated gates plus an independent AI review session on every diff; every task needs the owner's single acceptance, and what the owner reads depends on the change class in §10; the checklist is still filled in the PR description): tests describe behaviour, no new dependency without manifest entry, ADR written if a port or policy changed.

## 8. Definition of Ready / Done

**Ready:** the behaviour is stated as an example ("given X, when Y, then Z"), has an owner module and layer, and states its performance expectation if on a hot path.

**Done:** failing-first test exists; all gates green; benchmark added for hot paths; docs/ADR updated; no TODO without an issue; metrics not worse than the baseline on touched modules.

## 9. Per-port workflow (the Flexibility loop)

1. Write the **contract tests** for the new capability against the Null adapter (red).
2. Implement Null (green).
3. Implement the first real adapter; contract suite must pass (green).
4. Only after the second real adapter exists, mark the port capability `stable`.
5. Record any adapter-specific quirk in the adapter's own test file, never in the port.

## 10. AI-driven development protocol (Q19: all code is AI-written; the human is product owner / architect) `[P]`

**Roles**

| Human (product owner / architect) | AI agents |
|---|---|
| Owns vision, wiki, ADRs, port definitions, acceptance scenarios, gate configuration and thresholds, change-class decisions, final say on architecture | Own implementation, unit tests, refactors, benchmarks, design notes, doc drafts - under the controls below |

**Why this needs *more* rigor, not less.** AI makes writing code cheap; it does not make verification, design coherence, or performance/driver debugging cheap. Known failure modes: tautological tests (the test restates the implementation), hallucinated API behaviour (Win32, Vulkan, OpenGL, audio specifics), silently weakened gates, plausible-but-wrong numerics, architecture drift, and *comprehension debt* (code nobody understands). TDD, metrics and the architecture checks are therefore the **trust mechanism**, not overhead.

**Controls**

1. **Agent instruction file** (`CLAUDE.md` / `AGENTS.md`) at the repo root, derived from the wiki: layers and dependency rule, banned imports, the TDD loop, "no new dependency without a `THIRD_PARTY.md` entry", build/test commands, gate list. The wiki stays the source of truth; the file links to it.
2. **Task cards.** Every task states: module and layer, behaviour as given/when/then, tests to write first, performance expectation, files allowed to change, and "done = gates green". Template lives in `tools/templates/task-card.md`.
3. **Red-first evidence.** The agent runs the new test and shows it failing before implementing; `test:` commit precedes `feat:` on the branch. Because merges are squashed, the evidence lives in the PR/acceptance package (CI run of the red commit), and a script samples those records for compliance, not the squashed history of `main`.
4. **Test independence.** For L0-L2, write tests and implementation in separate agent sessions, or have a second session write adversarial tests against the first. The mutation-score gate is the backstop against tautological tests. Acceptance scenarios and contract-suite *semantics* are human-approved.
5. **Gate protection.** Gate config, thresholds, arch-check, contract suites, CI workflows, ADRs and `THIRD_PARTY.md` are *protected paths*: changes need human approval (Claude Code permissions and hooks, and the `governance-guard` check, which fails a PR touching `gates/protected-paths.txt` without a `Governance task: yes` line; not a required check on the Free plan; unlocked only for tasks declared as governance tasks). Agents are instructed never to lower thresholds, skip/disable tests, or add waivers; a waiver requires an ADR.
6. **Ground truth for platform, GPU and audio APIs.** Do not trust model memory. Verify by contract tests on real hardware plus API validation tools (Vulkan validation layers, D3D debug layer, GL debug output) as dev-time tooling with a zero-error gate; adapter file headers cite the official documentation used.
7. **Explain-back.** Each new module or port gets a short design note in the wiki (what, why, invariants, complexity, performance expectation). This is how the architect keeps comprehension without reading every line.
8. **Independent review pass.** A separate AI session reviews every diff (e.g. `/code-review`), because the authoring session shares its own blind spots.

**Change classes and autonomy (adopted by owner, Q21; the human reads only design notes, metric reports and Class 2/3 diffs - see [architect-review-surface](architect-review-surface.md); classes are assigned by a CI classifier, not by the author)**

| Class | Examples | Merge rule |
|---|---|---|
| **1 Routine** | implementation inside a module, tests, refactors, docs; no public API change | owner accepts on metrics and/or product (no diff reading) -> agent opens PR -> agent squash-merges after approval when G1 is green and the independent review is clean ([delivery-pipeline](delivery-pipeline.md)) |
| **2 Sensitive** | public API or port change, new dependency/adapter, concurrency / lock-free / unsafe code, SIMD, allocators, hot-path performance changes, math affecting determinism | you read the design note, diff summary and benchmark delta as part of your single approval (never the code), and an independent review by a different session or model is mandatory (D-2); no separate GitHub approval ([delivery-pipeline §2](delivery-pipeline.md)) |
| **3 Governance** | gate config and thresholds, arch-check, contract-suite semantics, ADRs, CI, `THIRD_PARTY.md` | only inside a task you declare as a governance task (protected paths unlocked for it); you read the governance diff before approving ([delivery-pipeline §3](delivery-pipeline.md)) |
