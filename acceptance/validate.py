"""Validator of the holdout acceptance scenarios (card 0c-6).

A scenario is one TOML file in a scenarios folder: an outcome the owner wants, written in
plain language, that the implementing agent does not author (see README.md). This tool checks
the shape only: it does not run anything. A scenario that is `active` must say how it is
checked, but nothing runs that check yet, so the summary reports active scenarios as unchecked;
a `pending` one is a promise not yet backed by a check and is counted as such. `--min N` is a
floor: fewer scenarios than N is a violation, so deleting scenarios cannot pass (raise it as
scenarios are added).

    validate.py --dir acceptance/scenarios [--min N]

Prints `VIOLATION <file>: <why>` per problem, then `OK ...` or `FAIL ...`. Exit 0 when valid,
1 otherwise. An unreadable, malformed, oversized or unknown-keyed file, a symlink, any file that is not
`<id>.toml` or README.md, an empty folder and a nested folder are violations, never skipped.
"""

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

ID = re.compile(r"[a-z0-9]+(-[a-z0-9]+)*")
REQUIRED = ("id", "title", "status", "phase", "expect")
OPTIONAL = ("check",)
STATUSES = ("pending", "active")
PHASE = re.compile(r"[0-9]+[a-z]?")
MIN_EXPECT = 20
MAX_BYTES = 64 * 1024
ALLOWED_OTHER = ("README.md",)


@dataclass
class Result:
    problems: list = field(default_factory=list)
    active: int = 0
    pending: int = 0


def _text_problems(path, data):
    problems = []
    for key in REQUIRED + OPTIONAL:
        if key in data and not isinstance(data[key], str):
            problems.append(f"{path.name}: {key} must be text")
        elif key in REQUIRED and not str(data.get(key, "")).strip():
            problems.append(f"{path.name}: {key} is missing or empty")
    problems += [f"{path.name}: unknown key {key}" for key in data if key not in REQUIRED + OPTIONAL]
    return problems


def _scenario_problems(path, data):
    problems = _text_problems(path, data)
    if problems:
        return problems
    if not PHASE.fullmatch(data["phase"]):
        problems.append(f"{path.name}: phase must be a phase number such as 1 or 0c")
    if len(data["expect"].strip()) < MIN_EXPECT:
        problems.append(f"{path.name}: expect must say what is expected (at least {MIN_EXPECT} characters)")
    if not ID.fullmatch(data["id"]) or data["id"] != path.stem:
        problems.append(f"{path.name}: id {data['id']!r} must be lowercase words with dashes and equal the file name {path.stem!r}")
    if data["status"] not in STATUSES:
        problems.append(f"{path.name}: status must be one of {', '.join(STATUSES)}")
    elif data["status"] == "active" and not data.get("check", "").strip():
        problems.append(f"{path.name}: an active scenario needs a check")
    return problems


def entry_problem(entry):
    """Why a folder entry is not an acceptable scenario file, or None. Nothing is skipped silently."""
    if entry.is_symlink():
        return f"{entry.name}: a symlink is not allowed"
    if entry.name in ALLOWED_OTHER:
        return None
    if entry.suffix != ".toml" or entry.name.startswith("."):
        return f"{entry.name}: not a scenario file (only <id>.toml and README.md are allowed; the suffix is case-sensitive)"
    return None


def _load(path):
    if path.stat().st_size > MAX_BYTES:
        return None, f"{path.name}: larger than {MAX_BYTES} bytes"
    try:
        return tomllib.loads(path.read_text(encoding="utf-8")), None
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        return None, f"{path.name}: cannot read as TOML ({error})"


def validate(directory):
    result = Result()
    if not directory.is_dir():
        result.problems.append(f"{directory}: not a directory")
        return result
    entries = sorted(directory.iterdir())
    result.problems += [f"{entry.name}: nested folders are not scanned, move the scenarios up" for entry in entries if entry.is_dir()]
    others = [entry for entry in entries if not entry.is_dir()]
    result.problems += [why for why in map(entry_problem, others) if why]
    files = [entry for entry in others if entry_problem(entry) is None and entry.name not in ALLOWED_OTHER]
    if not files:
        result.problems.append(f"{directory}: no scenario files")
    for path in files:
        data, why = _load(path)
        found = [why] if why else _scenario_problems(path, data)
        result.problems += found
        if not found:
            result.active += data["status"] == "active"
            result.pending += data["status"] == "pending"
    return result


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dir", type=Path, required=True)
    parser.add_argument("--min", type=int, default=0, dest="floor")
    args = parser.parse_args(argv)
    if args.floor < 0:
        parser.error("--min must not be negative")
    result = validate(args.dir)
    if result.active + result.pending < args.floor:
        result.problems.append(f"{args.dir}: fewer than the floor of {args.floor} scenarios")
    for problem in result.problems:
        print_line(f"VIOLATION {problem}")
    total = result.active + result.pending
    verdict = "FAIL" if result.problems else "OK"
    print_line(f"{verdict} {total} scenarios ({result.active} active (unchecked: nothing runs the check), {result.pending} pending), {len(result.problems)} violations")
    return 1 if result.problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
