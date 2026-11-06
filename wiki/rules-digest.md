# Rules digest (the short version agents load)

Status: derived from the wiki on 2026-10-24 (v0.12); section 9 updated for the GitHub Free plan; section 4 updated for the allocator spike (v0.18) and the allocator wiring (v0.43); section 11 updated for card ids (v0.19); section 6 updated for the UML rule (v0.23); sections 5 and 7 updated for the withdrawal of D2 (v0.25); section 5 updated for the within-run replay hash (v0.27) and for who enforces the bans (v0.30), and for the opt-in cross-platform determinism of physics (v0.36), and for the Clippy bans of the physics crate (v0.95); section 9 updated for the phase-branch workflow (v0.48) and replaced by the develop-branch workflow (v0.54). The wiki pages are canonical; if this digest and a page disagree, the page wins and the digest is fixed. A CI check (phase 0a) compares the numbers below with the gate configuration. `CLAUDE.md` imports this file and nothing else from the wiki by default.

## 1. Mission and priorities

DuckEngine: a game engine written from scratch in **Rust**. Defining properties: **Performance** (measured, not assumed) and **Flexibility** (no lock-in to any OS, graphics/audio API or third-party library: all are behind ports we own). Quality is governed by **TDD and metrics**. Private repository, proprietary. Owner = product owner / architect; all code is written by Claude Code. Priorities: correctness, performance, replaceability, simplicity, feature breadth (last).

## 2. What is wrapped and what is not

- **Not wrapped (use directly in L0-L2):** the Rust language and `std` (allocation, collections, atomics, threads, time).
- **Wrapped behind ports (adapters only, L3):** OS-specific services (window, input, file system, dynamic loading, lifecycle), graphics APIs, audio devices, CPU intrinsics (`simd` abstraction), compiler specifics, third-party libraries.
- **Tiers:** T0 nothing external in L0-L2. T1 adapter code against an OS/vendor API. **T1-bindings** declaration-only binding crates, inside adapters, pinned and listed. **T1-stopgap** a whole third-party library inside an adapter with an exit plan (**GLFW** window/input, **miniaudio** audio; SDL is not used). T2 dev-time tools only.
- `std` allow/deny list and the simulation-crate bans are in [ports-and-adapters](architecture/ports-and-adapters.md); conventions (f32, SI units, right-handed Y-up, depth 0..1, column-major, app owns the loop) are in [conventions](architecture/conventions.md).
- Every external thing is listed in `THIRD_PARTY.md` (tier, licence, version, port, exit plan). No GPL/AGPL.
- Platforms: **Windows is the real platform**, Linux is the second concrete platform (CI, proof of portability), Headless for tests. Ports are not shaped for web/mobile/console.

## 3. Architecture rules

- Layers L0 base, L1 runtime, L2 domain modules, L3 adapters, L4 apps. A layer includes only itself and lower layers. Ports are declared by the module that needs them; only the composition root names adapters.
- Every port has a **Null adapter**, a **contract test suite**, and is `experimental` until two real adapters pass it. Window/input/audio ports become `stable` only after a **native** adapter exists next to the stopgap.
- Physics is a standalone module (no ECS dependency), handle-based. 2D and 3D share a dimension-generic core (2D = D=2 instance); render uses one port/graph, 2D = orthographic frontend.
- Render: explicit-API-shaped `IRenderDevice`, render graph, POD command stream, external shader compilers as build-time tools behind `IShaderCompiler`. Backend order: Null, OpenGL 4.5, Vulkan, D3D12 (Metal later).
- Handles, not pointers, across module boundaries. Virtual dispatch only at coarse boundaries (per pass/batch). Public API between modules is ordinary Rust; a C ABI only at plug-in boundaries. ISA backends live inside `base` unless the phase-1 spike decides otherwise. Glue crates (`frame`, `sync_*`) are the only code that knows two domain modules.

## 4. Language and style rules

- `unsafe` only in allowlisted modules with a per-module budget; every block has a `// SAFETY:` comment; the count is a metric.
- No `unwrap`/`expect` in engine code; errors are values; `panic = abort`; no panics across module boundaries.
- `std` containers by default; own containers only where they do not fit or a benchmark shows a win (arena-backed containers are own code).
- Zero heap allocations per frame on hot paths (counting allocator with **per-thread counters** in tests).
- Allocation tools in this order: design the allocation away, reuse capacity (`clear()`, `with_capacity`), arena (per-frame or per-job scratch), pool + handles (long-lived objects with churn), then the general allocator. An arena has fixed capacity, holds only types without drop glue, resets with `reset(&mut self)` and reports failure as a value. The global allocator is one `#[global_allocator]` line in L4 (crate `app_runtime`): **mimalloc**, chosen by the phase-1 benchmark on Windows and Linux (ADR-0022); the default allocator is the exit plan (build with `--no-default-features`, tested in CI); a third-party allocator needs an owner decision (ADR-0022 defines its tier: L4, no port) and a `THIRD_PARTY.md` entry with the default as exit plan.
- Measurable clean-code limits only (see gate config): function length warn > 60, cyclomatic warn > 10 / fail > 15, cognitive fail > 20, nesting <= 3, parameters warn > 3 / fail > 5, duplication <= 3%, no dead code, no magic numbers, rustfmt, Clippy clean. A deviation in a hot path needs a `PERF:` comment naming an existing benchmark (counted and reported).

## 5. Determinism (simulation code)

D0 same machine and thread count, D1 across thread counts (both required); **D2 identical physics results on Windows and Linux x86-64 is opt-in (Cargo feature `cross-platform-determinism`, off by default, on in CI) and covers the physics crate only (owner, v0.36, reverses v0.25); ARM64 is not gated**. Rules: strict IEEE (Rust default: no fast-math, no implicit FMA); SIMD bit-identical to scalar on the same platform, with no reciprocal-estimate operation in the `simd` abstraction and fused multiply-add used where the CPU has it only through the math facade, never with the D2 feature on (with the feature off, results can differ between machines with and without FMA); no `HashMap`/`HashSet` iteration, no unstable sort on equal keys; own seeded RNG; floating-point control registers set and asserted at the start of each simulation job; the arch-check scan enforces the bans a scanner can see (`HashMap`/`HashSet`, unstable sort, clocks), Clippy `disallowed-methods` and `disallowed-types` in `engine/physics/clippy.toml` are the second line for the physics crate (card 4-19; the crate file repeats the clean-code thresholds, `tools/lint-profile` checks them); replay hash compared within one run (twice, and at 1 and N threads) on one machine; with the D2 feature on, physics calls `sin/cos/exp/...` only through the deterministic math facade in `base` (own functions, no `mul_add`, no libm in the physics crate's own source, scanned by arch-check; libm reached through `base` types is not scanned and must be routed through the facade first) and, once the physics crate exists (phase 4), a golden hash stored in `gates/golden/` is compared on Windows and Linux; with the feature off nothing is stored.

## 6. TDD and review

Test list first, one failing test at a time with red evidence, smallest step to green, refactor on green, bug fix = failing regression test first, never weaken or delete a test to get green. A documentation change is covered by a UML view in Mermaid, in the page or the PR (typo, link and number fixes exempt; [clean-code-and-review](workflow/clean-code-and-review.md) section 7). After implementation a **mandatory review phase R1-R8** (self, automated, independent, QA, architecture, security, performance, product fit) with a fix loop, at most 3 loops, then stop and report. Output: `reviews/<task-id>.md` with 0 open blocker/major.

## 7. Gates (values are hypotheses, ratcheting, calibrated in phase 0)

Line coverage L0-L2 >= 90% (new lines >= 95%), branch >= 80%, **mutation score on changed L0-L2 code >= 80%**, 0 skipped tests, 0 compiler/Clippy warnings, 0 dependency-rule or banned-import violations, Miri clean on changed `unsafe`, contract suites 100% per adapter, headless-only build green, PR benchmark gate on instruction counts (warn 5%, fail 10%), replay hash identical across runs and thread counts on each platform, review report present.

## 8. Change classes and protected paths

Class 1 routine, Class 2 sensitive (public API/port, new dependency or adapter, `unsafe`, concurrency, SIMD, allocators, determinism math, hot-path perf), Class 3 governance (gate config, thresholds, classifier, arch-check, contract-suite semantics, CI/release workflows, ADRs, `THIRD_PARTY.md` structure, this digest, `CLAUDE.md`). The classifier (a tool) assigns the class, highest wins, unsure rounds up. Class 3 files change only in a task the owner declares as a governance task. **Gate tools and thresholds are to be executed from a trusted checkout of `main`, never from the PR tree. Status (cards 0b-8, 0c-5): built for `pr-title`, `governance-guard` and the `ci` `gates` job (architecture check, unsafe budget, wiki lint, digest check and approval report run from a checkout of `main`, the pushed commit is data only, except that arch-check runs cargo inside it and so honours its toolchain file and cargo config, both protected paths; the workflow file itself is PR-controlled, so the owner reads any workflow diff); the `ci` unit tests of the tools still run from the PR tree, because they test the PR's own code and are not the gate.** Agents never lower thresholds, skip tests, add waivers or edit protected paths otherwise.

## 9. Delivery

**Develop branch (owner, cards 1-38 and 2-9, CI budget): ordinary card PRs target the permanent branch `develop` (local gates are their evidence, only `pr-title` and `governance-guard` run, no CI); governance cards go to `main` first with one `ci` run started by hand, then `develop` merges `main` in; `ci` runs only on `main` and by hand; `develop` goes into `main` only when the owner explicitly asks, by one PR with full CI, the owner's approval of the batch diff hash, a merge commit with the footer `Release-Batch: <label>` and one minor release** ([delivery-pipeline](workflow/delivery-pipeline.md)). Owner approves against a diff hash, not a commit SHA. Owner defines the task, Claude Code works as a software team and runs the review phase, the owner approves once (metric report + product view), Claude Code opens the PR and merges it with `gh` after green checks (GitHub Free plan: no auto-merge, no branch protection, so merging is policy, not enforcement; see [github-plan-findings](workflow/github-plan-findings.md)), the release workflow computes the version (every merge bumps at least patch, docs-only included), keeps baselines outside git, tags, and publishes the source workspace, tools, samples and metric report as a private GitHub Release. Pause after approval only if a fix changes product-visible behaviour or moves metrics outside tolerance.

## 10. Safety rules for agents

Work inside `DuckEngine/` only. Never read, print or commit `Api-keys.txt`, `GiaMat90_ssh.txt` or anything outside the repo. No secrets in code, logs or commits. Release credentials exist only as GitHub Actions secrets/OIDC. Never push to `main` directly; never bypass hooks (`--no-verify`).

## 11. Work board

The Trello board DuckEngine is the work queue; git and the wiki are canonical. Agents move cards by the column rules in [board-management](workflow/board-management.md), create subtasks (checklist item, or a separate card when it needs its own PR), propose next tasks only when asked and wait for the owner's pick, never delete (archive only), never touch other boards, never put secrets on cards. Every task card has an id `<phase>-<n>` from the moment it is created; the same id names its branch and review file (rule and how to find the next free number: board-management §3). If Trello is unreachable, pending card updates are an ASAP to-do, applied before the next task.
