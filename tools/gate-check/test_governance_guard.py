"""Tests for the governance guard: protected paths need a declared governance task (card 0b-8)."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import governance_guard as guard

PROTECTED = ["gates/", "CLAUDE.md", ".github/workflows/"]
GIT_ENV = {
    **os.environ,
    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
}


class LoadProtected(unittest.TestCase):
    def test_comments_and_blank_lines_are_skipped(self):
        text = "# header\n\ngates/\n  CLAUDE.md  \n# more\n"
        self.assertEqual(guard.load_protected(text), ["gates/", "CLAUDE.md"])

    def test_an_empty_list_is_an_error(self):
        with self.assertRaises(ValueError):
            guard.load_protected("# nothing\n")


class IsProtected(unittest.TestCase):
    def test_a_directory_entry_matches_everything_below_it(self):
        self.assertTrue(guard.is_protected("gates/gates.toml", PROTECTED))
        self.assertTrue(guard.is_protected(".github/workflows/ci.yml", PROTECTED))

    def test_a_file_entry_matches_only_that_file(self):
        self.assertTrue(guard.is_protected("CLAUDE.md", PROTECTED))
        self.assertFalse(guard.is_protected("CLAUDE.md.bak", PROTECTED))
        self.assertFalse(guard.is_protected("docs/CLAUDE.md", PROTECTED))

    def test_a_directory_entry_does_not_match_a_sibling_with_the_same_prefix(self):
        self.assertFalse(guard.is_protected("gates-extra/x", PROTECTED))

    def test_other_paths_are_not_protected(self):
        self.assertFalse(guard.is_protected("engine/src/lib.rs", PROTECTED))


class Declared(unittest.TestCase):
    def test_a_governance_line_declares_the_task(self):
        self.assertTrue(guard.declared("text\n\nGovernance task: yes (card 0b-8)\n"))

    def test_no_or_missing_does_not_declare(self):
        self.assertFalse(guard.declared("Governance task: no\n"))
        self.assertFalse(guard.declared("nothing here"))
        self.assertFalse(guard.declared("Governance task: yesterday\n"))

    def test_a_line_that_is_only_part_of_a_sentence_does_not_declare(self):
        self.assertFalse(guard.declared("This is not a Governance task: yes\n"))

    def test_hidden_comment_and_code_block_do_not_declare(self):
        self.assertFalse(guard.declared("<!-- Governance task: yes -->\n"))
        self.assertFalse(guard.declared("```\nGovernance task: yes\n```\n"))

    def test_an_unclosed_fence_or_comment_hides_the_rest(self):
        self.assertFalse(guard.declared("```\nGovernance task: yes (card 1)\n"))
        self.assertFalse(guard.declared("<!--\nGovernance task: yes (card 1)\n"))

    def test_a_card_id_is_required(self):
        self.assertFalse(guard.declared("Governance task: yes\n"))
        self.assertFalse(guard.declared("Governance task: yes (card )\n"))

    def test_windows_line_endings_are_accepted(self):
        self.assertTrue(guard.declared("a\r\nGovernance task: yes (card 1)\r\n"))


class Problems(unittest.TestCase):
    def test_no_protected_file_needs_no_declaration(self):
        self.assertEqual(guard.problems(["engine/a.rs"], PROTECTED, ""), [])

    def test_protected_file_without_declaration_is_a_problem_naming_the_file(self):
        found = guard.problems(["engine/a.rs", "gates/gates.toml"], PROTECTED, "body")
        self.assertEqual(len(found), 1)
        self.assertIn("gates/gates.toml", found[0])
        self.assertNotIn("engine/a.rs", found[0])

    def test_protected_file_with_declaration_passes(self):
        self.assertEqual(guard.problems(["CLAUDE.md"], PROTECTED, "Governance task: yes (card x)"), [])


class Git(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.path = Path(self._tmp.name)
        self.git("init", "-q", "-b", "main")
        self.git("config", "commit.gpgsign", "false")

    def git(self, *args):
        done = subprocess.run(["git", *args], cwd=self.path, env=GIT_ENV, capture_output=True, check=True)
        return done.stdout.decode().strip()

    def commit(self, name, text="x\n"):
        target = self.path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", name)

    def test_changed_files_are_those_of_the_head_since_the_merge_base(self):
        self.commit("base.txt")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("gates/gates.toml")
        self.commit("dir with space/é.txt")
        self.git("checkout", "-q", "main")
        self.commit("only-on-main.txt")
        self.assertEqual(
            sorted(guard.changed_files(self.path, "main", "topic")),
            ["dir with space/é.txt", "gates/gates.toml"],
        )

    def test_a_renamed_protected_file_is_listed_under_both_names(self):
        self.commit("gates/gates.toml", "a\nb\nc\nd\n")
        self.git("checkout", "-q", "-b", "topic")
        self.git("mv", "gates/gates.toml", "docs.toml")
        self.git("commit", "-q", "-m", "move")
        self.assertEqual(sorted(guard.changed_files(self.path, "main", "topic")), ["docs.toml", "gates/gates.toml"])

    def test_a_deleted_protected_file_is_listed(self):
        self.commit("gates/gates.toml")
        self.git("checkout", "-q", "-b", "topic")
        self.git("rm", "-q", "gates/gates.toml")
        self.git("commit", "-q", "-m", "rm")
        self.assertEqual(guard.changed_files(self.path, "main", "topic"), ["gates/gates.toml"])

    def test_an_unknown_revision_is_an_error(self):
        self.commit("a.txt")
        with self.assertRaises(guard.GuardError):
            guard.changed_files(self.path, "main", "missing")

    def test_main_fails_for_a_protected_change_and_passes_with_a_declaration(self):
        self.commit("base.txt")
        self.git("checkout", "-q", "-b", "topic")
        self.commit("gates/gates.toml")
        paths = self.path.parent / (self.path.name + "-paths.txt")
        paths.write_text("gates/\n", encoding="utf-8")
        self.addCleanup(paths.unlink)
        body = self.path.parent / (self.path.name + "-body.md")
        self.addCleanup(lambda: body.unlink(missing_ok=True))
        args = ["--repo", str(self.path), "--base", "main", "--head", "topic", "--paths", str(paths), "--body-file", str(body)]
        lines = []
        body.write_text("no declaration", encoding="utf-8")
        self.assertEqual(guard.main(args, lines.append), 1)
        self.assertTrue(lines[0].startswith("FAIL"))
        body.write_text("Governance task: yes (card 0b-8)", encoding="utf-8")
        lines.clear()
        self.assertEqual(guard.main(args, lines.append), 0)
        self.assertTrue(lines[0].startswith("OK"))

    def test_main_reports_a_git_error_as_failure(self):
        paths = self.path / "p.txt"
        paths.write_text("gates/\n", encoding="utf-8")
        body = self.path / "b.md"
        body.write_text("", encoding="utf-8")
        lines = []
        args = ["--repo", str(self.path), "--base", "nope", "--head", "nope", "--paths", str(paths), "--body-file", str(body)]
        self.assertEqual(guard.main(args, lines.append), 1)
        self.assertTrue(lines[0].startswith("FAIL"))


class ProtectedPathsFile(unittest.TestCase):
    """The real list must cover the governance files named in CLAUDE.md and the digest, section 8."""

    ROOT = Path(__file__).resolve().parents[2]

    def test_the_list_covers_the_governance_files(self):
        listed = guard.load_protected((self.ROOT / "gates" / "protected-paths.txt").read_text(encoding="utf-8"))
        for path in ("gates/gates.toml", "gates/protected-paths.txt", ".github/workflows/ci.yml",
                     ".github/workflows/release.yml", "wiki/rules-digest.md", "CLAUDE.md",
                     ".claude/settings.json", "THIRD_PARTY.md", "tools/gate-check/check_digest.py",
                     "tools/release/release_plan.py", "tools/classify-change/classify_change.py", "tools/arch-check/arch_check.py", "tools/wiki-lint/wiki_lint.py", "gates/wiki-lint.toml", "wiki/decisions/0001-x.md", "docs/adr/0001-x.md",
                     "rust-toolchain.toml", ".github/CODEOWNERS", "tools/templates/review-report.md",
                     "tools/metrics/report.py", "tools/metrics/approval_report.py",
                     "acceptance/scenarios/x.toml", "acceptance/validate.py"):
            with self.subTest(path=path):
                self.assertTrue(guard.is_protected(path, listed))


if __name__ == "__main__":
    unittest.main()
