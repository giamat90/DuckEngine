"""Tests for the squash-subject check: the merge commit subject equals the PR title (card 0b-8)."""

import unittest

import check_squash_subject as check


class PullRequestNumber(unittest.TestCase):
    def test_number_is_read_from_the_trailing_marker(self):
        self.assertEqual(check.pr_number("feat(x): add a thing (#42)"), 42)

    def test_a_marker_that_is_not_last_is_not_a_number(self):
        self.assertIsNone(check.pr_number("fix: handle (#7) badly"))

    def test_a_subject_without_a_marker_has_no_number(self):
        self.assertIsNone(check.pr_number("fix: direct push"))

    def test_leading_zeros_and_non_ascii_digits_are_rejected(self):
        self.assertIsNone(check.pr_number("fix: a (#007)"))
        self.assertIsNone(check.pr_number("fix: a (#٤٢)"))


class Problems(unittest.TestCase):
    def test_subject_equal_to_title_plus_number_passes(self):
        self.assertEqual(check.problems("fix(a): b (#3)", "fix(a): b"), [])

    def test_a_different_subject_is_a_problem_showing_both(self):
        found = check.problems("feat: other (#3)", "fix: real")
        self.assertEqual(len(found), 1)
        self.assertIn("feat: other (#3)", found[0])
        self.assertIn("fix: real (#3)", found[0])

    def test_a_subject_without_a_pr_number_is_a_problem(self):
        self.assertEqual(len(check.problems("fix: b", "fix: b")), 1)

    def test_a_title_that_is_not_a_conventional_commit_is_a_problem(self):
        self.assertEqual(len(check.problems("update stuff (#3)", "update stuff")), 1)

    def test_a_trailing_space_difference_is_a_problem(self):
        self.assertEqual(len(check.problems("fix: b  (#3)", "fix: b")), 1)


class Main(unittest.TestCase):
    def run_main(self, argv):
        lines = []
        return check.main(argv, lines.append), lines

    def test_number_mode_prints_the_number(self):
        code, lines = self.run_main(["number", "--subject", "fix: a (#12)"])
        self.assertEqual((code, lines), (0, ["12"]))

    def test_number_mode_fails_without_a_number(self):
        code, lines = self.run_main(["number", "--subject", "fix: a"])
        self.assertEqual(code, 1)
        self.assertTrue(lines[0].startswith("FAIL"))

    def test_check_mode_passes_and_fails(self):
        code, lines = self.run_main(["check", "--subject", "fix: a (#1)", "--title", "fix: a"])
        self.assertEqual(code, 0)
        self.assertTrue(lines[0].startswith("OK"))
        code, lines = self.run_main(["check", "--subject", "fix: b (#1)", "--title", "fix: a"])
        self.assertEqual(code, 1)
        self.assertTrue(lines[0].startswith("FAIL"))


if __name__ == "__main__":
    unittest.main()
