"""Check that a PR title is a Conventional Commit (card 0b-6).

PRs are squash-merged and the title becomes the commit message that
next_version.py reads, so the title decides the release version.
"""

import argparse
import re
import sys

TYPES = ("feat", "fix", "perf", "refactor", "docs", "test", "build", "ci", "chore", "style", "revert")
TITLE = re.compile(rf"(?:{'|'.join(TYPES)})(?:\([A-Za-z0-9._/-]+\))?!?: \S.*")
FORM = "type(scope)!: summary"


def problems(title):
    if "\n" in title or "\r" in title or title != title.strip() or not TITLE.fullmatch(title):
        return [f"title must be a single-line Conventional Commit: {FORM}, type one of {', '.join(TYPES)}"]
    return []


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--title", required=True)
    found = problems(parser.parse_args(argv).title)
    for problem in found:
        print_line(f"FAIL {problem}")
    if not found:
        print_line("OK title is a Conventional Commit")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
