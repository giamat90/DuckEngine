"""Wiki lint (card 0c-3): stale terms and threshold numbers in the wiki and docs.

Rules and word lists are in gates/wiki-lint.toml (a protected path); the gate
numbers are in gates/gates.toml. Pages under [history] are exempt, as are the
changelog lines named by [[history_line]] (per page) and struck-through (~~x~~) spans.
Text is normalised before matching: Unicode NFKC, `≥`/`≤`, emphasis and code marks,
zero-width characters, "85 percent" and "85 %" become "85%", and ">=85" becomes ">= 85".

Findings:
  stale-term           a term that was removed or resolved appears outside history
  threshold-mismatch   a page states a gate number that differs from gates.toml
  threshold-uncovered  a bound (`>= 80%`, `warn > 60`, `under 9%`, `warn at 15`) that no
                       [[threshold]] rule recognises and no [[known_number]] allows

Known gaps: a number written so that no bound-shape matches (a sentence such as "ninety
percent", HTML entities, LaTeX, a number wrapped onto the next line) is not seen; prose
without a number is not checked; the scan covers the paths in [scan] only. The tool runs
from the PR tree in `ci` (policy plus the owner's reading of the governance diff, as for
the other `ci` tools; trusted-checkout wiring is card 0c-5).

    wiki_lint.py --config gates/wiki-lint.toml --gates gates/gates.toml [--repo .]
"""

import argparse
import re
import sys
import tomllib
import unicodedata
from dataclasses import dataclass
from pathlib import Path

# The shapes a gate number takes in prose: a comparator and a percentage, warn/fail with a bound,
# and wording such as "under 9%" or "longer than 99 lines".
THRESHOLD_LITERAL = re.compile(
    r"(?:>=|<=|>|<)\s*\d+(?:\.\d+)?\s*%"
    r"|\b(?:warn|fail)\s*(?:[<>]=?|at)\s*\d+"
    r"|\b(?:at least|at most|under|over|below|above|more than|less than|longer than|fewer than)\s+\d+(?:\.\d+)?\s*(?:%|lines\b|parameters\b)"
)
STRUCK = re.compile(r"~~.*?~~")
COMPARATOR = re.compile(r"([<>]=?)\s*(?=\d)")
PERCENT = re.compile(r"(\d)\s*(?:%|percent\b)")
DROPPED = dict.fromkeys(map(ord, "*`​‌‍⁠﻿"))
ARROWS = {"≥": ">=", "≤": "<="}


@dataclass(frozen=True)
class Finding:
    rule: str
    page: str
    line: int
    message: str


@dataclass(frozen=True)
class Stale:
    pattern: re.Pattern
    reason: str


@dataclass(frozen=True)
class Threshold:
    key: str
    number: re.Pattern
    expected: int


@dataclass(frozen=True)
class Config:
    scan_paths: tuple
    history_pages: tuple
    history_lines: tuple
    stale: tuple
    thresholds: tuple
    known_numbers: tuple


def _regex(text):
    try:
        return re.compile(text)
    except re.error as error:
        raise ValueError(f"bad regex '{text}': {error}") from error


def _stale(entry):
    if not entry.get("pattern") or not entry.get("reason"):
        raise ValueError("a [[stale]] entry needs a pattern and a reason")
    return Stale(_regex(entry["pattern"]), entry["reason"])


def _gate_value(gates, key):
    table, _, name = key.partition(".")
    value = gates.get(table, {}).get(name)
    if not isinstance(value, int):
        raise ValueError(f"[[threshold]] key '{key}' is not an integer in the gate configuration")
    return value


def _threshold(entry, gates):
    key, number = entry.get("key", ""), _regex(entry.get("number", ""))
    if number.groups != 1:
        raise ValueError(f"[[threshold]] '{key}': the number regex needs exactly one group")
    return Threshold(key, number, _gate_value(gates, key))


def _known(entry):
    if not all(entry.get(field) for field in ("page", "text", "reason")):
        raise ValueError("a [[known_number]] entry needs a page, a text and a reason")
    return (entry["page"], entry["text"])


def _history_line(entry):
    if not entry.get("page") or not entry.get("pattern"):
        raise ValueError("a [[history_line]] entry needs a page and a pattern")
    return (entry["page"], _regex(entry["pattern"]))


def _gate_keys(gates):
    return {f"{table}.{key}" for table, values in gates.items() for key in values}


def load_config(text, gates, require_all_keys=False):
    data = tomllib.loads(text)
    config = Config(
        scan_paths=tuple(data.get("scan", {}).get("paths", ())),
        history_pages=tuple(data.get("history", {}).get("pages", ())),
        history_lines=tuple(_history_line(entry) for entry in data.get("history_line", [])),
        stale=tuple(_stale(entry) for entry in data.get("stale", [])),
        thresholds=tuple(_threshold(entry, gates) for entry in data.get("threshold", [])),
        known_numbers=tuple(_known(entry) for entry in data.get("known_number", [])),
    )
    missing = sorted(_gate_keys(gates) - {rule.key for rule in config.thresholds})
    if require_all_keys and missing:
        raise ValueError(f"no wiki rule watches these gate keys: {', '.join(missing)}")
    return config


def _glob_matches(page, glob):
    """fnmatch-style, but `*` stays inside one folder."""
    from fnmatch import fnmatchcase
    return page.count("/") == glob.count("/") and fnmatchcase(page, glob)


def normalise(line):
    """The line as the rules see it: struck spans, emphasis marks and hidden characters removed."""
    line = STRUCK.sub(" ", unicodedata.normalize("NFKC", line))
    for symbol, ascii_form in ARROWS.items():
        line = line.replace(symbol, ascii_form)
    line = COMPARATOR.sub(r"\1 ", line.translate(DROPPED))
    return PERCENT.sub(r"\1%", line)


def _is_history_line(page, line, config):
    return any(_glob_matches(page, glob) and pattern.search(line) for glob, pattern in config.history_lines)


def _stale_findings(page, number, line, config):
    return [Finding("stale-term", page, number, rule.reason) for rule in config.stale if rule.pattern.search(line)]


def _threshold_findings(page, number, line, config):
    found, covered = [], []
    for rule in config.thresholds:
        for match in rule.number.finditer(line):
            covered.append(match.span(1))
            if int(match.group(1)) != rule.expected:
                found.append(Finding("threshold-mismatch", page, number,
                                     f"{rule.key} is {rule.expected} in the gate configuration, the page says {match.group(1)}"))
    for literal in THRESHOLD_LITERAL.finditer(line):
        inside = any(literal.start() <= start and end <= literal.end() for start, end in covered)
        if not inside and (page, literal.group(0)) not in config.known_numbers:
            found.append(Finding("threshold-uncovered", page, number,
                                 f"'{literal.group(0)}' is a threshold-like number no rule covers; state it in gates/ and refer to it"))
    return found


def check(pages, config):
    """Findings for {repo-relative page path: text}, sorted by page and line."""
    found = []
    for page, text in sorted(pages.items()):
        if any(_glob_matches(page, glob) for glob in config.history_pages):
            continue
        for number, raw in enumerate(text.split("\n"), 1):
            if _is_history_line(page, raw, config):
                continue
            line = normalise(raw)
            found += _stale_findings(page, number, line, config) + _threshold_findings(page, number, line, config)
    return sorted(found, key=lambda finding: (finding.page, finding.line, finding.rule, finding.message))


def _read_pages(repo, paths):
    pages = {}
    for entry in paths:
        target = repo / entry
        if target.is_dir():
            files = sorted(target.rglob("*.md"))
        elif target.is_file():
            files = [target]
        else:
            raise OSError(f"scan path '{entry}' does not exist in {repo}")
        pages.update({path.relative_to(repo).as_posix(): path.read_text(encoding="utf-8", errors="replace") for path in files})
    if not pages:
        raise OSError("no pages found to check; is [scan] paths right?")
    return pages


def main(argv, print_line=print):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--gates", type=Path, required=True)
    parser.add_argument("--repo", type=Path, default=Path("."))
    args = parser.parse_args(argv)
    try:
        gates = tomllib.loads(args.gates.read_text(encoding="utf-8"))
        config = load_config(args.config.read_text(encoding="utf-8"), gates, require_all_keys=True)
        pages = _read_pages(args.repo, config.scan_paths)
    except (ValueError, OSError) as error:
        print_line(f"FAIL {error}")
        return 1
    found = check(pages, config)
    for finding in found:
        print_line(f"VIOLATION {finding.rule} {finding.page}:{finding.line} {finding.message}")
    print_line(f"{'FAIL' if found else 'OK'} {len(pages)} pages checked, {len(found)} violations")
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
