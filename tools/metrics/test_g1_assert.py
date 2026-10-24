"""Tests for the G1 assertion (ADR-0001 evaluation, section 6)."""

import json
import tempfile
import unittest
from pathlib import Path

import g1_assert


def doc(caught=0, missed=0, timeout=0):
    return {"caught": caught, "missed": missed, "timeout": timeout, "unviable": 0}


class Evaluate(unittest.TestCase):
    def test_passes_when_well_tested_is_high_control_low_and_fast(self):
        self.assertEqual(g1_assert.evaluate(doc(caught=15), doc(caught=2, missed=24), 7), [])

    def test_well_tested_score_below_seventy_fails(self):
        errors = g1_assert.evaluate(doc(caught=69, missed=31), doc(missed=10), 7)
        self.assertEqual(len(errors), 1)
        self.assertIn("well-tested", errors[0])

    def test_well_tested_score_of_exactly_seventy_passes(self):
        self.assertEqual(g1_assert.evaluate(doc(caught=7, missed=3), doc(missed=10), 7), [])

    def test_control_score_of_fifty_or_more_fails(self):
        errors = g1_assert.evaluate(doc(caught=15), doc(caught=5, missed=5), 7)
        self.assertEqual(len(errors), 1)
        self.assertIn("control", errors[0])

    def test_control_score_just_below_fifty_passes(self):
        self.assertEqual(g1_assert.evaluate(doc(caught=15), doc(caught=49, missed=51), 7), [])

    def test_run_longer_than_fifteen_minutes_fails(self):
        errors = g1_assert.evaluate(doc(caught=15), doc(missed=10), 901)
        self.assertEqual(len(errors), 1)
        self.assertIn("15 min", errors[0])

    def test_run_of_exactly_fifteen_minutes_passes(self):
        self.assertEqual(g1_assert.evaluate(doc(caught=15), doc(missed=10), 900), [])

    def test_a_set_with_no_testable_mutants_fails(self):
        errors = g1_assert.evaluate(doc(), doc(missed=10), 7)
        self.assertTrue(any("well-tested" in e for e in errors))


class Cli(unittest.TestCase):
    def run_cli(self, well, control, seconds):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "well.json").write_text(json.dumps(well))
            (tmp / "control.json").write_text(json.dumps(control))
            return g1_assert.main(
                [
                    "--well-tested", str(tmp / "well.json"),
                    "--control", str(tmp / "control.json"),
                    "--seconds", str(seconds),
                ]
            )

    def test_exit_code_zero_on_pass(self):
        self.assertEqual(self.run_cli(doc(caught=15), doc(caught=2, missed=24), 7), 0)

    def test_exit_code_one_on_failure(self):
        self.assertEqual(self.run_cli(doc(missed=15), doc(missed=10), 7), 1)


if __name__ == "__main__":
    unittest.main()
