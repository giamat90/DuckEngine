"""Decide which version a merge to main releases (card 0b-6).

The version is derived from the Conventional Commit messages since the highest
`vX.Y.Z` tag (see next_version.py). A commit that already carries a release tag
is skipped, so a re-run never releases twice.

    python release_plan.py --repo . >> "$GITHUB_OUTPUT"
"""

import argparse
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

import next_version

INITIAL_VERSION = "0.1.0"
TAG = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}


class PlanError(Exception):
    pass


@dataclass(frozen=True)
class Plan:
    version: str
    previous: str | None
    skip: bool

    @property
    def tag(self):
        return f"v{self.version}"


def _key(tag):
    return tuple(int(part) for part in tag[1:].split("."))


def latest_tag(tags):
    """The highest plain `vX.Y.Z` tag, or None."""
    releases = [tag for tag in tags if TAG.fullmatch(tag)]
    return max(releases, key=_key, default=None)


def plan(tags, head_tags, messages):
    """Pure decision: `messages` are the commits since latest_tag(tags)."""
    on_head = latest_tag(head_tags)
    if on_head:
        return Plan(on_head[1:], latest_tag(tags), skip=True)
    previous = latest_tag(tags)
    if previous is None:
        return Plan(INITIAL_VERSION, None, skip=False)
    return Plan(next_version.next_version(previous[1:], messages), previous, skip=False)


def _git(repo, *args):
    try:
        done = subprocess.run(["git", *args], cwd=repo, env=GIT_ENV, capture_output=True, check=False)
    except OSError as error:
        raise PlanError(f"cannot run git in {repo}: {error}") from error
    if done.returncode != 0:
        raise PlanError(f"git {' '.join(args)} failed: {done.stderr.decode(errors='replace').strip()}")
    return done.stdout.decode(errors="replace")


def git_plan(repo):
    # Only tags reachable from HEAD count: a stray tag elsewhere cannot stop releases.
    tags = _git(repo, "tag", "--list", "--merged", "HEAD").split()
    head_tags = _git(repo, "tag", "--points-at", "HEAD").split()
    previous = latest_tag(tags)
    messages = []
    if previous and not latest_tag(head_tags):
        log = _git(repo, "log", "-z", "--format=%B", f"{previous}..HEAD")
        messages = [m for m in log.split("\0") if m.strip()]
    result = plan(tags, head_tags, messages)
    if not result.skip and result.tag in _git(repo, "tag", "--list").split():
        raise PlanError(f"tag {result.tag} already exists but is not an ancestor of HEAD; delete or move it")
    return result


def format_output(result):
    return [
        f"version={result.version}",
        f"tag={result.tag}",
        f"previous={result.previous or ''}",
        f"skip={'true' if result.skip else 'false'}",
    ]


def main(argv, print_line=print, print_error=lambda message: print(message, file=sys.stderr)):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path("."))
    args = parser.parse_args(argv)
    try:
        result = git_plan(args.repo)
    except (PlanError, ValueError) as error:
        print_error(f"error: {error}")
        return 1
    for line in format_output(result):
        print_line(line)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
