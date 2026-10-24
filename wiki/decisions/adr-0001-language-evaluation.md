# ADR-0001 Implementation language: evaluation `[P]` / `[K]`

Status: **DECIDED by owner (v0.8): Rust.** The competition was narrowed to Rust vs C++ only (v0.7). C++20 stays documented as the contingency; the phase-0 walking skeleton validates the toolchain gates in section 6.
Confidence: everything about languages and tools below is **background knowledge `[K]`, not verified this session**. Items marked "verify" must be checked in a phase-0 spike before the decision is final.

## 1. What changed since the first proposal

ADR-0001 originally proposed C++20, mainly because the metrics chain (Mull, sanitizers, llvm-cov) and driver interop are proven there. Two owner facts change the weighing:

1. **The owner does not write or read the code** (Q19, Q21c). All code is written by Claude Code. So "which language does the owner like" is irrelevant; what matters is which language makes **AI-written code safest and most verifiable by machines** (compiler, tests, metrics).
2. **Policy narrowed by the owner (v0.6):** the language's standard library (`malloc`/`new`, containers, math, threads) may be used directly. Only the OS, graphics/audio/vendor APIs and third-party libraries are wrapped. So the language no longer needs a freestanding mode, and the standard library's quality now counts as an advantage.

## 2. Criteria (from the requirements)

| # | Criterion | Why it matters here |
|---|---|---|
| C1 | Runtime performance and control (memory layout, SIMD, no GC, no hidden allocation) | R1 Performance |
| C2 | Quality of the standard library for engine needs (containers, threads, atomics, math, allocators) | v0.6 policy: stdlib is used directly |
| C3 | Compiler-enforced safety (memory, data races) | owner does not read code; AI mistakes must be caught by machines |
| C4 | Metrics and test toolchain (coverage, mutation, sanitizers, benchmarks, lint, licence manifest) | R3 |
| C5 | Ability to express the design (dimension-generic physics core, handle-based APIs, ports as interfaces) | Q20 option B |
| C6 | Interop with graphics/audio/OS APIs and shader tools | adapters |
| C7 | Platform reach (Windows, Linux, ARM64 now; consoles/mobile/web later) | Q14, Q17 |
| C8 | Build and iteration speed | TDD loop and CI minutes (private repo) |
| C9 | Maturity and stability of language and tools | multi-year project |
| C10 | Quality of AI-generated code in that language `[K]` | sole implementer is Claude Code |

## 3. Alternatives

### 3.1 C++20
**Pros**
- Industry standard for engines (Unreal, O3DE, Jolt, Filament, The Forge); every graphics/audio/OS API has first-class C or C++ headers (C6, C7).
- Templates/concepts express the dimension-generic physics core and SIMD wrappers directly (C5).
- Mature metrics chain: llvm-cov, sanitizers (ASan/UBSan/TSan), Mull mutation testing (needs clang), clang-tidy, lizard (C4).
- Console SDKs are C++ first `[K]` (not needed in v1).
- Full control of layout, allocators, intrinsics (C1).

**Cons**
- No compiler-enforced memory or thread safety (C3); undefined behaviour is silent. AI-written C++ is the riskiest combination for an owner who reads no code; safety then depends entirely on sanitizers and tests.
- Exceptions and RTTI are switched off by project rule; `std` pieces that need them are avoided. Some `std` containers are not ideal for hot paths (allocation, node-based maps), so specialised containers are written where a benchmark justifies them (C2).
- Slow compile times, header/ODR pitfalls, many ways to do one thing (C8, C10).
- Build system (CMake) and dependency handling are a chronic source of friction.

### 3.2 Rust (`std` allowed; `unsafe` confined to named modules)
**Pros**
- **Good standard library for the job**: `Vec`, `HashMap`, slices, atomics, threads and channels are available directly, and safe containers reduce how much `unsafe` we write ourselves (C2). `#![no_std]` stays available if a restrictive platform ever needs it.
- **Memory and data-race safety by default** (C3). Safe code cannot have use-after-free or data races; `unsafe` is explicit, greppable and can be counted as a metric and restricted to named modules (allocators, SIMD, FFI, ECS internals). This is the strongest reason: it turns a class of AI mistakes into compile errors.
- Integrated toolchain: `cargo test` (unit, doc and integration tests), Clippy lint, rustfmt, coverage via LLVM (cargo-llvm-cov), mutation testing (cargo-mutants), benchmarks (criterion), licence/dependency policy (cargo-deny, which directly implements our `THIRD_PARTY.md` check), Miri for detecting undefined behaviour in `unsafe` code (C4) `[K, verify each]`.
- Traits and const generics suit the dimension-generic core and ports (C5). Ports are traits; Null adapters are easy.
- Bevy (ECS, render graph) shows engine-scale viability `[S]` (see state of the art); the ecosystem includes Rust ports of Box2D-like physics `[K]`.
- Fast incremental feedback through the compiler's strictness; refactoring is safer (C10 is generally good for compiler-guided loops `[K]`).

**Cons**
- Remaining `unsafe` is concentrated in allocators, SoA tables, lock-free queues, SIMD and FFI. There the safety benefit shrinks, and Miri/TSan coverage is thinner than for C++ `[K]`. Using `std` containers where possible keeps this smaller than a no-stdlib design.
- Borrow checker friction with graph-like, cross-referencing data; the data-oriented answer (handles/indices into arrays) is already our design, so this is a smaller issue than usual.
- SIMD: stable `std::arch` intrinsics are per-architecture; portable SIMD may still be nightly-only `[K, verify]`. Our `simd` abstraction wraps this either way.
- No stable Rust ABI: plug-in modules/DLLs need a C ABI boundary (already planned: C-compatible module boundaries).
- Longer clean builds, large `target/` directories; CI minutes matter on a private repo (C8).
- Graphics/OS API bindings are not "free": we must write or generate raw FFI declarations inside adapters (allowed: T1 inside adapters). Using community binding crates is a T1-stopgap and needs an exit plan.
- Console and some proprietary toolchains are less supported `[K]` (out of scope for v1, Q14/Q17).
- Mutation testing and some metric tools are younger than Mull/clang-tidy `[K]`.
- Two-language boundary exists anyway for shader tools (CLI, so harmless) and any C-only system library.

### 3.3 C (C11/C17)
**Pros**: simplest language; stable ABI; Box2D v3 shows a high-performance engine core in C; fastest compiles; same clang-based metrics chain as C++ (C4). **Cons (C2):** the standard library has no containers, so every vector, map and pool must be written by hand.
**Cons**: no generics (C5): the dimension-generic physics core, SIMD wrappers and typed containers need macros or code generation; no safety (C3); weakest abstraction mechanism for ports (hand-written vtables); AI-written C is easy to get subtly wrong.

### 3.4 Zig
**Pros**: explicit allocators everywhere; `comptime` generics; excellent C interop and cross-compiling (C1, C5, C6). Standard library is small and still changing (C2).
**Cons**: pre-1.0, language and std change between releases (C9); thin metrics tooling (no mature mutation testing or coverage ecosystem `[K]`); no compiler-enforced safety beyond runtime checks in debug builds; less training data for AI (C10).

### 3.5 Others considered and set aside
- **D**: GC-optional, good metaprogramming, but small ecosystem and tooling for our metrics chain.
- **Odin / Jai**: game-oriented, nice data-oriented ergonomics; too young/closed for a long metrics-governed project.
- **Swift, Go, C#, Java**: garbage-collected or runtime-heavy; fail C1/C2.
- **Mixed (Rust or C++ core + scripting)**: scripting is a later question, not part of this decision.

## 4. Comparison (judgement `[P]`, 1 = poor, 5 = excellent)

| Criterion | C++20 | Rust | C | Zig |
|---|---|---|---|---|
| C1 Performance and control | 5 | 5 | 5 | 5 |
| C2 Standard library for engine needs | 5 | 5 | 2 | 3 |
| C3 Compiler-enforced safety | 1 | 4 | 1 | 2 |
| C4 Metrics/test toolchain | 5 | 4 | 4 | 2 |
| C5 Express the design | 5 | 4 | 2 | 4 |
| C6 API/driver interop | 5 | 3 | 5 | 5 |
| C7 Platform reach (v1: Win/Linux/ARM64) | 5 | 4 | 5 | 4 |
| C8 Build/iteration speed | 2 | 2 | 5 | 4 |
| C9 Maturity/stability | 5 | 4 | 5 | 2 |
| C10 AI-code quality | 3 | 4 | 3 | 2 |

Unweighted totals: C++ 41, Rust 39, C 37, Zig 33. The totals are close for C++ and Rust, so the decision rests on **which weaknesses the process can neutralise** (C3 safety is the criterion that separates them):

- C++'s main weakness (C3, safety) can be reduced only by tests and sanitizers, which the owner cannot supplement by reading code.
- Rust's main weaknesses (C6 interop, C8 speed, `unsafe` cores) can be reduced by structure: confine `unsafe` and FFI to named modules, count them as metrics, accept slower builds.

## 5. Recommendation `[P]`

**Primary: Rust**, with `unsafe` confined to named modules and counted, a C ABI at module and plug-in boundaries, and FFI only inside L3 adapters.
**Fallback: C++20** (exceptions and RTTI off, standard library allowed) if the phase-0 spike fails any of its gates.

Reasoning: the owner reads no code, so compiler-enforced safety and a cargo toolchain where policies (licence manifest, unsafe count, lint) are enforced by tooling rather than custom scripts are worth more than C++'s ecosystem advantage. After the v0.6 policy change the "freestanding" argument for Rust is gone; the case now rests on C3 (safety) and C4 (integrated tooling). C++'s advantages (console SDKs, vendor headers, mature Mull) are largely outside v1 scope or sit in adapters. If safety is not valued enough, C++20 is a legitimate choice.

I changed my own earlier recommendation (C++20) because the owner's constraints changed the weighting, not because of new evidence about the languages.

**v0.7 note:** the owner made cross-platform identical results a goal (D2). Rust helps here: floating-point operations are strict IEEE with no implicit contraction or fast-math, and the compiler gives no surprise reassociation. In C++ the same result needs explicit flags (`-ffp-contract=off`, no `-ffast-math`) and discipline across compilers. Both languages can reach D2; Rust has fewer ways to break it by accident.

## 6. Toolchain validation gates (Rust decided; numeric criteria, N-5)

The gates are validated where the relevant code exists, not all in phase 0a. Numbers are proposals `[P]`, calibrated on the first measurement; if a gate cannot be met the owner decides whether to reopen Q1 (C++20 is the documented contingency).

| Gate | Where validated | Pass criterion |
|---|---|---|
| **G1 Mutation testing** | phase 0a (dummy crate + one small real module with an `unsafe` function) | `cargo-mutants` finishes within 15 min on a hosted Windows and a hosted Linux runner; score >= 70% on a deliberately well-tested module and < 50% on a deliberately weakly tested control (it discriminates) |
| **G2 CI chain** | phase 0a | coverage, rustfmt, Clippy, `cargo-deny`, benchmark and test jobs green on Windows and Linux in <= 10 min wall for the dummy workspace |
| **G3 `unsafe` containment** | phase 1 | `unsafe` is <= 5% of lines in L0-L2 crates and every block sits in an allowlisted module with a `// SAFETY:` comment |
| **G4 SIMD** | phase 1 | the SIMD kernel (8-wide f32 add/mul/min) is >= 2x faster than its scalar version on the reference machine (factor refined after first measurement) and bit-identical to scalar on x86-64 and ARM64 |
| **G5 Build cost** | phase 0a | PR pipeline without mutation <= 15 min wall; projected monthly Actions minutes at 20 PRs/month fit the plan's allowance (V1) |
| **G6 FFI** | phase 2 | one call each to Win32 and POSIX (monotonic time, open file) through hand-written declarations for small APIs and declaration-only binding crates (T1-bindings) for large ones, inside adapters only, passing the platform contract test |


## 7. Decision (Q1)

**Decided by the owner: Rust.**
