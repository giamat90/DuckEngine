# Wiki review v0.10: contradictions, ambiguities and critical review

Reviewed by the same agent that wrote the wiki, so treat it as a **self-review**: it finds inconsistencies reliably, but it shares the author's blind spots. A second, independent review (a fresh session that only gets the wiki and the decision list) is recommended before phase 0.

**Read in full:** README, vision, overview, ports-and-adapters, core-runtime, rendering, physics, audio, tdd-workflow, quality-metrics, tooling-and-ci, roadmap, delivery-pipeline, architect-review-surface, clean-code-and-review, decisions index, open-questions, the language evaluation. **Not re-read:** `01-state-of-the-art.md` (research notes; its claims were not re-verified). **No code exists**, so nothing here is tested.

Confidence tags as in the README: `[K]` = from background knowledge, to verify.

## Part A. Contradictions and ambiguities against decided items

"Fixed" = corrected in this pass using an existing decision (no new decision made). "Decision" = needs the owner (Part D).

| ID | Where | Problem | Status |
|---|---|---|---|
| A1 | core-runtime §7 | SIMD "same results within tolerance" contradicted bit-exact SIMD for D2 (Q16) | Fixed |
| A2 | ports-and-adapters §3 | `IVirtualMemory` described as "root of all allocators (no `malloc`)", against the v0.6 standard-library decision | Fixed |
| A3 | ports-and-adapters §4 rule 1 | "shape ports after the most restrictive target" contradicted Q17 (and the design rule two sections above it) | Fixed: graphics/audio after the most explicit API, platform after Windows + Linux |
| A4 | ports-and-adapters | leftovers: Q13 reference (resolved), "each stdlib header" in the manifest, `Caps.hasThreads` justified by web, lifecycle justified by mobile | Fixed |
| A5 | overview | C++ wording after the Rust decision: "no exceptions, no RTTI", "no global `new`", C++ composition-root snippet, "templates"; section order 5, 7, 6 | Fixed (cross-cutting rules renumbered to 8) |
| A6 | vision | pointer to "remaining detail questions Q13-Q17" although all are resolved | Fixed |
| A7 | quality-metrics, tdd-workflow, tooling | C++-era tools and terms: lizard, Google Benchmark, ASan/UBSan (Rust has no UBSan), `#ifdef`, clang/MSVC matrix | Fixed |
| A8 | quality-metrics | two mutation thresholds for the same code (70% in 2.1, 80% in 2.7) | Fixed: 80%, calibrate after baseline |
| A9 | quality-metrics | D1 described as "same platform", whereas physics defines D1 as "across thread counts" | Fixed |
| A10 | tdd-workflow | "boy-scout rule" in Definition of Done (dropped in Q24); "tolerance helpers" for floats against bit-exact simulation; spikes "<= 2 days" against "no time constraint" (Q19); test-code bar "readable, no logic" (subjective, dropped in Q24) | Fixed |
| A11 | tdd-workflow vs delivery | red-first compliance was sampled from commit history, but merges are **squashed**, so the history of `main` has no red commits | Fixed: evidence is the CI run of the red commit on the branch, recorded in the PR |
| A12 | delivery-pipeline | "every merge produces a version" vs SemVer computed from Conventional Commits, where `refactor:`/`docs:`/`test:` bump nothing | Fixed: those types bump patch |
| A13 | roadmap | cut order listed "D1/D2 determinism" as droppable, but D1 is required (Q9); ARM64 CI/NEON (Q15) was in no phase; phase 5 listed native WASAPI although miniaudio is the agreed first adapter | Fixed |
| A14 | architect-review-surface | classifier mentioned `CODEOWNERS` (removed from the flow), "public `include/` directory" (no such thing in Rust), `volatile`; it had **no trigger for `unsafe`** although Class 2 is defined to include it | Fixed |
| A15 | decisions index, open-questions | header said "none accepted"; Q4 row kept the v0.2 wording and a closed question; Q14 said "designed-for" against Q17; R-18 mentioned a human Class 1 sample that is OFF | Fixed |
| A16 | language evaluation | §6 still described building the slice in both languages, §7 still offered the choice | Fixed |
| A17 | physics §3 | D2 rule for containers did not cover Rust's randomly seeded `HashMap` (breaks even D0) | Fixed |
| A18 | roadmap phase 2 | "Win32 + POSIX/Linux adapters" for window/input, but the agreed first window adapter is GLFW (one library on both OSes). See B9 | Resolved by D-5 |
| A19 | Q21 vs the owner | Q21 says the owner reads Class 2/3 **diffs**; the owner does not know Rust. See B2 | Resolved by D-2 |
| A20 | delivery-pipeline §6 | SDK artifacts "libraries and public headers" is a C++-era model. See B1 | Resolved by D-1 |
| A21 | Q10 | resolved row still hides two open items: scripting, and whether command-line asset tools exist in v0. See B10 | Resolved by D-6 |
| A22 | roadmap phase 1 vs ADR-0018 | phase 1 builds dimension-generic math before the ADR-0018 spike (before phase 4) decides A/B/C | Note only; harmless because Vec2/Vec3 are needed in every option |
| A23 | wiki structure | history notes ("v0.2 said..., v0.6 says...") sit inside policy pages; section numbers out of order in tooling-and-ci; reading-order numbers 12b/12c/12d | Not fixed (cosmetic, see B20) |

## Part B. Critical review

Ranked by how much each could hurt the project. Severity: **High** = could stall or invalidate a core decision; **Medium** = costs time or quality; **Low** = hygiene.

### High

**B1. How is the engine consumed? (Rust has no stable ABI)** `[K]` The delivery pipeline ships "libraries, public headers, samples, tools" as the SDK. For Rust that is the wrong shape: prebuilt Rust libraries are only usable by the exact same compiler version, and headers apply only to a C ABI. The real consumer is your future game, written in Rust (vision: "game code is Rust against the engine API"). *Consequence:* the "versioned SDK artifacts" that Q22 defined need a redefinition. *Suggestion:* the release publishes a tagged, versioned **source workspace** (consumed by Cargo through a git tag or a private registry), plus prebuilt **tools and samples** (shader compiler wrapper, asset tools, sample binaries) and the metric report; a C ABI + headers only for plug-ins or non-Rust consumers. This still gives "versioned SDK artifacts" and keeps the automatic release.

**B2. The owner's review surface does not fit the owner's skills.** Q21 has you read Class 2/3 diffs; Class 2 is exactly `unsafe`, SIMD, lock-free and allocator code, in a language you do not know. In practice the approval becomes a stamp on a summary. The same applies to "architect": ADRs are decided by spikes the same agent runs and evaluates. *Consequence:* the human gate is weaker than the wiki implies; the real safety is the independent agents and the metrics. *Suggestion:* say so explicitly: for Class 2 you read the **design note, the diff summary and the benchmark delta**, never the code; make the independent Class 2 review (R3 + R6 + R7) mandatory with a different model/session; have a second agent evaluate every spike result; and let the holdout acceptance suite be authored by a separate agent from a spec you write in plain language (today it is described as "owned by you" but you cannot write Rust tests).

**B3. Security of the automation on your own machine.** The tooling page suggests the dev machine as the first self-hosted runner for GPU/audio tests and benchmarks. That machine would then execute AI-written code from every branch push, with your user privileges, next to `Api-keys.txt` and `GiaMat90_ssh.txt` in the parent workspace. The agent token is scoped, but code running on the runner is not. Claude Code sessions also run on this machine. *Suggestion:* never run the runner under your own account; use a separate Windows user or a VM with no access to the parent folder; deny read access to the parent folder's secret files in the Claude Code permissions of the DuckEngine sessions; keep the session's working directory at `DuckEngine`; prefer GitHub-hosted runners plus software rasterizers (B12) so no self-hosted runner is needed at first.

**B4. The automated flow rests on unverified GitHub features.** Auto-merge needs a protection rule with required checks; rulesets, merge queue and environment protection on **private repositories depend on the plan** `[K]`. If the free plan lacks them, "wait for green CI, then merge automatically" has no enforcement. This is the cheapest high-impact fact to check and it is only marked `[K]`. *Suggestion:* make "verify GitHub plan features" the very first phase-0 task, before anything is built on it, and decide plan (or fallback: Claude Code runs `gh pr checks --watch` then merges; weaker, but works).

**B5. Phase 0 is a software project of its own, built without the protections it creates.** It contains CI, classifier, metrics generator, architecture checker, governance guard, release workflow, templates and the Rust toolchain validation, with exit criteria "pipeline green end-to-end". These tools are Class 3 but are written before any protection exists, by the agent that will later be governed by them. *Suggestion:* split phase 0 into 0a (repo, Cargo workspace, CI G1, toolchain gates, plan-feature check), 0b (delivery pipeline with a dummy artifact), 0c (classifier, metrics report, arch-check, governance guard), each with its own exit criteria and a short independent review, and run the first engine task only after 0b.

**B6. Per-frame arenas vs standard containers (Rust).** `[K, verify]` The wiki says "use `std` containers by default" and also "per-frame arena, zero heap allocations per frame, allocator-aware containers". On stable Rust, `Vec`/`HashMap` cannot take a per-instance allocator (that feature is nightly-only), so arena-backed storage means own small containers or a third-party arena crate. Amended in core-runtime (A1/containers) but it is a real cost on the R1 Performance path. *Suggestion:* phase 1 decides explicitly: own arena containers (small, tested) vs pinned arena crate behind a facade.

**B7. D2 (identical results everywhere) vs Performance (R1).** Your two goals pull in opposite directions in the hottest code: no FMA, no reciprocal-estimate instructions, own `sin/cos/exp`, fixed reduction order. All reduce solver and mixer throughput by an amount nobody has measured. *Suggestion:* keep D2 strictly to **simulation code** (already stated), measure the cost in phase 1, and consider a compile-time "deterministic profile" so code outside simulation (rendering math, audio DSP) can use FMA. Revisit the D2 goal only if the measured cost is large.

### Medium

**B8. Bindings policy is unrealistic.** The toolchain gate demands FFI "without third-party binding crates". Hand-writing Win32, OpenGL, Vulkan and D3D12 declarations is a large, error-prone job (Vulkan's are generated from an XML spec). *Suggestion:* add a tier **T1-bindings**: declaration-only, generated or community-maintained binding crates, allowed inside adapters only, pinned and listed in `THIRD_PARTY.md`; hand-written bindings only for the small C APIs (GLFW, miniaudio).

**B9. "Rule of two" with stopgaps.** GLFW is one library on both OSes, so a GLFW window adapter on Windows and Linux does not prove the window port is independent of any library; it is one implementation, twice. And with `std` allowed, clock, threads and files need no per-OS adapters at all, so it is unclear what "Win32 vs POSIX adapters" differ in apart from window/input, audio device, dynamic loading and async I/O. *Suggestion:* state it: ports that are fully served by `std` stay as thin seams for testing only; the window/input/audio ports count as `stable` only when a **native** adapter exists next to the stopgap; the Win32/POSIX native adapters are therefore needed before the "platform ports stable" milestone (not necessarily before phase 7).

**B10. Assets and file formats are undefined.** The module map has `assets`, `scene`, `input`, but no phase delivers them. Nothing says how images (PNG?), sounds (WAV, then Ogg/Opus) or fonts are decoded: from scratch, by a dev-time converter tool, or by a library inside an adapter. Phase 3 (sprites) needs textures. *Suggestion:* decide before phase 3: own simple runtime formats + an offline converter tool (T2, may use any library), so the runtime has no decoder dependency.

**B11. Build order is not sample-driven.** ECS and job system are built in phase 2 but first needed by the sample in phase 7 (physics is standalone, the 2D render slice can use plain structs). *Suggestion:* build the job system with physics (phase 4), and build the ECS when the sample or render extraction needs it (phase 6/7), to reach a visible result earlier and avoid an unused abstraction.

**B12. CI realism.** GPU contract tests need a GPU, and hosted runners have none. Software rasterizers (Mesa llvmpipe for OpenGL, lavapipe for Vulkan, WARP for D3D) `[K]` can run contract and golden-image tests on hosted runners. A 5% benchmark gate is unrealistic on a shared PC or hosted runner. *Suggestion:* gate PRs with deterministic **instruction-count** benchmarks (for example iai-style tools on Linux), keep wall-clock benchmarks for nightly trend reports, and measure Actions minutes per run from the first week (Windows minutes are expensive, mutation-on-diff and a two-OS matrix per PR add up).

**B13. Thresholds are unvalidated.** 90% line coverage, 80% branch coverage and 80% mutation score on code that is mostly `unsafe`, SIMD, FFI and numerical kernels will produce many equivalent mutants and may stall tasks; the 3-loop stop rule then pushes the problem to you. The wiki already says "calibrate after baseline"; the point is to make that calibration a phase-0 exit criterion with a documented exemption list.

**B14. Dimension-generic physics in Rust.** Rust has no template specialization or `if constexpr`, and const-generic expressions are limited `[K]`. Option B (generic skeleton, specialised parts) is doable with traits and associated types, but is harder than in C++. The ADR-0018 spike must include a Rust feasibility check (compile time, code shared), not only bytes per body and step time.

**B15. Process lock-in.** The engine is deliberately independent of every library, but the development process depends on one AI tool and on features marked `[K]` (subagents, hooks, permissions, skills). *Suggestion:* keep the process specification tool-neutral in the wiki, keep `CLAUDE.md` thin and generated or checked against the wiki, and keep CI authoritative (already the case).

**B16. The wiki is the source of truth but is 180 KB.** An agent cannot load it per task, and `CLAUDE.md` will drift from it. *Suggestion:* write one short **rules digest** (3 pages: layers, dependency rule, determinism rules, measurable clean-code limits, TDD loop, gates, protected paths), have `CLAUDE.md` import it, and add a CI check that the digest and the thresholds in the gate config agree.

**B17. Cost is not zero.** Hours do not matter (Q19), but each task runs a team of agents plus up to eight review steps in separate sessions, and each PR runs a two-OS CI matrix. Nobody has estimated AI usage cost or CI minutes. *Suggestion:* track both as process metrics from the first task and set a per-task budget; adjust the review depth by change class (already defined) if the cost is high.

### Low

**B18. Approval is recorded by the agent itself.** The "approved" is a chat message that Claude Code copies into the PR, and the governance guard is "traceable, not unforgeable" (already documented as accepted residual risk). Given your explicit instruction not to add steps, I do not propose another approval; the mitigations that remain are the scoped token, rulesets and the post-hoc audit trail.

**B19. Unverified claims accumulate.** Dozens of `[K]` items; Part C lists the ones that can change a decision.

**B20. Wiki readability.** Policy pages still carry inline history and some stale numbering; this is a risk for an AI that reads them as instructions. *Suggestion:* a cosmetic pass that leaves one current statement per policy and keeps history only in the changelog, done together with the rules digest (B16).

## Part C. Phase-0 verification list (claims that can change decisions)

| # | Claim to verify | Changes |
|---|---|---|
| V1 | GitHub private-repo plan: branch protection/rulesets, required checks, auto-merge, environments, merge queue, Actions minutes and multipliers | the whole delivery flow (B4) |
| V2 | `cargo-mutants` on Windows and Linux: runtime, usable score on unsafe/SIMD code | mutation gate, language gate 1 |
| V3 | Miri and sanitizers: availability on the MSVC target, nightly pinning | robustness gates |
| V4 | Stable Rust: portable SIMD status, per-instance allocators, const-generic limits | `simd` abstraction, arenas (B6), physics core (B14) |
| V5 | Clippy lint names and defaults (`cognitive_complexity`, nesting, `unwrap_used`, `too_many_lines`), `cargo-geiger` upkeep, `rust-code-analysis` upkeep | clean-code and unsafe metrics |
| V6 | Licences of GLFW, miniaudio, DXC/SPIRV-Cross/glslang/Slang and their outputs | `THIRD_PARTY.md`, ADR-0009 |
| V7 | Software rasterizers (llvmpipe, lavapipe, WARP) usable for contract/golden tests on hosted runners | self-hosted runner need (B3, B12) |
| V8 | Claude Code features used by the process: hooks, permissions deny rules for the parent folder, subagents, skills | `CLAUDE.md`, protected paths |
| V9 | Binding crates or generators for Win32, GL, Vulkan, D3D12 (maintenance, licence) | T1-bindings tier (B8) |

## Part D. Decisions (all accepted by the owner and applied in v0.12)

The owner accepted every recommendation below. The independent second review of the wiki (part of D-11) still has to be run in a fresh session.

| # | Decision | Recommendation |
|---|---|---|
| D-1 | **What is the "SDK" for a Rust engine?** (B1) | Tagged source workspace consumed by Cargo (git tag or private registry) + prebuilt tools, samples, metric report; C ABI + headers only for plug-ins |
| D-2 | **What does the owner actually read for Class 2?** (B2) | Design note + diff summary + benchmark delta, never code; mandatory independent review; separate agent writes the holdout suite from a plain-language spec |
| D-3 | **Where does CI run GPU/audio tests and benchmarks?** (B3, B12) | Hosted runners + software rasterizers first; any self-hosted runner in a VM or separate Windows user; PR perf gate on instruction counts |
| D-4 | **Split phase 0 into 0a/0b/0c and verify GitHub plan features first** (B4, B5) | Yes |
| D-5 | **Rule of two with stopgaps** (B9, A18) | Window/input/audio ports are `stable` only after a native adapter exists; phase 2 uses GLFW for window/input and std-backed seams for clock/threads/files |
| D-6 | **Asset and file-format policy; scripting; CLI asset tools in v0** (B10, A21) | Own simple runtime formats + offline converter tool (T2); no scripting in v1; CLI tools allowed |
| D-7 | **Binding crates policy** (B8) | Add tier T1-bindings (declaration-only crates, inside adapters, pinned and listed) |
| D-8 | **Build order: job system with physics, ECS when needed** (B11) | Yes |
| D-9 | **Arena containers: own code vs pinned arena crate** (B6) | Decide in phase 1 by benchmark; default own small arena containers |
| D-10 | **Deterministic profile for D2** (B7) | Keep D2 to simulation code, measure the cost in phase 1, revisit if large |
| D-11 | **Rules digest + wiki clean-up pass; independent second review of the wiki** (B16, B20) | Yes, before phase 0 |
