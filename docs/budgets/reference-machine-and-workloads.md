# Reference machine and reference workloads (N-3)

Status: fixed on 2026-10-24 by the owner (task 0a-3). The numeric budgets are **provisional hypotheses**: phase 0c calibrates them against real measurements and they only ratchet tighter after that, never looser without an owner-approved change to this file. The workload definitions (what is measured) are fixed; phase 1 does not start without this file.

Sources: [00-vision-and-requirements](../../wiki/00-vision-and-requirements.md) (N-3), [roadmap](../../wiki/workflow/roadmap.md) (phase 0a), [ADR-0001 evaluation](../../wiki/decisions/adr-0001-language-evaluation.md) section 6.

## 1. Reference machine

The owner's development laptop. Every wall-clock budget below is judged on this machine only.

| Item | Value |
|---|---|
| Model | MSI Katana GF66 12UC |
| CPU | 12th Gen Intel Core i7-12700H, 14 cores / 20 threads (6 performance + 8 efficiency), 24 MB L3 |
| RAM | 15.7 GB usable (2 x 8 GB, 3200 MT/s) |
| GPU | NVIDIA GeForce RTX 3050 Laptop (driver 32.0.15.6109) and Intel Iris Xe (driver 30.0.101.2079) |
| OS | Windows 11 Home, build 26200 |
| Audio | the default Windows output device; W3 is measured offline and needs no device |
| Toolchain | the version pinned in `rust-toolchain.toml`, release profile (`panic = abort`) |

Hardware values were read from the machine on 2026-10-24. A change of hardware, driver major version or OS build is recorded here with a date; budgets are then re-measured, not edited.

### Measurement conditions (needed for reproducible numbers)

A laptop is noisy, so every budget run follows this protocol:

1. Plugged in to mains power, power plan set to **High performance** (the plan was `Balanced` when the spec was read, so it must be switched for budget runs).
2. No other heavy application running (browser, game launcher, video call); antivirus scan not active.
3. Release build, one warm-up pass discarded, then at least 30 timed iterations; the reported value is the **median**, the 95th percentile is recorded next to it.
4. Single-thread workloads are pinned to one performance core; multi-thread workloads state their thread count.
5. A run whose median differs by more than 5% from the previous run on an idle machine is repeated; if it stays noisy the noise is reported with the result.

The **CI gate does not use wall-clock time**. It compares deterministic instruction counts on GitHub-hosted Linux runners (warn 5%, fail 10%, see rules digest section 7). The wall-clock budgets below are checked on the reference machine in the nightly/on-demand benchmark run and in the metric report.

## 2. Reference workloads

| ID | Workload | Fixed parameters | Provisional budget (reference machine) |
|---|---|---|---|
| W1 | 2D rigid-body stack | 1,000 boxes of 0.5 m x 0.5 m in a pyramid on a static ground, density 1 kg/m2, friction 0.6, restitution 0, gravity 9.81 m/s2 along -Y; fixed step 1/60 s; 10 velocity and 4 position solver iterations; 300 steps after the first contact; one thread | <= **2.0 ms** median per step; the pyramid is still standing after 300 steps (no box moved more than 0.05 m from its rest position); the replay hash of the final state is stable (D0) |
| W2 | Sprite scene at 144 Hz | 10,000 sprites of 32 x 32 px from one 2048 x 2048 atlas, 20% moving every frame, orthographic camera, 1920 x 1080 target, one sprite batcher pass; 1,000 timed frames | <= **6.9 ms** median total frame time (144 Hz); CPU time to build the command stream <= **1.5 ms**; zero heap allocations per frame |
| W3 | Simultaneous audio voices | 128 voices playing looped 16-bit mono 48 kHz samples with per-voice gain and pan, mixed to a stereo bus at 48 kHz, 512-frame blocks (10.7 ms per block), offline renderer, 10,000 blocks | <= **1.0 ms** median to mix one block (about 10% of the block time); zero heap allocations and zero locks in the mix call |

Parameters that the lines above leave open (random seed, texture content, sample content) are fixed when each benchmark is written and stored next to it; the seed is constant so the workload is identical on every run.

## 3. What these numbers are not

- They are not a promise for other hardware; they are the yardstick that makes "fast enough" a measured statement.
- They are hypotheses until phase 0c: if the first honest measurement shows a budget is unreachable or far too loose, the owner is told with the numbers and decides; the budget is never quietly changed.
- The budget for W1 to W3 is checked from the phase in which the matching module exists (W1 phase 4, W2 phase 3, W3 phase 5); until then only the definition is binding.
