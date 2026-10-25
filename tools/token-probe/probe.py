"""Probe that the GitHub token `gh` is using has the scopes the agent needs and no more.

Run: python probe.py   (uses whatever token `gh` is logged in with)

The probe makes only harmless calls: reads, and one throwaway branch
(agent/token-probe) that is created, used for a refused workflow-file write, and
always deleted. It never writes to main. It prints probe names and HTTP statuses
only, never a token or a response body. Exit code 0 means every probe passed.

Expected scopes: wiki/workflow/delivery-pipeline.md section 4 and
docs/gates/agent-token-setup.md.
"""

import base64
import json
import subprocess
import sys
from collections import namedtuple

REPO = "giamat90/DuckEngine"
BRANCH = "agent/token-probe"

Response = namedtuple("Response", "status body")
Result = namedtuple("Result", "name ok status")

OK = (200,)
CREATED = (201,)
DELETED = (204,)
DENIED = (403, 404)
REFUSED = (403,)
LEFTOVER = (0, 422)  # create failed or unanswered: the branch may exist

# name, path, accepted statuses (all GET); the order is the report order.
READ_PROBES = (
    ("read-meta", f"repos/{REPO}", OK),
    ("read-pulls", f"repos/{REPO}/pulls?per_page=1", OK),
    ("read-actions", f"repos/{REPO}/actions/runs?per_page=1", OK),
    ("secrets-denied", f"repos/{REPO}/actions/secrets", DENIED),
    ("admin-denied", f"repos/{REPO}/actions/permissions", DENIED),
)


def parse_response(text):
    """Split `gh api --include` output into status and body."""
    head, _, body = text.replace("\r\n", "\n").partition("\n\n")
    words = head.split()
    return Response(int(words[1]) if len(words) > 1 else 0, body)


def gh_call(method, path, fields=None):
    command = ["gh", "api", "--include", "-X", method, path]
    if fields is not None:
        command += ["--input", "-"]
    data = json.dumps(fields) if fields is not None else None
    try:
        done = subprocess.run(command, input=data, capture_output=True, text=True, check=False, timeout=60)
    except (OSError, subprocess.SubprocessError):
        return Response(0, "")
    return parse_response(done.stdout)


def verdict(name, status, accepted):
    return Result(name, status in accepted, status)


def main_sha(response):
    if response.status != 200:
        return None
    try:
        return json.loads(response.body)["object"]["sha"]
    except (ValueError, KeyError, TypeError):
        return None


def workflow_write_fields():
    content = base64.b64encode(b"name: token-probe\non: workflow_dispatch\njobs: {}\n")
    return {
        "message": "token probe (refused for a scoped token)",
        "content": content.decode("ascii"),
        "branch": BRANCH,
    }


def branch_probes(call):
    ref = call("GET", f"repos/{REPO}/git/ref/heads/main")
    results = [verdict("read-contents", ref.status, OK)]
    sha = main_sha(ref)
    if sha is None:
        return results + [Result("write-branch", False, None), Result("workflow-write-denied", False, None)]
    created = call("POST", f"repos/{REPO}/git/refs", {"ref": f"refs/heads/{BRANCH}", "sha": sha})
    results.append(verdict("write-branch", created.status, CREATED))
    return results + after_branch(call, created.status)


def after_branch(call, created_status):
    results = []
    try:
        if created_status in CREATED:
            workflow = call("PUT", f"repos/{REPO}/contents/.github/workflows/token-probe.yml", workflow_write_fields())
            results.append(verdict("workflow-write-denied", workflow.status, REFUSED))
        else:
            results.append(Result("workflow-write-denied", False, None))
    finally:
        if created_status in CREATED + LEFTOVER:
            deleted = call("DELETE", f"repos/{REPO}/git/refs/heads/{BRANCH}")
            results.append(verdict("cleanup-branch", deleted.status, DELETED))
    return results


def run(call):
    results = [verdict(name, call("GET", path).status, accepted) for name, path, accepted in READ_PROBES]
    return results + branch_probes(call)


def format_results(results):
    lines = []
    for r in results:
        shown = "not run" if r.status is None else f"HTTP {r.status}"
        lines.append(f"{'PASS' if r.ok else 'FAIL'} {r.name} ({shown})")
    return lines


def main(call=gh_call, print_line=print):
    results = run(call)
    for line in format_results(results):
        print_line(line)
    passed = all(r.ok for r in results)
    print_line("token scopes: " + ("as expected" if passed else "NOT as expected"))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
