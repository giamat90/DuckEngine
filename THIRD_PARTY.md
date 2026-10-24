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

## Planned (allowed by the owner, not yet added)

Licences below come from background knowledge and must be re-checked when the entry moves to `in tree`.

| Name | Tier | Licence | Version | Port | Exit plan | Status |
|---|---|---|---|---|---|---|
| GLFW | T1-stopgap | Zlib | pinned when added | window and input port | native Win32 adapter before phase 7 closes; entry removed when replaced | planned |
| miniaudio | T1-stopgap | MIT-0 or Unlicense (dual) | pinned when added | audio device port | native WASAPI adapter at the replacement milestone; entry removed when replaced | planned |
