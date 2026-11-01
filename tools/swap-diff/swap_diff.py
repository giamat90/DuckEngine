"""Swap-diff check (card 2-16): did adding a backend or platform touch engine or game code?

Rule (wiki/architecture/ports-and-adapters.md, quality-metrics "Swap diff", decision N-4): when backend or
platform N+1 is added, the files changed in engine and game code are at most `max_engine_game_files`
(0). Adapters, the composition root, port files (reported as a port revision, at most one before a port is
`stable`), workspace plumbing and non-code files are not engine or game code. The tool counts the files
changed between two revisions and puts each into one class, first match wins:

    adapter, composition_root, ports, plumbing, non_code, engine_game (everything else)

    swap_diff.py --config gates/swap-diff.toml --base <rev> --head <rev> [--repo .]

Exit 1 when more than `max_engine_game_files` files are engine or game code, and also when the range holds
no adapter and no composition-root file: a range that adds no backend proves nothing, so an empty, swapped
or wrong range cannot read as a clean swap. The range is the two trees (`git diff base head`, two-dot):
choose a base that is the state before the card. Port files never fail the check: how many port revisions a
port has had is history the tool cannot see, so it prints whether the range contains one and the owner
counts. Run it from a trusted checkout of main, like the other gate tools.
"""

import argparse
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

CLASSES = ("adapter", "composition_root", "ports", "plumbing", "non_code")
ENGINE_GAME = "engine_game"
LIMIT_KEY = "max_engine_game_files"
NUL = chr(0)


class SwapDiffError(Exception):
    pass


@dataclass(frozen=True)
class Rules:
    limit: int
    patterns: dict


@dataclass(frozen=True)
class Report:
    counts: dict
    ok: bool
    port_revision: bool
    vacuous: bool


def _glob(pattern):
    """`*` stays inside one path segment, `**` crosses segments."""
    out, pos = [], 0
    while pos < len(pattern):
        if pattern.startswith("**", pos):
            out.append(".*")
            pos += 2
        elif pattern[pos] == "*":
            out.append("[^/]*")
            pos += 1
        else:
            out.append(re.escape(pattern[pos]))
            pos += 1
    return re.compile("".join(out) + r"\Z")


def load_rules(text):
    data = tomllib.loads(text)
    limit = data.pop(LIMIT_KEY, None)
    if isinstance(limit, bool) or not isinstance(limit, int) or limit < 0:
        raise ValueError(f"{LIMIT_KEY} must be a non-negative integer")
    unknown = sorted(set(data) - set(CLASSES))
    if unknown:
        raise ValueError(f"unknown key(s): {', '.join(unknown)}")
    patterns = {}
    for name in CLASSES:
        globs = data.get(name, [])
        if not isinstance(globs, list) or not all(isinstance(item, str) for item in globs):
            raise ValueError(f"{name} must be a list of strings")
        patterns[name] = [_glob(item) for item in globs]
    return Rules(limit, patterns)


def classify(path, rules):
    for name in CLASSES:
        if any(pattern.match(path) for pattern in rules.patterns[name]):
            return name
    return ENGINE_GAME


def check(files, rules):
    counts = {name: [] for name in (*CLASSES, ENGINE_GAME)}
    for path in files:
        counts[classify(path, rules)].append(path)
    vacuous = not counts["adapter"] and not counts["composition_root"]
    return Report(counts, len(counts[ENGINE_GAME]) <= rules.limit and not vacuous, bool(counts["ports"]), vacuous)


def _git(repo, *args):
    try:
        done = subprocess.run(["git", "-c", "core.quotePath=false", *args], cwd=repo, check=True, capture_output=True)
    except (subprocess.CalledProcessError, OSError) as error:
        detail = getattr(error, "stderr", b"").decode("utf-8", "replace").strip() or str(error)
        raise SwapDiffError(f"git {args[0]} failed: {detail}") from error
    return done.stdout.decode("utf-8")


def resolve(repo, revision):
    """The commit a revision names; a value that git could read as an option is refused."""
    if revision.startswith("-"):
        raise SwapDiffError(f"revision '{revision}' looks like an option")
    return _git(repo, "rev-parse", "--verify", "--end-of-options", f"{revision}^{{commit}}").strip()


def changed_files(repo, base, head):
    """The paths that differ between two revisions; a rename counts both its paths."""
    base_sha, head_sha = resolve(repo, base), resolve(repo, head)
    out = _git(repo, "diff", "--name-only", "--no-renames", "-z", base_sha, head_sha, "--")
    return sorted(item for item in out.split(NUL) if item)


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--repo", type=Path, default=Path("."))
    args = parser.parse_args(argv)
    try:
        rules = load_rules(args.config.read_text(encoding="utf-8"))
        base_sha, head_sha = resolve(args.repo, args.base), resolve(args.repo, args.head)
        print_line(f"range: {base_sha}...{head_sha}")
        report = check(changed_files(args.repo, base_sha, head_sha), rules)
    except (SwapDiffError, ValueError, OSError) as error:
        print_line(f"FAIL {error}")
        return 1
    for name, paths in report.counts.items():
        print_line(f"{name}: {len(paths)}")
    shown = report.counts[ENGINE_GAME]
    verdict = "OK" if report.ok else "FAIL"
    detail = f" ({', '.join(shown)})" if shown else ""
    port = "contains a port revision (count it against the port's one allowed before stable)" if report.port_revision else "no port revision"
    note = "; the range has no adapter or composition-root file, so it proves nothing" if report.vacuous else ""
    print_line(f"{verdict} swap-diff: {len(shown)} engine/game files, limit {rules.limit}{detail}; {port}{note}")
    return 0 if report.ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
