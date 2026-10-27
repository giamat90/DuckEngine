"""Tests for the release version computation."""

import subprocess
import sys
import unittest

import next_version


class CommitLevel(unittest.TestCase):
    def test_feat_is_minor(self):
        self.assertEqual(next_version.level("feat: add sprite batcher"), "minor")

    def test_fix_and_perf_are_patch(self):
        self.assertEqual(next_version.level("fix: clamp the index"), "patch")
        self.assertEqual(next_version.level("perf: avoid a copy"), "patch")

    def test_other_types_are_patch(self):
        for kind in ("docs", "refactor", "test", "chore"):
            with self.subTest(kind=kind):
                self.assertEqual(next_version.level(f"{kind}: tidy"), "patch")

    def test_scoped_type_is_read(self):
        self.assertEqual(next_version.level("feat(core): add arena"), "minor")

    def test_bang_is_major(self):
        self.assertEqual(next_version.level("feat!: drop the old port"), "major")
        self.assertEqual(next_version.level("fix(core)!: change handle layout"), "major")

    def test_breaking_change_footer_is_major(self):
        message = "refactor: rename port\n\nBREAKING CHANGE: IWindow is now IWindowPort"
        self.assertEqual(next_version.level(message), "major")

    def test_hyphenated_breaking_footer_is_major(self):
        message = "fix: a\n\nBREAKING-CHANGE: removed"
        self.assertEqual(next_version.level(message), "major")

    def test_breaking_text_inside_a_line_is_not_a_footer(self):
        self.assertEqual(next_version.level("fix: mention BREAKING CHANGE: no"), "patch")

    def test_type_is_case_insensitive(self):
        self.assertEqual(next_version.level("Feat: a"), "minor")
        self.assertEqual(next_version.level("FEAT!: a"), "major")
        self.assertEqual(next_version.level("Fix(core)!: a"), "major")

    def test_header_must_be_the_first_line(self):
        message = "Merge pull request #1 from x\n\nfeat: add"
        self.assertEqual(next_version.level(message), "patch")

    def test_leading_blank_lines_are_ignored(self):
        self.assertEqual(next_version.level("\nfeat: a"), "minor")

    def test_empty_scope_or_missing_space_is_malformed(self):
        for message in ("feat(): x", "feat(a\nb): x", "feat:x"):
            with self.subTest(message=message):
                self.assertEqual(next_version.level(message), "patch")

    def test_tab_or_extra_spaces_after_colon_are_accepted(self):
        self.assertEqual(next_version.level("feat:  a"), "minor")
        self.assertEqual(next_version.level("feat:\ta"), "minor")

    def test_malformed_message_is_patch(self):
        malformed = ("", "update stuff", "Merge pull request #13 from x/y", "feat add", "feat: ")
        for message in malformed:
            with self.subTest(message=message):
                self.assertEqual(next_version.level(message), "patch")


class HighestLevel(unittest.TestCase):
    def test_highest_level_wins(self):
        messages = ["fix: a", "feat: b", "docs: c"]
        self.assertEqual(next_version.highest_level(messages), "minor")
        self.assertEqual(next_version.highest_level(messages + ["chore!: d"]), "major")

    def test_no_commits_is_patch(self):
        self.assertEqual(next_version.highest_level([]), "patch")


class NextVersion(unittest.TestCase):
    def test_stable_versions_bump_by_level(self):
        self.assertEqual(next_version.next_version("1.2.3", ["fix: a"]), "1.2.4")
        self.assertEqual(next_version.next_version("1.2.3", ["feat: a"]), "1.3.0")
        self.assertEqual(next_version.next_version("1.2.3", ["feat!: a"]), "2.0.0")

    def test_pre_one_breaking_bumps_minor(self):
        self.assertEqual(next_version.next_version("0.4.7", ["feat!: a"]), "0.5.0")

    def test_pre_one_feature_bumps_patch(self):
        self.assertEqual(next_version.next_version("0.4.7", ["feat: a"]), "0.4.8")

    def test_docs_only_merge_bumps_patch(self):
        self.assertEqual(next_version.next_version("0.4.7", ["docs: a"]), "0.4.8")

    def test_a_phase_footer_bumps_the_minor_number_before_1_0(self):
        merge = "feat(phase-1): phase 1 (#80)\n\nRelease-Phase: 1\n"
        self.assertEqual(next_version.next_version("0.1.9", ["feat: a", merge]), "0.2.0")

    def test_a_phase_footer_alone_is_enough(self):
        self.assertEqual(next_version.next_version("0.1.9", ["docs: a\n\nRelease-Phase: 2"]), "0.2.0")

    def test_a_phase_footer_inside_a_line_is_not_a_footer(self):
        self.assertEqual(next_version.next_version("0.1.9", ["docs: say Release-Phase: 1 in prose"]), "0.1.10")

    def test_a_phase_footer_needs_a_value(self):
        self.assertEqual(next_version.next_version("0.1.9", ["docs: a\n\nRelease-Phase: "]), "0.1.10")

    def test_a_phase_footer_is_at_least_minor_after_1_0_and_never_beats_a_break(self):
        self.assertEqual(next_version.next_version("1.4.2", ["fix: a\n\nRelease-Phase: 3"]), "1.5.0")
        self.assertEqual(next_version.next_version("1.4.2", ["fix!: a", "docs: b\n\nRelease-Phase: 3"]), "2.0.0")

    def test_bad_current_version_is_rejected(self):
        for current in ("", "1.2", "v1.2.3", "1.2.3-rc1", "a.b.c", "1.2.3\n", "01.2.3"):
            with self.subTest(current=current):
                with self.assertRaises(ValueError):
                    next_version.next_version(current, ["fix: a"])


class Cli(unittest.TestCase):
    def test_reads_nul_separated_messages_and_prints_version(self):
        stdin = "fix: a\n\0feat: b\n\nbody\n\0"
        self.assertEqual(next_version.run(["--current", "1.2.3"], stdin), (0, "1.3.0\n", ""))

    def test_git_log_format_with_newline_after_each_separator(self):
        stdin = "fix: a\n\0\nfeat: b\n\0\n"
        self.assertEqual(next_version.run(["--current", "1.2.3"], stdin), (0, "1.3.0\n", ""))

    def test_empty_input_bumps_patch(self):
        self.assertEqual(next_version.run(["--current", "1.2.3"], ""), (0, "1.2.4\n", ""))

    def test_bad_current_version_exits_with_bad_version_code(self):
        code, out, err = next_version.run(["--current", "v1"], "fix: a\0")
        self.assertEqual((code, out), (next_version.EXIT_BAD_VERSION, ""))
        self.assertIn("not a plain X.Y.Z version: 'v1'", err)

    def test_script_entry_point_fails_on_a_bad_version(self):
        done = subprocess.run(
            [sys.executable, next_version.__file__, "--current", "v1"],
            input="",
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(done.returncode, 2)
        self.assertIn("not a plain X.Y.Z version", done.stderr)

    def test_script_entry_point_prints_version(self):
        done = subprocess.run(
            [sys.executable, next_version.__file__, "--current", "0.1.0"],
            input="feat!: a\0",
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertEqual(done.stdout, "0.2.0\n")
