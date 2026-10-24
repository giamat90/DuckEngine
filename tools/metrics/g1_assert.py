"""Gate G1 assertion: mutation testing must discriminate and finish in time.

Criteria from ADR-0001 evaluation, section 6: score >= 70% on the well-tested
set, score < 50% on the weakly tested control, run within 15 minutes.
"""

import argparse
import json
import sys
from pathlib import Path

WELL_TESTED_MIN_PERCENT = 70
CONTROL_MAX_PERCENT = 50
MAX_SECONDS = 15 * 60


def _score(doc):
    testable = doc["caught"] + doc["missed"] + doc["timeout"]
    return None if testable == 0 else 100.0 * doc["caught"] / testable


def evaluate(well_tested, control, seconds):
    errors = []
    well_score, control_score = _score(well_tested), _score(control)
    if well_score is None or well_score < WELL_TESTED_MIN_PERCENT:
        errors.append(
            f"well-tested score {well_score} is below {WELL_TESTED_MIN_PERCENT}% (or has no testable mutants)"
        )
    if control_score is None or control_score >= CONTROL_MAX_PERCENT:
        errors.append(
            f"control score {control_score} is not below {CONTROL_MAX_PERCENT}% (the score does not discriminate)"
        )
    if seconds > MAX_SECONDS:
        errors.append(f"mutation run took {seconds} s, more than 15 min")
    return errors


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--well-tested", required=True)
    parser.add_argument("--control", required=True)
    parser.add_argument("--seconds", required=True, type=int)
    args = parser.parse_args(argv)
    load = lambda path: json.loads(Path(path).read_text(encoding="utf-8"))  # noqa: E731
    errors = evaluate(load(args.well_tested), load(args.control), args.seconds)
    for error in errors:
        print(f"FAIL G1: {error}", file=sys.stderr)
    if not errors:
        print("G1 holds: well-tested >= 70%, control < 50%, run <= 15 min")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
