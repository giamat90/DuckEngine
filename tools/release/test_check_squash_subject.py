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


class Footer(unittest.TestCase):
    def test_a_phase_merge_needs_the_footer_with_its_own_number(self):
        self.assertEqual(check.footer_problems("phase/1", "Release-Phase: 1"), [])
        self.assertEqual(check.footer_problems("phase/12", "x\n\nRelease-Phase: 12\n"), [])

    def test_a_phase_merge_without_the_footer_is_a_problem(self):
        self.assertEqual(len(check.footer_problems("phase/1", "")), 1)

    def test_the_footer_number_must_match_the_branch(self):
        self.assertEqual(len(check.footer_problems("phase/1", "Release-Phase: 2")), 1)

    def test_two_footers_are_a_problem(self):
        self.assertEqual(len(check.footer_problems("phase/1", "Release-Phase: 1\nRelease-Phase: 1")), 1)

    def test_trailing_blanks_and_carriage_returns_are_tolerated(self):
        self.assertEqual(check.footer_problems("phase/1", "Release-Phase: 1 \r\n"), [])

    def test_an_ordinary_branch_must_not_carry_the_footer(self):
        self.assertEqual(len(check.footer_problems("agent/1-40-x", "Release-Phase: 1")), 1)
        self.assertEqual(check.footer_problems("agent/1-40-x", "fix: a"), [])

    def test_a_footer_line_with_trailing_text_still_counts_like_next_version_reads_it(self):
        self.assertEqual(len(check.footer_problems("agent/x", "Release-Phase: 1 foo")), 1)
        self.assertEqual(len(check.footer_problems("phase/1", "Release-Phase: 1 foo")), 1)
        self.assertEqual(len(check.footer_problems("phase/1", "Release-Phase: 1\nRelease-Phase: 2 x")), 1)

    def test_phase_only_mode_ignores_an_ordinary_branch(self):
        self.assertEqual(check.footer_problems("agent/x", "Release-Phase: 1", phase_only=True), [])
        self.assertEqual(len(check.footer_problems("phase/1", "", phase_only=True)), 1)

    def test_phase_zero_and_other_prefixes_are_not_phase_branches(self):
        for ref in ("phase/0", "phase/01", "phase/1-x", "xphase/1", "phase/"):
            with self.subTest(ref=ref):
                self.assertEqual(len(check.footer_problems(ref, "Release-Phase: 1")), 1)


class Main(unittest.TestCase):
    def test_footer_mode_passes_and_fails(self):
        ok, lines = self.run_main(["footer", "--head-ref", "phase/1", "--message", "Release-Phase: 1"])
        self.assertEqual(ok, 0)
        self.assertTrue(lines[0].startswith("OK"))
        bad, lines = self.run_main(["footer", "--head-ref", "phase/1", "--message", ""])
        self.assertEqual(bad, 1)
        self.assertTrue(lines[0].startswith("FAIL"))

    def test_footer_mode_in_phase_only_mode_skips_other_branches(self):
        code, _ = self.run_main(["footer", "--head-ref", "agent/x", "--message", "Release-Phase: 3", "--phase-only"])
        self.assertEqual(code, 0)

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
