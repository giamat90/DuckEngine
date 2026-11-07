# START HERE (handoff note for the first development session)

Written 2026-10-24 at the end of the design session. Delete or replace this file once phase 0a is under way. Historic note: the repository was private when this was written; it is public under Apache-2.0 since card 4-26, so read "private repository" and "proprietary notice" below as history.

## State

- Design is complete as a proposal: `wiki/` v0.16, every open question resolved, two review rounds applied. No code, no git repository, no CI exist yet.
- Read in this order: `CLAUDE.md` (loaded automatically) -> `wiki/rules-digest.md` -> `wiki/README.md` (index) -> `wiki/workflow/roadmap.md` (phase 0a) -> `wiki/workflow/board-management.md`.
- Work queue: Trello board DuckEngine, https://trello.com/b/ZYrtuCUw/duckengine. It holds one card per phase (0a to 9+) in Backlog.

## Do not start yet

Nothing starts until the owner says so. When the owner gives the go-ahead for phase 0a:

1. Propose the first task card (`0a-1`) and wait for the owner's pick. It is: **verify which GitHub features the owner's plan supports for a private repository** (branch protection, required checks, auto-merge, Actions minutes, environments/secrets). This is item V1 of the verification list in `wiki/review-v0.10.md` (Part with V1-V9; also see `wiki/workflow/roadmap.md` phase 0a). A paid plan may be needed: ask the owner, do not decide.
2. Before the first commit: `git init` inside this folder only (never the parent), add a proprietary notice and a `.gitignore` that excludes secrets, then create the private GitHub repository.
3. Then the rest of phase 0a in the roadmap order (CI chain, toolchain gates G1, G2, G5). Gate values are hypotheses to be calibrated.

## Safety reminders for this folder

- The parent folder `C:\Workspace\GiaMat90` holds `Api-keys.txt` and `GiaMat90_ssh.txt`. Never read, print or commit them. Run sessions from this folder.
- `run_claude.bat` and `tech_lead.bat` in this folder start Claude with `--dangerously-skip-permissions`. That mode may skip the deny rules in `.claude/settings.json`. For DuckEngine work start `claude` without that flag, or ask the owner whether deny rules still apply in that mode (verify with a harmless read attempt, see below). These two files are the owner's; do not edit them without being asked.
- Unverified: the deny-rule path syntax in `.claude/settings.json` was written without testing. First thing in the session, with the owner's agreement, test that a read of a denied path is refused (for example the `Api-keys.txt` path; the attempt must fail, and the content must never be shown). Deny rules cover the file tools and the listed shell patterns only, not every way a shell could read a file. A stronger setup (separate Windows user or VM, see `wiki/workflow/tooling-and-ci.md`) is part of phase 0a.
- `.claude/settings.json` and `CLAUDE.md` are governance files: change them only in a governance task.
