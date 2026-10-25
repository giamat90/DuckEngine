"""Tests for the checks that run from the base commit on a pull request (card 0c-14)."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import pr_gates

REQUIRED = "ci.yml\ngovernance-guard.yml\n"
VALID = "name: x\non:\n  push:\njobs:\n  a:\n    runs-on: x\n"
GIT_ENV = {
    **os.environ,
    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
}


def record(hash_value, task="0c-14"):
    return f"\n## Owner approval\n\nTask: {task}\nDiff hash: {hash_value}\nBasis: Metrics\nApproved: 2026-10-25\n"


class RequiredList(unittest.TestCase):
    def test_comments_and_blank_lines_are_skipped(self):
        self.assertEqual(pr_gates.load_required("# h\n\nci.yml\n  pr-title.yml  \n"), ["ci.yml", "pr-title.yml"])

    def test_an_empty_list_is_an_error(self):
        with self.assertRaises(ValueError):
            pr_gates.load_required("# nothing\n")

    def test_a_name_with_a_path_is_an_error(self):
        for bad in ("../ci.yml", "sub/ci.yml", "a\\b.yml", "ci.yml.txt"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                pr_gates.load_required(bad + "\n")


class WorkflowsPresent(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.repo = Path(self._tmp.name)
        self.folder = self.repo / ".github" / "workflows"
        self.folder.mkdir(parents=True)

    def test_all_present_is_clean(self):
        (self.folder / "ci.yml").write_text(VALID)
        (self.folder / "governance-guard.yml").write_text(VALID)
        self.assertEqual(pr_gates.missing_workflows(self.repo, ["ci.yml", "governance-guard.yml"]), [])

    def test_a_deleted_workflow_is_named(self):
        (self.folder / "ci.yml").write_text(VALID)
        problems = pr_gates.missing_workflows(self.repo, ["ci.yml", "governance-guard.yml"])
        self.assertEqual(len(problems), 1)
        self.assertIn("governance-guard.yml", problems[0])

    def test_an_empty_workflow_counts_as_missing(self):
        (self.folder / "ci.yml").write_text("  \n")
        self.assertTrue(pr_gates.missing_workflows(self.repo, ["ci.yml"]))

    def test_a_hollowed_out_workflow_counts_as_missing(self):
        for hollow in ("name: x\n", "on: push\n", "jobs:\n  a: {}\n", "name: x\non:\n  push:\njobs: {}\n"):
            with self.subTest(hollow=hollow):
                (self.folder / "ci.yml").write_text(hollow)
                self.assertTrue(pr_gates.missing_workflows(self.repo, ["ci.yml"]))

    def test_a_symlinked_workflows_folder_counts_as_missing(self):
        class Link:  # symlinks cannot be created without privileges on Windows
            def is_symlink(self):
                return True

        self.assertIn("symlink", pr_gates.folder_problem(Link()))
        self.assertIsNone(pr_gates.folder_problem(self.folder))

    def test_a_folder_in_place_of_a_file_counts_as_missing(self):
        (self.folder / "ci.yml").mkdir()
        self.assertTrue(pr_gates.missing_workflows(self.repo, ["ci.yml"]))

    def test_a_symlink_counts_as_missing(self):
        class Link:  # symlinks cannot be created without privileges on Windows
            def is_symlink(self):
                return True

        self.assertIn("symlink", pr_gates.workflow_problem("ci.yml", Link()))

    def test_no_workflows_folder_at_all_is_reported_for_every_name(self):
        self.assertEqual(len(pr_gates.missing_workflows(self.repo / "nope", ["ci.yml", "x.yml"])), 2)


class ApprovalVerdict(unittest.TestCase):
    def test_no_record_yet_is_pending_not_a_failure(self):
        verdict, problems = pr_gates.approval_verdict("Just a description.\n", "a" * 16)
        self.assertEqual(verdict, "pending")
        self.assertEqual(problems, [])

    def test_a_matching_record_is_ok(self):
        verdict, problems = pr_gates.approval_verdict(record("a" * 16), "a" * 16)
        self.assertEqual((verdict, problems), ("ok", []))

    def test_a_stale_hash_fails(self):
        verdict, problems = pr_gates.approval_verdict(record("b" * 16), "a" * 16)
        self.assertEqual(verdict, "fail")
        self.assertTrue(any("does not match" in p for p in problems))

    def test_a_record_inside_a_code_block_is_not_a_record(self):
        body = "```\n" + record("a" * 16) + "```\n"
        self.assertEqual(pr_gates.approval_verdict(body, "a" * 16)[0], "pending")

    def test_an_incomplete_record_fails(self):
        body = "## Owner approval\n\nTask: 0c-14\n"
        self.assertEqual(pr_gates.approval_verdict(body, "a" * 16)[0], "fail")


class Main(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.repo = Path(self._tmp.name)
        self.git("init", "-q", "-b", "main")
        (self.repo / "a.txt").write_text("one\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")
        self.git("checkout", "-q", "-b", "feature")
        (self.repo / "a.txt").write_text("two\n")
        self.git("commit", "-q", "-am", "change")

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.repo, env=GIT_ENV, check=True, capture_output=True)

    def run_approval(self, body):
        body_file = self.repo.parent / (self.repo.name + "-body.md")
        body_file.write_text(body, encoding="utf-8")
        self.addCleanup(body_file.unlink)
        lines = []
        code = pr_gates.main(["approval", "--repo", str(self.repo), "--base", "main", "--head", "feature",
                              "--body-file", str(body_file)], lines.append)
        return code, lines

    def test_pending_exits_zero_and_says_so(self):
        code, lines = self.run_approval("no record\n")
        self.assertEqual(code, 0)
        self.assertTrue(lines[-1].startswith("PENDING"))

    def test_a_record_for_the_real_diff_passes(self):
        import diff_hash
        real = diff_hash.compute("main", "feature", repo=str(self.repo))
        code, lines = self.run_approval(record(real))
        self.assertEqual(code, 0)
        self.assertTrue(lines[-1].startswith("OK"))

    def test_a_record_for_another_diff_fails(self):
        code, lines = self.run_approval(record("c" * 16))
        self.assertEqual(code, 1)
        self.assertTrue(any(line.startswith("FAIL") for line in lines))

    def test_an_unknown_revision_fails_instead_of_passing(self):
        body_file = self.repo.parent / (self.repo.name + "-b.md")
        body_file.write_text("x", encoding="utf-8")
        self.addCleanup(body_file.unlink)
        lines = []
        code = pr_gates.main(["approval", "--repo", str(self.repo), "--base", "main", "--head", "nope",
                              "--body-file", str(body_file)], lines.append)
        self.assertEqual(code, 1)

    def test_the_workflows_command_reports_a_missing_file(self):
        required = self.repo.parent / (self.repo.name + "-req.txt")
        required.write_text(REQUIRED, encoding="utf-8")
        self.addCleanup(required.unlink)
        lines = []
        code = pr_gates.main(["workflows", "--repo", str(self.repo), "--required", str(required)], lines.append)
        self.assertEqual(code, 1)
        self.assertTrue(lines[-1].startswith("FAIL"))

    def test_an_unreadable_required_list_fails(self):
        lines = []
        code = pr_gates.main(["workflows", "--repo", str(self.repo), "--required", str(self.repo / "nope")], lines.append)
        self.assertEqual(code, 1)


class RealList(unittest.TestCase):
    def test_every_name_in_the_real_list_exists_in_this_repository(self):
        root = Path(__file__).resolve().parents[2]
        required = pr_gates.load_required((root / "gates" / "required-workflows.txt").read_text(encoding="utf-8"))
        self.assertEqual(pr_gates.missing_workflows(root, required), [])
        self.assertIn("trusted-pr-gates.yml", required)


if __name__ == "__main__":
    unittest.main()
