# Software Quality Metrics Proposal `[P]`

## 1. Approach: Goal-Question-Metric

A metric exists only if it answers a question that serves a goal. Otherwise we are collecting numbers for decoration (and invite gaming, like coverage-chasing).

| Goal | Question | Metric(s) |
|---|---|---|
| **Correctness (TDD)** | Do tests actually verify behaviour? | line + branch coverage, **mutation score**, flaky-test rate |
| **Maintainability** | Is the code simple enough to change safely? | cyclomatic complexity, cognitive complexity, function/file length, duplication |
| **Flexibility (R2)** | Are boundaries really respected? Can a backend be swapped cheaply? | dependency-rule violations, third-party manifest coverage, instability/abstractness per module, adapters-per-port, contract pass rate, swap-diff size |
| **Performance (R1)** | Is it fast, and does it stay fast? | benchmark time vs baseline, frame budget, allocations/frame, scaling efficiency, binary size |
| **Robustness** | Is it free of memory/UB/race bugs? | sanitizer findings, static-analysis findings, compiler warnings |
| **Process health** | Is the workflow sustainable? | CI duration, test-suite duration, lead time, red-first compliance |

## 2. Metric catalogue, tools, initial thresholds

Thresholds are **starting hypotheses**, to be calibrated after the first baseline (phase 0); the principle is *ratchet*: gates only tighten.

### 2.1 Test effectiveness

| Metric | Tool `[K]` | Initial gate |
|---|---|---|
| Line coverage | cargo-llvm-cov | L0-L2 >= 90%; new/changed lines >= 95% |
| Branch coverage | cargo-llvm-cov | L0-L2 >= 80% |
| **Mutation score** | cargo-mutants `[K, verify in phase 0]` | changed code in L0-L2 >= 80% (all code is AI-authored, Q19; calibrate after the phase-0 baseline); full run nightly |
| Flaky tests | CI re-run statistics | 0 tolerated; quarantine within 1 day |
| Adapters | contract-suite pass | 100% per adapter (coverage not used for adapters) |

Why mutation score: coverage says code *ran*; mutation testing says tests would *notice* a change. In a TDD project this is the metric that detects tests written to satisfy coverage rather than behaviour.

### 2.2 Code quality

| Metric | Tool `[K]` | Initial gate |
|---|---|---|
| Cyclomatic complexity / function | rust-code-analysis / Clippy | warn > 10, fail > 15 (hot kernels may be waived with a `PERF:` comment) |
| Cognitive complexity | Clippy `cognitive_complexity` | fail > 20 |
| Function length | Clippy `too_many_lines` / rust-code-analysis | warn > 60 lines |
| Duplication | jscpd or similar | <= 3% |
| Nesting depth / parameter count | Clippy (`too_many_arguments`, nesting lints) / rust-code-analysis | nesting <= 3; parameters warn > 3, fail > 5 ([clean-code-and-review §1](clean-code-and-review.md)) |
| Magic numbers, dead code | `dead_code` lint (magic numbers: no Clippy lint, not enforced yet) | 0 new |
| `PERF:` waivers | scan + benchmark link check | each waiver names an existing benchmark; count reported |
| Compiler warnings | `RUSTFLAGS=-D warnings` | 0 |
| **`unsafe` blocks** | scan (`cargo-geiger` or grep) | only in allowlisted modules; each has a `// SAFETY:` comment; count reported per PR; **per-module `unsafe` budget** fixed in the module's design note; Miri passes on changed `unsafe` that does not cross FFI/GPU/intrinsics, and other `unsafe` is covered by sanitizers and tests (M8) |
| Clippy | CI | 0 new findings |
| Format | rustfmt | check-only, 0 diffs |

### 2.3 Architecture / Flexibility

| Metric | How | Initial gate |
|---|---|---|
| **Dependency-rule violations** | include/link-graph checker ([ports-and-adapters §5](../architecture/ports-and-adapters.md)) | 0 |
| **Banned-import violations** (OS, vendor, third-party headers in L0-L2) | denylist scan | 0 |
| **Replay hash per platform** (D0/D1) | CI job compares simulation state hashes of repeated runs and of different thread counts with each other, in one run on one machine; no stored value (D0, D1; always on). **D2 golden hash** (feature `cross-platform-determinism` on): the hash of each step is compared with `gates/golden/physics-replay.toml` on Windows x86-64 and Linux x86-64 (N-10 without ARM64; built with the physics crate in phase 4) | identical |
| **SIMD vs scalar bitwise mismatches** | property test | 0 |
| Headless-only build (Null adapters + headless platform) passes full suite | CI job | must pass |
| **Platform adapters passing contracts** | count of real OS families green in CI matrix | >= 2 (Win32 + POSIX/Linux) before platform ports are `stable` |
| Platform-specific code outside `adapter_*` (`#[cfg(windows)]`-style conditionals, OS crate imports) | grep/AST scan | 0 |
| Third-party manifest coverage | script | 100% of vendored deps listed |
| **Instability I = Ce/(Ca+Ce)** and **Abstractness A** per module (R. Martin) | script from include graph | track; stable modules (low I) should be abstract; flag distance from main sequence D > 0.5 |
| Cyclic dependencies between modules | script | 0 |
| **Wiki-lint** (stale terms from removed or resolved decisions; gate numbers stated in a page must equal `gates/gates.toml`, and a comparator number no rule recognises is refused) | `tools/wiki-lint`, rules in `gates/wiki-lint.toml` (Class 3) | 0 hits outside history |
| Adapters per port | count | >= 1 (Null) from day one; >= 2 real before port is `stable` |
| **Swap diff** | when adding backend/platform N+1: files changed in engine and game code (port files excluded) | 0, with at most one counted port revision before `stable` (N-4) |
| Result equality | same replay tests (within-run hash comparison) and golden tests (libm-free outputs) on every platform in the matrix | identical hashes: D0 across runs and D1 across thread counts on each platform; with the D2 feature on, also equal to the stored golden hash on both platforms |
| Port-call frequency | profiler counter: port calls per frame | tracked; budget per port |

### 2.4 Performance

| Metric | Tool | Initial gate |
|---|---|---|
| Micro-benchmark time | deterministic **instruction-count** benchmarks for PR gates (iai-style tools, Linux) and `criterion`/`divan` wall-clock for nightly trends `[K]` | regression > 5% warn, > 10% fail vs main (instruction counts on PRs; median of repeated runs for wall-clock) |
| Scene frame time (CPU & GPU) | benchmark scenes + engine timers | within published budget per scene |
| Allocations per frame (steady state) | counting allocator | 0 on hot paths |
| Parallel efficiency | scaling benchmark at 1/2/4/8.. threads | >= target set after baseline |
| Binary size / startup | CI artifact | tracked, soft gate |
| Abstraction overhead | same workload through port vs direct raw API | <= X% (X chosen after first measurement, proposal 3-5%) |

Benchmarks run on a **dedicated, stable machine** (shared CI runners are too noisy for 5% thresholds); PR runs use a "perf bot" comparing with main, nightly runs maintain the trend.

### 2.5 Robustness

| Check | Gate |
|---|---|
| Sanitizers (ASan on nightly) + **Miri on `unsafe` modules** | clean on every PR (Miri on changed `unsafe` modules) |
| TSan (job system, physics queries, audio queues) | clean on `main` and on demand (job `tsan` of `ci`, card 4-21; not nightly, Actions minutes); required on changes to concurrency code |
| Static analysis (Clippy groups, `cargo-deny`) | 0 new |
| Fuzzing (asset parsers, decoders, serialisation) | nightly short run, corpus kept |

### 2.7 Controls specific to AI-written code (Q19)

| Metric / check | Gate |
|---|---|
| Mutation score on changed L0-L2 code | >= 80% (same gate as 2.1; tests are AI-authored, so tautology risk is real) |
| Gate / threshold / waiver changes without human approval | 0 (protected paths, [tdd-workflow §10](tdd-workflow.md)) |
| Newly skipped / disabled / expected-fail tests | 0 |
| Graphics/OS validation errors (Vulkan validation layers, D3D debug layer, GL debug output) in contract and sample runs | 0 |
| Red-first compliance (`test:` commit failing before `feat:`) in L0-L2 | 100% of sampled commits |
| Design note present for each new module / port | required |
| **Review report** `reviews/<task-id>.md` present for the reviewed SHA, **0 open blocker/major findings**, reviewer session differs from author session, loop count <= 3 | required (traceable, not unforgeable) |
| Revert / rework rate | trend only |

### 2.6 Process

| Metric | Use |
|---|---|
| CI wall time, unit-suite time | keep feedback fast (budgets in tdd-workflow) |
| Lead time per change | trend only, no gate |
| Red-first compliance | sampled in review (PR shows failing test first) |
| ADR count / open ADRs | make decision debt visible |

## 3. Quality gates

| Gate | When | Contents |
|---|---|---|
| **G0 pre-commit (local)** | on commit | format, fast unit tests of touched modules, warnings |
| **G1 PR** | every PR | build matrix, full unit + acceptance + golden, ASan (nightly toolchain) and Miri on changed `unsafe`, static analysis, coverage + mutation-on-diff, dependency rules, Null-only build, contract suites available |
| **G2 nightly** | daily | TSan, long fuzz, full mutation run, benchmarks on perf machine, image/audio-fidelity on GPU runners, trend report |
| **G3 milestone** | per phase | exit criteria in [roadmap](roadmap.md), metric report archived in the wiki |
| **G4 release** | after every merge to `main` | release artifacts built on the matrix, smoke test of the *packaged* SDK, version computed, tag + Release created, deploy succeeded ([delivery-pipeline §5](delivery-pipeline.md)) |

## 4. Reporting

- A generated `metrics/` report per CI run (markdown/HTML + CSV history) and a trend view across commits. Start simple (CSV in repo or CI artifacts); dashboards later.
- Each milestone review includes: metrics vs previous milestone, top 5 worst-complexity functions, dependency graph, benchmark trend.

## 5. Caveats (read these)

- Coverage and mutation scores can be gamed or misleading on code that is mostly numerical kernels or GPU interaction; use property-based/oracle tests there and judge by review.
- Complexity limits can fight with performance-oriented code (unrolled SIMD, switch-heavy decoders); waivers must be explicit, local, and justified.
- Mutation testing is slow; therefore diff-based on PRs, full run nightly. `cargo-mutants` works on Windows and Linux with the normal toolchain `[K]`.
- **Goodhart's law.** Since the owner judges the work by metrics and the product only (Q21c), metrics can be optimised instead of quality. Counter-measures (holdout suite, protected metric definitions, product view, independent agents) are in [architect-review-surface §7](architect-review-surface.md).
- Numbers are hypotheses until baselined. We will adjust thresholds with evidence, never silently lower them (changes go through an ADR).
