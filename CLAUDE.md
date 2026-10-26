# DuckEngine - instructions for Claude Code

This file is a governance file (Class 3): it changes only in a task the owner declares as a governance task.

Project: DuckEngine, a game engine written from scratch in Rust. Private, proprietary. The owner is product owner and architect; you (Claude Code) work as a software team and write all the code. The owner reads design notes, metric reports and sensitive/governance diffs, not the code.

@wiki/rules-digest.md

## How to work

- The wiki in `wiki/` is canonical. Start at `wiki/README.md`. If `wiki/rules-digest.md` and a wiki page disagree, the page wins and you report the digest as stale. Read a page when your task touches its area (architecture, workflow, decisions).
- Use the `software-team` skill on every engineering task. Follow TDD (test list first, red evidence, smallest step to green, refactor on green) and finish with the review phase R1-R8 described in `wiki/workflow/tdd-workflow.md` and `wiki/workflow/clean-code-and-review.md`.
- The work queue is the Trello board DuckEngine (https://trello.com/b/ZYrtuCUw/duckengine). Follow `wiki/workflow/board-management.md`: you move and create cards; the repo stays canonical.
- Phases 0a, 0b and 0c (repo, delivery pipeline, governance tooling) are done. Do not start phase 1 or any later phase until the owner gives an explicit go-ahead. Propose next tasks only when asked, then wait for the owner's pick.
- How CI, the trusted PR gates and the release work: `wiki/workflow/ci-diagrams.md`. A red check is a stop; merging is policy on the GitHub Free plan, not enforcement.
- Owner approval is given once, in the session, against a diff hash. Before you ask for it, open the PR and give the owner its full link (URL) together with the hash; merge only after the approval. Never treat a card move, a comment or silence as approval.
- A task is closed when its PR is merged: move the card to Done at once, walking it through the columns in order, without asking.
- Every change to a documentation page is covered by a UML view in Mermaid, in the page or in the PR (typo, link and number fixes are exempt): `wiki/workflow/clean-code-and-review.md` section 7.

## Hard rules (also in the digest, repeated because they matter most)

- Work only inside this folder (`DuckEngine/`). The git repository is this folder only, never the parent. Never read, print, copy or commit `Api-keys.txt`, `GiaMat90_ssh.txt` or anything outside this folder.
- No secrets in code, logs, commits, cards or comments.
- Never push to `main` directly. Never bypass hooks (`--no-verify`) or signing. Never lower a threshold, skip or delete a test, or add a waiver to get green.
- Do not edit governance files (gate configuration, thresholds, classifier, arch-check, CI/release workflows, ADRs, `THIRD_PARTY.md` structure, `wiki/rules-digest.md`, this file, `.claude/settings.json`) unless the owner declared a governance task.
- Report outcomes faithfully: failing tests, skipped steps and unverified claims are stated as such.
