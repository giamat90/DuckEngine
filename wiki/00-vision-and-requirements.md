# 00 - Vision and Requirements

## 1. The three requirements, made testable

A requirement we cannot check automatically will rot. Each one below has an **acceptance test** (how we'd know it holds) and a **metric** (see [quality-metrics](workflow/quality-metrics.md)).

### R1 - Performance: "efficient at runtime"

"Efficient" is vague, so we define it as budgets plus regression protection:

| Aspect | Proposed definition | How it is checked |
|---|---|---|
| Frame time | Reference scenes meet a budget (e.g. 60 Hz = 16.6 ms, 144 Hz = 6.9 ms) on a *named reference machine* | Benchmark scenes in CI-nightly, tracked over time |
| CPU scaling | Simulation/ECS/physics/jobs scale with core count (target: >= 70% parallel efficiency up to N cores, N to be chosen) | Scaling benchmark |
| Memory | Zero heap allocations per frame in steady state on hot paths; the standard allocator is the default (v0.6), observed through a counting global allocator and our `IAllocator`/arena layer | Allocation-counting test harness (fails the test if a frame allocates) |
| Cache behaviour | Hot data is contiguous, SoA where it matters | Micro-benchmarks + hardware-counter spot checks (manual) |
| Abstraction cost | Port overhead is bounded: **<= 1 indirect call per pass/batch**, never per object | Benchmark of "port vs raw API" for the same workload |
| Reference workloads (N-3) | 2-3 named workloads with numeric budgets on a **named reference machine**, fixed in phase 0a and recorded in `docs/budgets` (proposal: W1 2D rigid-body stack at a fixed step, W2 sprite scene at 144 Hz, W3 N simultaneous audio voices); **phase 1 does not start until they exist** | Benchmarks and scene frame times against the budgets |
| Regression | No benchmark regresses >5% (warn) / >10% (fail) vs main, noise-aware | CI benchmark comparison |

Rule of thumb: **we do not optimise without a benchmark, and we do not merge a hot-path change without one.**

### R2 - Flexibility: "no third-party lock-in"

| Aspect | Definition | Acceptance test |
|---|---|---|
| Physics | Written from scratch, no physics library anywhere | Dependency scan: zero third-party crates or imports in `physics/` |
| Graphics API | Engine code never includes or links OpenGL/Vulkan/D3D/Metal. Backends are adapters. | Dependency scan + CI job that builds the engine with *only* the Null backend |
| Audio | Same as graphics: mixer/DSP ours, device access behind a port | Same two checks |
| **Operating system** | Engine code never imports an OS crate or header or calls an OS API. Windows, input, file system, dynamic loading, lifecycle and platform queries sit behind the platform port family | Banned-import check; **headless-only build**; **two-OS build matrix** (Win32 + POSIX) |
| **Language standard library, compiler, CPU** | The standard library may be used directly (v0.6, owner). There is one compiler (rustc, pinned by `rust-toolchain.toml`), so no shim layer (card 1-37); CPU intrinsics through the `simd` abstraction | Banned-import check (OS/vendor/third-party only) |
| Any other external lib (window, file formats, ...) | Must be behind a port **or** be dev-time-only **or** be explicitly listed with an exit plan | `THIRD_PARTY.md` manifest checked in CI |
| **Portability** | Moving to a new OS/CPU/GPU API/audio API = new adapters + composition root, plus whatever port changes the new target proves necessary (Q17: ports are not pre-shaped for hypothetical targets) | Swap-diff reported for each added platform/backend; the threshold is open ([review-independent](review-independent.md) A1) |
| Swap cost | Replacing/adding a backend touches **only** the adapter module + composition root **in engine and game code**; port definition files may change, with **at most one counted port revision before the port is `stable`** (the second adapter reshapes the port) | "Second backend" milestone: engine and game sources unchanged (diff = 0 outside adapter and port files) |
| Port quality | Every port has a shared **contract test suite** that every adapter must pass | Contract suite results per adapter |

### R3 - Development discipline: TDD + measured quality

- Every behaviour change starts with a failing test.
- Quality metrics are collected on every CI run and enforced by gates (not just reported).
- Metrics are chosen with Goal-Question-Metric: each metric exists because it answers a question about R1 or R2 (see [quality-metrics](workflow/quality-metrics.md)).

## 2. What "from scratch" means (scope boundary)

**Policy (owner):** "no dependencies" is not literal - a program must run on *something*. **The operating system, graphics/audio/vendor APIs and third-party libraries are wrapped** so the engine can run on every platform. The language's own standard library is not treated as a dependency. Engine code (L0-L2) depends on the language, its standard library and ports it defines; everything else is an adapter. History is in the README changelog.

1. **Operating system, platform-specific APIs, compiler specifics, CPU ISA, driver/vendor APIs** (Win32, POSIX, Cocoa, Vulkan, D3D12, WASAPI, SSE/NEON intrinsics...). Always wrapped behind ports; reachable **only inside adapters**. The language standard library is **not** in this category (v0.6).
2. **Libraries that implement an engine capability** (physics engines, bgfx, FMOD, EnTT, GLM, GLFW, miniaudio). **Not allowed in engine libraries.** Allowed inside an *adapter* only as a temporary stopgap with a written exit plan.
3. **Dev-time tools** (test framework, benchmark lib, profiler, sanitizers, formatters). Allowed, never linked into shipped runtime binaries.

Full classification and enforcement: [ports-and-adapters](architecture/ports-and-adapters.md). All detail questions (standard-library scope, platform set, SIMD, determinism) are resolved; see [open-questions-and-risks](open-questions-and-risks.md).

## 3. Non-goals (for now) `[P]`

- Not a general-purpose Unity/Unreal replacement. **Editor descoped for v0 (owner decision, Q10): not a priority, but the door stays open.** No editor work is scheduled; the architecture only carries the low-cost "editor-ready" rules in [overview §7](architecture/overview.md).
- No scripting VM in v1 (game code is Rust against the engine API); revisit later.
- No networking/multiplayer in v1, but **determinism and fixed-timestep simulation are designed in** so it is not precluded.
- No console, mobile or web *adapters* in v1, and the ports are **not** shaped for them in advance (Q17, owner: the real platform is Windows). Ports are designed against two concrete platforms, Windows and Linux (Q14); keeping OS code behind ports means another platform later is adapter work, with whatever port changes it then proves necessary.

## 4. Quality attributes ranked `[P]`

1. Correctness (tests) 2. Performance (budgets) 3. Replaceability (ports) 4. Simplicity/readability 5. Feature breadth.

Feature breadth is deliberately last: a small engine that is fast and swappable beats a broad one that is neither.
