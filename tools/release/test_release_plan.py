"""Tests for the release plan: which version a merge to main releases (card 0b-6)."""

import unittest

import release_plan
from repo_fixture import Repo


class LatestTag(unittest.TestCase):
    def test_highest_version_wins_numerically(self):
        self.assertEqual(release_plan.latest_tag(["v0.9.0", "v0.10.0", "v0.2.5"]), "v0.10.0")

    def test_tags_that_are_not_plain_versions_are_ignored(self):
        tags = ["v1", "vx.1.2", "release-9.9.9", "v1.2.3-rc1", "1.2.3", "v1.2.3.4", "v0.1.0"]
        self.assertEqual(release_plan.latest_tag(tags), "v0.1.0")

    def test_no_release_tag_gives_none(self):
        self.assertIsNone(release_plan.latest_tag([]))
        self.assertIsNone(release_plan.latest_tag(["nightly"]))


class Plan(unittest.TestCase):
    def test_first_release_is_the_initial_version(self):
        plan = release_plan.plan([], head_tags=[], messages=["feat: a", "fix: b"])
        self.assertEqual((plan.version, plan.previous, plan.skip), ("0.1.0", None, False))

    def test_next_release_follows_the_commit_types(self):
        cases = (
            (["docs: x"], "0.1.1"),
            (["fix: x", "chore: y"], "0.1.1"),
            (["feat: x"], "0.1.1"),  # pre-1.0: a feature is a patch
            (["feat(api)!: x"], "0.2.0"),  # pre-1.0: a break moves the minor number
            (["not conventional at all"], "0.1.1"),
        )
        for messages, expected in cases:
            with self.subTest(messages=messages):
                plan = release_plan.plan(["v0.1.0"], head_tags=[], messages=messages)
                self.assertEqual((plan.version, plan.previous), (expected, "v0.1.0"))

    def test_a_head_that_already_has_a_release_is_skipped(self):
        plan = release_plan.plan(["v0.1.0", "v0.1.1"], head_tags=["v0.1.1"], messages=[])
        self.assertEqual((plan.skip, plan.version), (True, "0.1.1"))

    def test_a_non_release_tag_on_head_does_not_skip(self):
        plan = release_plan.plan(["v0.1.0"], head_tags=["nightly"], messages=["fix: x"])
        self.assertFalse(plan.skip)

    def test_each_release_moves_past_the_previous_one(self):
        tags = []
        seen = []
        for _ in range(4):
            plan = release_plan.plan(tags, head_tags=[], messages=["fix: x"])
            seen.append(plan.version)
            tags.append("v" + plan.version)
        self.assertEqual(seen, ["0.1.0", "0.1.1", "0.1.2", "0.1.3"])
        self.assertEqual(len(set(seen)), len(seen))


class Output(unittest.TestCase):
    def test_lines_are_key_value_pairs_for_github_output(self):
        plan = release_plan.plan(["v0.1.0"], head_tags=[], messages=["feat!: x"])
        self.assertEqual(
            release_plan.format_output(plan),
            ["version=0.2.0", "tag=v0.2.0", "previous=v0.1.0", "skip=false"],
        )

    def test_unicode_digits_are_not_a_version(self):
        self.assertIsNone(release_plan.latest_tag(["v\u0661.\u0662.\u0663"]))

    def test_first_release_has_an_empty_previous(self):
        plan = release_plan.plan([], head_tags=[], messages=[])
        self.assertIn("previous=", release_plan.format_output(plan))


class GitPlan(unittest.TestCase):
    def setUp(self):
        self.repo = Repo()
        self.addCleanup(self.repo.close)

    def plan(self):
        return release_plan.git_plan(self.repo.path)

    def test_repository_without_tags_releases_the_initial_version(self):
        self.repo.commit("feat: first")
        self.assertEqual(self.plan().version, "0.1.0")

    def test_only_commits_after_the_last_tag_count(self):
        self.repo.commit("feat: before the tag")
        self.repo.git("tag", "v0.1.0")
        self.repo.commit("docs: after the tag")
        plan = self.plan()
        self.assertEqual((plan.version, plan.previous), ("0.1.1", "v0.1.0"))

    def test_breaking_commit_after_the_tag_bumps_the_minor_number(self):
        self.repo.commit("fix: a")
        self.repo.git("tag", "v0.1.0")
        self.repo.commit("feat!: b")
        self.assertEqual(self.plan().version, "0.2.0")

    def test_breaking_footer_in_a_squash_message_body_counts(self):
        self.repo.commit("fix: a")
        self.repo.git("tag", "v0.3.0")
        self.repo.commit("refactor(core): rename\n\nBREAKING CHANGE: port renamed")
        self.assertEqual(self.plan().version, "0.4.0")

    def test_head_with_a_release_tag_is_skipped(self):
        self.repo.commit("fix: a")
        self.repo.git("tag", "v0.1.0")
        self.assertTrue(self.plan().skip)

    def test_a_tag_that_is_not_an_ancestor_of_head_is_an_error(self):
        self.repo.commit("fix: base")
        self.repo.git("checkout", "-q", "-b", "other")
        self.repo.commit("fix: elsewhere")
        self.repo.git("tag", "v0.5.0")
        self.repo.git("checkout", "-q", "main")
        self.repo.commit("fix: on main")
        with self.assertRaises(release_plan.PlanError):
            self.plan()

    def test_not_a_repository_is_an_error(self):
        with self.assertRaises(release_plan.PlanError):
            release_plan.git_plan(self.repo.path / "missing")


class CommandLine(unittest.TestCase):
    def test_prints_the_output_lines(self):
        repo = Repo()
        self.addCleanup(repo.close)
        repo.commit("feat: first")
        out = []
        self.assertEqual(release_plan.main(["--repo", str(repo.path)], print_line=out.append), 0)
        self.assertEqual(out[0], "version=0.1.0")

    def test_failure_exits_one_with_a_message(self):
        out, err = [], []
        code = release_plan.main(["--repo", "does-not-exist"], print_line=out.append, print_error=err.append)
        self.assertEqual((code, out), (1, []))  # stdout feeds $GITHUB_OUTPUT, so it stays clean
        self.assertTrue(err[0].startswith("error:"))


if __name__ == "__main__":
    unittest.main()
