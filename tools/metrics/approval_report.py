"""Metric report for the owner's approval (card 0c-4).

Runs the gate tools that exist (change classifier, architecture checker, wiki lint) on a
branch and renders one Markdown report: verdict, change class with the files that set
it, the number of waivers the diff adds (card 0c-12; listed, never counted as a failure:
the owner judges each one), one row per check, the violations. The report states results only; thresholds live in
gates/ and are enforced by the tools, not here. Sections of the full report that no tool
produces yet are listed under "Not in this report" rather than left out silently; checks that
exist but run in other CI jobs are listed under "Checks that run in other jobs". The verdict
counts only the rows of this report. Two rows guard the report itself: the diff hash must be
the hash of the diff, and the checked-out tree must be the head.

    approval_report.py --repo . --base origin/main --head origin/<branch> --diff-hash <hash> --out report.md

The tools come from --tools-dir (default: next to this file) and their configuration from
--gates-dir (default: <repo>/gates). Point both at a trusted checkout of main to judge a PR
by main's rules; with the defaults on a PR tree the PR also controls the rules it is judged
by, which is why the `ci` `gates` job (card 0c-5) passes both from a checkout of main.
A tool that cannot run, times out, exits non-zero, prints a violation or prints no summary line counts as a failed check; a classifier that
fails counts as class 3 and as a failed check. Exit codes: 0 all pass, 1 a check failed,
2 the report could not be written.
"""

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

CLASS_NAMES = {1: "routine", 2: "sensitive", 3: "governance"}
CLASS_LINE = re.compile(r"^class=([123])$")
WAIVERS_LINE = re.compile(r"^waivers=(\d+)$")
REASON_LINE = re.compile(r"^\s+(\S.*?): (.+)$")
SUMMARY_LINE = re.compile(r"^(OK|FAIL) (.*)$")
VIOLATION_LINE = re.compile(r"^VIOLATION (.*)$")
HASH = re.compile(r"[0-9a-f]{16,64}")
TIMEOUT_SECONDS = 600
MARKDOWN_SPECIALS = re.compile(r"([\\`*_{}\[\]<>#|&~])")
NOT_IN_REPORT = (
    "Coverage and mutation score per module on this diff (the `metrics` job reports the skeleton baseline).",
    "Performance: benchmark deltas against the baseline, allocations per frame.",
    "Robustness: sanitizer, Miri and static-analysis findings.",
    "Governance: Class 2 and 3 changes since the last report.",
    "Top risks of the code: complexity, churn, lowest mutation scores.",
    "Product view and agent audit findings.",
    "Complexity, duplication and magic-number limits (no tool enforces them yet).",
)
OTHER_JOBS = (
    "Format (rustfmt), Clippy, and Build and tests: the `ci` build jobs.",
    "Digest check against the gate configuration: the `ci` `gates` job.",
    "Governance guard and PR title: their own workflows; `trusted-pr-gates` repeats the guard from the base.",
    "Holdout acceptance suite: `trusted-pr-gates` (shape only; nothing runs a scenario yet).",
    "Licences, bans and advisories: the `ci` `deny` job.",
    "Third-party manifest (`THIRD_PARTY.md`): the file itself is not checked by any tool yet.",
)


@dataclass(frozen=True)
class Classification:
    klass: int
    reasons: list
    failed: bool = False
    waivers: list = field(default_factory=list)


@dataclass(frozen=True)
class Summary:
    passed: bool
    detail: str
    violations: list


def parse_classification(text):
    lines = [line for line in text.splitlines() if line.strip()]
    match = CLASS_LINE.match(lines[0]) if lines else None
    if not match:
        raise ValueError(f"not classifier output: {text[:60]!r}")
    body = lines[1:]
    marks = [index for index, line in enumerate(body) if WAIVERS_LINE.match(line)]
    if len(marks) != 1:
        raise ValueError("classifier output has no single 'waivers=<count>' line")
    reasons, listed = _pairs(body[:marks[0]]), _pairs(body[marks[0] + 1:])
    if int(WAIVERS_LINE.match(body[marks[0]]).group(1)) != len(listed):
        raise ValueError("classifier output: the waiver count does not match the waivers listed")
    return Classification(int(match.group(1)), reasons, waivers=listed)


def _pairs(lines):
    found = [REASON_LINE.match(line) for line in lines]
    if not all(found):
        raise ValueError("classifier output has a line that is not '<path>: <reason>'")
    return [(item.group(1), item.group(2)) for item in found]


def parse_summary(text):
    lines = [line.rstrip() for line in text.splitlines() if line.strip()]
    violations = [found.group(1) for found in map(VIOLATION_LINE.match, lines) if found]
    last = SUMMARY_LINE.match(lines[-1]) if lines else None
    if not last:
        return Summary(False, "no summary line in the tool output", violations)
    earlier_fail = any(SUMMARY_LINE.match(line) and line.startswith("FAIL") for line in lines[:-1])
    return Summary(last.group(1) == "OK" and not violations and not earlier_fail, last.group(2), violations)


def _run(command, timeout):
    """(completed process, None) or (None, why it could not complete)."""
    try:
        done = subprocess.run(command, capture_output=True, text=True, encoding="utf-8", errors="replace",
                              check=False, timeout=timeout)
    except subprocess.TimeoutExpired:
        return None, f"{Path(command[1] if len(command) > 1 else command[0]).name} timed out after {timeout} s"
    except OSError as error:
        return None, f"could not run {command[0]}: {error}"
    return done, None


def _last_line(text):
    return (text.strip().splitlines() or [""])[-1]


def run_check(command, timeout=TIMEOUT_SECONDS):
    done, problem = _run(command, timeout)
    if problem:
        return Summary(False, problem, [])
    summary = parse_summary(done.stdout)
    if done.returncode == 0:
        return summary
    detail = f"exit {done.returncode}: {_last_line(done.stderr) or summary.detail}"
    return Summary(False, detail, summary.violations)


def run_classifier(command, timeout=TIMEOUT_SECONDS):
    done, problem = _run(command, timeout)
    if problem:
        why = f"classifier failed: {problem}"
    elif done.returncode != 0:
        why = f"classifier failed (exit {done.returncode}) {_last_line(done.stderr) or _last_line(done.stdout)}".rstrip()
    else:
        try:
            return parse_classification(done.stdout)
        except ValueError as error:
            why = f"classifier failed: {error}"
    return Classification(3, [("(classifier)", why)], failed=True)


def check_hash(text):
    if not HASH.fullmatch(text):
        raise ValueError(f"not a diff hash: {text!r}")
    return text


def _md(text):
    """Text safe to put in a table cell or a list item: no markup, no links, one line."""
    return MARKDOWN_SPECIALS.sub(r"\\\1", " ".join(str(text).split()))


def _code(text):
    """A code span that the text cannot end."""
    return "`" + " ".join(str(text).split()).replace("`", "'").replace("|", "\\|") + "`"


def _failing(classification, checks):
    return sum(not summary.passed for summary in checks.values()) + int(classification.failed)


def _verdict(classification, checks):
    failing = _failing(classification, checks)
    if not failing:
        return f"all {len(checks) + 1} checks in this report pass"
    return f"{failing} check{'s' if failing > 1 else ''} failing"


def _waiver_row(classification):
    if classification.failed:
        return "| Waivers added | not known | the classifier failed |"
    if not classification.waivers:
        return "| Waivers added | 0 | none |"
    return f"| Waivers added | {len(classification.waivers)} | the owner reviews each one |"


def render(head, base, diff_hash, classification, checks):
    klass = classification.klass
    status = "FAIL (classifier failed, treated as class 3)" if classification.failed else f"{klass} ({CLASS_NAMES[klass]})"
    lines = [
        f"# Metric report: {_md(head)} against {_md(base)}",
        "",
        f"Diff hash: {_code(diff_hash)}",
        "",
        f"Verdict: {_verdict(classification, checks)}",
        "",
        "| Check | Result | Detail |",
        "|---|---|---|",
        f"| Change class | {status} | {len(classification.reasons)} file(s) set it |",
    ]
    lines.append(_waiver_row(classification))
    lines += [f"| {_md(name)} | {'pass' if summary.passed else 'FAIL'} | {_md(summary.detail)} |" for name, summary in checks.items()]
    if classification.waivers:
        lines += ["", "## Waivers added", ""] + [f"- {_code(path)}: {_code(text)}" for path, text in classification.waivers]
    if classification.reasons:
        lines += ["", "## Why the class", ""]
        lines += [f"- {_code(path)}: {_md(why)}" for path, why in classification.reasons]
    for name, summary in checks.items():
        if summary.violations:
            lines += ["", f"## {_md(name)}: violations", ""] + [f"- {_md(violation)}" for violation in summary.violations]
    lines += ["", "## Checks that run in other jobs", ""] + [f"- {item}" for item in OTHER_JOBS]
    lines += ["", "## Not in this report", ""] + [f"- {item}" for item in NOT_IN_REPORT]
    return "\n".join(lines) + "\n"


def _git_commit(repo, revision):
    done, problem = _run(["git", "-C", repo, "rev-parse", "--verify", "--quiet", revision + "^{commit}"], TIMEOUT_SECONDS)
    return None if problem or done.returncode != 0 else done.stdout.strip()


def check_tree(repo, head):
    """The tools judge the tree in --repo; it must be the head the report names."""
    tree, wanted = _git_commit(repo, "HEAD"), _git_commit(repo, head)
    if tree is None or wanted is None:
        return Summary(False, "could not resolve HEAD or the head revision", [])
    if tree != wanted:
        return Summary(False, f"the tree under check is {tree[:12]}, not the head {wanted[:12]}", [])
    return Summary(True, f"HEAD is {tree[:12]}", [])


def check_diff_hash(command, claimed):
    done, problem = _run(command, TIMEOUT_SECONDS)
    if problem:
        return Summary(False, problem, [])
    actual = done.stdout.strip()
    if done.returncode != 0 or actual != claimed:
        return Summary(False, f"--diff-hash {claimed} is not the hash of the diff ({actual or 'not computed'})", [])
    return Summary(True, f"{claimed} is the hash of the diff", [])


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path("."))
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--diff-hash", required=True, type=check_hash)
    parser.add_argument("--tools-dir", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--gates-dir", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)
    py, tools, repo = sys.executable, args.tools_dir, str(args.repo)
    gates = args.gates_dir or args.repo / "gates"
    classification = run_classifier([py, str(tools / "classify-change/classify_change.py"), "--repo", repo, "--base", args.base,
                                     "--head", args.head, "--paths", str(gates / "protected-paths.txt")])
    checks = {
        "Architecture check": run_check([py, str(tools / "arch-check/arch_check.py"), "--config", str(gates / "architecture.toml"), "--repo", repo]),
        "Wiki lint": run_check([py, str(tools / "wiki-lint/wiki_lint.py"), "--config", str(gates / "wiki-lint.toml"),
                                "--gates", str(gates / "gates.toml"), "--repo", repo]),
        "Diff hash matches the diff": check_diff_hash([py, str(tools / "diff-hash/diff_hash.py"), "--repo", repo, "compute",
                                                       "--base", args.base, "--head", args.head], args.diff_hash),
        "Checked-out tree is the head": check_tree(repo, args.head),
    }
    try:
        args.out.write_text(render(args.head, args.base, args.diff_hash, classification, checks), encoding="utf-8")
    except OSError as error:
        print(f"FAIL cannot write the report: {error}")
        return 2
    return 0 if not _failing(classification, checks) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
