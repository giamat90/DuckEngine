# THIRD_PARTY

Every external thing the repository touches is listed here: libraries, binding crates, OS and vendor API families, build-time and dev-time tools. A CI check (later phase) fails when something external appears that is not listed. Rules: [rules-digest](wiki/rules-digest.md) section 2, [ports-and-adapters](wiki/architecture/ports-and-adapters.md).

No GPL or AGPL entry is allowed. A licence must permit closed-source redistribution of the engine and redistribution in an Apache-2.0 repository (the engine's own licence, ADR-0023). LGPL only as an optional, dynamically linked adapter, never required.

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
| GitHub-hosted runner images (`windows-latest`, `ubuntu-latest`) | T2 | n/a (service) | floating labels, not pinned | none | another hosted CI runner service (never a self-hosted runner on a public repository) | in tree |
| RustSec advisory database (fetched by cargo-deny at run time) | T2 | CC0-1.0 | unpinned, always current | none | vendor a pinned copy and fetch on a schedule | in tree |
| mimalloc (crate `mimalloc`, its C library in `libmimalloc-sys`, and the build-time crates `cc`, `find-msvc-tools` and `shlex` that compile it) | T1-stopgap (placement defined by ADR-0022: L4 composition root as `#[global_allocator]`, no port, not an adapter) | MIT | =0.1.52 (`engine/app_runtime/Cargo.toml`, root `Cargo.lock`; also `tools/alloc-bench/Cargo.lock`) | none | delete the one `#[global_allocator]` line; the default `std` allocator takes over | in tree (benchmark tool `tools/alloc-bench`; linked as the global allocator by `engine/app_runtime`, feature `mimalloc`, default on) |
| GLFW (shared library loaded at run time by `engine/adapter_window_glfw`; no GLFW source or header in the repository, the C functions are declared by hand) | T1-stopgap | Zlib (checked against the `LICENSE.md` of the 3.4 release) | built and tested against 3.4; the adapter accepts 3.3 up to the end of the 3.x series and refuses anything else. Windows: `glfw3.dll` from `lib-vc2022` of the official `glfw-3.4.bin.WIN64.zip` (zip SHA-256 `54efa829400f2a0537f742b2b3bdd74e437bb4f2f048e4b7d3c5557d11a611e6`, DLL SHA-256 `4429adff46d4d1038bb5836cecc59661eef7126e4941de6fdfde8855e7c14bb1`). Linux: `libglfw.so.3` from the distribution package. Neither is stored in git. On Windows the build of `engine/adapter_window_glfw` (`build.rs`, helper crate `engine/adapter_runtime_libs`, manifest `engine/adapter_window_glfw/runtime-lib.txt`) downloads the zip over https from the official release, checks both SHA-256 values above, keeps the DLL in a per-user cache and copies it next to the programs (profile folder, `deps`, `examples`); a failure is a build warning, never a failed build, and `DUCK_RUNTIME_LIBS_OFFLINE=1` forbids the download. The program finds the library next to itself, on the search path, or through `DUCK_GLFW_LIB` (when set, nothing is downloaded) | window and input port (`IWindowSystem`) | native Win32 adapter before phase 7 closes; entry removed when replaced. Without the library the adapter reports `NotFound` and the engine runs on the Headless window system | in tree (owner choice O3 (c), 2026-11-01) |
| miniaudio (the single C header `miniaudio.h`, compiled by `engine/adapter_audio_miniaudio` together with a thin C shim of the repository's own, through the build-time crate `cc` already listed in the mimalloc row) | T1-stopgap | MIT-0 OR Unlicense (dual; checked against the `LICENSE` of the 0.11.26 tag) | 0.11.26. The header is not stored in git: the build of `engine/adapter_audio_miniaudio` (`build.rs`, helper crate `engine/adapter_runtime_libs`, manifest `engine/adapter_audio_miniaudio/source-file.txt`) downloads `https://raw.githubusercontent.com/mackron/miniaudio/0.11.26/miniaudio.h` over https, checks its SHA-256 `6b376c7fbb1352ba1e0e432ab66069b335ea9405e45cb6c30b97ef0f3c9c3e0e`, keeps it in the per-user cache and compiles it; a failure is a build warning, never a failed build, and the adapter then reports `NoDevice` (`DUCK_RUNTIME_LIBS_OFFLINE=1` forbids the download; `DUCK_MINIAUDIO_HEADER` names a header of the user's own, which is checked against the same hash) | audio device port (`IAudioDevice`) | native WASAPI adapter at the replacement milestone; entry removed when replaced | in tree (owner choice O5 (a), 2026-11-09) |

## Planned (allowed by the owner, not yet added)

Licences below come from background knowledge and must be re-checked when the entry moves to `in tree`.

| Name | Tier | Licence | Version | Port | Exit plan | Status |
|---|---|---|---|---|---|---|
| glslang | T2 | Apache-2.0 (GitHub shows NOASSERTION because of several licences; re-check when added) | 16.6.0 (release archive, SHA-256 checked against the release digest) | none (build-time tool behind `IShaderCompiler`, ADR-0009) | replace with another GLSL to SPIR-V compiler (shaderc wraps it) behind `IShaderCompiler` | planned |
| SPIRV-Cross | T2 | Apache-2.0 | tag `vulkan-sdk-1.4.363.0` (commit `f11ba9f`), built from source | none (build-time tool behind `IShaderCompiler`, ADR-0009) | write the translation as a Rust crate, or switch to Slang's direct targets, behind `IShaderCompiler` | planned |
| DXC (DirectX Shader Compiler) | T2 | LLVM Release License (UIUC/NCSA) plus Microsoft terms; re-check when added | v1.9.2609 (release archive, SHA-256 checked); used only once the D3D12 backend starts | none (build-time tool behind `IShaderCompiler`, ADR-0009) | Slang or SPIRV-Cross HLSL route to another DXIL producer; re-evaluated at D3D12 | planned |
