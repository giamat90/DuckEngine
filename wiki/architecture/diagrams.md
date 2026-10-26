# Architecture diagrams (UML) `[P]`

All diagrams are Mermaid text, so they are versioned with the wiki and render on GitHub. They are views of the pages named in each caption; if a diagram and a page disagree, the page wins and the diagram is fixed. Lines marked `…` are deliberately abbreviated.

Index: 1 Component view · 2 Module dependencies · 3 Ports and adapters (class) · 4 Port maturity (state) · 5 Frame loop (sequence) · 6 Render pipeline · 7 Physics pipeline · 8 Audio pipeline · 9 Composition root (object) · 10 Delivery flow (sequence) · 11 Change classes (activity)

---

## 1. Component view: who may talk to whom

Source: [overview §2-3](overview.md). Arrows are compile-time dependencies. Dashed arrows are "implements port".

```mermaid
flowchart TB
  subgraph L4["L4 Applications"]
    App["app_* / samples"]
    CR["composition root"]
  end
  subgraph L3["L3 Adapters (only place for OS / vendor / third-party code)"]
    direction LR
    GL["adapter_gl"]
    VK["adapter_vk"]
    D3["adapter_d3d12"]
    RN["adapter_render_null"]
    AU["adapter_audio_miniaudio / wasapi / null / offline"]
    PL["adapter_win32 / posix / headless / glfw"]
  end
  subgraph L2["L2 Domain modules and glue"]
    direction LR
    RD["render"]
    PH["physics"]
    AD["audio"]
    AS["assets"]
    SC["scene"]
    IN["input"]
    GLUE["frame, sync_physics, sync_render, sync_audio"]
  end
  subgraph L1["L1 Runtime"]
    direction LR
    JB["jobs"]
    EC["ecs"]
    MO["modules"]
    PP["platform (ports only)"]
  end
  subgraph L0["L0 base: language + std only"]
    BA["math, simd, handles, arena containers, assert, log iface"]
  end

  App --> CR
  CR --> GL & VK & D3 & RN & AU & PL
  CR --> GLUE
  GLUE --> RD & PH & AD & SC & EC
  L2 --> L1
  L1 --> L0
  L2 --> L0
  GL -.-> RD
  VK -.-> RD
  D3 -.-> RD
  RN -.-> RD
  AU -.-> AD
  PL -.-> PP
```

---

## 2. Module dependency graph (L1-L2)

Source: [overview §3](overview.md) module map. Domain modules never depend on each other; only glue crates do. `physics` has no dependency on `ecs`.

```mermaid
flowchart LR
  base
  platform --> base
  jobs --> base
  jobs --> platform
  ecs --> base
  ecs --> jobs
  modules --> base
  physics --> base
  physics --> jobs
  render --> base
  render --> jobs
  audio --> base
  audio --> jobs
  assets --> base
  assets --> jobs
  assets --> platform
  scene --> base
  scene --> ecs
  input --> base
  input --> platform
  frame --> base
  frame --> jobs
  sync_physics --> ecs
  sync_physics --> scene
  sync_physics --> physics
  sync_render --> ecs
  sync_render --> scene
  sync_render --> render
  sync_audio --> ecs
  sync_audio --> audio
```

---

## 3. Ports and adapters (class diagram)

Source: [ports-and-adapters](ports-and-adapters.md), [rendering](rendering.md), [audio](audio.md). Each port is declared by the module that needs it. Every port has a Null adapter and a contract test suite that all adapters must pass.

```mermaid
classDiagram
  direction LR

  class IRenderDevice {
    <<port, owned by render>>
    +capabilities() Caps
    +create_buffer(desc) BufferHandle
    +create_texture(desc) TextureHandle
    +create_pipeline(desc) PipelineHandle
    +submit(stream: CommandStream)
    +present()
  }
  class IShaderCompiler {
    <<port, owned by render, build time>>
    +compile(source, target) ShaderBlob
  }
  class IAudioDevice {
    <<port, owned by audio>>
    +start(callback: PullCallback)
    +stop()
    +format() DeviceFormat
  }
  class IAudioDecoder {
    <<port, owned by audio>>
    +open(bytes) Stream
    +read(frames) Samples
  }
  class IWindow {
    <<port, owned by platform>>
    +poll_events() Events
    +surface() SurfaceHandle
  }
  class IFileSystem {
    <<port, owned by platform>>
    +mount(path, source)
    +read(path) Bytes
    +read_background(path) Request
  }
  class IClock {
    <<port, owned by platform>>
    +now_seconds() f64
  }
  class IThreads {
    <<port, owned by platform>>
    +spawn(job)
    +hardware_threads() usize
  }

  class GlDevice { <<adapter>> }
  class VulkanDevice { <<adapter>> }
  class D3D12Device { <<adapter>> }
  class NullRenderDevice { <<adapter, test>> }
  class MiniaudioDevice { <<adapter, T1-stopgap>> }
  class WasapiDevice { <<adapter, native>> }
  class NullAudioDevice { <<adapter, test>> }
  class OfflineAudioDevice { <<adapter, test>> }
  class GlfwWindow { <<adapter, T1-stopgap>> }
  class Win32Window { <<adapter, native>> }
  class HeadlessWindow { <<adapter, test>> }

  IRenderDevice <|.. GlDevice
  IRenderDevice <|.. VulkanDevice
  IRenderDevice <|.. D3D12Device
  IRenderDevice <|.. NullRenderDevice
  IAudioDevice <|.. MiniaudioDevice
  IAudioDevice <|.. WasapiDevice
  IAudioDevice <|.. NullAudioDevice
  IAudioDevice <|.. OfflineAudioDevice
  IWindow <|.. GlfwWindow
  IWindow <|.. Win32Window
  IWindow <|.. HeadlessWindow

  class ContractSuite {
    <<test>>
    +run_against(adapter)
  }
  ContractSuite ..> IRenderDevice : verifies
  ContractSuite ..> IAudioDevice : verifies
  ContractSuite ..> IWindow : verifies
```

---

## 4. Port maturity (state machine)

Source: [ports-and-adapters §rule of two](ports-and-adapters.md). Window, input and audio ports can leave `experimental` only after a native adapter exists next to the stopgap.

```mermaid
stateDiagram-v2
  [*] --> Draft : port written by the module that needs it
  Draft --> Experimental : Null adapter + contract suite green
  Experimental --> Experimental : adapter 2 changes the port (counted revision)
  Experimental --> Stable : two real adapters pass, window/input/audio need a native one
  Stable --> Stable : engine and game swap-diff = 0 when an adapter is swapped
  Stable --> Experimental : Class 3 decision reopens the port
```

---

## 5. One frame (sequence diagram)

Source: [conventions §2-3](conventions.md), [core-runtime](core-runtime.md), [physics](physics.md). The application owns the loop. Glue crates are the only code that knows two domain modules. Fixed step for simulation, variable step for presentation.

```mermaid
sequenceDiagram
  autonumber
  participant App as Application (owns loop)
  participant Frame as frame (glue)
  participant Input as input
  participant SyncP as sync_physics
  participant Phys as physics
  participant ECS as ecs + scene
  participant SyncR as sync_render
  participant Rend as render
  participant SyncA as sync_audio
  participant Aud as audio
  participant Dev as IRenderDevice adapter

  App->>Frame: tick(dt_real)
  Frame->>Input: poll and map actions
  loop fixed steps while accumulator >= dt
    Frame->>SyncP: push targets and forces (ECS -> physics)
    SyncP->>Phys: step(dt) as jobs, deterministic
    Phys-->>SyncP: transforms and contact events
    SyncP->>ECS: write transforms, raise events
  end
  Frame->>ECS: variable-step systems (animation, camera)
  Frame->>SyncA: translate events
  SyncA->>Aud: audio commands (lock-free queue)
  Frame->>SyncR: extract RenderScene snapshot
  SyncR->>Rend: RenderScene (plain data)
  Rend->>Rend: build render graph, record POD command stream
  Rend->>Dev: submit(stream) once per pass
  Dev-->>App: present
```

---

## 6. Render pipeline

Source: [rendering](rendering.md). The renderer consumes only a plain `RenderScene`. Backends decode a POD command stream in bulk, so virtual dispatch is per pass, not per draw.

```mermaid
flowchart LR
  RS["RenderScene snapshot (plain data)"] --> Cull["visibility and sorting jobs"]
  Cull --> RG["render graph: passes, resources, barriers"]
  RG --> CS["POD command stream per pass"]
  CS -->|"one virtual submit per pass"| Dev["IRenderDevice adapter"]
  Dev --> GPU[("GPU API: GL 4.5, Vulkan, D3D12")]
  SH["shader sources"] -.->|"build time"| SC["IShaderCompiler (external tool)"]
  SC -.-> Blob["shader blobs in asset pack"]
  Blob --> Dev
```

---

## 7. Physics pipeline (Soft Step shape)

Source: [physics](physics.md). Standalone and handle-based, with a dimension-generic core (2D is the D = 2 instance). Determinism rules D0/D1 required, D2 not pursued (v0.25).

```mermaid
flowchart TB
  In["inputs: kinematic targets, forces, seed, fixed dt"] --> BP["broadphase"]
  BP --> NP["narrowphase: contact manifolds"]
  NP --> GC["constraint graph coloring"]
  GC --> SS["Soft Step solver: substeps, relax, restitution"]
  SS --> Int["integrate (SIMD, bit-identical to scalar)"]
  Int --> Out["outputs: transforms, contact events (sorted deterministically)"]
  Out --> Hash["replay hashes of two runs and of 1 and N threads compared with each other (D0, D1)"]
  subgraph Jobs["executed on the job system"]
    BP
    NP
    GC
    SS
    Int
  end
```

---

## 8. Audio pipeline

Source: [audio](audio.md). Pull model: the device thread asks the mixer for samples. The game thread never blocks the device thread; changes arrive as commands through a lock-free SPSC queue and a mixer-graph snapshot.

```mermaid
sequenceDiagram
  participant Game as game thread (sync_audio)
  participant Q as SPSC command queue
  participant Mix as mixer graph snapshot
  participant Dev as IAudioDevice adapter thread

  Game->>Q: play, stop, set parameter
  Game->>Mix: publish new snapshot (old one reclaimed later, off the audio thread)
  loop every device buffer
    Dev->>Mix: pull(frames)
    Mix->>Q: drain commands
    Mix->>Mix: voices, DSP nodes, bus mix
    Mix-->>Dev: samples
  end
```

---

## 9. Composition root (object diagram)

Source: [overview §4](overview.md). Concrete adapters are named only here. Swapping one is a change in this one place.

```mermaid
classDiagram
  direction LR
  class CompositionRoot {
    <<app_* main>>
  }
  class Platform
  class Allocators
  class Renderer
  class AudioService
  class PhysicsWorld
  class GlDevice { <<L3>> }
  class MiniaudioDevice { <<L3>> }
  class GlfwWindow { <<L3>> }

  CompositionRoot --> Platform : create
  CompositionRoot --> Allocators : create
  CompositionRoot --> Renderer : create(GlDevice)
  CompositionRoot --> AudioService : create(MiniaudioDevice)
  CompositionRoot --> PhysicsWorld : create(config)
  Renderer o-- GlDevice : IRenderDevice
  AudioService o-- MiniaudioDevice : IAudioDevice
  Platform o-- GlfwWindow : IWindow
```

---

## 10. Delivery flow (sequence diagram)

Source: [delivery-pipeline](../workflow/delivery-pipeline.md), [tdd-workflow](../workflow/tdd-workflow.md). The owner approves once, before the PR, against a diff hash. Everything after approval is automatic.

```mermaid
sequenceDiagram
  autonumber
  actor PO as Owner (product owner / architect)
  participant CC as Claude Code (software team)
  participant CI as GitHub CI (gates from trusted main)
  participant Rel as Release workflow

  PO->>CC: task definition
  CC->>CC: test list, red / green / refactor (TDD)
  CC->>CC: review phase R1-R8, fix loop (max 3)
  CC->>PO: design note + metric report + product view (diff hash)
  PO-->>CC: approve (once)
  CC->>CI: open PR
  CI->>CI: classifier, tests, coverage, mutation, lint, perf, determinism
  alt all green
    CC->>CC: squash-merge after green CI (Free plan: no auto-merge)
    CI->>Rel: merge event
    Rel->>Rel: version from Conventional Commits (every merge bumps)
    Rel-->>PO: private GitHub Release (source workspace, tools, samples, metrics)
  else red
    CI-->>CC: failures
    CC->>CC: fix and re-run (pause for owner only if product behaviour or metrics move)
  end
```

---

## 11. Change classes (activity diagram)

Source: [rules-digest §8](../rules-digest.md). The classifier is a tool, not the agent; the highest class wins and unsure rounds up.

```mermaid
flowchart TD
  Start(["diff"]) --> Cls{"classifier (trusted main)"}
  Cls -->|"gate config, workflows, ADRs, digest, CLAUDE.md"| C3["Class 3 governance"]
  Cls -->|"public API / port, new dependency, unsafe, concurrency, SIMD, determinism math, hot path"| C2["Class 2 sensitive"]
  Cls -->|"everything else"| C1["Class 1 routine"]
  C3 --> G{"task declared a governance task by the owner?"}
  G -->|no| Rej["rejected"]
  G -->|yes| Own["owner reads the diff"]
  C2 --> Ind["independent review + owner reads summary"]
  C1 --> Auto["automated gates only"]
  Own --> Merge(["approval, PR, merge"])
  Ind --> Merge
  Auto --> Merge
```
