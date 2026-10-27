# How CI works (UML) `[P]`

All diagrams are Mermaid text, so they are versioned with the wiki and render on GitHub. They are views of the workflow files in `.github/workflows/` and of the pages named in each caption; if a diagram and a workflow disagree, the workflow wins and the diagram is fixed. Merging is policy, not enforcement, on the GitHub Free plan ([github-plan-findings](github-plan-findings.md)): no check below can be made "required", so a red check is a stop by policy.

Index: 1 Workflows and triggers (component) · 2 Jobs of the `ci` workflow (activity) · 3 Trusted and untrusted inputs (component) · 4 One task from PR to release (sequence) · 5 The `trusted-pr-gates` checks (activity) · 6 Release (activity) · 7 Verdict of a pull request (state)

---

## 1. Workflows and triggers

Source: `.github/workflows/`, [tooling-and-ci](tooling-and-ci.md), [delivery-pipeline](delivery-pipeline.md). Five workflow files; all five are listed in `gates/required-workflows.txt`, so a pull request that deletes or hollows one is rejected by `trusted-pr-gates`.

```mermaid
flowchart LR
  push["push to any branch or tag"]
  pr["pull request opened, edited, reopened, synchronized"]
  prt["pull_request_target into main"]
  main["push to main (squash merge)"]
  manual["workflow_dispatch"]

  push --> ci
  manual --> ci
  pr --> title["pr-title"]
  pr --> guard["governance-guard"]
  prt --> trusted["trusted-pr-gates"]
  main --> release
  main --> ci

  ci["ci: build, test, gates, metrics, deny"]
  release["release: version, tag, GitHub Release"]
```

| Workflow | Tools come from | Gates the merge? | Gates the release? |
|---|---|---|---|
| `ci` | the pushed commit; its `gates` job uses the tools of `main` | by policy: the agent waits for green | no |
| `pr-title` | the base commit (the workflow file itself is the PR's own) | by policy | no |
| `governance-guard` | the base commit (the workflow file itself is the PR's own) | by policy | no |
| `trusted-pr-gates` | the base commit, and the workflow file too (`pull_request_target`) | by policy | no |
| `release` | the merged commit on `main`, after the merge | not applicable | it is the release |

---

## 2. Jobs of the `ci` workflow

Source: `.github/workflows/ci.yml`. Four independent jobs run in parallel. A new push to the same branch cancels the run in progress; a run on `main` is never cancelled.

```mermaid
flowchart TB
  start(["push"]) --> g1 & gates & metrics & deny

  subgraph g1["g1: build, test, fmt, clippy (Windows and Ubuntu)"]
    direction TB
    a1["install the pinned toolchain (rustup show active-toolchain)"] --> a2["cargo fmt --check"]
    a2 --> a3["cargo clippy -D warnings"]
    a3 --> a4["cargo build, cargo test"]
    a4 --> a5["unit tests of every Python tool"]
  end

  subgraph gates["gates from main (Ubuntu; the approval report is skipped on main and tags)"]
    direction TB
    b1["checkout main as trusted/, the push as head/"] --> b2["architecture check"]
    b2 --> b2b["unsafe budget (gate G3)"]
    b2b --> b3["wiki lint"]
    b3 --> b4["digest equals gate configuration"]
    b4 --> b5["approval report into the run summary<br/>(branches only, report, never fails the job)"]
  end

  subgraph metrics["metrics (Windows and Ubuntu; skipped on main and on tags)"]
    direction TB
    c1["coverage"] --> c2["mutation on the skeleton crates"]
    c2 --> c3["mutation control, gate G1"]
    c3 --> c4["benchmark"]
    c4 --> c4b["G1 assertion"]
    c4b --> c5["report artifact (report only)"]
  end

  subgraph deny["deny (Windows and Ubuntu)"]
    direction TB
    d1["cargo-deny: licences, bans, sources, advisories"]
  end

  g1 & gates & metrics & deny --> done(["all green = the commit may merge"])
```

---

## 3. Trusted and untrusted inputs

Source: [architect-review-surface](architect-review-surface.md), [rules-digest §8](../rules-digest.md). The rule: tools and thresholds come from a checkout of `main`; the pull request tree is data. Exceptions are stated, not hidden.

```mermaid
flowchart LR
  subgraph trusted["Trusted: checkout of main (base commit)"]
    tools["gate tools (tools/*)"]
    cfg["gates/*.toml, protected-paths.txt, required-workflows.txt"]
    twf["trusted-pr-gates (the workflow file too)"]
  end

  subgraph untrusted["Untrusted: the pull request tree"]
    code["engine code, wiki, docs"]
    pwf["ci.yml, pr-title.yml, governance-guard.yml (the PR's own copy runs; release.yml runs from main after the merge)"]
    ptools["tools/* (only their own unit tests run)"]
  end

  tools -- "judge, never execute" --> code
  cfg -- "decide" --> code
  twf -- "judges; the PR cannot edit it, and it rejects a deleted or hollowed pwf" --> pwf
  ptools -. "unit tests in job g1, not a gate" .-> tools

  note["Exceptions: arch-check runs cargo inside the PR tree and so honours its rust-toolchain.toml and .cargo/ (both protected paths). The PR can edit ci.yml, so the owner reads any workflow diff."]
  pwf --- note
```

---

## 4. One task from PR to release

Source: [delivery-pipeline](delivery-pipeline.md), [board-management](board-management.md), `CLAUDE.md`. The owner approves once, in the session, against a diff hash; the agent merges only after that.

```mermaid
sequenceDiagram
  actor Owner
  participant Agent as Claude Code
  participant GH as GitHub
  participant CI as ci, pr-title, governance-guard, trusted-pr-gates
  participant Rel as release

  Agent->>Agent: TDD, review phase R1-R8, review report
  Agent->>GH: push branch, open PR (title = Conventional Commit, governance line if protected paths)
  GH->>CI: ci on the branch push; pr-title, governance-guard, trusted-pr-gates on the PR
  CI-->>Agent: results per commit (gh run list --commit)
  Agent->>Owner: PR link, diff hash, metric report
  Owner-->>Agent: "I approve" (in the session)
  Agent->>GH: append approval record (task, diff hash, basis, date) to the PR body
  GH->>CI: trusted-pr-gates re-checks the record against the diff
  Agent->>Agent: diff_hash check, all four green on the same commit
  Agent->>GH: squash merge (subject = PR title, head commit pinned)
  GH->>Rel: push to main
  Rel->>GH: version, tag, Release with assets
  Agent->>Agent: card to Done with a comment
```

---

## 5. The `trusted-pr-gates` checks

Source: `.github/workflows/trusted-pr-gates.yml`, [github-plan-findings](github-plan-findings.md). It runs on `pull_request_target`, so GitHub loads the workflow file from the base branch: a pull request cannot edit or delete the version that judges it. A canary pull request (#36, closed unmerged, 2026-10-25) deleted `pr-title.yml`: `trusted-pr-gates` failed at step 2 with "pr-title.yml: missing" while the governance step passed.

```mermaid
flowchart TB
  s(["PR into main"]) --> co["checkout base as base/, PR head as head/ (data only)"]
  co --> g["1. governance guard: protected path changed, so the description must declare a governance task"]
  g --> w["2. every workflow in required-workflows.txt is present and has a trigger and a job"]
  w --> ap["3. approval record in the description matches the diff hash<br/>(no record yet = pending, a wrong record = fail)"]
  ap --> ac["4. acceptance scenarios are well formed"]
  ac --> ok(["green: no record contradicts the diff"])
  g -- "fails" --> red(["red: stop"])
  w -- "fails" --> red
  ap -- "fails" --> red
  ac -- "fails" --> red
```

Green means "no approval record contradicts the diff", not "the owner approved". The agent writes the description, so the explicit in-session approval stays the only approval.

---

## 6. Release

Source: `.github/workflows/release.yml`, [delivery-pipeline](delivery-pipeline.md). Every merge releases at least a patch version, docs-only included. One release at a time; a queued run waits.

```mermaid
flowchart TB
  p(["push to main"]) --> plan["plan the version from the last tag to this commit"]
  plan --> skip{"commit already tagged (a re-run)?"}
  skip -- "yes" --> end1(["stop"])
  skip -- "no" --> sq["check: the squash subject equals the PR title (else the run fails)"]
  sq --> mk["changelog from git log, build assets (source workspace, tools, samples)"]
  mk --> old["delete a draft left by a failed attempt (a published Release is never touched)"]
  old --> dr["create a draft Release at the merge commit"]
  dr --> one{"exactly one draft for the tag?"}
  one -- "no" --> fail(["fail"])
  one -- "yes" --> pub["publish the draft"]
  pub --> end2(["tag and Release exist"])
```

---

## 7. Verdict of a pull request

Source: sections 1 to 6. This is the policy view; GitHub itself does not enforce it on the Free plan.

```mermaid
stateDiagram-v2
  [*] --> Open: PR opened
  Open --> Checking: push or edit of the PR
  Checking --> Red: any workflow fails
  Red --> Open: fix pushed
  Checking --> Green: four workflows green on the head commit
  Green --> AwaitingOwner: link and diff hash given to the owner
  AwaitingOwner --> Approved: explicit "I approve" in the session
  AwaitingOwner --> Open: owner asks for a change
  Green --> Checking: new push or edit
  AwaitingOwner --> Checking: new push or edit (the diff hash changes, ask again)
  Approved --> Checking: new push or edit (approval void)
  Approved --> Merged: approval record appended, trusted-pr-gates re-checked, still green on the same head commit
  Merged --> Released: release workflow
  Released --> [*]
```

A card move, a comment, silence or a background notification is never an approval.
