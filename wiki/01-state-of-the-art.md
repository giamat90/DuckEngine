# 01 - State of the Art (competitor survey)

> Note (L1): in this survey, `[S]` on forum, aggregator or marketing pages marks a *secondary* source; treat those items as `[K]`. The research track (job systems, audio, Diligent, Bevy render internals) is scheduled in the roadmap's parallel tracks before the relevant phase.

Legend: **[S]** source read this session (linked), **[K]** background knowledge - verify before relying, **[P]** our judgement.

## 1. Whole engines

| Engine | Object model | Rendering abstraction | Notable lesson for us |
|---|---|---|---|
| **Unreal** | Actor + ActorComponent (OO) [S](https://medium.com/@Modexa/unity-vs-unreal-vs-godot-next-gen-face-off-e11cc168cb34) | RHI under a **Render Dependency Graph (RDG)**: records passes, compiles the graph, culls unused passes, aliases transient memory, schedules async compute, split barriers, parallel command recording [S](https://dev.epicgames.com/documentation/en-us/unreal-engine/render-dependency-graph-in-unreal-engine) | A render graph above a thin RHI is the proven way to get both portability and performance. Barriers/lifetimes are computed by the graph, not by game code. |
| **Unity** | GameObject/MonoBehaviour; **DOTS** (archetype ECS) as the data-oriented path [S](https://medium.com/@Modexa/unity-vs-unreal-vs-godot-next-gen-face-off-e11cc168cb34) | SRP over internal graphics layer [K] | Two object models living side by side is costly; cross-entity references hurt in archetype layouts [S](https://blog.innogames.com/entity-relationships-in-unity-dots-ecs/). Pick one core model early. |
| **Godot** | Node tree [S](https://medium.com/@Modexa/unity-vs-unreal-vs-godot-next-gen-face-off-e11cc168cb34) | `RenderingDevice`: low-level abstraction over Vulkan / D3D12 / Metal for the Forward+ and Mobile renderers; a separate **Compatibility** renderer uses OpenGL and bypasses it [S](https://docs.godotengine.org/fr/4.5/classes/class_renderingdevice.html) | Even a mature engine ended up with *two* render paths because one abstraction could not cover GL and modern APIs well. Design the port after the explicit APIs and emulate on GL, accept a degraded tier. |
| **Bevy** | Pure ECS for engine + game; hybrid storage (tables by default, sparse sets opt-in) [S](https://bevy.org/news/bevy-0-5/); extensible render graph [S](https://toolradar.com/tools/bevy-engine) | Built on wgpu (an external abstraction) [K] | The closest philosophical relative (data-oriented, modular plugins). Weakness: relies on wgpu - exactly the dependency we want to avoid. Hybrid ECS storage is a strong precedent. |
| **O3DE (Atom)** | Entity/Component + Gems (modules) [K] | Atom **RHI** with DX12, Vulkan, Metal backends; a **Frame Scheduler** for GPU submission [S](https://docs.o3de.org/docs/atom-guide/dev-guide/rhi/) (page body not fully read; summary from search) | Confirms the layering RHI -> frame scheduler/graph -> renderer, and the "Gem" module idea. Heavy/complex; lesson on scope creep. |
| **The Machinery** (Our Machinery) | Central *data model* (not ECS) giving serialization, undo/redo, collaboration for free; plugin-based, DLL hot-reload, jobs everywhere, C interfaces [S](https://www.gamedeveloper.com/programming/the-machinery-goes-open-beta) | n/a | Plugin architecture with a stable C ABI and hot reload is a dev-velocity win. Company status/outcome: [K] verify before citing as success story. |

## 2. Subsystem libraries (our "competitors" per capability)

### Rendering abstraction layers

| Library | Approach | Lesson |
|---|---|---|
| **bgfx** | API-agnostic "bring your own engine" renderer; supports OpenGL among others [S](https://forum.pioneerspacesim.net/viewtopic.php?p=5980) (older forum thread - dated) | Draw-call-level abstraction hides API differences well but caps what you can express (no explicit barriers/async compute). |
| **The Forge** | Thin cross-platform layer: D3D12/Vulkan on Win, Metal on Apple, descriptor mgmt, shader reflection, multithreaded command-buffer generation; no OpenGL [S](https://github.com/divecoder/the-forge) (mirror of the repo) | Thin explicit-API-shaped abstraction = high performance, but you give up GL/legacy hardware. |
| **Filament** | Backend ("driver") layer separating engine from OpenGL 4.1+/ES 3.0+, Metal, Vulkan, WebGPU, WebGL2; custom backends possible [S](https://www.mintlify.com/google/filament/advanced/custom-backends) (third-party doc mirror - verify at the official docs). No D3D12 listed. | A command-stream between engine and backend allows swapping/adding backends cleanly. |
| **Diligent Engine** | Cross-API (D3D11/12, Vulkan, GL, Metal) [K]; search gave nothing usable | Not evaluated - to research. |

### Physics

| Library | Design points | Lesson |
|---|---|---|
| **Box2D v3** | Written in **C**; **ids instead of pointers**; data-oriented layout; new **Soft Step** solver (soft constraints + sub-stepping, replaces iteration counts) [S](https://box2d.org/posts/2024/08/releasing-box2d-3.0) [S](https://github.com/erincatto/box2d/blob/main/docs/migration.md); **graph coloring** to parallelise the solver, inspired by bepu; threads/tasks delegated to **enkiTS**; AVX2 contact solver; sanitizers + unit tests + a CSV-emitting benchmark app compared against main; reports >2x faster than v2.4 [S](https://box2d.org/posts/2024/08/releasing-box2d-3.0) | A near-perfect template for our physics *and* for our test/benchmark workflow. Note: the task system is **injectable**, a pattern we copy. |
| **Jolt Physics** | Used in Horizon Forbidden West; designed around **multithreaded streaming + game-object update** bottlenecks; **lock-free broadphase** and **lock-free island building** [S](https://guerrilla-games.com/read/architecting-jolt-physics-for-horizon-forbidden-west). Job-graph `Update`, deterministic simulation (limits documented) [S, aggregator summary](https://github.com/hyv1001/JoltPhysics) | Physics must be designed for concurrent *queries* while simulating, not only parallel stepping. Determinism is a feature to design in, with documented limits. Job system interface being pluggable is [K]. |
| PhysX / Bullet / Havok | Industry incumbents | [K] only; not studied this session. |

### ECS libraries

| Library | Storage | Lesson |
|---|---|---|
| **Flecs** | Archetypes; entity **relationships**, "structured like a database" [S](https://moonside.games/posts/archetypal-ecs-considered-harmful/) | Relationships are a powerful feature; they are also where archetype layouts get awkward. |
| **EnTT** | Sparse sets [S](https://habr.com/en/articles/651921/) | Cheap add/remove, slower big iteration. |
| **Measured trade-off** | A 2025 CGVC paper built both in C++20: sparse sets = cheaper entity modification, poor iteration scaling; archetypes = large-scale iteration via cache efficiency, costlier composition changes [S](https://diglib.eg.org/handle/10.2312/cgvc20251224). A survey reports archetypes nearly doubling iteration throughput at high entity counts [S](https://arxiv.org/pdf/2606.14919) | There is no universal winner. **Decide with our own benchmark** (ADR-0003), likely a hybrid like Bevy. |

### Audio

Searches returned only marketing/comparison pages [S, low value](https://www.g2.com/compare/fmod-vs-soloud). Facts retained: OpenAL Soft is an LGPL software implementation of OpenAL; Wwise is proprietary with an indie tier; SoLoud is a portable C/C++ library. **[K]**: miniaudio is a single-file MIT/public-domain device+decoder library with a callback (pull) model over WASAPI/CoreAudio/ALSA/etc.; FMOD and Wwise are full middleware with authoring tools. **Audio design in this wiki is therefore mostly [K]/[P]** and needs a second research pass.

### Parallelism

- **Fiber-based job system** (Naughty Dog, GDC 2015: frame-centric design, fiber jobs, memory patterns, lock strategies) [S, listing only - slides not read](https://www.gdcvault.com/play/1022186).
- **Task pool** (enkiTS under Box2D) [S](https://box2d.org/posts/2024/08/releasing-box2d-3.0).
- Whether fibers beat a good work-stealing thread pool for *our* workloads is unproven -> spike (ADR-0004).

### Abstraction cost

Virtual dispatch costs a double lookup and can add cache misses; compilers do not always devirtualise; hot paths are better served by compile-time selection, while coarse boundaries (platform, windowing, I/O) are fine with virtual interfaces [S](https://gamedev.net/tutorials/programming/general-and-gameplay-programming/using-abstraction-to-optimize-runtime-polymorphism-r2763). Opinions differ (some sources say the cost is overstated) -> **measure** [P].

### Testing / quality tooling

Mull is an LLVM-based mutation testing tool for C/C++ that uses coverage to limit work [S](https://arxiv.org/pdf/1908.01540). Other tools in this wiki (llvm-cov, lizard, clang-tidy, sanitizers, Google Benchmark, Tracy) are **[K]**.

## 3. Synthesis - what the field converges on

1. **Thin, explicit-API-shaped render port + render graph above it** (Unreal RDG, O3DE Atom, Godot RenderingDevice, The Forge). GL-style backends are emulated or relegated to a lower tier (Godot Compatibility).
2. **Data-oriented cores** (Bevy, DOTS, Box2D v3, Jolt, The Machinery) with handles/ids instead of pointers.
3. **Injectable job/task system** and graph-colored or lock-free parallel algorithms.
4. **Plugins/modules with narrow stable interfaces** (The Machinery C ABI, O3DE Gems, Bevy plugins).
5. **Benchmarks compared against main** as a first-class engineering practice (Box2D).
6. **Determinism as an explicit, documented, tested property** (Jolt).

## 4. What is genuinely new in our brief

Unreal already combines a portable RHI (§1) with first-party physics and audio `[K]`, so owning physics/audio behind swappable graphics backends is not new by itself. Bevy and Godot rely on wgpu / platform libs / middleware for parts of this. What we add is the **enforced port boundary for every dependency (including the OS) with contract tests**, not any single algorithm or the combination of from-scratch subsystems. This also means the main risk is **scope**, not technology (see [risks](open-questions-and-risks.md)).

## 5. Gaps in this survey (be aware)

- Diligent Engine, Bevy's render-graph internals, Godot's internal render pipeline, O3DE RHI page body (only partly read).
- Audio middleware internals; no authoritative source retrieved.
- Job system internals (work stealing vs fibers) - only a talk listing was retrieved.
- Third-party summaries (aggregator/mirror pages) are marked; official docs should be checked in the next pass.
- No quantitative cross-engine benchmark data was gathered; all performance targets in this wiki are **hypotheses to validate** with our own benchmarks.
