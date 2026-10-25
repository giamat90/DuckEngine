"""Tests for the PR title check: the title becomes the squash commit message (card 0b-6)."""

import unittest

import check_pr_title

TYPES = ("feat", "fix", "perf", "refactor", "docs", "test", "build", "ci", "chore", "style", "revert")


class Title(unittest.TestCase):
    def test_valid_titles(self):
        titles = [f"{kind}: summary" for kind in TYPES] + [
            "feat(api): add a port",
            "fix(core)!: drop the old handle",
            "feat!: break something",
            "chore(release): 0b-6 release workflow",
        ]
        for title in titles:
            with self.subTest(title=title):
                self.assertEqual(check_pr_title.problems(title), [])

    def test_invalid_titles(self):
        titles = [
            "", "   ", "Add the thing", "feat add the thing", "feat:", "feat: ", "feat(): x",
            "feat(a b): x", "unknown: x", "Feat: x", "FIX: x", " feat: x", "feat: x ", "feat:x",
            "feat: x\nBREAKING CHANGE: y", "feat : x", "feat(core) : x",
        ]
        for title in titles:
            with self.subTest(title=title):
                self.assertTrue(check_pr_title.problems(title))

    def test_one_problem_names_the_expected_form(self):
        self.assertIn("type(scope)!: summary", check_pr_title.problems("nope")[0])


class CommandLine(unittest.TestCase):
    def run_main(self, title):
        out = []
        return check_pr_title.main(["--title", title], print_line=out.append), out

    def test_valid_title_exits_zero(self):
        self.assertEqual(self.run_main("fix: x")[0], 0)

    def test_invalid_title_exits_one_and_says_why(self):
        code, out = self.run_main("fix x")
        self.assertEqual(code, 1)
        self.assertTrue(out[0].startswith("FAIL"))


if __name__ == "__main__":
    unittest.main()
