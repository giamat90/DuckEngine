# THIRD_PARTY

Every external thing the repository touches is listed here: libraries, binding crates, OS and vendor API families, build-time and dev-time tools. A CI check (later phase) fails when something external appears that is not listed. Rules: [rules-digest](wiki/rules-digest.md) section 2, [ports-and-adapters](wiki/architecture/ports-and-adapters.md).

No GPL or AGPL entry is allowed. A licence must permit closed-source redistribution of the engine. LGPL only as an optional, dynamically linked adapter, never required.

This file is a governance file (Class 3): its structure changes only in a task the owner declares as a governance task. Adding or changing an entry is a Class 2 change.

## Columns

| Column | Meaning |
|---|---|
| Name | the library, crate, API family or tool |
| Tier | T0 nothing external in L0-L2 (no entries) / T1 adapter code against an OS or vendor API / T1-bindings declaration-only binding crate, adapters only, pinned / T1-stopgap a whole library inside an adapter, with an exit plan / T2 dev-time or build-time tool |
| Licence | SPDX identifier; must permit closed-source redistribution |
| Version | the pinned version (exact) |
| Port | the port or adapter that wraps it; `none` for T2 tools |
| Exit plan | how and when it is replaced; required for T1-stopgap, optional otherwise |
| Status | `in tree` (used by the repository now) or `planned` (allowed, not yet added) |

## In tree

| Name | Tier | Licence | Version | Port | Exit plan | Status |
|---|---|---|---|---|---|---|
| Rust toolchain (rustc, cargo, rustfmt, Clippy) | T2 | MIT OR Apache-2.0 | 1.94.1 (`rust-toolchain.toml`) | none | none; the language is the project's base | in tree |
| actions/checkout | T2 | MIT | v7.0.1 (pinned by commit in `.github/workflows/ci.yml`) | none | replace with a plain `git clone` step if the action is withdrawn | in tree |
| actions/upload-artifact | T2 | MIT | v7.0.2 (pinned by commit in `.github/workflows/ci.yml`) | none | replace with the `gh` CLI or a release asset upload if the action is withdrawn | in tree |
| cargo-llvm-cov | T2 | Apache-2.0 OR MIT | 0.9.1 (release binary, SHA256 pinned in `.github/workflows/ci.yml`) | none | use `llvm-cov` from the `llvm-tools` component directly | in tree |
| cargo-mutants | T2 | MIT | 27.1.0 (release binary, SHA256 pinned in `.github/workflows/ci.yml`) | none | none yet; gate G1 in phase 0a validates it, ADR-0001 names the fallback | in tree |
| cargo-deny | T2 | MIT OR Apache-2.0 | 0.20.2 (release binary, SHA256 pinned in `.github/workflows/ci.yml`) | none | run `cargo metadata` licence checks by hand or switch to `cargo-about` | in tree |
| Python 3 (standard library only) | T2 | PSF-2.0 | the runner's preinstalled 3.x; 3.11 or newer is required (`tomllib`) | none | rewrite `tools/metrics` as a Rust tool once the tools crate exists | in tree |
| `llvm-tools` rustup component | T2 | Apache-2.0 WITH LLVM-exception | follows the pinned toolchain 1.94.1 | none | use a system LLVM `llvm-cov` | in tree |
| GitHub-hosted runner images (`windows-latest`, `ubuntu-latest`) | T2 | n/a (service) | floating labels, not pinned | none | self-hosted or other CI runner | in tree |
| RustSec advisory database (fetched by cargo-deny at run time) | T2 | CC0-1.0 | unpinned, always current | none | vendor a pinned copy and fetch on a schedule | in tree |
| mimalloc (crate `mimalloc`, its C library in `libmimalloc-sys`, and the build-time crates `cc`, `find-msvc-tools` and `shlex` that compile it) | T1-stopgap (placement defined by ADR-0022: L4 composition root as `#[global_allocator]`, no port, not an adapter) | MIT | =0.1.52 (`engine/app_runtime/Cargo.toml`, root `Cargo.lock`; also `tools/alloc-bench/Cargo.lock`) | none | delete the one `#[global_allocator]` line; the default `std` allocator takes over | in tree (benchmark tool `tools/alloc-bench`; linked as the global allocator by `engine/app_runtime`, feature `mimalloc`, default on) |

## Planned (allowed by the owner, not yet added)

Licences below come from background knowledge and must be re-checked when the entry moves to `in tree`.

| Name | Tier | Licence | Version | Port | Exit plan | Status |
|---|---|---|---|---|---|---|
| GLFW | T1-stopgap | Zlib | pinned when added | window and input port | native Win32 adapter before phase 7 closes; entry removed when replaced | planned |
| miniaudio | T1-stopgap | MIT-0 or Unlicense (dual) | pinned when added | audio device port | native WASAPI adapter at the replacement milestone; entry removed when replaced | planned |
