"""Diff hash and approval check (card 0b-5).

The diff hash is the content hash of the proposed change against main
(wiki/workflow/delivery-pipeline.md section 2): the first 16 hex characters of
the sha256 of the normalized `git diff base...head`. It does not depend on
commit ids, messages or dates, nor on line numbers, so a rebase without
conflicts keeps it.

    python diff_hash.py compute --base origin/main --head origin/<branch>
    python diff_hash.py check   --base origin/main --head origin/<branch> --body-file pr.md

Run it from a trusted checkout of main: the tool is not taken from the PR tree.
"""

import argparse
import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

HASH_LENGTH = 16
SECTION_TITLE = "Owner approval"
BASES = ("Product view", "Metrics", "Both")
FIELDS = ("Task", "Diff hash", "Basis", "Approved")

HUNK_HEADER = re.compile(r"^@@ .*$")
HASH_VALUE = re.compile(rf"`?([0-9a-f]{{{HASH_LENGTH}}})`?")
HEADING = re.compile(r"^#{1,6} ")
SECTION_START = re.compile(rf"^#{{1,6}} +{SECTION_TITLE}\s*$", re.M | re.I)
# Text that renders as code or is hidden: a record inside it is an example, not a record.
FENCED_BLOCK = re.compile(r"^ {0,3}(`{3,}|~{3,})[^\n]*\n.*?(?:^ {0,3}\1[^\n]*$|\Z)", re.M | re.S)
HTML_COMMENT = re.compile(r"<!--.*?(?:-->|\Z)", re.S)

# Pin everything that could change git's diff output on another machine or config.
GIT_FLAGS = (
    "-c", "core.quotePath=true", "-c", "diff.relative=false",
    "diff", f"-O{os.devnull}", "--binary", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames",
    "--ignore-submodules=none", "--diff-algorithm=myers", "--no-indent-heuristic",
    "--unified=3", "--inter-hunk-context=0", "--src-prefix=a/", "--dst-prefix=b/",
)


class DiffHashError(Exception):
    """git could not produce the diff."""


def normalize(diff):
    """Drop what a rebase can change: blob ids and hunk positions.

    Split on "\\n" only: str.splitlines() would also split on form feed, \\r and
    others, which are ordinary content bytes here.
    """
    lines = diff.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    kept = ["@@" if HUNK_HEADER.match(line) else line for line in lines if not line.startswith("index ")]
    return "\n".join(kept) + "\n"


def git_diff(base, head, repo):
    for revision in (base, head):
        if revision.startswith("-"):
            raise DiffHashError(f"revision '{revision}' looks like an option")
    command = ["git", *GIT_FLAGS, f"{base}...{head}", "--"]
    env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
    try:
        done = subprocess.run(command, cwd=repo, env=env, capture_output=True, check=False, timeout=120)
    except (OSError, subprocess.SubprocessError) as error:
        raise DiffHashError(f"git could not run: {error}") from error
    if done.returncode != 0:
        raise DiffHashError(f"git diff {base}...{head} failed")
    return done.stdout.decode("latin-1")  # lossless for any bytes


def normalized_diff(base, head, repo="."):
    return normalize(git_diff(base, head, repo))


def hash_of(text):
    return hashlib.sha256(text.encode("latin-1")).hexdigest()[:HASH_LENGTH]


def compute(base, head, repo="."):
    return hash_of(normalized_diff(base, head, repo))


def visible_text(body):
    """The description as a reader sees it: no code blocks, no hidden comments, plain newlines."""
    text = body.replace("\r\n", "\n").replace("\r", "\n")
    return HTML_COMMENT.sub("", FENCED_BLOCK.sub("", text))


def approval_sections(body):
    """The lines under each 'Owner approval' heading, each up to the next heading."""
    text = visible_text(body)
    starts = list(SECTION_START.finditer(text))
    sections = []
    for start in starts:
        lines = []
        for line in text[start.end():].splitlines():
            if HEADING.match(line):
                break
            lines.append(line)
        sections.append(lines)
    return sections


def field_values(lines, name):
    pattern = re.compile(rf"^{re.escape(name)}:[ \t]*(.*?)\s*$")
    return [m.group(1) for m in map(pattern.match, lines) if m]


def one_value(lines, name, problems):
    values = field_values(lines, name)
    if len(values) > 1:
        problems.append(f"expected exactly one '{name}' line, found {len(values)}")
    elif not values:
        problems.append(f"missing '{name}' line")
    elif not values[0]:
        problems.append(f"'{name}' is empty")
    else:
        return values[0]
    return None


def check_approval(body, computed, task=None):
    """Problems found in the PR description's approval record; empty means it matches."""
    sections = approval_sections(body)
    if not sections:
        return [f"no '{SECTION_TITLE}' section in the PR description"]
    if len(sections) > 1:
        return [f"more than one '{SECTION_TITLE}' section in the PR description"]
    lines = sections[0]
    problems = []
    values = {name: one_value(lines, name, problems) for name in FIELDS}
    problems += hash_problems(values["Diff hash"], computed)
    if values["Basis"] is not None and values["Basis"] not in BASES:
        problems.append(f"Basis '{values['Basis']}' is not one of: {', '.join(BASES)}")
    if task is not None and values["Task"] is not None and values["Task"] != task:
        problems.append(f"Task '{values['Task']}' is not the expected task '{task}'")
    return problems


def hash_problems(recorded, computed):
    if recorded is None:
        return []
    match = HASH_VALUE.fullmatch(recorded)
    if match is not None and recorded.count("`") == 1:
        match = None
    if match is None:
        return [f"'Diff hash' is not {HASH_LENGTH} lowercase hex characters"]
    if match.group(1) != computed:
        return [f"recorded diff hash {match.group(1)} does not match the PR's diff hash {computed}"]
    return []


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", default=".", help="git checkout to read (default: current directory)")
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("compute", "check"):
        sub = commands.add_parser(name)
        sub.add_argument("--base", required=True)
        sub.add_argument("--head", required=True)
        if name == "check":
            sub.add_argument("--body-file", required=True, help="the PR description as a text file")
            sub.add_argument("--task", help="task id the record must name")
    return parser


def main(argv=None, print_line=print):
    args = build_parser().parse_args(argv)
    try:
        text = normalized_diff(args.base, args.head, repo=args.repo)
    except DiffHashError as error:
        print_line(f"error: {error}")
        return 1
    digest = hash_of(text)
    if args.command == "compute":
        print_line(digest)
        return 0
    if text == "\n":
        print_line(f"FAIL the diff {args.base}...{args.head} is empty: there is nothing to approve")
        return 1
    body = Path(args.body_file).read_text(encoding="utf-8")
    problems = check_approval(body, digest, task=args.task)
    for problem in problems:
        print_line(f"FAIL {problem}")
    print_line(f"diff hash {digest}: " + ("approval record matches" if not problems else "approval record NOT valid"))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
