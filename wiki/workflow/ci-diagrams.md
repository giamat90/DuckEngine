# How CI works (UML) `[P]`

All diagrams are Mermaid text, so they are versioned with the wiki and render on GitHub. They are views of the workflow files in `.github/workflows/` and of the pages named in each caption; if a diagram and a workflow disagree, the workflow wins and the diagram is fixed. Merging is policy, not enforcement, on the GitHub Free plan ([github-plan-findings](github-plan-findings.md)): no check below can be made "required", so a red check is a stop by policy.

Index: 1 Workflows and triggers (component) · 2 Jobs of the `ci` workflow (activity) · 3 Trusted and untrusted inputs (component) · 4 One task from PR to release (sequence) · 5 The `trusted-pr-gates` checks (activity) · 6 Release (activity) · 7 Verdict of a pull request (state)

---

## 1. Workflows and triggers

Source: `.github/workflows/`, [tooling-and-ci](tooling-and-ci.md), [delivery-pipeline](delivery-pipeline.md). Five workflow files; all five are listed in `gates/required-workflows.txt`, so a pull request that deletes or hollows one is rejected by `trusted-pr-gates`.

```mermaid
flowchart LR
  push["push to main"]
  pr["pull request opened, edited, reopened, synchronized"]
  prt["pull_request_target into main"]
  main["push to main (squash merge of a governance card, or the merge commit of a develop release)"]
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
| `ci` | runs on push to `main` and by hand (`workflow_dispatch`, on a branch before an approval or a release merge); the pushed commit; its `gates` job uses the tools of `main` | by policy: the agent waits for green | no |
| `pr-title` | the base commit (the workflow file itself is the PR's own) | by policy | no |
| `governance-guard` | the base commit (the workflow file itself is the PR's own) | by policy | no |
| `trusted-pr-gates` | the base commit, and the workflow file too (`pull_request_target`) | by policy | no |
| `release` | the merged commit on `main`, after the merge | not applicable | it is the release |

---

## 2. Jobs of the `ci` workflow

Source: `.github/workflows/ci.yml`. Nine independent jobs run in parallel. A new push to the same ref cancels the run in progress (a dispatched run on a branch); a run on `main` is never cancelled.

```mermaid
flowchart TB
  start(["push"]) --> g1 & gates & metrics & deny & headless & glfw_linux & gl_linux & arm64 & d2_determinism

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

  subgraph headless["headless (Ubuntu): no adapter, no application crate (card 2-8)"]
    direction TB
    h1["cargo tree (all targets, all features) without adapter_* and app_*: none of them in the graph"] --> h2["cargo test --workspace --exclude adapter_* --exclude app_*"]
  end

  subgraph glfw_linux["glfw_linux (Ubuntu, card 2-11): the real GLFW adapter under a virtual display"]
    direction TB
    l1{"adapter_window_glfw in the workspace?"} -- no --> l2["notice: not run, adapter not in this workspace yet"]
    l1 -- yes --> l3["apt: xvfb, libglfw3"] --> l4["xvfb-run cargo test -p adapter_window_glfw, DUCK_REQUIRE_GLFW=1"]
  end

  subgraph gl_linux["gl_linux (Ubuntu, card 3-12): the OpenGL adapter on llvmpipe under a virtual display"]
    direction TB
    g1x{"adapter_render_gl in the workspace?"} -- no --> g2x["notice: not run, adapter not in this workspace yet"]
    g1x -- yes --> g3x["apt: xvfb, libglfw3, mesa-utils, libgl1-mesa-dri"] --> g4x["glxinfo -B: report renderer and OpenGL version"] --> g5x["xvfb-run cargo test -p adapter_render_gl, DUCK_REQUIRE_GL=1"]
  end

  subgraph arm64["arm64 (hosted ubuntu-24.04-arm, card 2-10): native AArch64 hardware"]
    direction TB
    r1["test: uname -m is aarch64"] --> r2["cargo clippy --workspace --exclude adapter_*"] --> r3["cargo test --workspace --exclude adapter_*"]
  end

  subgraph d2_determinism["d2_determinism (Windows and Ubuntu x86-64, card 4-20): golden hashes with the feature on"]
    direction TB
    k0["checkout main as trusted/, the push as head/"] --> k1["lint the golden file (trusted)"]
    k1 --> k2{"engine/physics/Cargo.toml: in head, in trusted"}
    k2 -- "neither" --> k3["notice: not run, physics crate absent (the only skip)"]
    k2 -- "only in trusted" --> k4["RED: crate deleted"]
    k2 -- "in head, no recorder" --> k5["RED: recorder missing"]
    k2 -- "in head" --> k6["flags: no target-cpu, no +fma"] --> k7["cargo test -p physics --features cross-platform-determinism, golden:: lines"]
    k7 --> k8["golden_check verify against trusted golden: RED on any difference"] --> k9["cost report, feature on and off (report only)"]
  end

  g1 & gates & metrics & deny & headless & glfw_linux & gl_linux & arm64 & d2_determinism --> done(["all green = the commit may merge"])
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
  GH->>CI: pr-title, governance-guard on every PR; trusted-pr-gates on a PR into main; ci only by hand (governance card, develop checkpoint, release PR)
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
  skip -- "no" --> sq["check: the commit subject equals the PR title (else the run fails); the Release-Batch footer is present exactly when the head branch is develop, and bumps the minor number"]
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
