# Contributing to DuckEngine

Thank you for looking. Pull requests from outside are welcome.

## How to send a pull request

1. Fork the repository and branch from `develop`, not from `main`. Ordinary pull requests go into `develop`.
2. Make a small, focused change. Name the branch for what it does.
3. Give the pull request a Conventional Commit title, `type(scope): summary`, with type one of `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style`, `revert`. The title becomes the squash commit message and decides the release version; the `pr-title` check refuses anything else.
4. Describe what changed, why, and how you tested it.

Checks on pull requests into `develop` are deliberately few (`pr-title` and `governance-guard`; `trusted-pr-gates` runs on pull requests into `main` only); the full CI runs on `main` and by hand. Because a first-time contributor's workflow runs wait for approval, they may start late. Run the tests locally first (`cargo test --workspace --locked`).

## How the project works

Development is test-first: a list of tests, one failing test at a time, the smallest change to make it pass, then clean-up. Bug fixes begin with a failing regression test, and a test is never weakened or deleted to get green. Quality is measured: coverage, mutation score, complexity limits, zero warnings, and a benchmark gate on instruction counts. The values and the reasons are in [wiki/workflow/tdd-workflow.md](wiki/workflow/tdd-workflow.md) and [wiki/workflow/quality-metrics.md](wiki/workflow/quality-metrics.md).

Every change is reviewed (self, automated, independent, QA, architecture, security, performance, product fit) and the owner approves each merge against a hash of the diff; for an outside pull request the owner does that reading. Details: [wiki/workflow/clean-code-and-review.md](wiki/workflow/clean-code-and-review.md) and [wiki/workflow/delivery-pipeline.md](wiki/workflow/delivery-pipeline.md). Start with [wiki/rules-digest.md](wiki/rules-digest.md).

## What is owner-only

The governance files define the gates and the delivery rules: `gates/`, `.github/`, `tools/gate-check`, `tools/release` and the other tool folders listed in [gates/protected-paths.txt](gates/protected-paths.txt), `CLAUDE.md`, `wiki/rules-digest.md`, `wiki/decisions/`, `THIRD_PARTY.md`, `deny.toml`, `clippy.toml`, `rust-toolchain.toml`. A pull request that touches one of them is flagged by the `governance-guard` check and will be refused: only the owner changes them, in a task the owner declares as a governance task. If you think one of them should change, open an issue and say why.

## Licence of your contribution

The project is licensed under [Apache-2.0](LICENSE). By submitting a contribution you agree to the terms of Apache-2.0 section 5: it is licensed to the project and everyone else under the same licence as the rest (inbound = outbound). There is no contributor licence agreement and no sign-off line to add. Only submit work you have the right to submit.

## Bugs and vulnerabilities

Ordinary bugs: open an issue with the steps to reproduce. Vulnerabilities: do not open a public issue, follow [SECURITY.md](SECURITY.md).

## Be kind

Be patient and respectful with everyone, whatever their experience. Criticise the code, not the person. The owner may close a pull request that does not fit the project's direction; that is no judgement on you.
