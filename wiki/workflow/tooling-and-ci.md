# Tooling and CI Proposal `[P]` (tool names are `[K]` unless linked)

## 1. Toolchain

| Concern | Proposal | Notes |
|---|---|---|
| Language | **Rust** (stable, pinned in `rust-toolchain.toml`; nightly only for Miri and sanitizers) | ADR-0001, decided by owner (Q1) |
| Build | Cargo workspace, one crate per module/adapter | the crate graph (`cargo metadata`) makes the link-graph rule checkable |
| Compiler | `rustc` (LLVM backend); Windows target `x86_64-pc-windows-msvc` (needs the MSVC linker/Build Tools), Linux `x86_64-unknown-linux-gnu`; ARM64 later | no clang requirement; mutation testing works with the normal toolchain ([Q7](../open-questions-and-risks.md)) |
| Platforms (v1) | Windows 11 (dev machine) + Linux (CI, WSL locally) = **two different OS families**, which is what proves the platform ports (rule of two); headless platform always | TSan is not available on MSVC; Linux gives us TSan and broader CI. macOS/Android/Web/consoles: design constraints now, adapters later (Q14) |
| Banned-import enforcement | dedicated CI job + denylist script | proves L0-L2 contain no OS/vendor/third-party dependency (standard library allowed) |
| CPU architectures | x86-64 first; ARM64 added to the CI matrix early (cloud ARM runner or emulation) so the `simd` scalar/ISA split is exercised (Q15) | |
| Formatting | rustfmt (check-only in CI) | |
| Static analysis | Clippy (default + selected `pedantic`/`nursery` lints, `-D warnings`), `cargo-deny` (advisories, licences, bans) | |
| Unit test framework | built-in `cargo test` (plus `cargo-nextest` for speed) | no external framework needed ([Q6](../open-questions-and-risks.md)) |
| Property-based | `proptest` (dev-only, T2) or in-house generator on our RNG | |
| Benchmarks | `criterion` or `divan` (T2) | CSV/JSON output compared with main (Box2D practice [S](https://box2d.org/posts/2024/08/releasing-box2d-3.0)) |
| Profiling | Tracy adapter (T2) via our profiling hook | |
| Coverage | `cargo-llvm-cov` | |
| Mutation | `cargo-mutants` `[K, verify in phase 0]` | slower and younger than Mull, works on Windows and Linux; see [quality-metrics](quality-metrics.md) |
| Complexity / duplication | Clippy `cognitive_complexity` and size lints, `rust-code-analysis` `[K]`, jscpd | |
| Architecture checks | in-house script (Python or a Rust tool) reading `cargo metadata` + `use` statements; `unsafe` scan | part of the repo, itself unit-tested |
| Fuzzing | `cargo-fuzz` (libFuzzer) | |
| Version control | git (the folder is not a repo yet) | `git init` in phase 0 |

## 2. Repository layout (proposal)

```
DuckEngine/
  wiki/                 <- this documentation
  docs/adr/             <- accepted ADRs (moved from wiki/decisions when accepted)
  THIRD_PARTY.md
  Cargo.toml            <- workspace; rust-toolchain.toml pins the compiler
  engine/
    base/               L0
    platform/           L1 interfaces
    jobs/ ecs/ modules/ L1
    physics/ render/ audio/ assets/ scene/ input/   L2  (each crate: src/, tests/, benches/)
  adapters/
    render_null/ render_gl/ render_vk/ render_d3d12/
    audio_null/ audio_offline/ audio_wasapi/
    platform_win32/ platform_posix/ platform_headless/   (later: cocoa, android, web)
    (ISA backends live in `base` behind `cfg(target_arch)`; `cpu_*` crates only if the N-6 spike chooses the generic top-down design)
  contracts/            <- shared port contract test suites
  apps/                 <- samples, composition roots
  tools/                <- shader tooling, asset pipeline, arch-check, metrics
  metrics/              <- generated reports (ignored or archived)
```

## 3. CI pipeline

```mermaid
flowchart LR
  A[PR opened] --> B[G1: build matrix Rust stable, Win/Linux]
  B --> C[unit + acceptance + golden]
  C --> D[ASan nightly + Miri run]
  C --> E[coverage + mutation-on-diff]
  B --> F[arch checks: include/link graph, banned imports, manifest, headless-only build, two-OS contract matrix]
  B --> G[static analysis + format + warnings]
  D & E & F & G --> H{all green?}
  H -->|yes| I[merge]
  I --> J[G2 nightly: TSan, fuzz, full mutation, benchmarks on perf box, GPU/audio contract + fidelity]
  J --> K[trend report + regression alerts]
```

Hosting (decided by owner, Q11): **private GitHub repository**, not open source. GitHub Actions is the CI. Consequences of a *private* repo:

- **Usage limits apply.** Private repos on a free plan get a limited monthly pool of Actions minutes, and Windows/macOS minutes are billed at a multiplier **[K - verify current GitHub plan limits and multipliers]**. Our G1 matrix (Rust x Windows/Linux, sanitizers, mutation-on-diff) and nightly jobs can burn this quickly, so: keep G1 lean (diff-based mutation, caching with ccache/sccache), push heavy jobs (full mutation, fuzz, benchmarks) to nightly or to a self-hosted runner, and measure minutes per run as a process metric.
- **Runners (D-3):** start with **GitHub-hosted runners**. GPU/audio contract tests and golden-image tests use **software rasterizers** (Mesa llvmpipe for OpenGL, lavapipe for Vulkan, WARP for D3D) `[K, verify V7]`; real-GPU checks run on demand on the owner's machine. Any **self-hosted runner** runs in a VM or under a separate Windows user with no access to the parent workspace or the owner's keys, never under the owner's own account.
- **Gate tools run from the trusted base (N-2):** the classifier, arch-check, `governance-guard` and the threshold configuration are executed from a trusted checkout of `main` (or a pinned release); the PR tree is only data. A change to these tools therefore takes effect for later PRs, never for the PR that changes them. The exact mechanism (reusable workflow pinned to a ref, or a base-branch checkout step) is chosen in phase 0b/0c `[K]`.
- **PR performance gate on deterministic instruction counts** (iai-style tools, Linux `[K]`); wall-clock benchmarks run nightly on a dedicated machine for the trend report.
- Actions minutes and AI usage cost per run are tracked as process metrics from the first week.
- No secrets in the repo or logs; CI secrets only on protected branches. (Note: the `GiaMat90` workspace contains key and SSH files; the DuckEngine repo root must be the `DuckEngine` folder only, never the parent.)

## 7. Claude Code operating setup (Q21c: Claude Code drives development)

Feature names below are from background knowledge of Claude Code **[K]**; verify against the current Claude Code docs when setting up in phase 0. Principle: *instructions are advisory, hooks and CI are enforcement.*

| Mechanism | Use |
|---|---|
| `CLAUDE.md` at the repo root | Thin file that imports [rules-digest](../rules-digest.md) (layers, wrapped vs not wrapped, determinism rules, measurable limits, TDD loop, gate commands, protected paths). The wiki remains canonical; a CI check compares the digest numbers with the gate configuration (D-11) |
| Project `.claude/settings.json` permissions | Deny edits to protected paths (gate config, thresholds, arch-check, classifier, contract suites, CI, ADRs, the rules digest) in agent sessions; **deny reads outside `DuckEngine/`** (the parent folder holds `Api-keys.txt` and `GiaMat90_ssh.txt`); sessions start with `DuckEngine` as the working directory |
| Hooks (run by the harness, not by the model) | Block edits to protected paths and attempts to bypass checks (`--no-verify`, disabling tests); after edits run the fast gates (format, build, touched-module tests) so failures reach the agent immediately |
| `software-team` mode and subagents (`.claude/agents/`) | Claude Code works as a software team (PM, architect, developer, QA, security, DevOps, reviewer; mapping in [delivery-pipeline §1](delivery-pipeline.md)), with restricted tools per role, e.g. `qa`/`test-author`, `developer`, `reviewer` (read-only), `design-note-checker`, `auditor` |
| Skills / slash commands | `/task` (task card), `/gates` (local G0/G1), `/metrics` (generate report), `/review` (runs the review phase and writes `reviews/<task-id>.md`), built-in skills used by the review phase: `simplify`, `code-review`, `security-review` ([clean-code-and-review §4](clean-code-and-review.md)) |
| **CI on GitHub Actions** | **Authoritative.** The same gates run server-side, because local hooks can be bypassed by a misbehaving session. Branch protection requires G1 |
| Run mode (**confirmed by owner**) | **Per task, autonomous until all checks pass;** then the owner accepts via manual product test or metrics; only then PR -> auto-merge -> tag -> deploy via GitHub Actions ([delivery-pipeline](delivery-pipeline.md)). One approval only (the PO's, before the PR); no permission-prompt gesture and no GitHub review. Agent token scoped to this repo ([delivery-pipeline §4](delivery-pipeline.md)). No unattended/scheduled runs |

## 6. Private-repo hygiene (phase 0)

- `git init` inside `DuckEngine/`, private GitHub remote, `.gitignore`/`.gitattributes`, `CONTRIBUTING.md` renamed in practice to a `WORKFLOW.md` for yourself (TDD workflow + gates + self-review checklist), PR template with the checklist. Governance paths (gate config, thresholds, arch-check, contract suites, CI, ADRs, `THIRD_PARTY.md`) are protected by Claude Code permissions/hooks, rulesets and the `governance-guard` check, not by a GitHub review step ([tdd-workflow §10](tdd-workflow.md)). Add an agent instruction file (`CLAUDE.md`/`AGENTS.md`) and a task-card template in phase 0. Vulkan/D3D/GL validation tooling is added as dev-time tools when those adapters appear.
- Branch protection on `main`: required status checks = G1; no direct pushes. Branch protection and rulesets on **free private repositories are probably not available** `[K, verify V1]`; a local pre-push hook is **not** enforcement against an agent. If the plan lacks them, either pay for a tier that has them (preferred) or accept explicitly that merging is policy, not enforcement (Claude Code merges only after `gh pr checks --watch` is green).
- `LICENSE`/copyright header: proprietary "all rights reserved" notice (Q18), no public license.
- `THIRD_PARTY.md` has a **license column**; CI check that every vendored dependency's license permits closed-source redistribution (no GPL/AGPL in the tree; LGPL such as OpenAL Soft only as an optional dynamically-linked adapter, never required), so commercialisation stays possible.
- Keep copyright notices and authorship clean from commit 1; if anyone else contributes, obtain written IP assignment first.

## 4. Local developer experience

- `tools/check` runs the G0 subset (format, build, fast tests, arch checks) in seconds.
- Watch mode for the inner TDD loop on the module being worked on.
- One-command `tools/metrics` produces the same report as CI.

## 5. Phase-0 deliverables (tooling only, no engine features)

1. Repo + Cargo workspace skeleton, one trivial crate with a test, a benchmark and a mutation run to prove the whole chain works end-to-end ("walking skeleton" of the *workflow*). This slice validates gates G1, G2 and G5 of [ADR-0001 evaluation §6](../decisions/adr-0001-language-evaluation.md); G3 and G4 are validated in phase 1, G6 in phase 2.
2. CI running G1 on that module.
3. Arch-check script with its own tests (including a deliberately-violating fixture that must fail).
4. Baseline metrics report.
