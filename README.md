# DuckEngine

A game engine written from scratch in Rust. Its two defining properties are **performance** (measured, not assumed) and **flexibility** (no lock-in to any operating system, graphics or audio API, or third-party library: each one sits behind a port the engine owns).

## Status

Early and in active development; nothing here is a stable API. Windows is the primary platform, Linux the second one, and a headless build exists for tests. The phase-by-phase state of the work, with the evidence behind each claim, is in the wiki (the roadmap and the milestone reviews).

The code is written by an AI coding agent working as a software team under the direction of the owner, who is the product owner and architect. Every change goes through a pull request, test-driven development, an automated gate set and a review report in `reviews/`.

## Licence

[Apache-2.0](LICENSE), copyright Giacomo Matzeu (see [NOTICE](NOTICE)). Third-party components keep their own licences and are listed in [THIRD_PARTY.md](THIRD_PARTY.md).

## Build and test

You need the Rust toolchain pinned in `rust-toolchain.toml` (installed automatically by `rustup`).

```
cargo build --workspace --locked
cargo test --workspace --locked
```

The window and OpenGL adapters need the GLFW shared library next to the program to open a window; the headless tests do not.

## Where to read more

- [wiki/README.md](wiki/README.md): the index of the design, the rules and the decisions. Start with [wiki/rules-digest.md](wiki/rules-digest.md) for the short version.
- [CONTRIBUTING.md](CONTRIBUTING.md): how to send a pull request.
- [SECURITY.md](SECURITY.md): how to report a vulnerability.
