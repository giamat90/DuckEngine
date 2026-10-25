"""Checks that run from the base commit on a pull request (card 0c-14).

The workflows that run these (trusted-pr-gates.yml) are loaded from the base branch, so a
pull request cannot edit or delete them to switch the check off. The PR tree is data only.

    pr_gates.py workflows --repo head --required base/gates/required-workflows.txt
    pr_gates.py approval  --repo head --base <sha> --head <sha> --body-file pr.md

`workflows` fails when an enforcement workflow listed in the required file is missing in the
PR tree (deleted, emptied, a folder or a symlink). `approval` fails when the PR description
has an owner approval record that does not match the diff; with no record yet it prints
PENDING and exits 0 (the owner approves just before the merge, and every push or description
edit runs the check again). Exit 0 when nothing failed, 1 otherwise.
"""

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "diff-hash"))
import diff_hash  # noqa: E402  (tool from the same trusted checkout)

WORKFLOW_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*\.ya?ml")


def load_required(text):
    """Workflow file names from the required list: one per line, `#` comments, no paths."""
    names = [line.strip() for line in text.splitlines() if line.strip() and not line.strip().startswith("#")]
    if not names:
        raise ValueError("the required-workflows list is empty")
    for name in names:
        if not WORKFLOW_NAME.fullmatch(name):
            raise ValueError(f"'{name}' is not a plain workflow file name")
    return names


HAS_TRIGGER = re.compile(r"(?m)^[\"']?on[\"']?:")
HAS_JOBS = re.compile(r"(?m)^jobs:[ 	]*(#.*)?$")


def workflow_problem(name, path):
    """Why a required workflow is not acceptable in the PR tree, or None."""
    if path.is_symlink():
        return f"{name}: a symlink is not a workflow"
    if not path.is_file():
        return f"{name}: missing (deleted, or not a file)"
    text = path.read_text(encoding="utf-8", errors="replace")
    if not text.strip():
        return f"{name}: empty"
    # Shape only: a hollowed-out file (no trigger, or no job under `jobs:`) no longer enforces anything.
    if not HAS_TRIGGER.search(text) or not HAS_JOBS.search(text) or not re.search(r"(?m)^  \S+:", text.split("jobs:", 1)[1]):
        return f"{name}: no trigger or no job (hollowed out)"
    return None


def folder_problem(folder):
    """Why the workflows folder itself (or .github) cannot be trusted, or None."""
    for part in (folder, getattr(folder, "parent", None)):
        if part is not None and part.is_symlink():
            return f"{part}: a symlink in place of a folder"
    return None


def missing_workflows(repo, names):
    folder = Path(repo) / ".github" / "workflows"
    why_folder = folder_problem(folder)
    if why_folder:
        return [why_folder]
    return [why for why in (workflow_problem(name, folder / name) for name in names) if why]


def approval_verdict(body, computed, task=None):
    """('pending' | 'ok' | 'fail', problems) for the approval record in the PR description."""
    if not diff_hash.approval_sections(body):
        return "pending", []
    problems = diff_hash.check_approval(body, computed, task=task)
    return ("fail" if problems else "ok"), problems


def _run_workflows(args, print_line):
    try:
        names = load_required(Path(args.required).read_text(encoding="utf-8"))
    except (OSError, ValueError, UnicodeDecodeError) as error:
        print_line(f"FAIL cannot read the required-workflows list: {error}")
        return 1
    problems = missing_workflows(args.repo, names)
    for problem in problems:
        print_line(f"FAIL {problem}")
    print_line(("FAIL" if problems else "OK") + f" {len(names)} required workflows, {len(problems)} problems")
    return 1 if problems else 0


def _run_approval(args, print_line):
    try:
        computed = diff_hash.compute(args.base, args.head, repo=args.repo)
        body = Path(args.body_file).read_text(encoding="utf-8")
    except (diff_hash.DiffHashError, OSError, UnicodeDecodeError) as error:
        print_line(f"FAIL cannot check the approval record: {error}")
        return 1
    verdict, problems = approval_verdict(body, computed, task=args.task)
    for problem in problems:
        print_line(f"FAIL {problem}")
    summary = {"pending": "PENDING no owner approval record yet (the owner approves just before the merge)",
               "ok": f"OK approval record matches diff hash {computed}",
               "fail": f"FAIL approval record does not match the diff (diff hash {computed})"}
    print_line(summary[verdict])
    return 1 if verdict == "fail" else 0


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    flows = commands.add_parser("workflows")
    flows.add_argument("--repo", required=True)
    flows.add_argument("--required", required=True)
    approval = commands.add_parser("approval")
    approval.add_argument("--repo", required=True)
    approval.add_argument("--base", required=True)
    approval.add_argument("--head", required=True)
    approval.add_argument("--body-file", required=True)
    approval.add_argument("--task")
    return parser


def main(argv, print_line=print):
    args = build_parser().parse_args(argv)
    return (_run_workflows if args.command == "workflows" else _run_approval)(args, print_line)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
