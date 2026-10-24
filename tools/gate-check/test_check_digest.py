"""Tests for the digest-vs-gate-configuration check."""

import tempfile
import unittest
from pathlib import Path

import check_digest

DIGEST = """
- Measurable clean-code limits only (see gate config): function length warn > 60, cyclomatic warn > 10 / fail > 15, cognitive fail > 20, nesting <= 3, parameters warn > 3 / fail > 5, duplication <= 3%, no dead code.
Line coverage L0-L2 >= 90% (new lines >= 95%), branch >= 80%, **mutation score on changed L0-L2 code >= 80%**, 0 skipped tests, PR benchmark gate on instruction counts (warn 5%, fail 10%), cross-platform.
"""

CONFIG = {
    "coverage": {"line_min": 90, "new_line_min": 95, "branch_min": 80},
    "mutation": {"score_min": 80},
    "benchmark": {"warn_percent": 5, "fail_percent": 10},
    "clean_code": {
        "function_length_warn": 60,
        "cyclomatic_warn": 10,
        "cyclomatic_fail": 15,
        "cognitive_fail": 20,
        "nesting_max": 3,
        "parameters_warn": 3,
        "parameters_fail": 5,
        "duplication_max_percent": 3,
    },
}


def config_with(table, key, value):
    changed = {name: dict(values) for name, values in CONFIG.items()}
    changed[table][key] = value
    return changed


class Comparison(unittest.TestCase):
    def test_matching_digest_and_config_have_no_errors(self):
        self.assertEqual(check_digest.compare(DIGEST, CONFIG), [])

    def test_a_changed_config_value_is_reported_with_both_numbers(self):
        errors = check_digest.compare(DIGEST, config_with("coverage", "line_min", 85))
        self.assertEqual(len(errors), 1)
        self.assertIn("coverage.line_min", errors[0])
        self.assertIn("90", errors[0])
        self.assertIn("85", errors[0])

    def test_a_changed_digest_value_is_reported(self):
        digest = DIGEST.replace("nesting <= 3", "nesting <= 4")
        errors = check_digest.compare(digest, CONFIG)
        self.assertEqual(len(errors), 1)
        self.assertIn("clean_code.nesting_max", errors[0])

    def test_every_pair_in_a_multi_number_sentence_is_checked(self):
        for key in ("cyclomatic_warn", "cyclomatic_fail", "parameters_warn", "parameters_fail"):
            errors = check_digest.compare(DIGEST, config_with("clean_code", key, 99))
            self.assertEqual(len(errors), 1, key)

    def test_a_number_missing_from_the_digest_is_an_error(self):
        digest = DIGEST.replace("duplication <= 3%, ", "")
        errors = check_digest.compare(digest, CONFIG)
        self.assertTrue(any("duplication" in e for e in errors))

    def test_a_key_missing_from_the_config_is_an_error(self):
        config = {name: dict(values) for name, values in CONFIG.items()}
        del config["mutation"]["score_min"]
        errors = check_digest.compare(DIGEST, config)
        self.assertTrue(any("mutation.score_min" in e for e in errors))

    def test_a_config_key_the_digest_does_not_cover_is_an_error(self):
        config = {name: dict(values) for name, values in CONFIG.items()}
        config["mutation"]["surprise"] = 1
        errors = check_digest.compare(DIGEST, config)
        self.assertTrue(any("mutation.surprise" in e for e in errors))


class Cli(unittest.TestCase):
    def run_cli(self, digest, config_toml, claude_md="@wiki/rules-digest.md\n"):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "digest.md").write_text(digest, encoding="utf-8")
            (tmp / "gates.toml").write_text(config_toml, encoding="utf-8")
            (tmp / "CLAUDE.md").write_text(claude_md, encoding="utf-8")
            return check_digest.main(
                [
                    "--digest", str(tmp / "digest.md"),
                    "--config", str(tmp / "gates.toml"),
                    "--claude-md", str(tmp / "CLAUDE.md"),
                ]
            )

    TOML = """
[coverage]
line_min = 90
new_line_min = 95
branch_min = 80
[mutation]
score_min = 80
[benchmark]
warn_percent = 5
fail_percent = 10
[clean_code]
function_length_warn = 60
cyclomatic_warn = 10
cyclomatic_fail = 15
cognitive_fail = 20
nesting_max = 3
parameters_warn = 3
parameters_fail = 5
duplication_max_percent = 3
"""

    def test_exit_code_zero_when_everything_matches(self):
        self.assertEqual(self.run_cli(DIGEST, self.TOML), 0)

    def test_exit_code_one_on_a_seeded_mismatch(self):
        self.assertEqual(self.run_cli(DIGEST, self.TOML.replace("score_min = 80", "score_min = 70")), 1)

    def test_exit_code_one_when_claude_md_does_not_import_the_digest(self):
        self.assertEqual(self.run_cli(DIGEST, self.TOML, claude_md="no import here\n"), 1)


if __name__ == "__main__":
    unittest.main()
