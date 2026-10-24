"""Digest-vs-gate-configuration check.

wiki/rules-digest.md quotes the gate numbers in prose; gates/gates.toml holds
them for the tools. This check fails when the two disagree, when the digest no
longer states a number, or when the configuration has a key the digest does not
cover. It also checks that CLAUDE.md still imports the digest.
"""

import argparse
import re
import sys
import tomllib
from pathlib import Path

DIGEST_IMPORT = "@wiki/rules-digest.md"

# Each pattern captures integers in order; the paired keys say where each
# captured number must appear in the gate configuration.
PATTERNS = [
    (r"function length warn > (\d+)", ["clean_code.function_length_warn"]),
    (
        r"cyclomatic warn > (\d+) / fail > (\d+)",
        ["clean_code.cyclomatic_warn", "clean_code.cyclomatic_fail"],
    ),
    (r"cognitive fail > (\d+)", ["clean_code.cognitive_fail"]),
    (r"nesting <= (\d+)", ["clean_code.nesting_max"]),
    (
        r"parameters warn > (\d+) / fail > (\d+)",
        ["clean_code.parameters_warn", "clean_code.parameters_fail"],
    ),
    (r"duplication <= (\d+)%", ["clean_code.duplication_max_percent"]),
    (
        r"Line coverage L0-L2 >= (\d+)% \(new lines >= (\d+)%\), branch >= (\d+)%",
        ["coverage.line_min", "coverage.new_line_min", "coverage.branch_min"],
    ),
    (
        r"mutation score on changed L0-L2 code >= (\d+)%",
        ["mutation.score_min"],
    ),
    (
        r"instruction counts \(warn (\d+)%, fail (\d+)%\)",
        ["benchmark.warn_percent", "benchmark.fail_percent"],
    ),
]


def _lookup(config, dotted):
    table, key = dotted.split(".")
    return config.get(table, {}).get(key)


def _config_keys(config):
    return {f"{table}.{key}" for table, values in config.items() for key in values}


def compare(digest_text, config):
    errors = []
    covered = set()
    for pattern, keys in PATTERNS:
        match = re.search(pattern, digest_text)
        if match is None:
            errors.append(f"digest does not state: {pattern}")
            continue
        for key, number in zip(keys, match.groups()):
            covered.add(key)
            configured = _lookup(config, key)
            if configured is None:
                errors.append(f"{key}: missing from the gate configuration (digest says {number})")
            elif configured != int(number):
                errors.append(f"{key}: digest says {number}, gate configuration says {configured}")
    for key in sorted(_config_keys(config) - covered):
        errors.append(f"{key}: in the gate configuration but not covered by the digest check")
    return errors


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--digest", required=True)
    parser.add_argument("--config", required=True)
    parser.add_argument("--claude-md", required=True)
    args = parser.parse_args(argv)
    digest = Path(args.digest).read_text(encoding="utf-8")
    config = tomllib.loads(Path(args.config).read_text(encoding="utf-8"))
    errors = compare(digest, config)
    if DIGEST_IMPORT not in Path(args.claude_md).read_text(encoding="utf-8"):
        errors.append(f"CLAUDE.md does not import {DIGEST_IMPORT}")
    for error in errors:
        print(f"FAIL {error}", file=sys.stderr)
    if not errors:
        print("digest and gate configuration agree")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
