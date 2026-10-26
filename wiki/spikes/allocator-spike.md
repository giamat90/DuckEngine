# Allocator spike: custom allocators vs the default allocator `[S]`

Spike for DuckEngine, run on 2026-10-25. Question from the owner: *why do we need explicit allocators, and what do they actually buy when used the way the engine will use them?* This page is the evidence copy of that report. The spike code and the raw measurements are not engine code and are not in this repository (they are kept in the folder `RustAllocator`, next to the repository); the numbers below are from that run. Claims about the spike's tests, source files and raw result files cannot be checked from this repository. Decisions drawn from it are in [core-runtime §2](../architecture/core-runtime.md), the [roadmap](../workflow/roadmap.md) phase 1 and ADR-0022 in the [decision index](../decisions/README.md).

## 1. Summary

**Short answer:** explicit allocators are worth having, but they are one tool among five, and on Windows the cheapest big win is not an arena.

What the measurements say (all on the owner's machine: Windows 11, i7-12700H, Rust 1.94.1; details in sections 3-4 and 7):

1. **The default Windows allocator is slow for small objects.** One allocate + free pair costs about **25-38 ns** (16-64 B objects) and about 66 ns at 512 B. With more threads it gets worse: per-thread cost grows from 30 ns (1 thread) to 79 ns (8 threads), while a per-thread arena stays at about 2-3 ns.
2. **An arena makes small allocations nearly free:** about **1-2 ns per object**, 15-30x less than the default allocator for 16-64 B objects, with **0 allocator calls per frame** (verified with a counting allocator). For 512 B objects the saving shrinks to 2.5x because filling the memory dominates.
3. **But an arena is often not needed.** For the common "one scratch list per frame" case, simply **keeping the `Vec` and calling `clear()`** (or calling `with_capacity` once) is as fast as an arena and also makes **0 allocator calls**. The expensive version is the naive `Vec::new()` every frame: 1.4-4.5x slower than a reused `Vec` and 5-13 allocations per frame (a realloc counts as an allocation), caused by repeated growth.
4. **Many small containers are where arenas really win:** 2,000 tiny lists per frame cost 57 ns per list with `Vec<Vec<_>>` (3,001 allocator calls per frame), **5.5 ns with an arena**, and 3.4 ns when the structure is flattened into one array plus offsets, which removes the allocation entirely.
5. **Swapping the global allocator is the biggest cheap lever on Windows.** mimalloc (optional third-party allocator, one line in `main`) made small allocations **5-7x faster** (but only 1.0-1.7x for 512 B objects, `HashMap` churn and large growing `Vec`s) and 8-thread scaling flat (8.4 ns vs 79 ns). An arena is still about 3x faster than mimalloc for small objects, but the gap is only about 4 ns per object. Not measured: Linux, where the default allocator is probably much closer to mimalloc.
6. **A counting allocator is cheap only if the counters are per-thread.** Per-thread counters cost about 4% single-threaded and 2% at 8 threads. Shared atomic counters cost 29% single-threaded and make 8 threads **5.9x slower**.
7. **Generational-handle pools** are 3-9x faster than the alternatives for steady object churn (50 ns vs 160 ns for `Vec<Option<Box>>` vs 442 ns for `HashMap`), but they do **not** make reading faster: dense contiguous arrays do (2.5x faster than churned `Box`es for sequential reads of 1M objects).
8. **Scale check:** the saving is real but small until allocation counts are large. 2,000 lists per frame cost about 0.11 ms with the default allocator and 0.01 ms with an arena: 0.7% of a 16.7 ms frame. Allocation reaches a sixth of the frame at about **10^5 allocations per frame** (about 2% at 10^4), which is why the wiki rule "default to `std`, add explicit allocators where measured" holds up.

**Recommended order of tools** (adopted in [core-runtime §2](../architecture/core-runtime.md), v0.18):

1. *Design the allocation away* (flat arrays, `Vec` + `u32` indices).
2. *Reuse capacity* (`clear()`, `with_capacity` once).
3. *Arena* for heterogeneous per-frame or per-job scratch data.
4. *Pool + handles* for long-lived objects with churn; keep iteration data dense.
5. *General allocator* for everything else, and choose it once, in the application's `main`, by benchmark on Windows and Linux.

Section 8 lists the concrete proposals, section 7 what this spike did not measure.

## 2. What was compared

The question: DuckEngine's wiki says "default to the standard allocator, add explicit allocators (arena, pool, counting) only where a rule or a benchmark needs them". This spike measures what that costs and buys, on the owner's real platform (Windows), with the allocation patterns the engine will have.

**Strategies** (all written in this spike, std only, no dependencies; one optional extra binary uses `mimalloc`):

| Strategy | What it is | Where the engine would use it |
|---|---|---|
| Default allocator | Rust `Box`/`Vec` on the `System` allocator = Windows `HeapAlloc` | everything that is not hot |
| Arena | bump allocator over one block, fixed capacity, `reset()` frees all, only types without drop glue, failure is `None` | per-frame / per-job scratch data |
| Pool + handles | slot array with free list and generational handles (index + generation), safe Rust | long-lived objects addressed by handle (bodies, entities, resources) |
| Reused `Vec` | keep the `Vec`, call `clear()` each frame (no allocator code at all) | the baseline any arena must beat |
| Flat arrays / indices | design the allocation away (one array + offsets, `Vec` of nodes with `u32` links) | data-oriented structures |
| Counting allocator | a `#[global_allocator]` wrapper that counts calls: shared atomics vs per-thread counters | tests that enforce "zero allocations per frame" |
| mimalloc (optional) | a third-party general-purpose allocator installed as the global allocator | "is swapping the global allocator an alternative to arenas?" |

**Scenarios** (each variant of a scenario does the same logical work and returns the same checksum; this is enforced by `tests/equivalence.rs`):

| Id | Scenario | Engine use case it stands for |
|---|---|---|
| S1 | allocate n small objects (16 B, 64 B, 512 B), then free them all | contacts, particles, events, commands |
| S2 | build one list of m items per frame, consume it, drop it | per-frame scratch (visible set, draw list, contact list) |
| S3 | build 2,000 small lists (1-8 items) per frame | per-entity or per-island lists, many tiny containers |
| S4 | build a 131,071-node binary tree, then free it; separately traverse it | scene/BVH-like pointer structures, built per frame or loaded once |
| S5 | read back 64 B objects after allocation churn (allocate 2n, free a random n); sequential and random order | steady state of a long-running game: locality |
| S6 | 1-8 threads each doing alloc+free batches; and batches freed by another thread | job system: contention and cross-thread frees |
| S7 | the cost of a counting allocator (shared atomics vs per-thread) | the "zero allocations per frame" test tool |
| S9 | 100,000 live objects, steady churn (remove one, insert one, 8 lookups): generational pool vs `HashMap` vs `Vec<Option<Box>>` | handle tables |

## 3. Method

- **Build:** `cargo build --release` (Rust 1.94.1, MSVC), `opt-level=3`, fat LTO, `codegen-units=1`, `panic=abort`. No dependencies (except the optional mimalloc binary).
- **Harness** (own, std only, `src/harness.rs`): each benchmark is calibrated to about 30 ms per sample, then 2 warm-up samples and 15 timed samples; the result is the **median** ns per operation (min and max are kept in the raw files). Checksums go through `black_box` so the compiler cannot delete the work.
- **Repetition:** the whole suite was run **3 times**; the tables show the median of the three run-medians and the **run-to-run spread**. A spread above about 10% marks a noisy row and is called out in the text.
- **Warm-up:** every binary first runs the whole suite once, silently, then measures. Reason (found during the spike, see section 7): the first benchmarks of a fresh process were slower and noisier, e.g. 36 ns cold vs 25 ns warm for the first row.
- **Noise control (Windows):** the process is restricted to the 12 performance-core threads, the main thread is pinned to one core, priority is raised one step (`src/os.rs`).
- **Correctness first:** 11 unit tests (arena alignment, exhaustion, reset, recursion; pool stale handles and slot reuse), 8 equivalence tests (all variants agree) and 4 allocation-count tests (a frame using an arena makes 0 allocator calls, a naive frame makes more) pass before any timing is trusted.
- **Counting:** `alloc_counts` uses the per-thread counting allocator to report allocator calls per steady-state frame.

## 4. Environment

```
date: 2026-10-25T23:29:20+01:00
rustc: rustc 1.94.1 (e408947bf 2026-03-25)
profile: opt-level=3 lto=fat codegen-units=1 panic=abort
cpu: 12th Gen Intel(R) Core(TM) i7-12700H, cores 14, logical 20
ram_gb: 15.7
os: Microsoft Windows 11 Home 10.0.26200
power_scheme: Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)
battery_status: 2 (2 = on AC power)
```

No other benchmark or build ran at the same time, but the machine was not otherwise controlled; during the mimalloc rounds only light file editing and small Python scripts ran alongside. The CPU is a hybrid design (6 performance cores with 2 threads each, 8 efficiency cores); only the performance cores were used.

## 5. Results

_Aggregated from 3 complete runs; median of per-run medians. Spread = (slowest run - fastest run) / median._

### S1 - Small-object churn (allocate n objects, free n objects), ns per object


**16B x 16384**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box (default allocator) | 25.41 | 2% | 1.00x |
| arena, one alloc per object | 0.81 | 1% | 0.03x |
| arena, one slice | 0.33 | 0% | 0.01x |
| pool + handles | 3.00 | 0% | 0.12x |
| Vec<T>, capacity reused | 0.79 | 0% | 0.03x |

**64B x 4096**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box (default allocator) | 27.44 | 1% | 1.00x |
| arena, one alloc per object | 1.88 | 0% | 0.07x |
| arena, one slice | 0.96 | 2% | 0.04x |
| pool + handles | 5.37 | 11% | 0.20x |
| Vec<T>, capacity reused | 2.60 | 18% | 0.09x |

**64B x 262144**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box (default allocator) | 37.75 | 14% | 1.00x |
| arena, one alloc per object | 2.34 | 0% | 0.06x |
| arena, one slice | 1.63 | 2% | 0.04x |
| pool + handles | 10.59 | 6% | 0.28x |
| Vec<T>, capacity reused | 2.42 | 3% | 0.06x |

**512B x 16384**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box (default allocator) | 65.65 | 3% | 1.00x |
| arena, one alloc per object | 26.13 | 0% | 0.40x |
| arena, one slice | 24.41 | 6% | 0.37x |
| pool + handles | 47.75 | 1% | 0.73x |
| Vec<T>, capacity reused | 26.07 | 2% | 0.40x |

**Reading.**
- Default allocator: 25.4 / 27.4 / 37.8 / 65.7 ns per object for 16 B / 64 B / 64 B (262k objects, 16 MB) / 512 B. Arena, one allocation per object: 0.81 / 1.88 / 2.34 / 26.1 ns, i.e. **31x, 15x, 16x and 2.5x** cheaper. The 512 B case is dominated by writing 512 bytes (26 ns is memory work, which any approach pays), which is why the ratio collapses.
- A `Vec<T>` with reused capacity is as fast as the arena (0.79 / 2.6 / 2.4 / 26.1): a bump arena is, in effect, a `Vec` you do not have to size per type.
- The pool (`insert` + `remove` through handles) is 3-8x cheaper than `Box` for small objects (3.0 / 5.4 / 10.6 ns) and only 1.4x at 512 B. It costs more than an arena because it keeps a free list and a generation counter; in exchange individual objects can be freed and stale handles are detected.
- Noisy rows: `64B x 4096` pool (11%) and `Vec` (18%) vary between runs; their order of magnitude is reliable, the second digit is not. The `64B x 262144` box row (14%) is also noisy.
- The 16 B vs 64 B ordering of the default allocator (25 ns vs 27 ns) is within what Windows heap internals can produce; this spike did not investigate why.

### S2 - Per-frame scratch list, ns per item


**64 items x 32B**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| Vec::new() each frame | 4.43 | 3% | 1.00x |
| Vec::with_capacity each frame | 1.50 | 2% | 0.34x |
| Vec kept, clear() each frame | 0.98 | 4% | 0.22x |
| arena, reset each frame | 1.08 | 0% | 0.24x |

**1024 items x 32B**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| Vec::new() each frame | 2.66 | 1% | 1.00x |
| Vec::with_capacity each frame | 1.18 | 3% | 0.45x |
| Vec kept, clear() each frame | 1.04 | 3% | 0.39x |
| arena, reset each frame | 1.12 | 0% | 0.42x |

**16384 items x 32B**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| Vec::new() each frame | 2.47 | 1% | 1.00x |
| Vec::with_capacity each frame | 1.83 | 2% | 0.74x |
| Vec kept, clear() each frame | 1.75 | 3% | 0.71x |
| arena, reset each frame | 1.12 | 0% | 0.45x |

**Reading.**
- `Vec::new()` every frame costs 4.4 / 2.7 / 2.5 ns per item (64 / 1,024 / 16,384 items). Per frame that is 283 ns, 2.7 us and 40 us. The cause is growth: **5, 9 and 13 allocations per frame** (a realloc counts as an allocation) (see the call-count table below), because the capacity doubles repeatedly.
- `with_capacity(m)` (1 allocation) removes most of it: 1.5 / 1.2 / 1.8 ns. Keeping the `Vec` and calling `clear()` removes the allocation too (0 calls): 0.98 / 1.04 / 1.75 ns.
- The arena (1.08 / 1.12 / 1.12 ns) is **equal to the reused `Vec`** for 64 and 1,024 items and 1.6x faster at 16,384 items. The reason for that last difference was not investigated (the loops differ in code generation, not in allocation).
- **Takeaway:** for a single list per frame, the allocator is not the lever; knowing the capacity is. An arena only matters here if you would otherwise create the list fresh each time.

### S3 - Many small lists per frame, ns per list


**2000 lists of 1..8 u32**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| Vec<Vec<u32>>, push growth | 57.25 | 1% | 1.00x |
| Vec<Vec<u32>>, exact capacity | 35.47 | 1% | 0.62x |
| arena slices, reset each frame | 5.47 | 1% | 0.10x |
| flat arrays (data + offsets), reused | 3.41 | 7% | 0.06x |

**Reading.**
- Per frame (2,000 lists): `Vec<Vec<u32>>` with push growth 115 us and **3,001 allocator calls**; with exact capacity 71 us and 2,001 calls; arena slices **11 us and 0 calls**; flat arrays (one data array plus offsets, reused) **7 us and 0 calls**.
- The arena is **10x** faster than the naive version and 1.6x slower than the flat layout. The flat layout wins because it also removes the per-list bookkeeping, not only the allocation: design the allocation away when the data shape allows it.
- Exact capacity (a cheap source-level change) already gives 1.6x.

### S4 - Binary tree: build + free, and traverse, ns per node


**tree, 131071 nodes**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| build + free: Box nodes | 32.05 | 0% | 1.00x |
| build + free: arena nodes | 7.25 | 1% | 0.23x |
| build + free: Vec + u32 indices | 7.76 | 0% | 0.24x |
| traverse: Box nodes | 2.00 | 0% | 0.06x |
| traverse: arena nodes | 1.97 | 1% | 0.06x |
| traverse: Vec + u32 indices | 2.18 | 1% | 0.07x |

**Reading.**
- Building and freeing the tree: `Box` nodes 32 ns/node (4.2 ms for 131k nodes), arena 7.3 ns, `Vec` + `u32` indices 7.8 ns, i.e. **4.4x and 4.1x** cheaper. Dropping a `Box` tree also runs a recursive destructor; the arena frees everything with one reset.
- Traversal is the same for all three (about 2.0 ns/node). A tree that was just built with one allocation order has no locality penalty on this machine, so for **freshly built structures the gain is in build/free cost only**; locality differences appear after churn (next table).
- Arena nodes use references (`&ANode`) and indices use `u32`: both are stable under the borrow checker, but only the index form can be saved to disk or sent to another process.

### S5 - Reading back after churn (locality), ns per live object


**10000 live x 64B**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| seq: Box, fresh (no churn) | 0.48 | 5% | 1.00x |
| seq: Box, after churn | 0.97 | 2% | 2.04x |
| seq: pool, after churn (50% free slots) | 1.72 | 7% | 3.60x |
| seq: Vec<T>, dense | 0.34 | 0% | 0.72x |
| random: Box, after churn | 1.04 | 1% | 2.19x |
| random: pool handles | 1.91 | 2% | 4.02x |
| random: Vec<T>, dense | 0.68 | 1% | 1.42x |

**1000000 live x 64B**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| seq: Box, fresh (no churn) | 3.27 | 2% | 1.00x |
| seq: Box, after churn | 5.59 | 0% | 1.71x |
| seq: pool, after churn (50% free slots) | 6.34 | 1% | 1.94x |
| seq: Vec<T>, dense | 2.24 | 3% | 0.69x |
| random: Box, after churn | 14.04 | 1% | 4.30x |
| random: pool handles | 16.28 | 0% | 4.98x |
| random: Vec<T>, dense | 7.67 | 1% | 2.35x |

**Reading.**
- After churn (allocate 2n, free a random half), sequential reads of `Box`ed objects are **2.0x slower at 10k objects and 1.7x slower at 1M** than the same objects allocated fresh. A dense `Vec<T>` is 2.5x faster than churned `Box`es at 1M (2.24 vs 5.59 ns) and 1.5x faster than fresh `Box`es (3.27 ns).
- Random-order reads (what handle lookups look like): churned `Box` 14.0 ns, dense array 7.7 ns at 1M objects.
- **The pool did not help reads.** With 50% free slots it scans twice the slots, and each slot also carries a generation and a free-list link: 6.3 ns sequential and 16.3 ns random at 1M, slower than `Box`. This is a property of this simple pool with holes, not of pools in general; a dense slot map (values packed, an indirection table for handles) was not measured. The lesson stands: **the pool buys cheap allocation and safe handles (S1, S9); contiguous iteration needs dense storage.**

### S6 - Multithreaded alloc+free, ns per alloc+free pair per thread (flat = perfect scaling)


**1 threads**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box (default allocator) | 30.32 | 7% | 1.00x |
| arena per thread | 2.13 | 163% | 0.07x |

**2 threads**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box (default allocator) | 38.55 | 4% | 1.00x |
| arena per thread | 2.16 | 5% | 0.06x |

**4 threads**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box (default allocator) | 46.11 | 4% | 1.00x |
| arena per thread | 4.64 | 59% | 0.10x |

**8 threads**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box (default allocator) | 78.91 | 1% | 1.00x |
| arena per thread | 2.48 | 5% | 0.03x |

**4 threads, batches of 5000**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box, freed by the allocating thread | 54.49 | 9% | 1.00x |
| box, freed by another thread | 61.03 | 4% | 1.12x |

**8 threads, batches of 5000**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| box, freed by the allocating thread | 93.08 | 10% | 1.00x |
| box, freed by another thread | 99.03 | 4% | 1.06x |

**Reading.**
- The default allocator's cost per allocate + free pair, per thread, rises from 30 ns (1 thread) to 39, 46 and **79 ns at 8 threads (2.6x)**. A per-thread arena stays at 2.1-2.5 ns, so the slowdown is mostly allocator contention rather than shared hardware: the arena runs the same arithmetic without allocator calls and stays flat within noise at 8 threads (8 threads share 6 physical cores; the 1-thread arena row is itself noisy). Hyper-threading is not completely excluded as a contributor, because the arena is lighter on memory than the allocator is.
- Freeing a batch on a different thread than the one that allocated it costs **+12% (4 threads) and +6% (8 threads)** on the Windows heap. These differences are close to the run-to-run spread (4-10%), so the safe reading is "no large penalty": a job system that frees across threads pays little extra here. This may be different on other platforms and allocators (mimalloc below shows the opposite weakness).
- Noisy rows: the arena at 1 thread (one outlier run, spread 163%) and 4 threads (59%) run for under a millisecond per sample. Read them as "2-5 ns, flat", not as exact values.

### S9 - Handle pool vs HashMap vs Vec<Option<Box>>, ns per iteration (1 remove + 1 insert + 8 lookups)


**100000 live, 1 remove + 1 insert + 8 lookups**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| pool + generational handles | 50.15 | 1% | 1.00x |
| HashMap<u64, Box<T>> (SipHash) | 442 | 23% | 8.81x |
| Vec<Option<Box<T>>> + free list | 160 | 4% | 3.18x |

**Reading.**
- The generational pool needs 50 ns per iteration (one remove, one insert, eight lookups). `Vec<Option<Box<T>>>` with a free list: 160 ns (**3.2x**). `HashMap<u64, Box<T>>` with the default SipHash hasher: 442 ns (**8.8x**, spread 23%).
- The pool wins because freed slots are reused in place (no allocator call per insert) and a lookup is one array access plus a generation compare. The `HashMap` number includes the cost of the default hasher; a fast hasher would narrow the gap, which this spike did not measure.

### S7 - Cost of a counting allocator, ns per op


**box churn 64B x 4096, 1 thread**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| default (System) | 27.26 | 1% | 1.00x |
| counting, shared atomics | 35.04 | 1% | 1.29x |
| counting, per-thread counters | 28.27 | 3% | 1.04x |

**Vec::new() frame, 1024 items**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| default (System) | 2.67 | 2% | 1.00x |
| counting, shared atomics | 2.73 | 2% | 1.02x |
| counting, per-thread counters | 2.69 | 14% | 1.01x |

**box churn, 8 threads**

| Variant | ns/op | run-to-run spread | vs first |
|---|---:|---:|---:|
| default (System) | 79.72 | 2% | 1.00x |
| counting, shared atomics | 468 | 6% | 5.86x |
| counting, per-thread counters | 81.70 | 2% | 1.02x |

**Reading.**
- Single thread, allocation-heavy loop: default 27.3 ns, **per-thread counters 28.3 ns (+4%)**, shared atomic counters 35.0 ns (**+29%**, about 4 ns per atomic operation, two per allocate + free).
- Eight threads: default 79.7 ns, per-thread counters 81.7 ns (+2%), shared atomics **468 ns (5.9x slower)**, because every thread bumps the same cache line.
- For a frame with only a few allocations the overhead is invisible (about 1-2%).
- **Design consequence:** the counting allocator for "zero allocations per frame" tests must use per-thread (or per-job) counters. That is also what the assertion needs: "this job made 0 allocator calls".

### Allocator calls per steady-state frame (counting allocator, per-thread counters)

| Scenario | Variant | Size | allocs / frame | frees / frame |
|---|---|---|---:|---:|
| S2 | Vec::new() each frame | 64 items | 5.00 | 1.00 |
| S2 | Vec::with_capacity each frame | 64 items | 1.00 | 1.00 |
| S2 | Vec kept, clear() each frame | 64 items | 0.00 | 0.00 |
| S2 | arena, reset each frame | 64 items | 0.00 | 0.00 |
| S2 | Vec::new() each frame | 1024 items | 9.00 | 1.00 |
| S2 | Vec::with_capacity each frame | 1024 items | 1.00 | 1.00 |
| S2 | Vec kept, clear() each frame | 1024 items | 0.00 | 0.00 |
| S2 | arena, reset each frame | 1024 items | 0.00 | 0.00 |
| S2 | Vec::new() each frame | 16384 items | 13.00 | 1.00 |
| S2 | Vec::with_capacity each frame | 16384 items | 1.00 | 1.00 |
| S2 | Vec kept, clear() each frame | 16384 items | 0.00 | 0.00 |
| S2 | arena, reset each frame | 16384 items | 0.00 | 0.00 |
| S3 | Vec<Vec<u32>>, push growth | 2000 lists | 3,001.00 | 2,001.00 |
| S3 | Vec<Vec<u32>>, exact capacity | 2000 lists | 2,001.00 | 2,001.00 |
| S3 | arena slices, reset each frame | 2000 lists | 0.00 | 0.00 |
| S3 | flat arrays, reused | 2000 lists | 0.00 | 0.00 |

**Reading.** This is the rule "zero heap allocations per frame in hot systems" made concrete. A reused `Vec` and an arena make **0 allocator calls** in steady state; `with_capacity` makes exactly 1; the naive form makes 5-13 allocations per frame (one list) and `Vec<Vec<_>>` makes 3,001 per frame. `tests/zero_alloc.rs` asserts the 0 and the "more than 1" cases with the same counting allocator, so the mechanism is also covered by automated tests, not only by this report.

### Global allocator swap: Windows default (HeapAlloc) vs mimalloc (3 runs), ns per op

| Group | Case | Variant | default | mimalloc | mimalloc vs default |
|---|---|---|---:|---:|---:|
| S1 | 16B x 16384 | box (default allocator) | 25.41 | 3.91 | 0.15x |
| S1 | 64B x 4096 | box (default allocator) | 27.44 | 5.58 | 0.20x |
| S1 | 64B x 262144 | box (default allocator) | 37.75 | 6.91 | 0.18x |
| S1 | 512B x 16384 | box (default allocator) | 65.65 | 38.67 | 0.59x |
| S2 | 64 items x 32B | Vec::new() each frame | 4.43 | 2.30 | 0.52x |
| S2 | 64 items x 32B | Vec::with_capacity each frame | 1.50 | 1.32 | 0.88x |
| S2 | 1024 items x 32B | Vec::new() each frame | 2.66 | 2.30 | 0.86x |
| S2 | 1024 items x 32B | Vec::with_capacity each frame | 1.18 | 1.08 | 0.91x |
| S2 | 16384 items x 32B | Vec::new() each frame | 2.47 | 2.51 | 1.02x |
| S2 | 16384 items x 32B | Vec::with_capacity each frame | 1.83 | 1.80 | 0.98x |
| S3 | 2000 lists of 1..8 u32 | Vec<Vec<u32>>, push growth | 57.25 | 17.55 | 0.31x |
| S3 | 2000 lists of 1..8 u32 | Vec<Vec<u32>>, exact capacity | 35.47 | 9.52 | 0.27x |
| S4 | tree, 131071 nodes | build + free: Box nodes | 32.05 | 13.35 | 0.42x |
| S4 | tree, 131071 nodes | traverse: Box nodes | 2.00 | 1.84 | 0.92x |
| S5 | 10000 live x 64B | seq: Box, fresh (no churn) | 0.48 | 0.43 | 0.91x |
| S5 | 10000 live x 64B | seq: Box, after churn | 0.97 | 0.50 | 0.51x |
| S5 | 10000 live x 64B | random: Box, after churn | 1.04 | 0.81 | 0.78x |
| S5 | 1000000 live x 64B | seq: Box, fresh (no churn) | 3.27 | 2.74 | 0.84x |
| S5 | 1000000 live x 64B | seq: Box, after churn | 5.59 | 4.73 | 0.85x |
| S5 | 1000000 live x 64B | random: Box, after churn | 14.04 | 10.16 | 0.72x |
| S6 | 1 threads | box (default allocator) | 30.32 | 4.92 | 0.16x |
| S6 | 2 threads | box (default allocator) | 38.55 | 7.20 | 0.19x |
| S6 | 4 threads | box (default allocator) | 46.11 | 7.41 | 0.16x |
| S6 | 8 threads | box (default allocator) | 78.91 | 8.35 | 0.11x |
| S6 | 4 threads, batches of 5000 | box, freed by the allocating thread | 54.49 | 9.55 | 0.18x |
| S6 | 4 threads, batches of 5000 | box, freed by another thread | 61.03 | 33.40 | 0.55x |
| S6 | 8 threads, batches of 5000 | box, freed by the allocating thread | 93.08 | 20.63 | 0.22x |
| S6 | 8 threads, batches of 5000 | box, freed by another thread | 99.03 | 45.58 | 0.46x |
| S9 | 100000 live, 1 remove + 1 insert + 8 lookups | HashMap<u64, Box<T>> (SipHash) | 442 | 374 | 0.85x |
| S9 | 100000 live, 1 remove + 1 insert + 8 lookups | Vec<Option<Box<T>>> + free list | 160 | 64.62 | 0.40x |

_Arena, pool and reused-Vec variants are not listed: they allocate once, so the global allocator does not matter to them._

**Reading.**
- With mimalloc as the global allocator, small-object churn is **5-7x faster** (3.9 / 5.6 / 6.9 ns for 16 B / 64 B / 262k x 64 B vs 25 / 27 / 38 ns), `Vec<Vec<_>>` frames 3-4x faster, tree build/free 2.4x, and **8-thread scaling is nearly flat (8.4 vs 78.9 ns)**. The case it is weaker on is freeing across threads (33-46 ns), though that is still about 2x faster than the Windows default (61-99 ns).
- It does **not** change the strategies that never call the allocator: arenas, reused `Vec`s and flat arrays. Against mimalloc, the arena is still about 3x faster for 64 B objects (1.9 ns vs 5.6 ns) and 1.7-3.2x faster for the many-small-lists case (5.5 ns vs 17.6 / 9.5 ns), but the absolute difference is a few nanoseconds per object.
- So on Windows two independent levers exist: **use fewer allocations** (arena, reuse, flat) and **make the remaining ones cheaper** (global allocator). Doing the second for free in `main` does not remove the need for the first on the hot paths with tens of thousands of allocations per frame.
- mimalloc is a third-party library (C code built through the `cc` crate). It was used here only to measure. Whether the engine may adopt it is an owner decision: no existing tier fits a `#[global_allocator]` in L4 (`T1-stopgap` is limited to adapters), so ADR-0022 defines the tier (L4 composition root, no port, exit plan = the default allocator) and `THIRD_PARTY.md` lists it.
- Only the Windows default allocator was compared. On Linux the system allocator (glibc) is likely much closer to mimalloc, so this result must not be generalised before a Linux run.

## 6. What the numbers mean for the engine's frame budget

At 60 fps a frame is 16.7 ms. Using the measured costs (default allocator, small objects, about 28 ns per allocate + free pair; arena about 2 ns):

| Allocations per frame | Default allocator | Arena | Share of a 16.7 ms frame (default) |
|---:|---:|---:|---:|
| 1,000 | 0.03 ms | 0.002 ms | 0.2% |
| 10,000 | 0.28 ms | 0.02 ms | 1.7% |
| 100,000 | 2.8 ms | 0.2 ms | 17% |

So: below roughly 10,000 allocations per frame the allocator is not the problem on this machine; around 100,000 it is a sixth of the frame. That is the quantitative content behind "measure first". Contention (S6) multiplies the default-allocator figure when many threads allocate.

## 7. Limits and what went wrong

**Limits (read before using these numbers):**
- **One machine, one OS.** Windows 11, i7-12700H, balanced power plan on AC power, performance cores only. No Linux, no ARM64, no other Windows machine. The default allocator on Linux is a different program.
- **Micro-benchmarks isolate allocation.** In a real frame, allocation also competes for cache with the work around it; the benefits here are upper bounds measured in isolation.
- **Three runs, medians, no statistical tests.** Spreads are shown; rows above about 10% spread are named in the text.
- **Arena capacity is fixed.** Overflow policy (chained blocks, growth) and first-touch page-fault cost of a large fresh arena were not measured; arena memory here was already touched.
- **The pool is naive** (`Option<T>` per slot, holes stay). A dense slot map was not measured. `HashMap` used the default SipHash hasher.
- **No `allocator_api`.** Stable `Vec` cannot take a per-instance allocator, so arena-backed containers here are the spike's own (push-only `ArenaVec`, slices). That is consistent with the wiki's statement that arena containers are our own code.
- **Miri was not run** (only the stable toolchain is installed), so the `unsafe` code in `arena.rs` is covered by unit tests but not yet by Miri. The wiki requires Miri on changed `unsafe` before it enters the engine.
- **Churn is one round.** S5 simulates one allocate/free round, not hours of play; fragmentation over time is not measured.

**What went wrong during the spike (and was fixed):**
- The first full run had no warm-up. The first benchmark of a fresh process ran slower and noisier (36 ns cold vs 25 ns warm for 16 B boxes), and the S7 counting-allocator rows looked as if counting cost 60-70% (45 ns vs 27 ns) with 29-50% spread. A control run of the *default* allocator in a fresh process showed the same noise, so the cause was the measurement, not the counters. The harness now runs the whole suite once silently before measuring, and all tables above are from the corrected runs (the per-thread counter overhead is 4%, not 70%).

## 8. Proposals for the DuckEngine wiki (1-5 applied in v0.18, task 1-0; 6 is folded into the tool order in core-runtime §2, 7 into the phase-1 benchmark task)

1. **Keep the policy** "default allocator, explicit allocators where a rule or a benchmark needs them". The evidence supports it: the savings are real: about 2% of a frame at 10^4 allocations and a sixth at 10^5.
2. **Add an order of preference to `core-runtime`:** design the allocation away, then reuse capacity, then arena, then pool + handles, then the general allocator (section 1).
3. **Counting allocator:** specify per-thread (or per-job) counters, never process-wide shared atomics, in the test tooling.
4. **Choose the global allocator by benchmark, in one place:** a phase-1 task compares the Windows default and one third-party allocator on Windows and Linux; the choice is a single `#[global_allocator]` line in the application (L4), so engine crates stay independent of it, and an adoption is a `THIRD_PARTY.md` entry with the default allocator as the exit plan.
5. **Phase-1 arena scope:** `Arena` (fixed capacity, only types without drop glue, `reset(&mut self)`, failures as values), `ArenaVec` and slices are about 150 lines and were enough for every scenario here. Re-implement them test-first under the project's rules (do not copy the spike), and run Miri.
6. **Data guidance:** keep iteration data dense; use pools for handle-addressed long-lived objects but do not iterate over a pool with holes in a hot loop.
7. **Re-run this spike** on Linux x86-64 and Linux ARM64 (the CI hosts) before the allocator decisions are frozen.

## 9. Reproduce (the spike folder is outside this repository)

```
cd RustAllocator                               # not part of this repository
cargo test                                     # 23 tests: unit, equivalence, zero-allocation
bash run_benchmarks.sh                         # 3 rounds, several minutes; keep the machine idle
bash run_mimalloc.sh                           # optional, needs network once (builds mimalloc)
python aggregate.py                            # writes results/tables.md
```

Layout: `src/arena.rs`, `src/pool.rs`, `src/counting.rs` (the allocators), `src/scenarios.rs` and `src/suites.rs` (workloads and the benchmark matrix), `src/harness.rs` (timing), `src/os.rs` (Windows pinning), `src/bin/*` (one binary per global allocator), `tests/`, `results/` (raw output: `run*.txt`, `mi_run*.txt`, `counts.txt`, `environment.txt`, `tables.md`). Raw lines are `RESULT|group|variant|param|median|min|max|reps` in ns per operation.
