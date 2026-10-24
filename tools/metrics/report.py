"""Baseline metrics report generator.

Reads the JSON written by cargo-llvm-cov, cargo-mutants and the skeleton bench
harness and writes one Markdown report per operating system. The report states
values only; thresholds live in the gate configuration and are not enforced here.
"""

import argparse
import json
import sys
from pathlib import Path

NO_BRANCH_DATA = "n/a (needs nightly)"
NO_MUTANTS = "n/a (no testable mutants)"


def _percent(value):
    return f"{value:.1f}%"


def coverage_metrics(doc):
    totals = doc["data"][0]["totals"]
    branches = totals["branches"]
    return {
        "lines": _percent(totals["lines"]["percent"]),
        "regions": _percent(totals["regions"]["percent"]),
        "branches": _percent(branches["percent"]) if branches["count"] else NO_BRANCH_DATA,
    }


def mutation_metrics(doc):
    caught, missed, timeout = doc["caught"], doc["missed"], doc["timeout"]
    testable = caught + missed + timeout
    return {
        "score": _percent(100.0 * caught / testable) if testable else NO_MUTANTS,
        "caught": caught,
        "missed": missed,
        "timeout": timeout,
        "unviable": doc["unviable"],
    }


def render(os_name, coverage, mutation, mutation_seconds, bench, control=None):
    lines = [
        f"# Baseline metrics report: {os_name}",
        "",
        "Values only; thresholds are hypotheses in the gate configuration and are not enforced here.",
        "",
        "| Metric | Value |",
        "|---|---|",
        f"| Line coverage | {coverage['lines']} |",
        f"| Region coverage | {coverage['regions']} |",
        f"| Branch coverage | {coverage['branches']} |",
        f"| Mutation score | {mutation['score']} |",
        f"| Mutants caught / missed / timeout / unviable | {mutation['caught']} / {mutation['missed']} / {mutation['timeout']} / {mutation['unviable']} |",
        f"| Mutation run time | {mutation_seconds} s |",
    ]
    if control is not None:
        lines.append(f"| G1 control mutation score (weakly tested, must be low) | {control['score']} |")
    lines += [
        "",
        "| Benchmark | Median ns per call | p95 ns per call |",
        "|---|---|---|",
    ]
    for entry in bench["benchmarks"]:
        lines.append(f"| {entry['name']} | {entry['median_ns']:.2f} | {entry['p95_ns']:.2f} |")
    return "\n".join(lines) + "\n"


def _load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--os", required=True)
    parser.add_argument("--coverage", required=True)
    parser.add_argument("--mutants", required=True)
    parser.add_argument("--control-mutants")
    parser.add_argument("--bench", required=True)
    parser.add_argument("--mutation-seconds", required=True, type=int)
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)
    text = render(
        args.os,
        coverage_metrics(_load(args.coverage)),
        mutation_metrics(_load(args.mutants)),
        args.mutation_seconds,
        _load(args.bench),
        control=mutation_metrics(_load(args.control_mutants)) if args.control_mutants else None,
    )
    Path(args.out).write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
