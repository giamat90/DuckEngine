# Agent GitHub token: setup and verification (card 0b-1)

Why: the agent works under a token that can do the delivery flow and nothing
more, so a bug or a hostile instruction in a file it reads cannot change
settings, read secrets or edit CI. Design: [delivery-pipeline section 4](../../wiki/workflow/delivery-pipeline.md).
The token is created and stored by the owner. The agent never sees, prints or
stores it.

## 1. The token

Type: **fine-grained personal access token** (GitHub, Settings, Developer
settings, Personal access tokens, Fine-grained tokens).

| Setting | Value |
|---|---|
| Resource owner | `giamat90` |
| Repository access | **Only select repositories**: `giamat90/DuckEngine` |
| Expiration | owner's choice; 90 days or less is suggested, renew on expiry |
| Repository permission **Contents** | Read and write (push `agent/*` branches, merge PRs) |
| Repository permission **Pull requests** | Read and write (open PRs, merge them) |
| Repository permission **Actions** | Read (read CI runs and logs) |
| Repository permission **Metadata** | Read (GitHub adds it automatically) |
| Repository permission **Workflows** | Read and write was the decision of 2026-10-25 (card 0c-8). On the public repository (card 4-26) the owner drops it: for a governance task that changes `.github/workflows/*` the owner enables it for that push only; the owner reads every workflow diff, see section 4 |
| Everything else | **No access**: in particular not Commit statuses (tried, it does not help, see below), Administration, Secrets, Environments, Variables, Webhooks, Issues |

Finding (2026-10-25): `gh pr checks` and `gh pr view --json statusCheckRollup`
do **not** work with this token, even with Commit statuses added: reading check
runs needs a Checks permission that a personal fine-grained token does not
offer ("Resource not accessible by personal access token"). Use the Actions API
instead, which Actions: Read covers and which works:

```
gh run list --commit <sha>              # find the run id
gh run view <run-id> --json status,conclusion,jobs
gh run watch <run-id> --exit-status     # waits, exit code 0 only if green
```

Editing a fine-grained token's permissions does not change its value.

## 2. Putting the token to use (owner, in your own terminal)

Do this in a terminal of your own, **not** in the Claude session, so the token
never enters the conversation:

```
gh auth login --hostname github.com --with-token
```

Paste the token, press Enter, then Ctrl+Z and Enter (Windows). This replaces
the broad login that `gh` has today (OAuth token with `repo` and `workflow`).
To go back to a broad login for your own administrative work:
`gh auth login --web`. Administrative changes (repository settings, secrets)
are done by you in the browser.

## 3. Verifying (agent, harmless probe)

```
python tools/token-probe/probe.py
```

The probe uses whatever token `gh` has. Creating its branch triggers a CI run
on that branch (`ci.yml` runs on every push), a few Actions minutes each time
(free for standard runners on a public repository; 2,000 per month on a private Free one), so run it when the token changes, not casually.
It creates one throwaway branch `agent/token-probe`, always deletes it, never
touches `main`, and prints probe names and HTTP statuses only.

| Probe | Meaning | Expected |
|---|---|---|
| read-meta, read-pulls, read-actions, read-contents | Metadata, Pull requests, Actions, Contents read | 200 |
| write-branch | Contents write (create a ref) | 201 |
| secrets-denied | no Secrets permission | 403 or 404 |
| admin-denied | no Administration permission | 403 or 404 |
| cleanup-branch | Contents write (delete the ref) | 204 |

Workflows write is not probed either: it is granted on purpose, and GitHub offers
no read-only test of a write permission, so the probe never writes a workflow
file (before card 0c-8 it expected a refusal).
Pull-request write is not probed (it would leave a comment on a real PR). It is
shown by opening and merging the card's own PR with the new token.

Today's broad login fails three probes (checked 2026-10-25, before the scoped
token existed): secrets readable (200), administration readable (200), workflow
write accepted (201; that probe no longer exists). The scoped token removes the
first two.

## 4. Limits to know

- No branch protection or ruleset is configured: it was a plan limit while the repository was private and is the owner's choice now ([github-plan-findings](../../wiki/workflow/github-plan-findings.md)).
  A token with Contents write can push to `main`; "never push to `main`" is a
  written rule, not enforced by this token.
- With the Workflows permission the agent can push changes under
  `.github/workflows/`, and a workflow is PR-controlled: a changed workflow can
  switch its own check off. So the owner reads **every workflow diff** (the PR
  description names it), and a workflow change only happens in a task the owner
  declares as a governance task. The permission is a convenience, not a gate.

## 5. Result of the verification (2026-10-25)

Run by the agent in the 0b-1 session with the scoped token (output is in the
PR description): **9 of 9 probes as expected** (reads 200; branch create 201
and delete 204; workflow-file write, secrets and administration refused with
403; the workflow probe was removed in card 0c-8, leaving 8). The same probe with the previous
broad login failed 3 of 9. Not covered by the probe, to be shown by the card's own
PR (not yet verified when this was written): pull-request write and `gh pr merge` with this token.

Owner follow-up: the token value was pasted into the chat once, and it was
created without an expiry. Rotate it (Regenerate token, set an expiry) and log
`gh` in again in your own terminal; rerun the probe afterwards.
