"""Tests for the one path matcher shared by the governance guard and the classifier (card 0c-9)."""

import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

import governance_guard as guard
import protected_paths as pp

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "classify-change"))
import classify_change as cc  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
REAL = pp.load_protected((ROOT / "gates" / "protected-paths.txt").read_text(encoding="utf-8"))
WILD = ["**/NOTES.md", "**/secret/", "gates/"]


class Load(unittest.TestCase):
    def test_wildcard_entries_are_accepted(self):
        self.assertEqual(pp.load_protected("**/a.toml\n**/b/\n"), ["**/a.toml", "**/b/"])

    def test_malformed_entries_are_errors(self):
        for text in ("./gates/\n", "/gates/\n", "gates\\x/\n", "gates/ # c\n", "**/\n", "**/a/b\n", "a/**/b\n", "**\n", "*.rs\n",
                     "a//\n", "a/./b\n", "a/../b\n", "**/..\n", "a:b\n", "a.\n", "**/a./\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                pp.load_protected(text)

    def test_an_empty_list_is_an_error(self):
        with self.assertRaises(ValueError):
            pp.load_protected("# nothing\n")


class Matching(unittest.TestCase):
    def test_a_file_wildcard_matches_the_name_at_any_depth_including_the_root(self):
        for path in ("NOTES.md", "a/NOTES.md", "a/b/c/NOTES.md"):
            with self.subTest(path=path):
                self.assertTrue(pp.is_protected(path, WILD))

    def test_a_file_wildcard_does_not_match_a_directory_or_a_longer_name(self):
        for path in ("NOTES.md/x", "a/NOTES.md.bak", "a/MYNOTES.md", "a/NOTES.mdx"):
            with self.subTest(path=path):
                self.assertFalse(pp.is_protected(path, WILD))

    def test_a_directory_wildcard_matches_a_directory_component_anywhere(self):
        for path in ("secret/x", "a/secret/x", "a/b/secret/c/d.rs"):
            with self.subTest(path=path):
                self.assertTrue(pp.is_protected(path, WILD))

    def test_a_directory_wildcard_does_not_match_a_file_or_a_longer_name(self):
        for path in ("a/secret", "a/secrets/x", "a/mysecret/x"):
            with self.subTest(path=path):
                self.assertFalse(pp.is_protected(path, WILD))

    def test_case_is_ignored_because_windows_ignores_it(self):
        for path in ("notes.MD", "A/Secret/X", "GATES/x", "Gates/x"):
            with self.subTest(path=path):
                self.assertTrue(pp.is_protected(path, WILD))

    def test_paths_that_alias_another_file_on_windows_are_protected(self):
        for path in ("a\\b", "x.txt::$DATA", "x. ", "x.", "GATES~1/x", "a/CLAUDE~1.MD"):
            with self.subTest(path=path):
                self.assertTrue(pp.is_alias(path))
                self.assertTrue(pp.is_protected(path, WILD))

    def test_a_non_ascii_name_is_treated_as_an_alias_because_ntfs_folds_it_its_own_way(self):
        # casefold() keeps the dotless i (U+0131); NTFS may equate it with I, so it could name clippy.toml.
        for path in ("clıppy.toml", "engine/é/x.rs"):
            with self.subTest(path=path):
                self.assertTrue(pp.is_alias(path))
                self.assertTrue(pp.is_protected(path, WILD))

    def test_ordinary_paths_are_not_aliases(self):
        for path in ("a/b.rs", "wiki/page.md", "a~b/x", "docs/x.v2"):
            with self.subTest(path=path):
                self.assertFalse(pp.is_alias(path))

    def test_an_unlisted_path_is_not_protected(self):
        self.assertFalse(pp.is_protected("engine/a/src/lib.rs", WILD))


class GuardAndClassifierAgree(unittest.TestCase):
    """Both tools use the same matcher: whatever one protects, the other calls Class 3."""

    PATHS = (
        "Gates/x", "gates/gates.toml", "claude.md", "CLAUDE.md", "docs/CLAUDE.md", "CLAUDE.local.md", "AGENTS.md",
        "docs/AGENTS.md", ".mcp.json", "sub/.mcp.json", "run_claude.bat", ".Claude/s.json", "docs/.claude/x",
        ".cargo/config.toml", "engine/a/.cargo/config.toml", "engine/a/.CARGO/config", "rustfmt.toml",
        ".rustfmt.toml", "engine/a/rustfmt.toml", "clippy.toml", "engine/a/clippy.toml", "engine/a/deny.toml",
        "engine/a/rust-toolchain.toml", "engine/a/contracts/c.rs", "engine/a/CONTRACTS/c.rs", "GATES~1/x",
        "gates\\x", ".github/workflows/ci.yml",
    )

    def test_the_real_list_protects_each_path_in_the_guard_and_the_classifier(self):
        for path in self.PATHS:
            with self.subTest(path=path):
                self.assertTrue(guard.is_protected(path, REAL))
                self.assertEqual(cc.classify_path(path, REAL)[0], 3)

    def test_ordinary_code_and_docs_stay_unprotected(self):
        for path in ("engine/a/src/lib.rs", "wiki/page.md", "docs/guide.md", "reviews/0c-9.md", "engine/a/Cargo.toml"):
            with self.subTest(path=path):
                self.assertFalse(guard.is_protected(path, REAL))

    def test_the_any_depth_entries_of_the_real_list_are_exactly_these(self):
        # Deleting one of them silently unprotects a file; changing the set is a governance decision.
        self.assertEqual(
            sorted(entry for entry in REAL if entry.startswith("**/")),
            sorted(["**/CLAUDE.md", "**/CLAUDE.local.md", "**/AGENTS.md", "**/.mcp.json", "**/run_claude.bat",
                    "**/.claude/", "**/.cargo/", "**/clippy.toml", "**/.clippy.toml", "**/deny.toml",
                    "**/rust-toolchain.toml", "**/rustfmt.toml", "**/.rustfmt.toml", "**/contracts/",
                    "**/.gitattributes", "**/.gitmodules"]))

    def test_git_attribute_and_submodule_files_are_protected_at_any_depth(self):
        for path in (".gitattributes", "engine/a/.gitattributes", ".gitmodules", "x/.gitmodules"):
            with self.subTest(path=path):
                self.assertTrue(guard.is_protected(path, REAL))

    def test_both_tools_load_the_list_with_the_same_rules(self):
        self.assertIs(guard.load_protected, pp.load_protected)
        self.assertIs(cc.load_protected, pp.load_protected)


class SubmoduleBumps(unittest.TestCase):
    """A .gitmodules `ignore = all` must not hide a submodule bump from either tool (review 0c-7, F9)."""

    def git_calls(self, run):
        calls = []

        def fake(command, **kwargs):
            calls.append(command)
            return subprocess.CompletedProcess(command, 0, stdout=b"", stderr=b"")

        with mock.patch("subprocess.run", fake):
            run()
        return [call for call in calls if "diff" in call]

    def test_the_guard_asks_git_to_report_submodules(self):
        diffs = self.git_calls(lambda: guard.changed_files(".", "a", "b"))
        self.assertTrue(diffs)
        for call in diffs:
            self.assertIn("--ignore-submodules=none", call)

    def test_the_classifier_asks_git_to_report_submodules(self):
        diffs = self.git_calls(lambda: cc.changes(".", "a", "b"))
        self.assertTrue(diffs)
        for call in diffs:
            self.assertIn("--ignore-submodules=none", call)


if __name__ == "__main__":
    unittest.main()
