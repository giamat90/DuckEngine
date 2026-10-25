"""Assign a change class (1 routine, 2 sensitive, 3 governance) to a diff (card 0c-1).

Rules: wiki/workflow/architect-review-surface.md section 1. The highest class of any
changed file wins; a path or a line the rules do not recognise rounds up to class 2.
Class 3 comes from gates/protected-paths.txt (the same list and the same matcher, tools/gate-check/protected_paths.py, as the governance guard),
the `contracts/` directories, toolchain and lint configuration files, and any
path that can alias another file on Windows. Added and removed lines of .rs files are
scanned for `unsafe`, atomics, locks, intrinsics and public items.

Known gaps (reviews/0c-1.md): trait methods and enum variants are not parsed, so a
change to them is class 2 only in a module root (lib.rs, mod.rs); a hot-path change
whose benchmark delta exceeds the noise band needs benchmark data (the report step,
0c-4); cargo-public-api is not wired in. A consumer must treat a non-zero exit as
class 3.

    classify_change.py --repo . --base origin/main --head origin/<branch> --paths gates/protected-paths.txt

Run it from a trusted checkout of main: the tool is not taken from the PR tree.
"""

import argparse
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath

sys.path.append(str(Path(__file__).resolve().parents[1] / "gate-check"))
from protected_paths import is_alias, is_protected, load_protected  # noqa: E402,F401  (the matcher shared with the guard)

GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
ROUTINE, SENSITIVE, GOVERNANCE = 1, 2, 3

SENSITIVE_FILES = {"cargo.toml", "cargo.lock", "build.rs"}
GOVERNANCE_FILES = {"clippy.toml", ".clippy.toml", "deny.toml", "rust-toolchain.toml"}
# lib.rs and mod.rs declare the public surface; trait methods and enum variants are not parsed.
MODULE_ROOTS = {"lib.rs", "mod.rs"}
SENSITIVE_ROOTS = {"third_party", "vendor", "baselines"}
SENSITIVE_DIRS = {"jobs", "adapters"}
SENSITIVE_BASE_AREAS = ("alloc", "simd", "math")
ROUTINE_ROOTS = {"wiki", "docs", "reviews"}
ROUTINE_SUFFIXES = {".md", ".txt", ".rs"}
CRATE_ROUTINE_DIRS = {"src", "tests", "benches"}
TEST_DIRS = {"tests", "benches"}

CONTENT_RULES = (
    (re.compile(r'\bunsafe\b|\bUnsafeCell\b|\bstatic\s+mut\b|\bextern\s+"'), "adds `unsafe` or an unsafe-adjacent construct"),
    (re.compile(r"\bAtomic[A-Z]\w*|\batomic::"), "adds an atomic"),
    (re.compile(r"\b(?:Mutex|RwLock|Condvar|Barrier|mpsc)\b|\bthread::(?:spawn|scope)\b"), "adds a lock or thread"),
    (re.compile(r"\barch::|\b_mm\w*|\bintrinsics\b|\bno_mangle\b|\btarget_feature\b"), "adds an intrinsic or ABI attribute"),
    (re.compile(r"\bmacro_export\b|\bmacro_rules!"), "adds an exported or generating macro"),
)
SUBMODULE = re.compile(r"^Subproject commit ")
# `pub` anywhere on the line (after an attribute, inside an impl), but not pub(crate) and friends.
PUBLIC_ITEM = re.compile(r"(?<![\w:])pub\b(?!\s*\()")
PUBLIC_API_REMOVAL = re.compile(r"(?<![\w:])pub\b(?!\s*\()|\bmacro_export\b|\bmacro_rules!")


class ClassifyError(Exception):
    pass


@dataclass(frozen=True)
class Change:
    status: str
    added: list
    removed: list = field(default_factory=list)
    opaque: bool = False  # the diff has no readable lines (mode-only change, empty file)


@dataclass(frozen=True)
class Result:
    level: int
    reasons: list


def _in_base_area(parts):
    if "base" not in parts[:-1]:
        return False
    return any(part.startswith(area) for part in parts[parts.index("base") + 1:] for area in SENSITIVE_BASE_AREAS)


def _sensitive_reason(parts):
    if parts[-1] in SENSITIVE_FILES:
        return "dependency or build file"
    if parts[-1] in MODULE_ROOTS:
        return "module root (public surface)"
    if parts[0] in SENSITIVE_ROOTS:
        return f"{parts[0]}/ (vendored code or benchmark baseline)"
    if parts[-1].startswith("baseline") and parts[-1].endswith(".json"):
        return "benchmark baseline"
    if SENSITIVE_DIRS.intersection(parts[:-1]):
        return "concurrency or adapter area"
    if _in_base_area(parts):
        return "allocator, SIMD or determinism math area"
    return None


def _is_routine(parts):
    if PurePosixPath(parts[-1]).suffix not in ROUTINE_SUFFIXES:
        return False
    if parts[0] in ROUTINE_ROOTS or len(parts) == 1:
        return True
    return parts[0] == "engine" and len(parts) >= 4 and parts[2] in CRATE_ROUTINE_DIRS


def _is_crate_test_file(parts):
    return parts[0] == "engine" and len(parts) >= 4 and parts[2] in TEST_DIRS


def _folded_parts(path):
    return tuple(part.casefold() for part in PurePosixPath(path).parts)


def classify_path(path, protected):
    """(class, reason) from the path alone."""
    parts = _folded_parts(path)
    if is_alias(path):
        return GOVERNANCE, "path that can alias another file on Windows"
    if is_protected(path, protected):
        return GOVERNANCE, "protected path"
    if "contracts" in parts[:-1]:
        return GOVERNANCE, "contract suite"
    if parts[-1] in GOVERNANCE_FILES:
        return GOVERNANCE, "toolchain or lint configuration"
    reason = _sensitive_reason(parts)
    if reason:
        return SENSITIVE, reason
    if _is_routine(parts):
        return ROUTINE, "routine area"
    return SENSITIVE, "unrecognised path, rounded up"


def classify_content(path, added, removed=(), opaque=False):
    """(class, reason) from the diff lines of a file; only .rs files are scanned for code."""
    if opaque:
        return SENSITIVE, "diff has no readable lines"
    if any(SUBMODULE.match(line) for line in added):
        return SENSITIVE, "submodule change"
    if not path.endswith(".rs"):
        return ROUTINE, ""
    in_tests = _is_crate_test_file(_folded_parts(path))
    if not in_tests and any(PUBLIC_API_REMOVAL.search(line) for line in removed):
        return SENSITIVE, "removes or changes a public item"
    for line in added:
        for pattern, reason in CONTENT_RULES:
            if pattern.search(line):
                return SENSITIVE, reason
        if not in_tests and PUBLIC_ITEM.search(line):
            return SENSITIVE, "adds a public item"
    return ROUTINE, ""


def classify(changes, protected):
    per_file = []
    for path, change in sorted(changes.items()):
        by_path = classify_path(path, protected)
        by_content = classify_content(path, change.added, change.removed, change.opaque)
        per_file.append((path, max(by_path, by_content, key=lambda found: found[0])))
    level = max((found[0] for _, found in per_file), default=ROUTINE)
    reasons = [f"{path}: {found[1]}" for path, found in per_file if found[0] == level and level > ROUTINE]
    return Result(level, reasons)


def _git(repo, *args):
    try:
        done = subprocess.run(["git", *args], cwd=repo, env=GIT_ENV, capture_output=True, check=False)
    except OSError as error:
        raise ClassifyError(f"cannot run git in {repo}: {error}") from error
    if done.returncode != 0:
        raise ClassifyError(f"git {' '.join(args[:3])} failed: {done.stderr.decode(errors='replace').strip()}")
    return done.stdout.decode(errors="replace")


def _diff_lines(diff):
    """(added, removed, has_hunk) of a -U0 diff; everything before the first hunk is header, not content."""
    started, added, removed = False, [], []
    for line in diff.split("\n"):
        if line.startswith("@@"):
            started = True
        elif started and line.startswith("+"):
            added.append(line[1:].rstrip("\r"))
        elif started and line.startswith("-"):
            removed.append(line[1:].rstrip("\r"))
    return added, removed, started


def changes(repo, base, head):
    """Changed files as {path: Change}; a rename is a delete plus an add, so both names count."""
    # Resolve first: a revision that looks like an option must not reach git as one.
    shas = [_git(repo, "rev-parse", "--verify", "--end-of-options", f"{rev}^{{commit}}").strip() for rev in (base, head)]
    span = "...".join(shas)
    listing = _git(repo, "diff", "--name-status", "--no-renames", "--no-ext-diff", "--ignore-submodules=none", "-z", span).split("\0")
    found = {}
    for status, path in zip(listing[0::2], listing[1::2]):
        # --text: a NUL byte or a `-diff` attribute must not hide the lines from the scan.
        diff = _git(repo, "diff", "-U0", "--text", "--no-renames", "--no-ext-diff", "--ignore-submodules=none", "--no-color", span, "--", f":(literal){path}")
        added, removed, readable = _diff_lines(diff)
        found[path] = Change(status, added, removed, opaque=not readable and status != "D")
    return found


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path("."))
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--paths", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        protected = load_protected(args.paths.read_text(encoding="utf-8"))
        found = changes(args.repo, args.base, args.head)
        if not found:
            raise ClassifyError(f"no changed files between {args.base} and {args.head}; are the revisions right?")
        result = classify(found, protected)
    except (ClassifyError, ValueError, OSError) as error:
        print_line(f"FAIL {error}")
        return 1
    print_line(f"class={result.level}")
    for reason in result.reasons:
        print_line(f"  {reason}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
