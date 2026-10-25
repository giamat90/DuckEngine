"""Fail a PR that changes a protected path without a declared governance task (card 0b-8).

Protected paths are listed in gates/protected-paths.txt (format and matching rules in
protected_paths.py, shared with the change classifier). The PR description must then contain a line
`Governance task: yes (card <id>)`. Traceable, not unforgeable: the agent writes
the description, the owner reads it at approval. Run from the trusted base.

    governance_guard.py --repo head --base <sha> --head <sha> --paths protected-paths.txt --body-file body.md
"""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

from protected_paths import is_protected, load_protected  # noqa: F401  (the matcher shared with the classifier)

GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
DECLARATION = re.compile(r"Governance task: yes \(card [^()\s]+\)")
# An unclosed comment or fence hides the rest of the text, as it does when rendered.
HTML_COMMENT = re.compile(r"<!--.*?(?:-->|\Z)", re.S)
FENCED_BLOCK = re.compile(r"^[ ]{0,3}(`{3,}|~{3,}).*?(?:^[ ]{0,3}\1[`~]*[ \t]*$|\Z)", re.S | re.M)


class GuardError(Exception):
    pass


def declared(body):
    text = body.replace("\r\n", "\n").replace("\r", "\n")
    text = HTML_COMMENT.sub("", FENCED_BLOCK.sub("", text))
    return any(DECLARATION.fullmatch(line) for line in text.split("\n"))


def problems(changed, protected, body):
    hits = sorted(path for path in changed if is_protected(path, protected))
    if hits and not declared(body):
        return [f"protected paths changed without a 'Governance task: yes (card <id>)' line in the PR description: {', '.join(hits)}"]
    return []


def changed_files(repo, base, head):
    try:
        done = subprocess.run(
            ["git", "diff", "--name-only", "--no-renames", "--no-ext-diff", "--ignore-submodules=none", "-z", f"{base}...{head}"], cwd=repo, env=GIT_ENV, capture_output=True, check=False
        )
    except OSError as error:
        raise GuardError(f"cannot run git in {repo}: {error}") from error
    if done.returncode != 0:
        raise GuardError(f"git diff {base}...{head} failed: {done.stderr.decode(errors='replace').strip()}")
    return [name for name in done.stdout.decode(errors="replace").split("\0") if name]


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path("."))
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--paths", type=Path, required=True)
    parser.add_argument("--body-file", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        protected = load_protected(args.paths.read_text(encoding="utf-8"))
        body = args.body_file.read_text(encoding="utf-8")
        found = problems(changed_files(args.repo, args.base, args.head), protected, body)
    except (GuardError, ValueError, OSError) as error:
        print_line(f"FAIL {error}")
        return 1
    for problem in found:
        print_line(f"FAIL {problem}")
    if not found:
        print_line("OK no protected path changed without a declaration")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
