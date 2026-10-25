import os
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import approval_report as ar

CLASSIFIER_3 = """class=3
  gates/wiki-lint.toml: protected path
  tools/x.py: protected path
waivers=0
"""
CLASSIFIER_1 = "class=1\nwaivers=0\n"
CLASSIFIER_W = """class=2
  engine/a/src/x.rs: adds a lint waiver
waivers=2
  engine/a/src/x.rs: #[allow(dead_code)]
  engine/a/src/y.rs: #[ignore]
"""
OK_LINE = "OK 3 crates checked, 0 violations\n"
FAIL_TEXT = "VIOLATION layer a.rs:3 L1 includes L2\nFAIL 3 crates checked, 1 violations\n"


class ParseClassification(unittest.TestCase):
    def test_class_and_reasons(self):
        found = ar.parse_classification(CLASSIFIER_3)
        self.assertEqual(3, found.klass)
        self.assertEqual([("gates/wiki-lint.toml", "protected path"), ("tools/x.py", "protected path")], found.reasons)

    def test_a_routine_change_has_no_reasons(self):
        found = ar.parse_classification(CLASSIFIER_1)
        self.assertEqual((1, []), (found.klass, found.reasons))

    def test_waivers_are_parsed_after_the_reasons(self):
        found = ar.parse_classification(CLASSIFIER_W)
        self.assertEqual([("engine/a/src/x.rs", "adds a lint waiver")], found.reasons)
        self.assertEqual([("engine/a/src/x.rs", "#[allow(dead_code)]"), ("engine/a/src/y.rs", "#[ignore]")], found.waivers)

    def test_no_waivers_line_is_an_error_not_zero(self):
        with self.assertRaises(ValueError):
            ar.parse_classification("class=1\n")

    def test_a_waiver_count_that_does_not_match_the_list_is_an_error(self):
        for text in ("class=1\nwaivers=2\n  a.rs: x\n", "class=1\nwaivers=0\n  a.rs: x\n", "class=1\nwaivers=x\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                ar.parse_classification(text)

    def test_a_reason_may_contain_a_colon(self):
        found = ar.parse_classification("class=2\n  a/b.rs: public item: pub fn f\nwaivers=0\n")
        self.assertEqual([("a/b.rs", "public item: pub fn f")], found.reasons)

    def test_unrecognised_output_is_an_error(self):
        for text in ("", "oops\n", "class=9\n", "class=x\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                ar.parse_classification(text)


class ParseSummary(unittest.TestCase):
    def test_an_ok_line(self):
        found = ar.parse_summary(OK_LINE)
        self.assertEqual((True, "3 crates checked, 0 violations", []), (found.passed, found.detail, found.violations))

    def test_a_failing_run_keeps_its_violations(self):
        found = ar.parse_summary(FAIL_TEXT)
        self.assertEqual((False, "3 crates checked, 1 violations"), (found.passed, found.detail))
        self.assertEqual(["layer a.rs:3 L1 includes L2"], found.violations)

    def test_a_tool_error_is_a_failure_with_its_message(self):
        found = ar.parse_summary("FAIL scan path 'x' does not exist\n")
        self.assertEqual((False, "scan path 'x' does not exist"), (found.passed, found.detail))

    def test_output_without_a_summary_line_is_a_failure(self):
        found = ar.parse_summary("Traceback (most recent call last):\n")
        self.assertFalse(found.passed)
        self.assertIn("no summary", found.detail)

    def test_empty_output_is_a_failure(self):
        self.assertFalse(ar.parse_summary("").passed)


class Render(unittest.TestCase):
    def build(self, klass=3, arch=OK_LINE, wiki=OK_LINE, hash_="83f5093def889fac"):
        return ar.render(
            head="agent/x", base="main", diff_hash=hash_,
            classification=ar.parse_classification(CLASSIFIER_3 if klass == 3 else CLASSIFIER_1),
            checks={"Architecture check": ar.parse_summary(arch), "Wiki lint": ar.parse_summary(wiki)},
        )

    def test_the_header_names_the_diff_and_its_hash(self):
        text = self.build()
        self.assertIn("# Metric report: agent/x against main", text)
        self.assertIn("Diff hash: `83f5093def889fac`", text)

    def test_all_passing_gives_a_clear_verdict(self):
        self.assertIn("Verdict: all 3 checks in this report pass", self.build())

    def test_the_verdict_never_claims_more_than_the_report_shows(self):
        text = self.build()
        self.assertNotIn("only the checks listed", text)
        self.assertNotRegex(text, "Verdict: all checks")

    def test_a_failing_check_gives_a_failing_verdict_and_lists_violations(self):
        text = self.build(arch=FAIL_TEXT)
        self.assertIn("Verdict: 1 check failing", text)
        self.assertIn("| Architecture check | FAIL | 3 crates checked, 1 violations |", text)
        self.assertIn("- layer a.rs:3 L1 includes L2", text)

    def test_two_failing_checks_are_counted(self):
        self.assertIn("Verdict: 2 checks failing", self.build(arch=FAIL_TEXT, wiki=FAIL_TEXT))

    def test_the_class_row_lists_the_files_that_set_it(self):
        text = self.build()
        self.assertIn("| Change class | 3 (governance) |", text)
        self.assertIn("- `gates/wiki-lint.toml`: protected path", text)

    def test_no_waivers_is_a_visible_zero(self):
        text = self.build()
        self.assertIn("| Waivers added | 0 | none |", text)
        self.assertNotIn("## Waivers added", text)

    def test_waivers_are_counted_and_listed_but_do_not_fail_the_verdict(self):
        text = ar.render("agent/x", "main", "83f5093def889fac", ar.parse_classification(CLASSIFIER_W),
                         {"Wiki lint": ar.parse_summary(OK_LINE)})
        self.assertIn("| Waivers added | 2 | the owner reviews each one |", text)
        self.assertIn("## Waivers added", text)
        self.assertIn("- `engine/a/src/x.rs`: `#[allow(dead_code)]`", text)
        self.assertIn("Verdict: all 2 checks in this report pass", text)

    def test_the_waiver_count_is_no_longer_listed_as_missing(self):
        self.assertNotIn("waiver count", self.build())

    def test_a_routine_change_is_labelled_routine(self):
        text = self.build(klass=1)
        self.assertIn("| Change class | 1 (routine) |", text)
        self.assertNotIn("Why the class", text)

    def test_the_gaps_are_stated(self):
        text = self.build()
        self.assertIn("## Not in this report", text)
        for word in ("Coverage", "mutation", "Performance", "Product view"):
            self.assertIn(word, text)

    def test_the_checks_that_exist_but_run_elsewhere_are_named(self):
        text = self.build()
        self.assertIn("## Checks that run in other jobs", text)
        for word in ("Format", "Clippy", "Build and tests", "Digest", "Governance guard", "Holdout",
                     "Third-party", "Complexity", "duplication", "magic"):
            with self.subTest(word=word):
                self.assertIn(word, text)

    def test_a_pipe_in_a_detail_cannot_break_the_table(self):
        text = ar.render("h", "b", "0" * 16, ar.parse_classification(CLASSIFIER_1),
                         {"X": ar.Summary(False, "a | b", [])})
        self.assertIn("a \\| b", text)


class Gather(unittest.TestCase):
    def test_a_tool_that_cannot_start_is_a_failed_check_not_a_crash(self):
        summary = ar.run_check(["definitely-not-a-command-xyz"])
        self.assertFalse(summary.passed)
        self.assertIn("could not run", summary.detail)

    def test_a_tool_output_is_parsed(self):
        summary = ar.run_check(["python", "-c", "print('OK 2 pages checked, 0 violations')"])
        self.assertEqual((True, "2 pages checked, 0 violations"), (summary.passed, summary.detail))

    def test_a_non_zero_exit_with_an_ok_line_still_fails(self):
        summary = ar.run_check(["python", "-c", "print('OK 1 pages checked, 0 violations'); raise SystemExit(1)"])
        self.assertFalse(summary.passed)

    def test_the_classifier_error_message_is_shown(self):
        found = ar.run_classifier(["python", "-c", "import sys; sys.stderr.write('bad ref\\n'); raise SystemExit(1)"])
        self.assertIn("bad ref", found.reasons[0][1])

    def test_the_classifier_message_on_stdout_is_shown(self):
        found = ar.run_classifier(["python", "-c", "print('FAIL no changed files'); raise SystemExit(1)"])
        self.assertIn("FAIL no changed files", found.reasons[0][1])

    def test_the_classifier_failing_means_class_three(self):
        found = ar.run_classifier(["python", "-c", "raise SystemExit(2)"])
        self.assertEqual(3, found.klass)
        self.assertIn("classifier failed", found.reasons[0][1])


class FailClosed(unittest.TestCase):
    def test_a_violation_before_an_ok_line_still_fails(self):
        found = ar.parse_summary("VIOLATION layer a\nOK 3 crates checked, 0 violations\n")
        self.assertFalse(found.passed)

    def test_a_fail_line_before_an_ok_line_still_fails(self):
        self.assertFalse(ar.parse_summary("FAIL x\nOK 1 pages checked, 0 violations\n").passed)

    def test_garbled_classifier_lines_are_refused(self):
        for text in ("class=1\nFAIL garbage\n", "class=1\nrandom\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                ar.parse_classification(text)

    def test_a_classifier_that_prints_class_one_but_exits_nonzero_is_class_three(self):
        found = ar.run_classifier([sys.executable, "-c", "print('class=1'); raise SystemExit(1)"])
        self.assertEqual((3, True), (found.klass, found.failed))

    def test_a_classifier_success_is_parsed(self):
        found = ar.run_classifier([sys.executable, "-c", "print('class=2'); print('  a.rs: public item'); print('waivers=0')"])
        self.assertEqual((2, False, [("a.rs", "public item")]), (found.klass, found.failed, found.reasons))

    def test_a_failed_classifier_fails_the_verdict(self):
        text = ar.render("h", "b", "0" * 16, ar.Classification(3, [("(classifier)", "classifier failed")], True),
                         {"X": ar.Summary(True, "ok", [])})
        self.assertIn("Verdict: 1 check failing", text)

    def test_a_crash_after_ok_shows_the_exit_code_and_stderr(self):
        code = "import sys; print('OK 1 pages checked, 0 violations'); sys.stderr.write('boom\\n'); raise SystemExit(4)"
        found = ar.run_check([sys.executable, "-c", code])
        self.assertFalse(found.passed)
        self.assertIn("exit 4", found.detail)
        self.assertIn("boom", found.detail)

    def test_a_hung_tool_is_a_failed_check(self):
        found = ar.run_check([sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.5)
        self.assertFalse(found.passed)
        self.assertIn("timed out", found.detail)

    def test_a_hung_classifier_is_class_three(self):
        found = ar.run_classifier([sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.5)
        self.assertEqual((3, True), (found.klass, found.failed))


class Boundaries(unittest.TestCase):
    def test_the_tools_get_the_documented_timeout(self):
        seen = []

        def fake(command, **kwargs):
            seen.append(kwargs.get("timeout"))
            return subprocess.CompletedProcess(command, 0, stdout="OK x\n", stderr="")

        with mock.patch("subprocess.run", fake):
            ar.run_check(["x", "y"])
            ar.run_classifier(["x", "y"])
        self.assertEqual([ar.TIMEOUT_SECONDS] * 2, seen)
        self.assertEqual(600, ar.TIMEOUT_SECONDS)

    def test_the_markdown_escape_covers_hash_ampersand_and_tilde(self):
        self.assertEqual("a\\#b\\&c\\~d", ar._md("a#b&c~d"))


class Escaping(unittest.TestCase):
    def render(self, **overrides):
        args = dict(head="agent/x", base="main", diff_hash="0" * 16,
                    classification=ar.Classification(2, [("a`b|c.rs", "<img src=x> [x](http://e)")], False),
                    checks={"X": ar.Summary(False, "<b>d</b>", ["<b>v</b> `w` | z"])})
        args.update(overrides)
        return ar.render(**args)

    def test_html_and_links_are_neutralised(self):
        text = self.render()
        for special in "<>[]":
            self.assertNotRegex(text, "(?<!\\\\)" + re.escape(special))

    def test_a_backtick_in_a_path_cannot_end_the_code_span(self):
        self.assertIn("`a'b\\|c.rs`", self.render())

    def test_a_newline_cannot_start_a_new_line(self):
        text = self.render(checks={"X": ar.Summary(False, "a\n## fake", [])})
        self.assertNotIn("\n## fake", text)

    def test_the_branch_names_are_escaped(self):
        self.assertNotIn("<s>", self.render(head="<s>x", base="*b*"))

    def test_the_class_row_has_three_cells(self):
        row = next(line for line in self.render().splitlines() if line.startswith("| Change class"))
        self.assertEqual(4, row.count("|"))

    def test_the_diff_hash_must_look_like_a_hash(self):
        for bad in ("x`y", "", "ZZZ", "0" * 15, "0" * 65):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                ar.check_hash(bad)
        self.assertEqual("83f5093def889fac", ar.check_hash("83f5093def889fac"))


GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
           "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
           "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com"}


class Main(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(prefix="with space "))
        self.addCleanup(lambda: __import__("shutil").rmtree(self.dir, ignore_errors=True))
        self.tools = self.dir / "tools"
        for folder, name, body in (("classify-change", "classify_change.py", "print('class=1'); print('waivers=0')"),
                                   ("arch-check", "arch_check.py", "print('OK 3 crates checked, 0 violations')"),
                                   ("wiki-lint", "wiki_lint.py", "print('OK 9 pages checked, 0 violations')"),
                                   ("diff-hash", "diff_hash.py", "print('0' * 16)")):
            (self.tools / folder).mkdir(parents=True)
            (self.tools / folder / name).write_text(body + "\n", encoding="utf-8")
        for args in (("init", "-q", "-b", "a"), ("commit", "-q", "--allow-empty", "-m", "x"), ("branch", "b")):
            self.git(*args)
        self.git("checkout", "-q", "b")

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.dir, env=GIT_ENV, check=True, capture_output=True)

    def run_main(self, out=None):
        out = out or self.dir / "report.md"
        code = ar.main(["--repo", str(self.dir), "--base", "a", "--head", "b", "--diff-hash", "0" * 16,
                        "--tools-dir", str(self.tools), "--out", str(out)])
        return code, out

    def test_all_good_writes_the_report_and_exits_zero(self):
        code, out = self.run_main()
        text = out.read_text(encoding="utf-8")
        self.assertEqual(0, code)
        self.assertIn("| Wiki lint | pass | 9 pages checked, 0 violations |", text)
        self.assertIn("| Architecture check | pass | 3 crates checked, 0 violations |", text)

    def test_a_failing_tool_exits_one_and_the_report_says_so(self):
        (self.tools / "wiki-lint" / "wiki_lint.py").write_text("print('FAIL 9 pages checked, 2 violations')\n", encoding="utf-8")
        code, out = self.run_main()
        self.assertEqual(1, code)
        self.assertIn("Verdict: 1 check failing", out.read_text(encoding="utf-8"))

    def test_a_failed_classifier_exits_one(self):
        (self.tools / "classify-change" / "classify_change.py").write_text("raise SystemExit(1)\n", encoding="utf-8")
        self.assertEqual(1, self.run_main()[0])

    def test_the_config_comes_from_gates_dir(self):
        (self.tools / "wiki-lint" / "wiki_lint.py").write_text("import sys; print('OK 1 pages checked, 0 violations' if 'trusted/wiki-lint.toml' in ' '.join(sys.argv).replace(chr(92), '/') else 'FAIL x')\n", encoding="utf-8")
        out = self.dir / "r.md"
        code = ar.main(["--repo", str(self.dir), "--base", "a", "--head", "b", "--diff-hash", "0" * 16, "--tools-dir", str(self.tools),
                        "--gates-dir", str(self.dir / "trusted"), "--out", str(out)])
        self.assertEqual(0, code)

    def test_an_unresolvable_head_fails_the_tree_row(self):
        code = ar.main(["--repo", str(self.dir), "--base", "a", "--head", "nosuchref", "--diff-hash", "0" * 16,
                        "--tools-dir", str(self.tools), "--out", str(self.dir / "r.md")])
        self.assertEqual(1, code)
        self.assertIn("| Checked-out tree is the head | FAIL |", (self.dir / "r.md").read_text(encoding="utf-8"))

    def test_a_diff_hash_tool_that_exits_non_zero_fails_even_when_it_prints_the_claim(self):
        (self.tools / "diff-hash" / "diff_hash.py").write_text("print('0' * 16); raise SystemExit(1)", encoding="utf-8")
        text = self.run_main()[1].read_text(encoding="utf-8")
        self.assertIn("| Diff hash matches the diff | FAIL |", text)

    def test_a_diff_hash_that_is_not_the_hash_of_the_diff_fails(self):
        (self.tools / "diff-hash" / "diff_hash.py").write_text("print('1' * 16)\n", encoding="utf-8")
        code, out = self.run_main()
        self.assertEqual(1, code)
        self.assertIn("| Diff hash matches the diff | FAIL |", out.read_text(encoding="utf-8"))

    def test_a_matching_diff_hash_is_a_passing_row(self):
        text = self.run_main()[1].read_text(encoding="utf-8")
        self.assertIn("| Diff hash matches the diff | pass |", text)

    def test_a_tree_that_is_not_the_head_fails(self):
        self.git("checkout", "-q", "a")
        self.git("commit", "-q", "--allow-empty", "-m", "y")
        code, out = self.run_main()
        self.assertEqual(1, code)
        self.assertIn("| Checked-out tree is the head | FAIL |", out.read_text(encoding="utf-8"))

    def test_the_head_tree_is_a_passing_row(self):
        text = self.run_main()[1].read_text(encoding="utf-8")
        self.assertIn("| Checked-out tree is the head | pass |", text)

    def test_an_unwritable_output_is_exit_two_not_a_traceback(self):
        self.assertEqual(2, self.run_main(self.dir / "nodir" / "r.md")[0])


if __name__ == "__main__":
    unittest.main()
