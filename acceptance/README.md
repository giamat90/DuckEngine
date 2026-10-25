# Holdout acceptance suite (skeleton, card 0c-6)

Why: with governance by metrics and the product only, an agent can satisfy the numbers without delivering the intent ([architect-review-surface](../wiki/workflow/architect-review-surface.md) section 7). The holdout suite is the counter-measure: outcomes the owner specified in plain language, written by an agent other than the one implementing the feature, kept in a protected path: a change there is flagged by the governance guard and needs the owner's reading of the governance diff.

This folder is Class 3 (see `gates/protected-paths.txt`): it changes only in a task the owner declares as a governance task.

## Rules

- **Who writes what (policy, not checked by any tool).** The owner states the outcome in plain language. A separate agent (not the implementing one, D-2) turns it into a scenario file and, later, a check. The implementing agent never edits this folder.
- **One scenario, one TOML file** in `scenarios/`, named `<id>.toml`. Fields: `id` (lowercase words with dashes, equal to the file name), `title`, `status` (`pending` or `active`), `phase` (text, the phase it first applies to), `expect` (the outcome, plain language), `check` (how it is checked; required when `active`). No other keys.
- **`pending`** means the outcome is specified but no check backs it yet. It is counted and reported as pending, never as passing.
- **`active`** means a check exists and must pass. The skeleton does not run checks yet.
- **Rotation (not built yet).** Part of the suite is meant to live only in CI and to be replaced each phase, so agents cannot overfit to it. Its storage and the way CI reads it are not decided; they need a card and the owner's decision.
- **Never weaken a scenario** to get green: no deleting, no `active` back to `pending`, no loosened `expect`, without a governance task. The validator checks shape only and does not detect weakening (no baseline or count floor yet); the protection is the protected path plus the owner's reading.

## Validate

    python acceptance/validate.py --dir acceptance/scenarios

Checks the shape only (the unit tests run in `ci`). `scenarios/` may hold only `<id>.toml` files; any other file, a symlink, a nested folder or a file over 64 KiB is a violation. The approval metric report does not read the pending count yet. It is not yet run from a trusted checkout of `main` in the `gates` job, because the tool did not exist on `main` before this card; wiring it there is a follow-up.

`scenarios/example-window-opens.toml` is a placeholder that shows the format.
