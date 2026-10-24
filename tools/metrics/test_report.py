"""Tests for the baseline metrics report generator."""

import json
import tempfile
import unittest
from pathlib import Path

import report


def coverage_doc(lines_percent, branch_count=0, regions_percent=100.0):
    return {
        "data": [
            {
                "totals": {
                    "lines": {"count": 32, "covered": 32, "percent": lines_percent},
                    "regions": {"count": 45, "covered": 45, "percent": regions_percent},
                    "branches": {"count": branch_count, "covered": 0, "percent": 0},
                }
            }
        ]
    }


def mutants_doc(caught=0, missed=0, timeout=0, unviable=0):
    return {
        "caught": caught,
        "missed": missed,
        "timeout": timeout,
        "unviable": unviable,
        "total_mutants": caught + missed + timeout + unviable,
    }


class CoverageMetrics(unittest.TestCase):
    def test_reads_line_and_region_percent(self):
        got = report.coverage_metrics(coverage_doc(92.5, regions_percent=88.0))
        self.assertEqual(got["lines"], "92.5%")
        self.assertEqual(got["regions"], "88.0%")

    def test_branch_coverage_is_not_available_without_branch_data(self):
        got = report.coverage_metrics(coverage_doc(100.0, branch_count=0))
        self.assertEqual(got["branches"], "n/a (needs nightly)")

    def test_branch_coverage_is_reported_when_branch_data_exists(self):
        doc = coverage_doc(100.0, branch_count=4)
        doc["data"][0]["totals"]["branches"]["percent"] = 75.0
        self.assertEqual(report.coverage_metrics(doc)["branches"], "75.0%")


class MutationMetrics(unittest.TestCase):
    def test_all_caught_scores_one_hundred_percent(self):
        got = report.mutation_metrics(mutants_doc(caught=9))
        self.assertEqual(got["score"], "100.0%")

    def test_missed_mutants_lower_the_score(self):
        got = report.mutation_metrics(mutants_doc(caught=3, missed=1))
        self.assertEqual(got["score"], "75.0%")

    def test_timeouts_count_against_the_score(self):
        got = report.mutation_metrics(mutants_doc(caught=3, timeout=1))
        self.assertEqual(got["score"], "75.0%")

    def test_unviable_mutants_are_excluded_from_the_score(self):
        got = report.mutation_metrics(mutants_doc(caught=2, unviable=5))
        self.assertEqual(got["score"], "100.0%")

    def test_no_testable_mutants_gives_no_score(self):
        got = report.mutation_metrics(mutants_doc(unviable=3))
        self.assertEqual(got["score"], "n/a (no testable mutants)")

    def test_counts_are_passed_through(self):
        got = report.mutation_metrics(mutants_doc(caught=2, missed=1, timeout=1, unviable=4))
        self.assertEqual(
            (got["caught"], got["missed"], got["timeout"], got["unviable"]), (2, 1, 1, 4)
        )


class Rendering(unittest.TestCase):
    def render(self, **overrides):
        args = {
            "os_name": "ubuntu-latest",
            "coverage": report.coverage_metrics(coverage_doc(100.0)),
            "mutation": report.mutation_metrics(mutants_doc(caught=9)),
            "mutation_seconds": 7,
            "bench": {"benchmarks": [{"name": "align_up", "median_ns": 1.5, "p95_ns": 2.0}]},
        }
        args.update(overrides)
        return report.render(**args)

    def test_report_names_the_os(self):
        self.assertIn("ubuntu-latest", self.render())

    def test_report_lists_coverage_mutation_and_duration(self):
        text = self.render()
        self.assertIn("| Line coverage | 100.0% |", text)
        self.assertIn("| Mutation score | 100.0% |", text)
        self.assertIn("| Mutation run time | 7 s |", text)

    def test_report_lists_each_benchmark(self):
        self.assertIn("| align_up | 1.50 | 2.00 |", self.render())

    def test_report_lists_the_control_score_when_given(self):
        control = report.mutation_metrics(mutants_doc(caught=1, missed=3))
        text = self.render(control=control)
        self.assertIn("| G1 control mutation score (weakly tested, must be low) | 25.0% |", text)

    def test_report_omits_the_control_row_when_not_given(self):
        self.assertNotIn("G1 control", self.render())

    def test_report_says_thresholds_are_not_enforced(self):
        self.assertIn("not enforced", self.render())


class Cli(unittest.TestCase):
    def test_main_accepts_a_control_mutants_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "cov.json").write_text(json.dumps(coverage_doc(100.0)))
            (tmp / "mut.json").write_text(json.dumps(mutants_doc(caught=9)))
            (tmp / "ctl.json").write_text(json.dumps(mutants_doc(caught=1, missed=1)))
            (tmp / "bench.json").write_text(
                json.dumps({"benchmarks": [{"name": "x", "median_ns": 1.0, "p95_ns": 1.0}]})
            )
            out = tmp / "report.md"
            report.main(
                [
                    "--os", "x",
                    "--coverage", str(tmp / "cov.json"),
                    "--mutants", str(tmp / "mut.json"),
                    "--control-mutants", str(tmp / "ctl.json"),
                    "--bench", str(tmp / "bench.json"),
                    "--mutation-seconds", "5",
                    "--out", str(out),
                ]
            )
            self.assertIn("50.0%", out.read_text())

    def test_main_writes_the_report_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "cov.json").write_text(json.dumps(coverage_doc(100.0)))
            (tmp / "mut.json").write_text(json.dumps(mutants_doc(caught=9)))
            (tmp / "bench.json").write_text(
                json.dumps({"benchmarks": [{"name": "x", "median_ns": 1.0, "p95_ns": 1.0}]})
            )
            out = tmp / "report.md"
            code = report.main(
                [
                    "--os", "windows-latest",
                    "--coverage", str(tmp / "cov.json"),
                    "--mutants", str(tmp / "mut.json"),
                    "--bench", str(tmp / "bench.json"),
                    "--mutation-seconds", "5",
                    "--out", str(out),
                ]
            )
            self.assertEqual(code, 0)
            self.assertIn("windows-latest", out.read_text())


if __name__ == "__main__":
    unittest.main()
